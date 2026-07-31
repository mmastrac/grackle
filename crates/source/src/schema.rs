//! Per-subtree schema (§5b): a `.schema.toml` declares typed fields for
//! every row beneath its directory — the tree says where, the file says
//! what. Resolution accumulates down the tree, nearest wins per key, the
//! same law as markers and slot fills.
//!
//! The payoff is the §5b one: a declared field is *checked*. Front matter
//! carrying an undeclared key under a schema is a load error naming the
//! file and the knowns; a declared key with the wrong type likewise. The
//! same declarations feed view `order_by` validation, and — later — the
//! §5f expression environment.

use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use grackle_db::filter::{self, Schema, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Str,
    Int,
    Bool,
    List,
    /// A root-relative image path: thumbnailed via §6b, dimension facts
    /// attached, eligible as a `hero` source (q23).
    Image,
    /// Calendar day as `YYYY-MM-DD` (bare `YYYY-MM` means the first of that
    /// month). Stored as an ISO string so filters order it correctly.
    Date,
}

impl FieldType {
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

    pub(crate) fn name(self) -> &'static str {
        match self {
            FieldType::Str => "string",
            FieldType::Int => "int",
            FieldType::Bool => "bool",
            FieldType::List => "list",
            FieldType::Image => "image",
            FieldType::Date => "date",
        }
    }

    /// How the filter language sees this field. An image is a path, so it
    /// reads as a string — a `where` could compare or `glob()` on one, though
    /// nothing does yet. A date is ISO-8601 text for the same reason.
    pub fn filter_type(self) -> filter::Type {
        match self {
            FieldType::Str | FieldType::Image | FieldType::Date => filter::Type::Str,
            FieldType::Int => filter::Type::Int,
            FieldType::Bool => filter::Type::Bool,
            FieldType::List => filter::Type::List,
        }
    }
}

/// The three fields the engine reads off a row BY NAME: which theme renders
/// it (§5a), which shell wraps it (§5g), which slot cuts the render chain.
/// `base.toml` declares them in `[schema]`, which is what routes a marker's
/// or a rule's value for one of them through `apply_defaults` — the same
/// typed path every other key takes (MERGE.md C1).
///
/// They were `load.rs`'s `CASCADE_KEYS` until then: names `apply_defaults`
/// SKIPPED, read back out of raw TOML with `as_str()`/`as_bool()`, so
/// `defaults = { theme = 1 }` silently vanished. §4e made the flag family
/// declared fields for exactly this reason and left these behind as
/// "genuinely engine vocabulary"; being engine vocabulary is a statement
/// about who READS a field, not about who types it. `toc` joined the flags.
///
/// The types are the engine's, not a site's: a declaration may restate a pair
/// below, and declaring one of these names at another type is a load error
/// (`parse_fields`) — the value would be typed one way and read the other,
/// which is the silence this item closed.
///
/// Public because a surface that prints a row's named fields AND its declared
/// columns has to know which names are both, or it prints them twice — which
/// `grackle explain` did, for `layout`, until IO.md IR3. `debug::row_fields`
/// reads this list rather than restating it, so a fourth cascade key lands in
/// one place.
pub const CASCADE: &[(&str, FieldType)] = &[
    ("theme", FieldType::Str),
    ("shell", FieldType::Str),
    ("slot", FieldType::Str),
];

