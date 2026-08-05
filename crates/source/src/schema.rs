//! Per-subtree schema (§5b): `.schema.toml` declares typed fields; nearest wins.
//! Undeclared or mistyped front matter is a load error; also feeds view `order_by`.

use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use grackle_db::filter::{self, Schema, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    Str,
    Int,
    Bool,
    List,
    /// Root-relative image path (§6b thumbs; hero source, q23).
    Image,
    /// `YYYY-MM-DD` (bare `YYYY-MM` = first of month); ISO so filters order.
    Date,
    /// List of maps (q40); nested `fields` are scalars; group_by does not multi-key.
    Records {
        fields: BTreeMap<String, FieldType>,
    },
}

impl FieldType {
    /// Scalar type name; Records need [`parse_fields`].
    pub fn parse(s: &str) -> Option<FieldType> {
        Some(match s {
            "string" => FieldType::Str,
            "int" => FieldType::Int,
            "bool" => FieldType::Bool,
            "list" => FieldType::List,
            "image" => FieldType::Image,
            "date" => FieldType::Date,
            _ => return None,
        })
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            FieldType::Str => "string",
            FieldType::Int => "int",
            FieldType::Bool => "bool",
            FieldType::List => "list",
            FieldType::Image => "image",
            FieldType::Date => "date",
            FieldType::Records { .. } => "records",
        }
    }

    /// Filter view: image/date as Str, records as List.
    pub fn filter_type(&self) -> filter::Type {
        match self {
            FieldType::Str | FieldType::Image | FieldType::Date => filter::Type::Str,
            FieldType::Int => filter::Type::Int,
            FieldType::Bool => filter::Type::Bool,
            FieldType::List | FieldType::Records { .. } => filter::Type::List,
        }
    }
}

/// Engine-read fields by name: theme (§5a), shell (§5g), slot. Declared in
/// `base.toml` `[schema]` so markers/rules take the typed path.
/// Restating at another type is a load error. Public so `debug::row_fields`
/// can avoid printing cascade keys twice.
pub const CASCADE: &[(&str, FieldType)] = &[
    ("theme", FieldType::Str),
    ("shell", FieldType::Str),
    ("slot", FieldType::Str),
];

pub(crate) fn cascade_type(name: &str) -> Option<FieldType> {
    CASCADE
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| t.clone())
}

/// One rung's writer (for collision messages), its typed fields, and the
/// `default =` value each declaration carried (§5b), keyed the same as fields.
type Declared = (
    String,
    BTreeMap<String, FieldType>,
    BTreeMap<String, toml::Value>,
);

#[derive(Debug)]
pub struct Schemas {
    by_dir: BTreeMap<PathBuf, Declared>,
    /// `[collections.<name>.schema]`: one collection, possibly several sources.
    by_collection: BTreeMap<String, Declared>,
    /// `[schema]`: site-wide fields.
    site: BTreeMap<String, FieldType>,
    /// `default =` values from the site `[schema]` rung, keyed like `site`.
    site_defaults: BTreeMap<String, toml::Value>,
    /// Names `Row::field` owns first; declaring one is unread. No `Default`:
    /// an empty reserved set would accept shadows in silence.
    reserved: Schema,
}

/// Two writers of a colliding name, ordered so the message is walk-order stable.
fn conflict(a: &str, a_ty: FieldType, b: &str, b_ty: FieldType) -> String {
    let mut both = [(a, a_ty), (b, b_ty)];
    both.sort_by(|x, y| x.0.cmp(y.0));
    format!(
        "{} says {}, {} says {}",
        both[0].0,
        both[0].1.name(),
        both[1].0,
        both[1].1.name()
    )
}

/// Validated extras plus image-typed paths for the thumb pass.
#[derive(Debug, Default)]
pub struct Fields {
    pub values: BTreeMap<String, Value>,
    pub images: BTreeMap<String, String>,
}

impl Schemas {
    /// `reserved`: row schema a declaration may not shadow (`row_schema()`).
    pub fn new(reserved: Schema) -> Schemas {
        Schemas {
            by_dir: BTreeMap::new(),
            by_collection: BTreeMap::new(),
            site: BTreeMap::new(),
            site_defaults: BTreeMap::new(),
            reserved,
        }
    }

    pub fn add(&mut self, dir: &Path, text: &str, file: &Path) -> Result<()> {
        let table: toml::Table = text
            .parse()
            .map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
        let whose = file.display().to_string();
        let (fields, defaults) = parse_fields(table, &whose, &self.reserved)?;
        self.check_positional_collision(dir, &whose, &fields)?;
        self.by_dir
            .insert(dir.to_path_buf(), (whose, fields, defaults));
        Ok(())
    }

    /// Same-rung type disagreement across unrelated dirs is an error
    ///: nearness orders ancestor/descendant; nothing else does.
    fn check_positional_collision(
        &self,
        dir: &Path,
        whose: &str,
        fields: &BTreeMap<String, FieldType>,
    ) -> Result<()> {
        for (name, ty) in fields {
            for (other_dir, (other_whose, other_fields, _)) in &self.by_dir {
                let Some(other) = other_fields.get(name) else {
                    continue;
                };
                if other == ty || dir.starts_with(other_dir) || other_dir.starts_with(dir) {
                    continue;
                }
                bail!(
                    "{} — two .schema.toml files declare field {name:?} with \
                     different types and neither directory contains the other, \
                     so nothing orders them. The site's filter vocabulary \
                     (`where`, `order_by`, `group_by`) has one entry per name: \
                     give them the same type, or rename one.",
                    conflict(whose, ty.clone(), other_whose, other.clone())
                );
            }
        }
        Ok(())
    }

