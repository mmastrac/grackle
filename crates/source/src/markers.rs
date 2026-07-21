//! Marker files: defaults declared by the tree (DESIGN.md §4b).
//!
//! A `.hidden` file makes its directory and everything below it hidden. The
//! config says what a marker means; the tree says where it applies.
//!
//! Resolution is the same shape as bucket lookup (§6a): walk up, nearest wins.

use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub type Defaults = BTreeMap<String, toml::Value>;

#[derive(Debug, Default)]
pub struct Markers {
    /// Root-relative directory -> the defaults its markers declare.
    by_dir: HashMap<PathBuf, Defaults>,
    /// Marker filenames, so the tree walk can refuse to route them.
    names: Vec<String>,
    pub found: usize,
}

impl Markers {
    /// Scan the tree for marker files.
    ///
    /// Deliberately does not honour the dotfile/underscore skip: markers *are*
    /// dotfiles, and they live under `_posts`, so that skip would hide the very
    /// thing we're looking for. That leaves `.gitignore` (via `store::walker`)
    /// as what keeps this walk out of `_site*`, `vendor` and `target` — without
    /// some form of pruning it costs ~80ms instead of ~6ms. Only names are
    /// inspected; no file is read.
    pub fn scan(root: &Path, cfg: &BTreeMap<String, Defaults>, gitignore: bool) -> Result<Self> {
        let mut m = Markers {
            names: cfg.keys().cloned().collect(),
            ..Default::default()
        };
        if cfg.is_empty() {
            return Ok(m);
        }
        let mut b = crate::store::walker(root, gitignore);
        b.filter_entry(|e| !(e.file_type().is_some_and(|t| t.is_dir()) && e.file_name() == ".git"));
        for entry in b.build().filter_map(|e| e.ok()) {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(defaults) = cfg.get(&name) else {
                continue;
            };
            let Ok(rel) = entry.path().strip_prefix(root) else {
                continue;
            };
            let dir = rel.parent().unwrap_or(Path::new("")).to_path_buf();
            let slot = m.by_dir.entry(dir).or_default();
            for (k, v) in defaults {
                slot.insert(k.clone(), v.clone());
            }
            m.found += 1;
        }
        Ok(m)
    }

    pub fn is_marker(&self, path: &Path) -> bool {
        path.file_name()
            .map(|n| self.names.iter().any(|m| m == &n.to_string_lossy()))
            .unwrap_or(false)
    }

    /// Defaults for a row, given its **root-relative** path.
    ///
    /// Walks up from the row's directory; first writer wins per key, so the
    /// nearest marker shadows a shallower one.
    pub fn defaults_for(&self, rel: &Path) -> Defaults {
        let mut out = Defaults::new();
        if self.by_dir.is_empty() {
            return out;
        }
        let mut dir = rel.parent();
        while let Some(d) = dir {
            if let Some(defs) = self.by_dir.get(d) {
                for (k, v) in defs {
                    out.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
            if d.as_os_str().is_empty() {
                break;
            }
            dir = d.parent();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> BTreeMap<String, Defaults> {
        let mut c = BTreeMap::new();
        let mut hidden = Defaults::new();
        hidden.insert("hidden".into(), toml::Value::Boolean(true));
        c.insert(".hidden".into(), hidden);
        let mut noindex = Defaults::new();
        noindex.insert("noindex".into(), toml::Value::Boolean(true));
        c.insert(".noindex".into(), noindex);
        c
    }

    /// Build a Markers directly, bypassing the filesystem.
    fn markers(dirs: &[(&str, &str, bool)]) -> Markers {
        let mut m = Markers {
            names: vec![".hidden".into(), ".noindex".into()],
            ..Default::default()
        };
        for (dir, key, val) in dirs {
            m.by_dir
                .entry(PathBuf::from(dir))
                .or_default()
                .insert(key.to_string(), toml::Value::Boolean(*val));
        }
        m
    }

    #[test]
    fn applies_to_the_directory_and_below() {
        let m = markers(&[("_posts/hidden", "hidden", true)]);
        let d = m.defaults_for(Path::new("_posts/hidden/2003-07-31-x.md"));
        assert_eq!(d.get("hidden").and_then(|v| v.as_bool()), Some(true));
        // deeper still
        let d = m.defaults_for(Path::new("_posts/hidden/deep/er/x.md"));
        assert_eq!(d.get("hidden").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn does_not_leak_to_siblings_or_parents() {
        let m = markers(&[("_posts/hidden", "hidden", true)]);
        assert!(m
            .defaults_for(Path::new("_posts/2003-07-31-x.md"))
            .is_empty());
        assert!(m.defaults_for(Path::new("_posts/other/x.md")).is_empty());
        assert!(m.defaults_for(Path::new("index.html")).is_empty());
    }

    #[test]
    fn nearest_marker_shadows_a_shallower_one() {
        // root says noindex, but a deeper marker says otherwise
        let m = markers(&[("", "noindex", true), ("code/legacy", "noindex", false)]);
        let deep = m.defaults_for(Path::new("code/legacy/romtool/index.html"));
        assert_eq!(deep.get("noindex").and_then(|v| v.as_bool()), Some(false));
        let shallow = m.defaults_for(Path::new("code/other/index.html"));
        assert_eq!(shallow.get("noindex").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn a_root_marker_reaches_everything() {
        let m = markers(&[("", "noindex", true)]);
        for p in ["index.html", "a/b/c/d.html", "_posts/2003/x.md"] {
            assert_eq!(
                m.defaults_for(Path::new(p))
                    .get("noindex")
                    .and_then(|v| v.as_bool()),
                Some(true),
                "{p}"
            );
        }
    }

    #[test]
    fn distinct_keys_from_different_depths_accumulate() {
        let m = markers(&[("a", "noindex", true), ("a/b", "hidden", true)]);
        let d = m.defaults_for(Path::new("a/b/x.md"));
        assert_eq!(d.get("noindex").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(d.get("hidden").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn recognises_marker_filenames() {
        let m = markers(&[]);
        assert!(m.is_marker(Path::new("a/b/.hidden")));
        assert!(m.is_marker(Path::new(".noindex")));
        assert!(!m.is_marker(Path::new("a/b/.hiddenish")));
        assert!(!m.is_marker(Path::new("index.html")));
    }

    #[test]
    fn no_markers_configured_is_a_cheap_noop() {
        let m = Markers::default();
        assert!(m.defaults_for(Path::new("a/b/c.md")).is_empty());
    }
}