/// The type the engine reads this cascade key at, if it is one of its own.
pub(crate) fn cascade_type(name: &str) -> Option<FieldType> {
    CASCADE.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

/// One rung's worth of declarations: the fields, and who wrote them — the
/// file for a `.schema.toml`, the table name for a `[collections.*.schema]`.
/// The writer is kept so a collision can name both sides.
type Declared = (String, BTreeMap<String, FieldType>);

/// Every `.schema.toml` in the tree, keyed by its directory.
#[derive(Debug)]
pub struct Schemas {
    by_dir: BTreeMap<PathBuf, Declared>,
    /// `[collections.<name>.schema]` — the axis a positional file cannot
    /// express, because a collection may have several sources.
    by_collection: BTreeMap<String, Declared>,
    /// `[schema]` — fields every row of the site has.
    site: BTreeMap<String, FieldType>,
    /// Names the row type already owns. `Row::field` matches these FIRST and
    /// falls through to declared fields, so a schema declaring one of them
    /// parses, validates, and is then never read — the value goes in and no
    /// query can reach it.
    ///
    /// The check was latent until q51's merge made it live: the page schema
    /// growing `date`/`year`/`month`/`day` for parity turned `month = { type
    /// = "string" }` (field-notes' stand-in for the date a page could not
    /// have) from a working field into a shadowed one, and only a diff of the
    /// built site would have said so.
    ///
    /// Held rather than looked up: the database's row schema is the
    /// authority, and this is the layer that reads it. There is deliberately
    /// no `Default` — a `Schemas` with no reserved names would accept every
    /// shadowing declaration in silence, which is the bug this rejects.
    reserved: Schema,
}

/// "a/.schema.toml says string, b/.schema.toml says int" — the two writers of
/// a colliding name, ordered by writer so the message reads the same however
/// the walk happened to find the files (the declaration walk is unsorted).
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

/// A page's validated extra fields: typed values plus the image-typed
/// subset (root-relative paths the thumb pass picks up).
#[derive(Debug, Default)]
pub struct Fields {
    pub values: BTreeMap<String, Value>,
    pub images: BTreeMap<String, String>,
}

impl Schemas {
    /// `reserved` is the row schema a declaration may not shadow — pass
    /// `grackle_model::row_schema()`.
    pub fn new(reserved: Schema) -> Schemas {
        Schemas {
            by_dir: BTreeMap::new(),
            by_collection: BTreeMap::new(),
            site: BTreeMap::new(),
            reserved,
        }
    }

    /// Parse one `.schema.toml` found at `dir` (root-relative).
    pub fn add(&mut self, dir: &Path, text: &str, file: &Path) -> Result<()> {
        let table: toml::Table = text
            .parse()
            .map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
        let whose = file.display().to_string();
        let fields = parse_fields(table, &whose, &self.reserved)?;
        self.check_positional_collision(dir, &whose, &fields)?;
        self.by_dir.insert(dir.to_path_buf(), (whose, fields));
        Ok(())
    }

    /// Two `.schema.toml` files may declare one name — `series = { type =
    /// "string" }` under two subtrees is ordinary — but only if they AGREE.
    ///
    /// The tree axis orders declarations by nearness, so an ancestor and a
    /// descendant disagreeing is the §5b law working (`books/.schema.toml`
    /// says `author` is a string, `books/special/.schema.toml` says int, and
    /// a row picks the nearer). Two directories with **neither inside the
    /// other** have no such order. Nothing ranks them, yet `declared()` must
    /// flatten them into ONE site-wide filter vocabulary — the environment
    /// `where`, `order_by`, `group_by` and a route's schema parse against —
    /// and it did so with `or_insert` over a `BTreeMap<PathBuf>`, i.e. by
    /// **alphabetical directory order**. The winner was silent, arbitrary,
    /// and free to disagree with what `resolve()` hands the rows themselves.
    ///
    /// So the disagreement is the error, at the point of declaration, naming
    /// both files (MERGE.md A4).
    fn check_positional_collision(
        &self,
        dir: &Path,
        whose: &str,
        fields: &BTreeMap<String, FieldType>,
    ) -> Result<()> {
        for (name, ty) in fields {
            for (other_dir, (other_whose, other_fields)) in &self.by_dir {
                let Some(other) = other_fields.get(name) else {
                    continue;
                };
                // Agreement is legal; nearness is an order, and an order is
                // an answer.
                if other == ty || dir.starts_with(other_dir) || other_dir.starts_with(dir) {
                    continue;
                }
                bail!(
                    "{} — two .schema.toml files declare field {name:?} with \
                     different types and neither directory contains the other, \
                     so nothing orders them. The site's filter vocabulary \
                     (`where`, `order_by`, `group_by`) has one entry per name: \
                     give them the same type, or rename one.",
                    conflict(whose, *ty, other_whose, *other)
                );
            }
        }
        Ok(())
    }

    /// `[schema]` in `grackle.toml`: fields every row of the site has.
    ///
    /// The tree axis says *where* a field applies; this says *always*. It is
    /// how the base config declares the flag family (§4d) — those are
    /// properties of a row, not of a directory, and no positional file could
    /// state that without sitting at the root of every site.
    pub fn set_site(&mut self, table: toml::Table, whose: &str) -> Result<()> {
        self.site = parse_fields(table, whose, &self.reserved)?;
        Ok(())
    }

    /// `[collections.<name>.schema]`: fields every row of one collection has.
    ///
    /// The axis `.schema.toml` could not express. A collection may have
    /// SEVERAL sources (`_posts` and `_drafts` are two sources of one corpus,
    /// §4), so a positional declaration would have to be copied once per
    /// source and could then drift — the disease `[sets.published]` exists to
    /// cure, one layer down.
    pub fn add_collection(&mut self, name: &str, table: toml::Table, whose: &str) -> Result<()> {
        let fields = parse_fields(table, whose, &self.reserved)?;
        // The same law one rung down, and here there is no nearness at all to
        // fall back on: collections are siblings by construction. Two of them
        // declaring one name differently is `declared()` picking by
        // alphabetical COLLECTION order — and when they share a kind they
        // even feed one table, so the rows disagree too.
        for (other_name, (other_whose, other_fields)) in &self.by_collection {
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
                    conflict(whose, *ty, other_whose, *other)
                );
            }
        }
        self.by_collection
            .insert(name.to_string(), (whose.to_string(), fields));
        Ok(())
    }

    /// The schema governing a row: its collection's declarations and the
    /// site-wide ones, plus every `.schema.toml` up its directory chain.
    ///
    /// Three axes, one law — **nearest wins**. Positional beats collection
    /// beats site-wide, because a `.schema.toml` sitting beside the rows is
    /// the most specific statement anyone made about them.
    ///
    /// **Every row is governed** (Matt, 2026-07-25): declare a field before
    /// you use it. The map may be empty — that is a site that declared
    /// nothing, and undeclared front matter on it is a load error naming
    /// zero knowns, which is the correct and legible thing to say.
    pub fn resolve(&self, collection: &str, dir: &Path) -> BTreeMap<&str, FieldType> {
        let mut out: BTreeMap<&str, FieldType> = BTreeMap::new();
        let mut cur = Some(dir);
        while let Some(d) = cur {
            if let Some((_, fields)) = self.by_dir.get(d) {
                for (k, t) in fields {
                    out.entry(k.as_str()).or_insert(*t);
                }
            }
            if d.as_os_str().is_empty() {
                break;
            }
            cur = d.parent();
        }
        for src in [
            self.by_collection.get(collection).map(|(_, f)| f),
            Some(&self.site),
        ] {
            let Some(fields) = src else { continue };
            for (k, t) in fields {
                out.entry(k.as_str()).or_insert(*t);
            }
        }
        out
    }

    /// Every declared field name and type, across all schemas — what a view's
    /// `where` and `order_by` validate against when the set spans the tree.
    ///
    /// Flattening rungs is what makes this a *site* vocabulary, and the rungs
    /// are ordered (positional, then collection, then site — nearest first,
    /// §5b), so a cross-rung disagreement resolves by law. Within a rung the
    /// only disagreements that survive to here are between an ancestor
    /// directory and its descendant, where the ancestor — the broader claim —
    /// takes the global name; every other same-rung disagreement was refused
    /// at declaration time.
    pub fn declared(&self) -> BTreeMap<&str, FieldType> {
        let mut out = BTreeMap::new();
        for fields in self
            .by_dir
            .values()
            .map(|(_, f)| f)
            .chain(self.by_collection.values().map(|(_, f)| f))
            .chain(std::iter::once(&self.site))
        {
            for (k, t) in fields {
                out.entry(k.as_str()).or_insert(*t);
            }
        }
        out
    }

    /// Just the declared fields, as a filter schema — what a ROUTE gains
    /// beyond its own vocabulary (§4e). A route has no title or body, so it
    /// takes the site's declarations and none of the row built-ins.
    pub fn declared_schema(&self) -> Schema {
        let mut s = Schema::new();
        for (name, ty) in self.declared() {
            insert_declared(&mut s, name, ty);
        }
        s
    }

    /// The filter environment for content rows: the built-in row schema plus
    /// every declared field.
    ///
    /// One definition, because there is no defensible world in which a field
    /// can be sorted by (`order_by`), grouped by (`group_by`) and ranked by (a
    /// relation's `rank`) but not *filtered* on. `where` was the one consumer
    /// parsing against the bare row schema, so a site could declare a `bool`,
    /// set it from a marker, group by it — and then get `unknown field` from
    /// its own `where`.
    ///
    /// Declarations are positional but the environment is global: a name
    /// declared anywhere is nameable everywhere, and a row that never declared
    /// it simply has no value. That is already how `order_by` and relations
    /// read it; this makes `where` the third consumer of one rule rather than
    /// the one exception.
    pub fn row_filter_schema(&self) -> Schema {
        let mut s = grackle_model::row_schema();
        for (name, ty) in self.declared() {
            // A declaration never shadows a built-in (enforced in `add`), so a
            // plain insert is right. Interned rather than leaked, so `serve`
            // reloads do not accumulate keys.
            insert_declared(&mut s, name, ty);
        }
        s
    }
}