    /// `[schema]` in `grackle.toml`: site-wide fields (§4d flags live here).
    pub fn set_site(&mut self, table: toml::Table, whose: &str) -> Result<()> {
        let (fields, defaults) = parse_fields(table, whose, &self.reserved)?;
        self.site = fields;
        self.site_defaults = defaults;
        Ok(())
    }

    /// `[collections.<name>.schema]`: one collection across several sources (§4).
    pub fn add_collection(&mut self, name: &str, table: toml::Table, whose: &str) -> Result<()> {
        let (fields, defaults) = parse_fields(table, whose, &self.reserved)?;
        // Sibling collections: no nearness; disagreement would be alphabetical.
        for (other_name, (other_whose, other_fields, _)) in &self.by_collection {
            if other_name == name {
                continue;
            }
            for (field, ty) in &fields {
                let Some(other) = other_fields.get(field) else {
                    continue;
                };
                if other == ty {
                    continue;
                }
                bail!(
                    "{} — two collections declare field {field:?} with different \
                     types. The site's filter vocabulary has one entry per name, \
                     and collections have no nearness to rank them by: give them \
                     the same type, or rename one.",
                    conflict(whose, ty.clone(), other_whose, other.clone())
                );
            }
        }
        self.by_collection
            .insert(name.to_string(), (whose.to_string(), fields, defaults));
        Ok(())
    }

    /// Governing schema: `.schema.toml` chain + collection + site. Nearest
    /// wins (positional > collection > site). Empty map is still governing.
    pub fn resolve(&self, collection: &str, dir: &Path) -> BTreeMap<&str, FieldType> {
        let mut out: BTreeMap<&str, FieldType> = BTreeMap::new();
        let mut cur = Some(dir);
        while let Some(d) = cur {
            if let Some((_, fields, _)) = self.by_dir.get(d) {
                for (k, t) in fields {
                    out.entry(k.as_str()).or_insert_with(|| t.clone());
                }
            }
            if d.as_os_str().is_empty() {
                break;
            }
            cur = d.parent();
        }
        for src in [
            self.by_collection.get(collection).map(|(_, f, _)| f),
            Some(&self.site),
        ] {
            let Some(fields) = src else { continue };
            for (k, t) in fields {
                out.entry(k.as_str()).or_insert_with(|| t.clone());
            }
        }
        out
    }

    /// Declared defaults (§5b `default =`), resolved by the same nearest-wins
    /// walk as [`resolve`]: the nearest rung that *declares* a name owns its
    /// default, and a nearer redeclaration carrying no `default` shadows a
    /// farther one that did (whole-entry shadowing, §4d). The floor of the
    /// value ladder; `apply_schema_defaults` spends it.
    pub fn schema_defaults(&self, collection: &str, dir: &Path) -> BTreeMap<&str, &toml::Value> {
        let mut claimed: BTreeSet<&str> = BTreeSet::new();
        let mut out: BTreeMap<&str, &toml::Value> = BTreeMap::new();
        let mut cur = Some(dir);
        while let Some(d) = cur {
            if let Some((_, fields, defs)) = self.by_dir.get(d) {
                claim_defaults(fields, defs, &mut claimed, &mut out);
            }
            if d.as_os_str().is_empty() {
                break;
            }
            cur = d.parent();
        }
        if let Some((_, fields, defs)) = self.by_collection.get(collection) {
            claim_defaults(fields, defs, &mut claimed, &mut out);
        }
        claim_defaults(&self.site, &self.site_defaults, &mut claimed, &mut out);
        out
    }

    /// Site-wide declared vocabulary for view `where`/`order_by` (§5b).
    pub fn declared(&self) -> BTreeMap<&str, FieldType> {
        let mut out = BTreeMap::new();
        for fields in self
            .by_dir
            .values()
            .map(|(_, f, _)| f)
            .chain(self.by_collection.values().map(|(_, f, _)| f))
            .chain(std::iter::once(&self.site))
        {
            for (k, t) in fields {
                out.entry(k.as_str()).or_insert_with(|| t.clone());
            }
        }
        out
    }

    /// Declared fields as a filter schema for routes (§4e); no row built-ins.
    pub fn declared_schema(&self) -> Schema {
        let mut s = Schema::new();
        for (name, ty) in self.declared() {
            insert_declared(&mut s, name, ty);
        }
        s
    }

    /// Built-ins plus every declared field: one env for `where`, `order_by`,
    /// `group_by`, and relation `rank`.
    pub fn row_filter_schema(&self) -> Schema {
        let mut s = grackle_model::row_schema();
        for (name, ty) in self.declared() {
            // Never shadows a built-in (`add`); interned for `serve` reload.
            insert_declared(&mut s, name, ty);
        }
        s
    }
}

/// One rung's contribution to [`Schemas::schema_defaults`]: the first (nearest)
/// rung to name a field claims it, so its `default` — or its silence — shadows
/// any farther rung's default for the same name.
fn claim_defaults<'a>(
    fields: &'a BTreeMap<String, FieldType>,
    defs: &'a BTreeMap<String, toml::Value>,
    claimed: &mut BTreeSet<&'a str>,
    out: &mut BTreeMap<&'a str, &'a toml::Value>,
) {
    for name in fields.keys() {
        if claimed.insert(name.as_str()) {
            if let Some(v) = defs.get(name) {
                out.insert(name.as_str(), v);
            }
        }
    }
}

fn insert_declared(s: &mut Schema, name: &str, ty: FieldType) {
    let key = grackle_model::intern(name.to_string());
    s.insert(key, ty.filter_type());
    if matches!(ty, FieldType::Date) {
        for part in ["year", "month", "day"] {
            s.insert(
                grackle_model::intern(format!("{name}.{part}")),
                filter::Type::Int,
            );
        }
    }
}

/// The error a `[schema.fields]` entry raises when it lacks a valid `type =`.
fn needs_type_error(whose: &str, name: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{whose}: field {name:?} needs type = \"string\" | \"int\" | \
         \"bool\" | \"list\" | \"image\" | \"date\" | \"records\""
    )
}

