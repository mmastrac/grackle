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
}

impl FieldType {
    pub(crate) fn parse(s: &str) -> Option<FieldType> {
        Some(match s {
            "string" => FieldType::Str,
            "int" => FieldType::Int,
            "bool" => FieldType::Bool,
            "list" => FieldType::List,
            "image" => FieldType::Image,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            FieldType::Str => "string",
            FieldType::Int => "int",
            FieldType::Bool => "bool",
            FieldType::List => "list",
            FieldType::Image => "image",
        }
    }

    /// How the filter language sees this field. An image is a path, so it
    /// reads as a string — a relation could `match` on one, though nothing
    /// does yet.
    pub fn filter_type(self) -> filter::Type {
        match self {
            FieldType::Str | FieldType::Image => filter::Type::Str,
            FieldType::Int => filter::Type::Int,
            FieldType::Bool => filter::Type::Bool,
            FieldType::List => filter::Type::List,
        }
    }
}

/// Every `.schema.toml` in the tree, keyed by its directory.
#[derive(Debug)]
pub struct Schemas {
    by_dir: BTreeMap<PathBuf, BTreeMap<String, FieldType>>,
    /// `[collections.<name>.schema]` — the axis a positional file cannot
    /// express, because a collection may have several sources.
    by_collection: BTreeMap<String, BTreeMap<String, FieldType>>,
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
        let fields = self.parse_fields(table, &file.display().to_string())?;
        self.by_dir.insert(dir.to_path_buf(), fields);
        Ok(())
    }

    /// `[schema]` in `grackle.toml`: fields every row of the site has.
    ///
    /// The tree axis says *where* a field applies; this says *always*. It is
    /// how the base config declares the flag family (§4d) — those are
    /// properties of a row, not of a directory, and no positional file could
    /// state that without sitting at the root of every site.
    pub fn set_site(&mut self, table: toml::Table, whose: &str) -> Result<()> {
        self.site = self.parse_fields(table, whose)?;
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
        let fields = self.parse_fields(table, whose)?;
        self.by_collection.insert(name.to_string(), fields);
        Ok(())
    }