fn insert_declared(s: &mut Schema, name: &str, ty: FieldType) {
    let key = grackle_model::intern(name.to_string());
    s.insert(key, ty.filter_type());
    if ty == FieldType::Date {
        for part in ["year", "month", "day"] {
            s.insert(
                grackle_model::intern(format!("{name}.{part}")),
                filter::Type::Int,
            );
        }
    }
}

/// Parse one declaration table — `[schema]`, `[collections.*.schema]`, or a
/// `.schema.toml` — into typed names.
///
/// A free function rather than a method because a `Config` has to ask the same
/// question before there is a [`Schemas`] to ask it of: `check_profiles`
/// type-checks a profile's `force` block against the site's own `[schema]` at
/// `validate` time, one tree walk before the positional files exist
/// (MERGE.md E1). One parser, so the two cannot disagree about what a
/// declaration says.
fn parse_fields(
    table: toml::Table,
    whose: &str,
    reserved: &Schema,
) -> Result<BTreeMap<String, FieldType>> {
    let mut fields = BTreeMap::new();
    for (name, v) in table {
        let ty = v
            .as_table()
            .and_then(|t| t.get("type"))
            .and_then(|t| t.as_str())
            .and_then(FieldType::parse);
        let Some(ty) = ty else {
            bail!(
                "{whose}: field {name:?} needs type = \"string\" | \"int\" | \
                 \"bool\" | \"list\" | \"image\" | \"date\""
            );
        };
        // Cascade keys are declarable at the type the engine reads them at:
        // restating is fine; a different type would be typed one way and read
        // another.
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
        // A declaration table has exactly one key. Anything else was read
        // as a property of the field and dropped, which is how `default =`
        // and `required =` get written by someone reasonably expecting
        // them to work.
        let extra: Vec<&str> = v
            .as_table()
            .map(|t| {
                t.keys()
                    .map(String::as_str)
                    .filter(|k| *k != "type")
                    .collect()
            })
            .unwrap_or_default();
        if !extra.is_empty() {
            bail!(
                "{whose}: field {name:?} has unknown key(s) {} — a field \
                 declaration takes: type",
                extra.join(", ")
            );
        }
        fields.insert(name, ty);
    }
    Ok(fields)
}