/// Parse a declaration table into typed names. Free fn so `Config` can
/// type-check profile `force` before a [`Schemas`] exists.
fn parse_fields(
    table: toml::Table,
    whose: &str,
    reserved: &Schema,
) -> Result<(BTreeMap<String, FieldType>, BTreeMap<String, toml::Value>)> {
    let mut fields = BTreeMap::new();
    let mut defaults = BTreeMap::new();
    for (name, v) in table {
        let Some(t) = v.as_table() else {
            return Err(needs_type_error(whose, &name));
        };
        let Some(ty_name) = t.get("type").and_then(|x| x.as_str()) else {
            return Err(needs_type_error(whose, &name));
        };
        let ty = if ty_name == "records" {
            parse_records_fields(t, &name, whose)?
        } else {
            let Some(ty) = FieldType::parse(ty_name) else {
                return Err(needs_type_error(whose, &name));
            };
            ty
        };
        // Restating a cascade key is fine; a different type is not.
        if let Some(engine) = cascade_type(&name) {
            if ty != engine {
                bail!(
                    "{whose}: field {name:?} is one of the engine's own cascade \
                     keys: it reads {name:?} off every row, so it must be \
                     declared {} or not at all",
                    engine.name()
                );
            }
        } else if reserved.contains_key(name.as_str()) {
            bail!(
                "{whose}: field {name:?} is a built-in row field, so declaring \
                 it would be silently overruled — every row already has \
                 one. Rename the declaration."
            );
        }
        // Records take no `default`: a map literal spells nothing a front-matter
        // block would not, and checking it would drag whole-array coercion in.
        let allowed = match &ty {
            FieldType::Records { .. } => ["type", "fields"].as_slice(),
            _ => ["type", "default"].as_slice(),
        };
        let extra: Vec<&str> = t
            .keys()
            .map(String::as_str)
            .filter(|k| !allowed.contains(k))
            .collect();
        if !extra.is_empty() {
            bail!(
                "{whose}: field {name:?} has unknown key(s) {} — a field \
                 declaration takes: {}",
                extra.join(", "),
                allowed.join(", ")
            );
        }
        if let Some(def) = t.get("default") {
            // Type-check now so a mismatch names this declaration, not a far-off
            // row; the raw value re-coerces through `write_typed` at fill time.
            typed(&ty, &name, def, whose)?;
            defaults.insert(name.clone(), def.clone());
        }
        fields.insert(name, ty);
    }
    Ok((fields, defaults))
}

fn parse_records_fields(table: &toml::Table, name: &str, whose: &str) -> Result<FieldType> {
    let Some(fields_v) = table.get("fields") else {
        bail!(
            "{whose}: field {name:?} is type = \"records\" and needs \
             fields = {{ … }} naming each item key"
        );
    };
    let Some(fields_t) = fields_v.as_table() else {
        bail!("{whose}: field {name:?}: fields must be a table");
    };
    if fields_t.is_empty() {
        bail!("{whose}: field {name:?}: fields must name at least one key");
    }
    let mut fields = BTreeMap::new();
    for (k, v) in fields_t {
        let ty_name = match v {
            toml::Value::String(s) => s.as_str(),
            toml::Value::Table(t) => t.get("type").and_then(|x| x.as_str()).ok_or_else(|| {
                anyhow::anyhow!(
                    "{whose}: field {name:?}.fields.{k} needs type = \
                         \"string\" | \"int\" | \"bool\""
                )
            })?,
            other => bail!(
                "{whose}: field {name:?}.fields.{k} must be a type name or \
                 {{ type = … }}, got {other}"
            ),
        };
        let Some(ty) = FieldType::parse(ty_name)
            .filter(|t| matches!(t, FieldType::Str | FieldType::Int | FieldType::Bool))
        else {
            bail!(
                "{whose}: field {name:?}.fields.{k} must be \"string\", \
                 \"int\", or \"bool\" (got {ty_name:?})"
            );
        };
        fields.insert(k.clone(), ty);
    }
    Ok(FieldType::Records { fields })
}

/// Site `[schema]` alone: what `Config` knows before the tree walk.
/// Profile `force` may only name this rung (not positional/collection schemas).
pub(crate) fn site_fields(table: &toml::Table, whose: &str) -> Result<BTreeMap<String, FieldType>> {
    Ok(parse_fields(table.clone(), whose, &grackle_model::row_schema())?.0)
}

/// Fold marker/rule defaults into declared fields (§4b, §4). Front matter
/// already won those keys; undeclared defaults are a load error.
pub fn apply_defaults(
    schema: &BTreeMap<&str, FieldType>,
    defaults: &BTreeMap<&str, &toml::Value>,
    fields: &mut Fields,
    path: &Path,
) -> Result<()> {
    let whose = format!("{}: a marker or rule", path.display());
    for (name, v) in defaults {
        let Some(ty) = schema.get(name) else {
            bail!(
                "{whose} sets {name:?}, which no schema declares \
                 — nothing would read it\n  declared fields: {}",
                knowns(schema)
            );
        };
        if fields.values.contains_key(*name) {
            continue; // front matter is nearer
        }
        write_typed(ty.clone(), name, v, fields, &whose)?;
    }
    Ok(())
}

/// The floor of the value ladder (§5b `default =`): fill declared defaults
/// only where every nearer writer — front matter, filename captures, markers,
/// rules — left the key unset. Runs below `force`, which still overwrites.
/// Types were checked when the default parsed, so a mismatch is impossible;
/// the `schema` lookup only supplies the coercion type.
pub fn apply_schema_defaults(
    schema: &BTreeMap<&str, FieldType>,
    defaults: &BTreeMap<&str, &toml::Value>,
    fields: &mut Fields,
    path: &Path,
) -> Result<()> {
    let whose = format!("{}: a schema default", path.display());
    for (name, v) in defaults {
        if fields.values.contains_key(*name) {
            continue; // a nearer writer already spoke
        }
        let Some(ty) = schema.get(name) else { continue };
        write_typed(ty.clone(), name, v, fields, &whose)?;
    }
    Ok(())
}

