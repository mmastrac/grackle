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
    fn parse(s: &str) -> Option<FieldType> {
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
            reserved,
        }
    }

    /// Parse one `.schema.toml` found at `dir` (root-relative).
    pub fn add(&mut self, dir: &Path, text: &str, file: &Path) -> Result<()> {
        let table: toml::Table = text
            .parse()
            .map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
        let mut fields = BTreeMap::new();
        for (name, v) in table {
            let ty = v
                .as_table()
                .and_then(|t| t.get("type"))
                .and_then(|t| t.as_str())
                .and_then(FieldType::parse);
            let Some(ty) = ty else {
                bail!(
                    "{}: field {name:?} needs type = \"string\" | \"int\" | \
                     \"bool\" | \"list\" | \"image\"",
                    file.display()
                );
            };
            if self.reserved.contains_key(name.as_str()) {
                bail!(
                    "{}: field {name:?} is a built-in row field, so declaring \
                     it would be silently overruled — every row already has \
                     one. Rename the declaration.",
                    file.display()
                );
            }
            fields.insert(name, ty);
        }
        self.by_dir.insert(dir.to_path_buf(), fields);
        Ok(())
    }

    /// The schema governing a row in `dir`: ancestors accumulated,
    /// nearest declaration winning per field name. None when no schema
    /// governs the path at all — undeclared front matter stays tolerated
    /// there, exactly as before schemas existed.
    pub fn resolve(&self, dir: &Path) -> Option<BTreeMap<&str, FieldType>> {
        let mut out: BTreeMap<&str, FieldType> = BTreeMap::new();
        let mut found = false;
        let mut cur = Some(dir);
        while let Some(d) = cur {
            if let Some(fields) = self.by_dir.get(d) {
                found = true;
                for (k, t) in fields {
                    out.entry(k.as_str()).or_insert(*t);
                }
            }
            if d.as_os_str().is_empty() {
                break;
            }
            cur = d.parent();
        }
        found.then_some(out)
    }

    /// Every declared field name and type, across all schemas — what view
    /// `order_by` validates against when the target set spans the tree.
    pub fn declared(&self) -> BTreeMap<&str, FieldType> {
        let mut out = BTreeMap::new();
        for fields in self.by_dir.values() {
            for (k, t) in fields {
                out.entry(k.as_str()).or_insert(*t);
            }
        }
        out
    }
}

/// Validate a row's extra front matter against its governing schema:
/// unknown keys and type mismatches are load errors naming the file.
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
        let base = s.resolve(Path::new("books")).unwrap();
        assert_eq!(base["author"], FieldType::Str);
        let deep = s.resolve(Path::new("books/special")).unwrap();
        assert_eq!(deep["author"], FieldType::Int, "nearest wins");
        assert_eq!(deep["shelf"], FieldType::Str, "ancestors accumulate");
        assert!(
            s.resolve(Path::new("recipes")).is_none(),
            "ungoverned dirs stay free"
        );
    }

    #[test]
    fn undeclared_and_mistyped_fields_are_load_errors() {
        let s = schemas();
        let schema = s.resolve(Path::new("books")).unwrap();
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

    #[test]
    fn image_fields_split_out_for_the_thumb_pass() {
        let s = schemas();
        let schema = s.resolve(Path::new("books")).unwrap();
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