/// The site-wide declared vocabulary, read from a `[schema]` table alone —
/// what a [`crate::config::Config`] knows before the tree walk (MERGE.md E1).
///
/// This is exactly the rung a profile's `force` block may name. A positional
/// `.schema.toml` governs a subtree, and a `[collections.*.schema]` governs one
/// collection; a forced field is written onto EVERY row, so a name from either
/// of those would be undeclared for the rows outside it — which is
/// `apply_defaults`' "no schema declares it" error, arriving per row instead of
/// once at load.
pub(crate) fn site_fields(table: &toml::Table, whose: &str) -> Result<BTreeMap<String, FieldType>> {
    parse_fields(table.clone(), whose, &grackle_model::row_schema())
}

/// Validate a row's extra front matter against its governing schema:
/// unknown keys and type mismatches are load errors naming the file.
/// Fold marker and rule defaults (§4b, §4) into a row's declared fields.
///
/// Front matter has already been validated into `fields`; these are the
/// farther half of the same cascade, so a key front matter set is left alone —
/// **nearest wins, first writer per key**, the law from §4.
///
/// Before this, `cascade()`'s seven hardcoded names were the *only* keys a
/// marker or rule could set. `[markers] ".archived" = { archived = true }`
/// parsed, matched, and then did nothing at all — no error, the key simply
/// dropped on the floor. So a default naming something no schema declares is
/// now a load error: a marker whose key nothing reads is a typo, and a typo
/// that does nothing silently is the failure mode this codebase keeps finding.
///
/// There is no exemption list any more. The engine's own cascade keys
/// (`CASCADE`) are declared in `base.toml` like anything else, so they arrive
/// here typed — MERGE.md C1, and the last of §4e's "the flag family is not
/// engine vocabulary".
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
        write_typed(*ty, name, v, fields, &whose)?;
    }
    Ok(())
}

