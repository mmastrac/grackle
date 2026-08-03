//! One content file in the site database.

use crate::{Key, RowAxis};
use chrono::NaiveDate;
use grackle_db::{filter, Keyed};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize)]
pub struct Row {
    /// Stable identity: the source path.
    pub key: Key,
    /// Collection whose source claimed this file.
    pub collection: String,
    /// `match` glob of the rule that claimed this file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Path relative to the site root.
    pub rel: PathBuf,
    #[serde(serialize_with = "hex")]
    pub version: u64,
    pub slug: String,
    /// Filename without extension.
    pub stem: String,
    /// Absent on a titleless page; posts always have one.
    pub title: Option<String>,
    /// Theme that renders this row.
    pub theme: Option<String>,
    /// Serialization shell (`html`, `raw`, …).
    pub shell: Option<String>,
    /// Declared schema fields (lists, flags, strings, …).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, filter::Value>,
    /// Provider overlay, keyed by qualified name (`git.commit`). Filled at load
    /// when `metadata` enables a provider; read by `field` under its namespace.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, filter::Value>,
    /// Image-typed fields: name -> resolved source (site row or absolute URL).
    #[serde(skip)]
    pub images: BTreeMap<String, String>,
    /// Author-declared sort position when a view asks for it.
    pub order: Option<i64>,
    /// Shared identity for file-axis twins (root-relative, no extension).
    #[serde(skip)]
    pub logical: String,
    /// Canonical published URL, empty when unrouted.
    pub url: String,
    /// Hash address (`/static/{hash}.{ext}`) when the embed policy published one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strong_url: Option<String>,
    /// Body size in bytes (body itself is not kept).
    pub body_bytes: usize,
    /// Parsed through the document pipeline (vs copied as bytes).
    pub rendered: bool,
    /// File carried front matter (block or sidecar).
    pub front_mattered: bool,
    /// Identity came from a sidecar `.toml`, not an in-file block.
    pub sidecar: bool,
    /// Publishes only when something references it.
    pub on_demand: bool,
    /// Canonical output route, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Key>,
    /// Non-canonical outputs (`rel="alternate"`), by URL.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternates: Vec<Key>,
    /// Listing / archive / feed routes that include this row.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub viewed_by: Vec<Key>,
    pub size: u64,
    /// Pixel width when this is an image.
    pub width: Option<u32>,
    /// Pixel height when this is an image.
    pub height: Option<u32>,
    /// Owned by a landing view; no standalone route.
    #[serde(skip)]
    pub claimed: bool,
    /// Axes the route rule spends.
    #[serde(skip)]
    pub axis: Vec<RowAxis>,
    /// Route templates with path tokens filled and axis tokens left.
    #[serde(skip)]
    pub route_templates: Vec<String>,
}

impl Keyed for Row {
    fn key(&self) -> &Key {
        &self.key
    }
}

impl Row {
    /// Parsed value of a declared `date`-typed field (`YYYY-MM-DD`).
    pub fn as_date(&self, name: &str) -> Option<NaiveDate> {
        parse_date_str(self.string(name)?)
    }

    /// Declared bool field; false when absent.
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.fields.get(name), Some(filter::Value::Bool(true)))
    }

    /// Declared string field, if present.
    pub fn string(&self, name: &str) -> Option<&str> {
        match self.fields.get(name) {
            Some(filter::Value::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Declared list field values, or empty.
    pub fn list(&self, name: &str) -> Vec<String> {
        filter::Row::field(self, name).as_str_list()
    }
}

fn hex<S: serde::Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{v:016x}"))
}

/// `YYYY-MM-DD`, or bare `YYYY-MM` as the first of that month.
pub fn parse_date_str(raw: &str) -> Option<NaiveDate> {
    let s = raw.trim();
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d"))
        .ok()
}

impl filter::Row for Row {
    fn field(&self, name: &str) -> filter::Value {
        use chrono::Datelike;
        use filter::Value as V;
        let opt_str = |o: &Option<String>| match o {
            Some(s) => V::Str(s.clone()),
            None => V::Null,
        };
        match name {
            "title" => opt_str(&self.title),
            "slug" => V::Str(self.slug.clone()),
            "stem" => V::Str(self.stem.clone()),
            "url" => V::Str(self.url.clone()),
            "body_bytes" => V::Int(self.body_bytes as i64),
            "order" => self.order.map_or(V::Null, V::Int),
            "rendered" => V::Bool(self.rendered),
            "front_mattered" => V::Bool(self.front_mattered),
            "output" => V::Bool(self.output.is_some()),
            "output.url" => self
                .output
                .as_ref()
                .map_or(V::Null, |k| V::Str(k.to_string())),
            "alternates" => V::str_list(self.alternates.iter().map(|k| k.to_string())),
            "path" => V::Str(self.rel.to_string_lossy().to_string()),
            "dir" => V::Str(
                self.rel
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            "collection" => V::Str(self.collection.clone()),
            "name" => self
                .rel
                .file_name()
                .map_or(V::Null, |s| V::Str(s.to_string_lossy().to_string())),
            "ext" => self.rel.extension().map_or(V::Str(String::new()), |s| {
                V::Str(s.to_string_lossy().to_lowercase())
            }),
            other => match other.split_once('.') {
                // Metadata namespaces. `.file.*` is filesystem, `.image.*` the
                // decoded header (`Null` off images).
                Some(("file", "size")) => V::Int(self.size as i64),
                Some(("image", "width")) => self.width.map_or(V::Null, |w| V::Int(w as i64)),
                Some(("image", "height")) => self.height.map_or(V::Null, |h| V::Int(h as i64)),
                Some((field, "year")) => self
                    .as_date(field)
                    .map_or(V::Null, |d| V::Int(d.year() as i64)),
                Some((field, "month")) => self
                    .as_date(field)
                    .map_or(V::Null, |d| V::Int(d.month() as i64)),
                Some((field, "day")) => self
                    .as_date(field)
                    .map_or(V::Null, |d| V::Int(d.day() as i64)),
                // Provider namespaces (`git.commit`, …) live in the overlay;
                // declared fields fall through to it.
                _ => self
                    .metadata
                    .get(other)
                    .or_else(|| self.fields.get(other))
                    .cloned()
                    .unwrap_or(V::Null),
            },
        }
    }
}