/// Rung 0 (§2): profile `[profiles.NAME.force]` overwrites the ladder
///. Runs last (top rung). Real lookup: a nearer `.schema.toml`
/// may retype a site-wide name (§5b).
pub fn force(
    forced: &BTreeMap<String, toml::Value>,
    schema: &BTreeMap<&str, FieldType>,
    fields: &mut Fields,
    path: &Path,
) -> Result<()> {
    for (name, v) in forced {
        let Some(ty) = schema.get(name.as_str()) else {
            bail!(
                "{}: the profile forces {name:?}, which no schema governing this \
                 row declares\n  declared fields: {}",
                path.display(),
                knowns(schema)
            );
        };
        let whose = format!("{}: the profile", path.display());
        write_typed(ty.clone(), name, v, fields, &whose)?;
    }
    Ok(())
}

pub(crate) fn knowns(schema: &BTreeMap<&str, FieldType>) -> String {
    let mut known: Vec<&str> = schema.keys().copied().collect();
    known.sort_unstable();
    known.join(", ")
}

/// Coerce one TOML value at its declared type into `fields` (shared by
/// markers, rules, and profile `force`).
pub(crate) fn write_typed(
    ty: FieldType,
    name: &str,
    v: &toml::Value,
    fields: &mut Fields,
    whose: &str,
) -> Result<()> {
    let value = typed(&ty, name, v, whose)?;
    if matches!(ty, FieldType::Image) {
        if let Value::Str(s) = &value {
            fields.images.insert(name.to_string(), s.clone());
        }
    }
    fields.values.insert(name.to_string(), value);
    Ok(())
}

