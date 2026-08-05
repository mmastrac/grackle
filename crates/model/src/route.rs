//! One published URL.

use crate::{AxisMember, Key, Rendition, RouteKind};
use grackle_db::{filter, Keyed};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct Route {
    /// Identity: the URL.
    #[serde(skip)]
    pub id: Key,
    pub url: String,
    /// Hash address when this output is the embed-policy store entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strong_url: Option<String>,
    pub kind: RouteKind,
    /// Whether the source row carried front matter. False for view routes.
    pub front_mattered: bool,
    /// Source row's collection; absent on view routes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    /// Row this route renders, if any (views have none).
    #[serde(skip)]
    pub row: Option<Key>,
    /// Axis members this route sits on.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub axis: Vec<AxisMember>,
    /// Landing body claimed by a templated `content` / `default_content`.
    #[serde(skip)]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    /// View that produced this route, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// Group key for subdivided views.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Member count for listing routes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<usize>,
    /// 1-based page number when paginated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    /// Group parameters for title / crumb templates.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<(String, String)>,
    /// Declared fields copied from the source row (for all-outputs filters).
    #[serde(skip)]
    pub fields: BTreeMap<String, filter::Value>,
    /// Other outputs this one reads facts from (fold members, embedded renditions).
    #[serde(skip)]
    pub route_members: Vec<Key>,
    /// Input rows this route materializes, in order.
    #[serde(skip)]
    pub members: Vec<Key>,
    /// Input rows that feed this output (invalidation / pull edges).
    #[serde(skip)]
    pub inputs: Vec<Key>,
    /// Transform parameters when this output is a rendition.
    #[serde(skip)]
    pub rendition: Option<Rendition>,
}

impl Keyed for Route {
    fn key(&self) -> &Key {
        &self.id
    }
}

impl Route {
    /// Minimal route: URL and kind.
    pub fn new(url: String, kind: RouteKind) -> Route {
        Route {
            id: Key::new(&url),
            url,
            strong_url: None,
            kind,
            front_mattered: false,
            collection: None,
            row: None,
            axis: Vec::new(),
            content: None,
            source: None,
            view: None,
            key: None,
            rows: None,
            page: None,
            params: Vec::new(),
            fields: BTreeMap::new(),
            members: Vec::new(),
            route_members: Vec::new(),
            inputs: Vec::new(),
            rendition: None,
        }
    }

    /// URL ends in `/` (output is an index.html).
    fn is_dir(&self) -> bool {
        self.url.ends_with('/')
    }

    /// Extension of the last URL segment, or empty for directories / extensionless paths.
    fn ext(&self) -> &str {
        if self.is_dir() {
            return "";
        }
        let last = self.url.rsplit('/').next().unwrap_or("");
        match last.rsplit_once('.') {
            Some((_, e)) => e,
            None => "",
        }
    }
}

impl filter::Row for Route {
    fn field(&self, name: &str) -> filter::Value {
        use filter::Value as V;
        match name {
            "kind" => V::Str(self.kind.as_str().to_string()),
            "front_mattered" => V::Bool(self.front_mattered),
            "collection" => match &self.collection {
                Some(c) => V::Str(c.clone()),
                None => V::Null,
            },
            "view" => match &self.view {
                Some(v) => V::Str(v.clone()),
                None => V::Null,
            },
            "url" => V::Str(self.url.clone()),
            "ext" => V::Str(self.ext().to_string()),
            "dir" => V::Bool(self.is_dir()),
            "key" => match &self.key {
                Some(k) => V::Str(k.clone()),
                None => V::Null,
            },
            "page" => self.page.map_or(V::Null, |p| V::Int(p as i64)),
            "rows" => self.rows.map_or(V::Null, |r| V::Int(r as i64)),
            "stem" => self
                .source
                .as_deref()
                .and_then(|p| p.file_stem())
                .map_or(V::Null, |s| {
                    // Strip a pairing-axis filename suffix (`index.fr` -> `index`).
                    let s = s.to_string_lossy();
                    let logical = self
                        .fields
                        .iter()
                        .find_map(|(_, v)| match v {
                            filter::Value::Str(l) => s
                                .strip_suffix(l.as_str())
                                .and_then(|rest| rest.strip_suffix('.'))
                                .map(|stem| stem.to_owned()),
                            _ => None,
                        })
                        .unwrap_or_else(|| s.into_owned());
                    V::Str(logical)
                }),
            other => self.fields.get(other).cloned().unwrap_or(V::Null),
        }
    }
}
