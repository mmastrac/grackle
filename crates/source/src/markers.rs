//! Marker files: defaults declared by the tree (DESIGN.md §4b).
//!
//! A `.hidden` file makes its directory and everything below it hidden. The
//! config says what a marker means; the tree says where it applies.
//!
//! Resolution is the same shape as bucket lookup (§6a): walk up, nearest wins.

use anyhow::{bail, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub type Defaults = BTreeMap<String, toml::Value>;

/// What one marker MEANS: the defaults `[markers] ".draft" = { … }` declares
/// for the marker of that filename.
///
/// A newtype over the payload, `#[serde(transparent)]` so the TOML is
/// unchanged, because the wrapper is a statement about the MERGE (MERGE.md §7
/// q10): a marker's meaning is a *definition* under a user-chosen name — the
/// filename — and Law 2 stops at definitions, so a site redeclaring `.draft`
/// says what `.draft` means, whole. The bare `BTreeMap<String, toml::Value>`
/// said the opposite by structure (a map of maps descends twice, composing a
/// meaning out of two files); DESIGN.md §4d has always listed `[markers]`
/// among the registries that shadow by name, entry whole. The type says it
/// now, so `config::merge_base` can read the law off it rather than be told.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct MarkerDef(pub Defaults);

/// Which marker file set each key, per directory. Lives only as long as a
/// scan: nothing downstream needs a key's provenance, only the collision
/// check below does.
type Writers = HashMap<PathBuf, BTreeMap<String, String>>;

/// "`.draft` says true, `.retired` says false" — the two markers claiming one
/// key, ordered by filename so the message reads the same however the walk
/// happened to find them (the declaration walk is unsorted).
fn conflict(a: &str, av: &toml::Value, b: &str, bv: &toml::Value) -> String {
    let mut both = [(a, av), (b, bv)];
    both.sort_by(|x, y| x.0.cmp(y.0));
    format!(
        "{} says {}, {} says {}",
        both[0].0, both[0].1, both[1].0, both[1].1
    )
}

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
    /// thing we're looking for. The other two §4c layers do apply —
    /// `.gitignore` is what keeps this walk out of `_site*`, `vendor` and
    /// `target` (without some form of pruning it costs ~80ms instead of ~6ms),
    /// and `exclude` keeps it out of an embedded site that is not this one,
    /// and the positional layer keeps it out of `themes/` (IO.md IR6).
    /// `store::walker_declarations` owns all three. Only names are inspected;
    /// no file is read.
    pub fn scan(
        root: &Path,
        cfg: &BTreeMap<String, MarkerDef>,
        gitignore: bool,
        not: &crate::store::NotContent,
    ) -> Result<Self> {
        let mut m = Markers {
            names: cfg.keys().cloned().collect(),
            ..Default::default()
        };
        if cfg.is_empty() {
            return Ok(m);
        }
        let b = crate::store::walker_declarations(root, not, gitignore);
        let mut writers = Writers::new();
        for entry in b.build().filter_map(|e| e.ok()) {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(MarkerDef(defaults)) = cfg.get(&name) else {
                continue;
            };
            let Ok(rel) = entry.path().strip_prefix(root) else {
                continue;
            };
            let dir = rel.parent().unwrap_or(Path::new(""));
            m.fold(dir, &name, defaults, &mut writers)?;
            m.found += 1;
        }
        Ok(m)
    }

    /// Fold one marker file's defaults into the directory it sits in.
    ///
    /// Two markers in one directory setting **different** keys compose —
    /// that is the point of having more than one marker. Two setting the
    /// **same** key do not: `defaults_for` walks *directory levels*, and
    /// nearness ranks levels, not files at one level. Whichever the walk
    /// reached last would win, which from the config's point of view is no
    /// answer at all. So a disagreement here is the error (MERGE.md A5).
    ///
    /// Agreement is not a disagreement: two markers writing the same value
    /// for one key leave the directory with the same defaults whatever the
    /// order, so the walk's arbitrariness is unobservable and there is
    /// nothing to rank. That is the line `check_positional_collision` draws
    /// for `.schema.toml` too (A4) — the error is the *unrankable
    /// disagreement*, not the second writer.
    fn fold(
        &mut self,
        dir: &Path,
        name: &str,
        defaults: &Defaults,
        writers: &mut Writers,
    ) -> Result<()> {
        let slot = self.by_dir.entry(dir.to_path_buf()).or_default();
        let wrote = writers.entry(dir.to_path_buf()).or_default();
        for (k, v) in defaults {
            // `wrote` and `slot` are written together below, so a key in one
            // of them is in the other.
            if let (Some(prev), Some(prev_v)) = (wrote.get(k), slot.get(k)) {
                if prev_v != v {
                    let place = if dir.as_os_str().is_empty() {
                        "the site root".to_string()
                    } else {
                        format!("{}/", dir.display())
                    };
                    bail!(
                        "{} — two marker files in {place} set {k:?} and \
                         disagree. Nearest-wins ranks directory levels; two \
                         markers at one level have no order between them, so \
                         the winner would be whichever the tree walk reached \
                         last. Give them different keys, give them the same \
                         value, or move one.",
                        conflict(name, v, prev, prev_v)
                    );
                }
            }
            slot.insert(k.clone(), v.clone());
            wrote.insert(k.clone(), name.to_string());
        }
        Ok(())
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

    /// One marker file's payload, as `[markers] ".x" = { … }` would spell it.
    fn payload(pairs: &[(&str, toml::Value)]) -> Defaults {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn s(v: &str) -> toml::Value {
        toml::Value::String(v.into())
    }

    /// Fold a sequence of markers into one directory, as `scan` would.
    fn fold_all(dir: &str, files: &[(&str, Defaults)]) -> Result<Markers> {
        let mut m = Markers::default();
        let mut writers = Writers::new();
        for (name, defaults) in files {
            m.fold(Path::new(dir), name, defaults, &mut writers)?;
        }
        Ok(m)
    }

    /// The control: composition. Two markers in one directory saying
    /// different things is the whole reason more than one marker exists.
    #[test]
    fn two_markers_in_one_directory_with_disjoint_keys_compose() {
        let m = fold_all(
            "_posts/2003",
            &[
                (".hidden", payload(&[("hidden", true.into())])),
                (".noindex", payload(&[("noindex", true.into())])),
            ],
        )
        .expect("disjoint keys are composition, not conflict");
        let d = m.defaults_for(Path::new("_posts/2003/x.md"));
        assert_eq!(d.get("hidden").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(d.get("noindex").and_then(|v| v.as_bool()), Some(true));
    }

    /// The guard. Delete the `bail!` in `fold` and this loads silently, with
    /// the theme decided by whichever file the (unsorted) walk saw last.
    #[test]
    fn two_markers_in_one_directory_may_not_disagree_on_a_key() {
        let e = fold_all(
            "blog",
            &[
                (
                    ".draft",
                    payload(&[("draft", true.into()), ("theme", s("a"))]),
                ),
                (".featured", payload(&[("theme", s("b"))])),
            ],
        )
        .expect_err("same key, two files, one directory: nothing ranks them")
        .to_string();
        assert!(e.contains(".draft"), "{e}");
        assert!(e.contains(".featured"), "{e}");
        assert!(e.contains("\"theme\""), "{e}");
        assert!(e.contains("blog/"), "{e}");

        // Sorted by filename, so the walk order does not reach the message.
        let flipped = fold_all(
            "blog",
            &[
                (".featured", payload(&[("theme", s("b"))])),
                (".draft", payload(&[("theme", s("a"))])),
            ],
        )
        .expect_err("still a conflict the other way round")
        .to_string();
        assert!(
            flipped.starts_with(".draft says \"a\", .featured says \"b\""),
            "{flipped}"
        );
        assert!(
            e.starts_with(".draft says \"a\", .featured says \"b\""),
            "{e}"
        );
    }

    /// Agreement is not a conflict (A4's line, applied here): the walk's
    /// order is unobservable when both files write the same value.
    #[test]
    fn two_markers_agreeing_on_a_key_is_legal() {
        let m = fold_all(
            "archive",
            &[
                (".retired", payload(&[("noindex", true.into())])),
                (".noindex", payload(&[("noindex", true.into())])),
            ],
        )
        .expect("same key, same value, same directory either way round");
        let d = m.defaults_for(Path::new("archive/x.md"));
        assert_eq!(d.get("noindex").and_then(|v| v.as_bool()), Some(true));
    }

    /// The law that stays: levels are ordered, so a deeper marker overriding
    /// a shallower one is not a collision even for the same key.
    #[test]
    fn the_same_key_at_two_levels_is_still_nearest_wins() {
        let mut m = Markers::default();
        let mut writers = Writers::new();
        m.fold(
            Path::new(""),
            ".noindex",
            &payload(&[("noindex", true.into())]),
            &mut writers,
        )
        .unwrap();
        m.fold(
            Path::new("code/legacy"),
            ".indexed",
            &payload(&[("noindex", false.into())]),
            &mut writers,
        )
        .expect("different directories are ranked by nearness");
        let d = m.defaults_for(Path::new("code/legacy/x.html"));
        assert_eq!(d.get("noindex").and_then(|v| v.as_bool()), Some(false));
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