/// Whitespace-separated list items (YAML front matter and TOML defaults).
pub(crate) fn list_from_words(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

/// One TOML value at its declared type (also used by profile `force` checks).
pub(crate) fn typed(ty: &FieldType, name: &str, v: &toml::Value, whose: &str) -> Result<Value> {
    Ok(match (ty, v) {
        (FieldType::Str | FieldType::Image, toml::Value::String(s)) => Value::Str(s.clone()),
        (FieldType::Date, toml::Value::String(s)) => Value::Str(date_str(s, name, whose)?),
        (FieldType::Int, toml::Value::Integer(i)) => Value::Int(*i),
        (FieldType::Bool, toml::Value::Boolean(b)) => Value::Bool(*b),
        (FieldType::List, toml::Value::Array(a)) => Value::str_list(
            a.iter()
                .map(|x| match x.as_str() {
                    Some(s) => Ok(s.to_string()),
                    None => bail!("{whose}: {name:?}: list items must be strings, got {x}"),
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        (FieldType::List, toml::Value::String(s)) => Value::str_list(list_from_words(s)),
        (FieldType::Records { fields: shape }, toml::Value::Array(a)) => Value::List(
            a.iter()
                .enumerate()
                .map(|(i, x)| record_item_toml(shape, name, i, x, whose))
                .collect::<Result<Vec<_>>>()?,
        ),
        (ty, other) => bail!(
            "{whose} sets {name:?} to {other}, but it is declared {}",
            ty.name()
        ),
    })
}

fn record_item_toml(
    shape: &BTreeMap<String, FieldType>,
    name: &str,
    i: usize,
    v: &toml::Value,
    whose: &str,
) -> Result<Value> {
    let Some(t) = v.as_table() else {
        bail!("{whose}: {name:?}[{i}] must be a table, got {v}");
    };
    let mut entries = Vec::new();
    for (k, raw) in t {
        let Some(fty) = shape.get(k) else {
            let known: Vec<&str> = shape.keys().map(String::as_str).collect();
            bail!(
                "{whose}: {name:?}[{i}] has unknown key {k:?} (known: {})",
                known.join(", ")
            );
        };
        entries.push((Value::Str(k.clone()), typed(fty, k, raw, whose)?));
    }
    Ok(Value::Map(entries))
}

fn record_item_yaml(
    shape: &BTreeMap<String, FieldType>,
    name: &str,
    i: usize,
    v: &serde_yaml_ng::Value,
    path: &Path,
) -> Result<Value> {
    use serde_yaml_ng::Value as Y;
    let Y::Mapping(map) = v else {
        bail!(
            "{}: {name:?}[{i}] must be a mapping, got {v:?}",
            path.display()
        );
    };
    let mut entries = Vec::new();
    for (k, raw) in map {
        let Y::String(key) = k else {
            bail!(
                "{}: {name:?}[{i}] keys must be strings, got {k:?}",
                path.display()
            );
        };
        let Some(fty) = shape.get(key) else {
            let known: Vec<&str> = shape.keys().map(String::as_str).collect();
            bail!(
                "{}: {name:?}[{i}] has unknown key {key:?} (known: {})",
                path.display(),
                known.join(", ")
            );
        };
        let val = match (fty, raw) {
            (FieldType::Str, Y::String(s)) => Value::Str(s.clone()),
            (FieldType::Int, Y::Number(n)) if n.as_i64().is_some() => {
                Value::Int(n.as_i64().unwrap())
            }
            (FieldType::Bool, Y::Bool(b)) => Value::Bool(*b),
            // YAML bare numbers as ints; stringify so `amount: 2` works.
            (FieldType::Str, Y::Number(n)) => Value::Str(n.to_string()),
            (fty, other) => bail!(
                "{}: {name:?}[{i}].{key} is declared {}, got {other:?}",
                path.display(),
                fty.name()
            ),
        };
        entries.push((Value::Str(key.clone()), val));
    }
    Ok(Value::Map(entries))
}

fn date_str(raw: &str, name: &str, whose: &str) -> Result<String> {
    let Some(d) = grackle_model::parse_date_str(raw) else {
        bail!("{whose}: {name:?}: {raw:?} is not YYYY-MM-DD (or YYYY-MM)");
    };
    Ok(d.format("%Y-%m-%d").to_string())
}

pub fn validate(
    schema: &BTreeMap<&str, FieldType>,
    extra: &BTreeMap<String, serde_yaml_ng::Value>,
    path: &Path,
) -> Result<Fields> {
    use serde_yaml_ng::Value as Y;
    let mut out = Fields::default();
    for (name, v) in extra {
        let Some(ty) = schema.get(name.as_str()) else {
            let known: Vec<&str> = schema.keys().copied().collect();
            bail!(
                "{}: front matter field {name:?} is not declared by any \
                 .schema.toml governing it\n  declared fields: {}",
                path.display(),
                known.join(", ")
            );
        };
        let whose = path.display().to_string();
        let value = match (ty, v) {
            (FieldType::Str, Y::String(s)) => Value::Str(s.clone()),
            (FieldType::Date, Y::String(s)) => Value::Str(date_str(s, name, &whose)?),
            (FieldType::Image, Y::String(s)) => {
                out.images.insert(name.clone(), s.clone());
                Value::Str(s.clone())
            }
            (FieldType::Int, Y::Number(n)) if n.as_i64().is_some() => {
                Value::Int(n.as_i64().unwrap())
            }
            (FieldType::Bool, Y::Bool(b)) => Value::Bool(*b),
            (FieldType::List, Y::Sequence(seq)) => Value::str_list(
                seq.iter()
                    .map(|x| match x {
                        Y::String(s) => Ok(s.clone()),
                        other => bail!(
                            "{}: field {name:?}: list items must be strings, got {other:?}",
                            path.display()
                        ),
                    })
                    .collect::<Result<Vec<_>>>()?,
            ),
            (FieldType::List, Y::String(s)) => Value::str_list(list_from_words(s)),
            // `tags:` with no value.
            (FieldType::List, Y::Null) => Value::str_list(Vec::<String>::new()),
            (FieldType::Records { fields: shape }, Y::Sequence(seq)) => Value::List(
                seq.iter()
                    .enumerate()
                    .map(|(i, x)| record_item_yaml(shape, name, i, x, path))
                    .collect::<Result<Vec<_>>>()?,
            ),
            (FieldType::Records { .. }, Y::Null) => Value::List(Vec::new()),
            (ty, other) => bail!(
                "{}: field {name:?} is declared {}, but the front matter has {other:?}",
                path.display(),
                ty.name()
            ),
        };
        out.values.insert(name.clone(), value);
    }
    Ok(out)
}

/// Seed `CASCADE` keys from named `FrontMatter` fields before defaults, so
/// front matter stays nearest (§4e). Undeclared names are a load error.
pub(crate) fn cascade_front(
    schema: &BTreeMap<&str, FieldType>,
    front: &crate::store::FrontMatter,
    fields: &mut Fields,
    path: &Path,
) -> Result<()> {
    let worn: [(&str, Option<Value>); 3] = [
        ("theme", front.theme.clone().map(Value::Str)),
        ("shell", front.shell.clone().map(Value::Str)),
        ("slot", front.slot.clone().map(Value::Str)),
    ];
    for (name, value) in worn {
        let Some(value) = value else { continue };
        if !schema.contains_key(name) {
            let known: Vec<&str> = schema.keys().copied().collect();
            bail!(
                "{}: front matter field {name:?} is not declared by any \
                 schema governing it\n  declared fields: {}",
                path.display(),
                known.join(", ")
            );
        }
        fields.values.insert(name.to_string(), value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schemas() -> Schemas {
        let mut s = Schemas::new(grackle_model::row_schema());
        s.add(
            Path::new("books"),
            "author = { type = \"string\" }\nshelf = { type = \"string\" }\ncover = { type = \"image\" }\n",
            Path::new("books/.schema.toml"),
        )
        .unwrap();
        s.add(
            Path::new("books/special"),
            "author = { type = \"int\" }\n", // deliberately shadows
            Path::new("books/special/.schema.toml"),
        )
        .unwrap();
        s
    }

    #[test]
    fn resolution_accumulates_nearest_wins() {
        let s = schemas();
        let base = s.resolve("entries", Path::new("books"));
        assert_eq!(base["author"], FieldType::Str);
        let deep = s.resolve("entries", Path::new("books/special"));
        assert_eq!(deep["author"], FieldType::Int, "nearest wins");
        assert_eq!(deep["shelf"], FieldType::Str, "ancestors accumulate");
        assert!(
            s.resolve("entries", Path::new("recipes")).is_empty(),
            "a dir nothing declares for is governed by an EMPTY schema now — \
             every row is governed (§4e), so this says `no fields`, not \
             `anything goes`"
        );
    }

    #[test]
    fn collection_and_site_schemas_join_the_positional_ones() {
        let mut s = schemas();
        s.add_collection(
            "notes",
            "series = { type = \"string\" }\nauthor = { type = \"int\" }\n"
                .parse()
                .unwrap(),
            "[collections.notes.schema]",
        )
        .unwrap();
        s.set_site(
            "archived = { type = \"bool\" }\n".parse().unwrap(),
            "[schema]",
        )
        .unwrap();

        let free = s.resolve("notes", Path::new("recipes"));
        assert_eq!(free["series"], FieldType::Str);
        assert_eq!(free["archived"], FieldType::Bool);

        let books = s.resolve("notes", Path::new("books"));
        assert_eq!(
            books["author"],
            FieldType::Str,
            "positional beats collection"
        );

        let other = s.resolve("pages", Path::new("recipes"));
        assert!(!other.contains_key("series"), "{other:?}");
        assert_eq!(other["archived"], FieldType::Bool, "site-wide reaches all");
    }

    /// §4e: a marker/rule may set any declared field.
    #[test]
    fn markers_and_rules_fill_declared_fields() {
        let mut s = schemas();
        s.set_site(
            "archived = { type = \"bool\" }\nseries = { type = \"string\" }\n"
                .parse()
                .unwrap(),
            "[schema]",
        )
        .unwrap();
        let schema = s.resolve("notes", Path::new("books"));

        let yes = toml::Value::Boolean(true);
        let name = toml::Value::String("Old".into());
        let defaults = BTreeMap::from([("archived", &yes), ("series", &name)]);

        let mut fields = Fields::default();
        fields
            .values
            .insert("series".into(), Value::Str("Mine".into()));
        apply_defaults(&schema, &defaults, &mut fields, Path::new("x.md")).unwrap();
        assert_eq!(fields.values["archived"], Value::Bool(true));
        assert_eq!(
            fields.values["series"],
            Value::Str("Mine".into()),
            "front matter is nearer than a marker"
        );
    }

    /// Undeclared marker key is a load error (§4e).
    #[test]
    fn a_default_naming_nothing_is_a_load_error() {
        let s = schemas();
        let schema = s.resolve("entries", Path::new("books"));
        let yes = toml::Value::Boolean(true);
        let defaults = BTreeMap::from([("archvied", &yes)]);
        let e = apply_defaults(
            &schema,
            &defaults,
            &mut Fields::default(),
            Path::new("x.md"),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("no schema declares"), "{e}");
        assert!(e.contains("author"), "it names the knowns: {e}");
    }

    /// Cascade keys are declared fields; wrong type is an error.
    #[test]
    fn a_cascade_key_is_typed_like_any_other() {
        let mut s = schemas();
        s.set_site(
            CASCADE
                .iter()
                .map(|(n, t)| format!("{n} = {{ type = \"{}\" }}\n", t.name()))
                .collect::<String>()
                .parse()
                .unwrap(),
            "[schema]",
        )
        .unwrap();
        let schema = s.resolve("entries", Path::new("books"));

        let one = toml::Value::Integer(1);
        let defaults = BTreeMap::from([("theme", &one)]);
        let e = apply_defaults(
            &schema,
            &defaults,
            &mut Fields::default(),
            Path::new("x.md"),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("x.md"), "{e}");
        assert!(e.contains("declared string"), "{e}");

        let yes = toml::Value::Boolean(true);
        let mut with_toc = schema.clone();
        with_toc.insert("toc", FieldType::Bool);
        let defaults = BTreeMap::from([("toc", &yes)]);
        let mut fields = Fields::default();
        apply_defaults(&with_toc, &defaults, &mut fields, Path::new("x.md")).unwrap();
        assert_eq!(fields.values["toc"], Value::Bool(true));

        let quoted = toml::Value::String("true".into());
        let defaults = BTreeMap::from([("toc", &quoted)]);
        let e = apply_defaults(
            &with_toc,
            &defaults,
            &mut Fields::default(),
            Path::new("x.md"),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("x.md"), "{e}");
        assert!(e.contains("declared bool"), "{e}");
    }

    /// §5b `default =`: a declared default fills only where nothing nearer did.
    #[test]
    fn a_schema_default_is_the_floor_of_the_ladder() {
        let mut s = schemas();
        s.set_site(
            "show_toc = { type = \"bool\", default = false }\n\
             show_hero = { type = \"bool\", default = true }\n"
                .parse()
                .unwrap(),
            "[schema]",
        )
        .unwrap();
        let schema = s.resolve("notes", Path::new("books"));
        let defaults = s.schema_defaults("notes", Path::new("books"));
        assert_eq!(defaults.len(), 2, "both defaults resolve: {defaults:?}");

        // Front matter is nearer, so the floor leaves it alone.
        let mut fields = Fields::default();
        fields.values.insert("show_hero".into(), Value::Bool(false));
        apply_schema_defaults(&schema, &defaults, &mut fields, Path::new("x.md")).unwrap();
        assert_eq!(
            fields.values["show_hero"],
            Value::Bool(false),
            "a nearer writer wins the key"
        );
        assert_eq!(
            fields.values["show_toc"],
            Value::Bool(false),
            "the unset key takes its declared default"
        );
    }

    /// A `default` is type-checked where it is declared, not at some far row.
    #[test]
    fn a_mistyped_default_names_its_declaration() {
        let mut s = Schemas::new(grackle_model::row_schema());
        let e = s
            .add(
                Path::new("books"),
                "show_toc = { type = \"bool\", default = \"yes\" }\n",
                Path::new("books/.schema.toml"),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("books/.schema.toml"), "{e}");
        assert!(e.contains("declared bool"), "{e}");
    }

    /// Whole-entry shadowing (§4d): a nearer redeclaration carrying no
    /// `default` silences a farther one that did.
    #[test]
    fn a_nearer_redeclaration_shadows_a_farther_default() {
        let mut s = Schemas::new(grackle_model::row_schema());
        s.add(
            Path::new("books"),
            "toc = { type = \"bool\", default = true }\n",
            Path::new("books/.schema.toml"),
        )
        .unwrap();
        s.add(
            Path::new("books/special"),
            "toc = { type = \"bool\" }\n", // redeclared, no default
            Path::new("books/special/.schema.toml"),
        )
        .unwrap();
        assert!(
            s.schema_defaults("notes", Path::new("books/special"))
                .is_empty(),
            "the nearer silent declaration wins the whole entry"
        );
        assert_eq!(
            s.schema_defaults("notes", Path::new("books")).len(),
            1,
            "farther down, the default still stands"
        );
    }

    /// `records` fields take no `default`; the key is rejected by name.
    #[test]
    fn a_records_field_declines_a_default() {
        let mut s = Schemas::new(grackle_model::row_schema());
        let e = s
            .add(
                Path::new("books"),
                "links = { type = \"records\", fields = { url = \"string\" }, \
                 default = [] }\n",
                Path::new("books/.schema.toml"),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("unknown key(s) default"), "{e}");
    }

    #[test]
    fn undeclared_and_mistyped_fields_are_load_errors() {
        let s = schemas();
        let schema = s.resolve("entries", Path::new("books"));
        let mut extra = BTreeMap::new();
        extra.insert(
            "autor".to_string(),
            serde_yaml_ng::Value::String("x".into()),
        );
        let e = validate(&schema, &extra, Path::new("books/j.md"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("not declared"), "{e}");
        assert!(e.contains("author, cover, shelf"), "{e}");

        let mut extra = BTreeMap::new();
        extra.insert("author".to_string(), serde_yaml_ng::Value::Number(3.into()));
        let e = validate(&schema, &extra, Path::new("books/j.md"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("declared string"), "{e}");
    }

    #[test]
    fn a_date_field_accepts_iso_and_rejects_noise() {
        let mut schema = BTreeMap::new();
        schema.insert("published", FieldType::Date);
        let mut extra = BTreeMap::new();
        extra.insert(
            "published".to_string(),
            serde_yaml_ng::Value::String("2020-01".into()),
        );
        let fields = validate(&schema, &extra, Path::new("x.md")).unwrap();
        assert_eq!(fields.values["published"], Value::Str("2020-01-01".into()));

        extra.insert(
            "published".to_string(),
            serde_yaml_ng::Value::String("soon".into()),
        );
        let e = validate(&schema, &extra, Path::new("x.md"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("YYYY-MM-DD"), "{e}");
    }

    #[test]
    fn a_list_field_coerces_a_whitespace_separated_string() {
        let mut s = schemas();
        s.set_site(
            "keywords = { type = \"list\" }\n".parse().unwrap(),
            "[schema]",
        )
        .unwrap();
        let schema = s.resolve("entries", Path::new("books"));

        let mut extra = BTreeMap::new();
        extra.insert(
            "keywords".to_string(),
            serde_yaml_ng::Value::String("rust c  meta".into()),
        );
        let fields = validate(&schema, &extra, Path::new("books/j.md")).unwrap();
        assert_eq!(
            fields.values["keywords"],
            Value::str_list(["rust".into(), "c".into(), "meta".into()])
        );

        let mut extra = BTreeMap::new();
        extra.insert(
            "keywords".to_string(),
            serde_yaml_ng::Value::Sequence(vec![
                serde_yaml_ng::Value::String("rust".into()),
                serde_yaml_ng::Value::String("c".into()),
            ]),
        );
        let fields = validate(&schema, &extra, Path::new("books/j.md")).unwrap();
        assert_eq!(
            fields.values["keywords"],
            Value::str_list(["rust".into(), "c".into()])
        );

        let words = toml::Value::String("alpha beta".into());
        assert_eq!(
            typed(&FieldType::List, "keywords", &words, "the profile").unwrap(),
            Value::str_list(["alpha".into(), "beta".into()])
        );
        let empty = toml::Value::String("".into());
        assert_eq!(
            typed(&FieldType::List, "keywords", &empty, "the profile").unwrap(),
            Value::str_list(Vec::<String>::new())
        );
    }

    #[test]
    fn a_records_field_parses_item_maps() {
        let mut s = Schemas::new(grackle_model::row_schema());
        s.add(
            Path::new("recipes"),
            "ingredients = { type = \"records\", fields = { amount = \"string\", name = \"string\" } }\n",
            Path::new("recipes/.schema.toml"),
        )
        .unwrap();
        let schema = s.resolve("pages", Path::new("recipes"));
        let FieldType::Records { fields } = &schema["ingredients"] else {
            panic!("expected records");
        };
        assert_eq!(fields["amount"], FieldType::Str);
        assert_eq!(fields["name"], FieldType::Str);

        let yaml: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("- amount: 200 g\n  name: spaghetti\n- name: pepper\n")
                .unwrap();
        let mut extra = BTreeMap::new();
        extra.insert("ingredients".into(), yaml);
        let fields = validate(&schema, &extra, Path::new("recipes/x.md")).unwrap();
        let Value::List(items) = &fields.values["ingredients"] else {
            panic!("expected list");
        };
        assert_eq!(items.len(), 2);
        let Value::Map(first) = &items[0] else {
            panic!("expected map");
        };
        assert!(first.iter().any(|(k, v)| {
            matches!((k, v), (Value::Str(a), Value::Str(b)) if a == "amount" && b == "200 g")
        }));
    }

    #[test]
    fn records_reject_unknown_item_keys_and_nested_lists() {
        let e = Schemas::new(grackle_model::row_schema())
            .add(
                Path::new("r"),
                "ingredients = { type = \"records\", fields = { name = \"list\" } }\n",
                Path::new("r/.schema.toml"),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("must be \"string\""), "{e}");

        let mut s = Schemas::new(grackle_model::row_schema());
        s.add(
            Path::new("r"),
            "ingredients = { type = \"records\", fields = { name = \"string\" } }\n",
            Path::new("r/.schema.toml"),
        )
        .unwrap();
        let schema = s.resolve("pages", Path::new("r"));
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str("- name: x\n  qty: 1\n").unwrap();
        let mut extra = BTreeMap::new();
        extra.insert("ingredients".into(), yaml);
        let e = validate(&schema, &extra, Path::new("r/x.md"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("unknown key \"qty\""), "{e}");
    }

    /// Built-in names are closed: `Row::field` would answer first.
    #[test]
    fn declaring_a_built_in_field_is_a_load_error() {
        let mut s = Schemas::new(grackle_model::row_schema());
        let e = s
            .add(
                Path::new("books"),
                "slug = { type = \"string\" }\n",
                Path::new("books/.schema.toml"),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("built-in row field"), "{e}");
        assert!(e.contains("books/.schema.toml"), "{e}");

        // Reserved across both tables (one file can govern posts and pages).
        assert!(s
            .add(
                Path::new("books"),
                "rendered = { type = \"bool\" }\n",
                Path::new("books/.schema.toml"),
            )
            .is_err());

        s.set_site("tags = { type = \"list\" }\n".parse().unwrap(), "[schema]")
            .unwrap();
    }

    /// Declaration table is closed (unknown keys are a load error).
    #[test]
    fn an_unknown_key_in_a_declaration_is_a_load_error() {
        let mut s = Schemas::new(grackle_model::row_schema());
        let e = s
            .add(
                Path::new("books"),
                "blurb = { type = \"string\", optional = true, required = true }\n",
                Path::new("books/.schema.toml"),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("books/.schema.toml"), "{e}");
        assert!(e.contains("unknown key(s) optional, required"), "{e}");
        assert!(e.contains("takes: type, default"), "{e}");
    }

    /// Unrelated same-rung type disagreement is an error.
    #[test]
    fn a_same_rung_type_disagreement_is_a_load_error() {
        let mut s = Schemas::new(grackle_model::row_schema());
        s.add(
            Path::new("recipes"),
            "series = { type = \"string\" }\n",
            Path::new("recipes/.schema.toml"),
        )
        .unwrap();
        let e = s
            .add(
                Path::new("books"),
                "series = { type = \"int\" }\n",
                Path::new("books/.schema.toml"),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("books/.schema.toml says int"), "{e}");
        assert!(e.contains("recipes/.schema.toml says string"), "{e}");
        assert!(e.contains("\"series\""), "it names the field: {e}");

        // Message order is stable regardless of walk order.
        let mut s = Schemas::new(grackle_model::row_schema());
        s.add(
            Path::new("books"),
            "series = { type = \"int\" }\n",
            Path::new("books/.schema.toml"),
        )
        .unwrap();
        let flipped = s
            .add(
                Path::new("recipes"),
                "series = { type = \"string\" }\n",
                Path::new("recipes/.schema.toml"),
            )
            .unwrap_err()
            .to_string();
        assert_eq!(e, flipped);
    }

    /// Agreement and nested nearest-wins redeclaration stay legal (§5b).
    #[test]
    fn agreeing_and_nested_redeclarations_stay_legal() {
        let mut s = Schemas::new(grackle_model::row_schema());
        for dir in ["books", "recipes"] {
            s.add(
                Path::new(dir),
                "series = { type = \"string\" }\n",
                &Path::new(dir).join(".schema.toml"),
            )
            .unwrap();
        }
        assert_eq!(s.declared()["series"], FieldType::Str);

        let nested = schemas();
        assert_eq!(
            nested.resolve("entries", Path::new("books/special"))["author"],
            FieldType::Int
        );
        assert_eq!(
            nested.declared()["author"],
            FieldType::Str,
            "the broader claim takes the global name"
        );
    }

    /// Sibling collections: same-rung disagreement is an error.
    #[test]
    fn two_collections_may_not_disagree_about_a_field() {
        let mut s = Schemas::new(grackle_model::row_schema());
        s.add_collection(
            "notes",
            "series = { type = \"string\" }\n".parse().unwrap(),
            "grackle.toml [collections.notes.schema]",
        )
        .unwrap();
        s.add_collection(
            "essays",
            "series = { type = \"string\" }\n".parse().unwrap(),
            "grackle.toml [collections.essays.schema]",
        )
        .unwrap();
        let e = s
            .add_collection(
                "drafts",
                "series = { type = \"int\" }\n".parse().unwrap(),
                "grackle.toml [collections.drafts.schema]",
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("[collections.drafts.schema] says int"), "{e}");
        assert!(e.contains("says string"), "{e}");
        assert!(e.contains("two collections declare"), "{e}");
    }

    /// Cross-rung redeclaration is nearest-wins, not a collision.
    #[test]
    fn cross_rung_redeclaration_is_nearest_wins_not_a_collision() {
        let mut s = Schemas::new(grackle_model::row_schema());
        s.set_site("series = { type = \"int\" }\n".parse().unwrap(), "[schema]")
            .unwrap();
        s.add_collection(
            "notes",
            "series = { type = \"bool\" }\n".parse().unwrap(),
            "grackle.toml [collections.notes.schema]",
        )
        .unwrap();
        s.add(
            Path::new("books"),
            "series = { type = \"string\" }\n",
            Path::new("books/.schema.toml"),
        )
        .unwrap();
        assert_eq!(
            s.resolve("notes", Path::new("books"))["series"],
            FieldType::Str,
            "positional beats collection beats site"
        );
        assert_eq!(
            s.resolve("notes", Path::new("x"))["series"],
            FieldType::Bool
        );
        assert_eq!(s.resolve("other", Path::new("x"))["series"], FieldType::Int);
    }

    #[test]
    fn image_fields_split_out_for_the_thumb_pass() {
        let s = schemas();
        let schema = s.resolve("entries", Path::new("books"));
        let mut extra = BTreeMap::new();
        extra.insert(
            "cover".to_string(),
            serde_yaml_ng::Value::String("books/covers/j.png".into()),
        );
        let f = validate(&schema, &extra, Path::new("books/j.md")).unwrap();
        assert_eq!(f.images["cover"], "books/covers/j.png");
        assert_eq!(f.values["cover"], Value::Str("books/covers/j.png".into()));
    }
}