/// Rung 0 (§2): the selected profile's `[profiles.NAME.force]` fields, written
/// over whatever the row's own ladder resolved to (MERGE.md E1).
///
/// **This runs LAST because it is the TOP rung.** Every rung below it — front
/// matter, then the nearest marker, then the rules — is "first writer wins", so
/// the only way to sit above all three without disturbing their order among
/// themselves is to write after them. Front matter is what the seam is
/// measured against: a row declaring `noindex: false` under a forcing profile
/// still comes out forced, which is the whole of what rung 0 means.
///
/// Force decides the VALUE, not whether the row is well formed: a row whose
/// front matter mistypes a forced field, or a marker that does, is the same
/// load error it was before, because those rungs still run and still speak.
///
/// The lookup cannot fail — `Config::check_profiles` accepts only site
/// `[schema]` names and `Schemas::resolve` chains that table into every row's
/// schema — but it is a real lookup rather than an `unwrap`, because a nearer
/// `.schema.toml` may legally RETYPE a site-wide name for its own subtree
/// (§5b), and a forced value that does not fit where it lands should say so
/// naming the row.
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
        write_typed(*ty, name, v, fields, &whose)?;
    }
    Ok(())
}

/// The declared names, sorted — what an error about an undeclared one lists.
pub(crate) fn knowns(schema: &BTreeMap<&str, FieldType>) -> String {
    let mut known: Vec<&str> = schema.keys().copied().collect();
    known.sort_unstable();
    known.join(", ")
}

/// Convert one TOML value at its declared type and write it into `fields`.
///
/// The three writers of a typed field value — a marker, a rule, and a profile's
/// `force` block — share this, so "declared bool, given a string" is one
/// sentence with one author and the image side channel is fed from one place.
/// `whose` is the prefix each caller owns ("x.md: a marker or rule").
pub(crate) fn write_typed(
    ty: FieldType,
    name: &str,
    v: &toml::Value,
    fields: &mut Fields,
    whose: &str,
) -> Result<()> {
    let value = typed(ty, name, v, whose)?;
    if ty == FieldType::Image {
        if let Value::Str(s) = &value {
            fields.images.insert(name.to_string(), s.clone());
        }
    }
    fields.values.insert(name.to_string(), value);
    Ok(())
}

/// Coerce a whitespace-separated string into a list of items. Shared by the
/// YAML front-matter path and the TOML defaults path so a `type = "list"`
/// field accepts `a b c` or a YAML sequence.
pub(crate) fn list_from_words(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

/// One TOML value read at its declared type — see [`write_typed`], and
/// [`crate::config::Config::check_profiles`], which type-checks a `force`
/// block through this without a row to write into.
pub(crate) fn typed(ty: FieldType, name: &str, v: &toml::Value, whose: &str) -> Result<Value> {
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
        (ty, other) => bail!(
            "{whose} sets {name:?} to {other}, but it is declared {}",
            ty.name()
        ),
    })
}

/// Canonical `YYYY-MM-DD` from a declared date value.
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
            // `tags:` with no value: a form the corpus still carries.
            (FieldType::List, Y::Null) => Value::str_list(Vec::<String>::new()),
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