    fn parse_fields(&self, table: toml::Table, whose: &str) -> Result<BTreeMap<String, FieldType>> {
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
                     \"bool\" | \"list\" | \"image\""
                );
            };
            if self.reserved.contains_key(name.as_str()) {
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
            if let Some(fields) = self.by_dir.get(d) {
                for (k, t) in fields {
                    out.entry(k.as_str()).or_insert(*t);
                }
            }
            if d.as_os_str().is_empty() {
                break;
            }
            cur = d.parent();
        }
        for src in [self.by_collection.get(collection), Some(&self.site)] {
            let Some(fields) = src else { continue };
            for (k, t) in fields {
                out.entry(k.as_str()).or_insert(*t);
            }
        }
        out
    }

    /// Every declared field name and type, across all schemas — what a view's
    /// `where` and `order_by` validate against when the set spans the tree.
    pub fn declared(&self) -> BTreeMap<&str, FieldType> {
        let mut out = BTreeMap::new();
        for fields in self
            .by_dir
            .values()
            .chain(self.by_collection.values())
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
            s.insert(grackle_model::intern(name.to_string()), ty.filter_type());
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
            s.insert(grackle_model::intern(name.to_string()), ty.filter_type());
        }
        s
    }
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
/// dropped on the floor. So a default naming something no schema declares, and
/// that is not one of the engine's own cascade keys, is now a load error: a
/// marker whose key nothing reads is a typo, and a typo that does nothing
/// silently is the failure mode this codebase keeps finding.
pub fn apply_defaults(
    schema: &BTreeMap<&str, FieldType>,
    defaults: &BTreeMap<&str, &toml::Value>,
    reserved: &[&str],
    fields: &mut Fields,
    path: &Path,
) -> Result<()> {
    for (name, v) in defaults {
        if reserved.contains(name) {
            continue; // the engine's own cascade reads these
        }
        let Some(ty) = schema.get(name) else {
            let mut known: Vec<&str> = schema.keys().copied().collect();
            known.extend_from_slice(reserved);
            known.sort_unstable();
            bail!(
                "{}: a marker or rule sets {name:?}, which no schema declares \
                 — nothing would read it\n  declared fields: {}",
                path.display(),
                known.join(", ")
            );
        };
        if fields.values.contains_key(*name) {
            continue; // front matter is nearer
        }
        let value = match (ty, v) {
            (FieldType::Str, toml::Value::String(s)) => Value::Str(s.clone()),
            (FieldType::Image, toml::Value::String(s)) => {
                fields.images.insert(name.to_string(), s.clone());
                Value::Str(s.clone())
            }
            (FieldType::Int, toml::Value::Integer(i)) => Value::Int(*i),
            (FieldType::Bool, toml::Value::Boolean(b)) => Value::Bool(*b),
            (FieldType::List, toml::Value::Array(a)) => Value::List(
                a.iter()
                    .map(|x| match x.as_str() {
                        Some(s) => Ok(s.to_string()),
                        None => bail!(
                            "{}: default {name:?}: list items must be strings, got {x}",
                            path.display()
                        ),
                    })
                    .collect::<Result<_>>()?,
            ),
            (ty, other) => bail!(
                "{}: a marker or rule sets {name:?} to {other}, but it is \
                 declared {}",
                path.display(),
                ty.name()
            ),
        };
        fields.values.insert(name.to_string(), value);
    }
    Ok(())
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
        let value = match (ty, v) {
            (FieldType::Str, Y::String(s)) => Value::Str(s.clone()),
            (FieldType::Image, Y::String(s)) => {
                out.images.insert(name.clone(), s.clone());
                Value::Str(s.clone())
            }
            (FieldType::Int, Y::Number(n)) if n.as_i64().is_some() => {
                Value::Int(n.as_i64().unwrap())
            }
            (FieldType::Bool, Y::Bool(b)) => Value::Bool(*b),
            (FieldType::List, Y::Sequence(seq)) => Value::List(
                seq.iter()
                    .map(|x| match x {
                        Y::String(s) => Ok(s.clone()),
                        other => bail!(
                            "{}: field {name:?}: list items must be strings, got {other:?}",
                            path.display()
                        ),
                    })
                    .collect::<Result<_>>()?,
            ),
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
        apply_defaults(
            &schema,
            &defaults,
            &["theme"],
            &mut fields,
            Path::new("x.md"),
        )
        .unwrap();
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
            &["draft"],
            &mut Fields::default(),
            Path::new("x.md"),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("no schema declares"), "{e}");
        assert!(e.contains("author"), "it names the knowns: {e}");
        assert!(e.contains("draft"), "including the engine's own: {e}");
    }

    /// A reserved cascade key is the engine's, not a declaration's.
    #[test]
    fn a_cascade_key_is_left_to_the_engine() {
        let s = schemas();
        let schema = s.resolve("entries", Path::new("books"));
        let yes = toml::Value::Boolean(true);
        let defaults = BTreeMap::from([("draft", &yes)]);
        let mut fields = Fields::default();
        apply_defaults(
            &schema,
            &defaults,
            &["draft"],
            &mut fields,
            Path::new("x.md"),
        )
        .unwrap();
        assert!(fields.values.is_empty(), "{fields:?}");
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

    /// A declaration that collides with a base row field parsed, validated
    /// and was then unreachable — `Post::field`/`Page::field` answer the
    /// base name first. field-notes had a live one (`month`, the stand-in
    /// for the date a page could not hold), and nothing said so.
    #[test]
    fn declaring_a_built_in_field_is_a_load_error() {
        let mut s = Schemas::new(grackle_model::row_schema());
        let e = s
            .add(
                Path::new("books"),
                "month = { type = \"string\" }\n",
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