/// Front matter's half of the engine's cascade keys (`CASCADE`).
///
/// They arrive on named `FrontMatter` fields rather than in `extra`, so
/// `validate` never sees them — serde has already typed them, which is why
/// front matter never had this item's disease. Seeding them into the row's
/// fields BEFORE the defaults is what keeps front matter the nearest writer
/// for these exactly as it is for every other declared key:
/// `apply_defaults` leaves a key the row already carries alone.
///
/// Governance is the same sentence `validate` says (§4e, "every row is
/// governed"): a row wearing a name no schema declares is a load error naming
/// the file and the knowns. On a base-inheriting site all three are declared;
/// a site that declined the base declares the ones its rows use.
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

    /// Three axes, one law. A field declared for the collection or the site
    /// governs a row no `.schema.toml` mentions; a positional declaration is
    /// nearer and wins the name.
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

        // A directory nothing positional governs is governed anyway now.
        let free = s.resolve("notes", Path::new("recipes"));
        assert_eq!(free["series"], FieldType::Str);
        assert_eq!(free["archived"], FieldType::Bool);

        // …and the positional declaration still wins its own name.
        let books = s.resolve("notes", Path::new("books"));
        assert_eq!(
            books["author"],
            FieldType::Str,
            "positional beats collection"
        );

        // A collection's fields are its own.
        let other = s.resolve("pages", Path::new("recipes"));
        assert!(!other.contains_key("series"), "{other:?}");
        assert_eq!(other["archived"], FieldType::Bool, "site-wide reaches all");
    }

    /// §4e: a marker or rule may set any declared field. Before this it could
    /// set exactly seven hardcoded names and dropped everything else silently.
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

        // Front matter is nearer, so it keeps its value; the rest fills in.
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

    /// A marker whose key nothing declares would do nothing at all. That is
    /// the silent failure §4e found, so it is a load error naming the knowns.
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

    /// The engine's own four are declared fields now (MERGE.md C1), so a
    /// default for one lands in `fields` TYPED — there is no exemption list
    /// left for `apply_defaults` to skip them through, and a wrong type is
    /// the same error any other field gets.
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
        // An ordinary declared bool — same typed path the cascade keys take,
        // and what `toc` is now that it left CASCADE for the flag family.
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

    /// A `type = "list"` field accepts a whitespace-separated string as well
    /// as a sequence.
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
            typed(FieldType::List, "keywords", &words, "the profile").unwrap(),
            Value::str_list(["alpha".into(), "beta".into()])
        );
        let empty = toml::Value::String("".into());
        assert_eq!(
            typed(FieldType::List, "keywords", &empty, "the profile").unwrap(),
            Value::str_list(Vec::<String>::new())
        );
    }

    /// A declaration that collides with a base row field parsed, validated
    /// and was then unreachable — `Row::field` answers the base name first.
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

        // Reserved across BOTH tables: one `.schema.toml` can govern posts
        // and pages, so a page-only name is not free on the post side.
        assert!(s
            .add(
                Path::new("books"),
                "rendered = { type = \"bool\" }\n",
                Path::new("books/.schema.toml"),
            )
            .is_err());

        // Tags is an ordinary list declaration (base ships it), not reserved.
        s.set_site("tags = { type = \"list\" }\n".parse().unwrap(), "[schema]")
            .unwrap();
    }

    /// The declaration table is closed, like every other config table. A
    /// `default =` beside the type parsed, was dropped, and left a field the
    /// author believed had a default and the engine had never heard of.
    #[test]
    fn an_unknown_key_in_a_declaration_is_a_load_error() {
        let mut s = Schemas::new(grackle_model::row_schema());
        let e = s
            .add(
                Path::new("books"),
                "blurb = { type = \"string\", default = \"\", required = true }\n",
                Path::new("books/.schema.toml"),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("books/.schema.toml"), "{e}");
        assert!(e.contains("unknown key(s) default, required"), "{e}");
        assert!(e.contains("takes: type"), "{e}");
    }

    /// MERGE.md A4. Two `.schema.toml` files in unrelated subtrees are the
    /// same rung with no nearness between them, so a type disagreement had no
    /// answer — `declared()` picked one by alphabetical directory order and
    /// said nothing.
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

        // The pair reads in path order whichever way the walk found them —
        // the declaration walk is not sorted.
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

    /// The control, and the common case: agreement is not a collision. Two
    /// subtrees both declaring `series = { type = "string" }` is ordinary —
    /// and a descendant retyping its ancestor's field stays legal, because
    /// the tree ORDERS those two and §5b's nearest-wins is the answer.
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

        // Ancestor and descendant: `schemas()` above already disagrees about
        // `author`, on purpose, and resolves nearest-wins.
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

    /// The same rung one level down. Collections are siblings by
    /// construction — there is no nearness to appeal to at all — and when
    /// they share a kind they feed one table.
    #[test]
    fn two_collections_may_not_disagree_about_a_field() {
        let mut s = Schemas::new(grackle_model::row_schema());
        s.add_collection(
            "notes",
            "series = { type = \"string\" }\n".parse().unwrap(),
            "grackle.toml [collections.notes.schema]",
        )
        .unwrap();
        // Agreement is fine.
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

    /// Cross-rung is NOT this error: a positional file outranks the
    /// collection table outranks `[schema]`, and nearest-wins is the law
    /// (MERGE.md table B). Only the built-in row schema is closed to
    /// redeclaration, which is a different guard with a different reason —
    /// a built-in cannot be shadowed at ANY rung, because `Row::field`
    /// answers first and the declaration would never be read.
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
