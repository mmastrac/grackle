//! FsStore: the filesystem as storage engine.
//!
//! Phase 0 hydrates stage 1 (stat/version) and stage 2 (front matter). The body
//! is split out but not parsed or rendered — see DESIGN.md §2.

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Front matter. Unknown keys are tolerated (Jekyll front matter is open).
#[derive(Debug, Default, Deserialize)]
pub struct FrontMatter {
    pub title: Option<String>,
    pub description: Option<String>,
    pub layout: Option<String>,
    pub permalink: Option<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub tags: Vec<String>,
    /// `YYYY-MM-DD`. A post's date comes from its filename (§3), so this is
    /// the override there and the ONLY source on a tree page — which is
    /// what makes "has a date" a property of the row's data rather than of
    /// which table holds it (q51). Parsed at load, so a malformed one is a
    /// load error naming the file rather than a row that quietly sorts last.
    pub date: Option<String>,
    pub hidden: Option<bool>,
    pub draft: Option<bool>,
    pub noindex: Option<bool>,
    /// Declared position within a section tree (§6e). Unset sorts last.
    pub order: Option<i64>,
    /// Render this document's heading outline (§6e) — §5a's canonical
    /// "render directive" example. Cascades from markers/rules like any
    /// default.
    pub toc: Option<bool>,
    /// Which theme renders this row (§5a: theme is chosen per row).
    /// Cascades from rules, so a subtree changes look with one rule.
    pub theme: Option<String>,
    /// Which shell wraps this row (§5g, q44): `none` emits the body with
    /// no skeleton at all, so an imported artifact can carry front matter
    /// without being nested inside a second document. Cascades like
    /// `theme`.
    pub shell: Option<String>,
    /// Everything else: captured for per-subtree schema validation (§5b).
    /// Ungoverned rows tolerate unknown keys exactly as before; a row under
    /// a `.schema.toml` gets them checked.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_yaml_ng::Value>,
}

/// `tags:` is a YAML sequence in all 44 posts that have it, but Jekyll also
/// permits a whitespace-separated string. Accept both rather than fail loudly
/// on a form the old site would have taken.
fn string_or_seq<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum V {
        S(String),
        Seq(Vec<String>),
    }
    Ok(match Option::<V>::deserialize(d)? {
        Some(V::S(s)) => s.split_whitespace().map(String::from).collect(),
        Some(V::Seq(v)) => v,
        None => Vec::new(),
    })
}

/// A hydrated source file.
#[derive(Debug)]
pub struct RawRow {
    /// Identity (DESIGN.md §3): the source path, absolute.
    pub path: PathBuf,
    /// Path relative to the collection source; what rule globs match against.
    pub rel: PathBuf,
    /// Cheap version: mtime ^ size. Phase 0 uses this as the pre-check the
    /// design calls for; content hashing lands with the cache.
    pub version: u64,
    pub front: FrontMatter,
    pub body: String,
}

/// The body of a source file, read from disk. No row holds its body: the
/// posts loader used to keep one in memory while the tree loader did not,
/// and that asymmetry outlived the row-type merge (q51) for no reason
/// except that it was already there. Reading is a few ms over ~800 files
/// and costs nothing to reason about.
pub fn read_body(path: &Path) -> Result<String> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(split_front_matter(&text).1.to_string())
}

/// Split `---\nyaml\n---\nbody`. Returns (yaml, body).
/// A file with no front matter is all body. The one front-matter fence
/// parser: page schema reads and the SCSS/template splits all come here.
pub fn split_front_matter(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---") else {
        return ("", text);
    };
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            let yaml = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return (yaml, body);
        }
        offset += line.len();
    }
    ("", text)
}

fn version_of(meta: &std::fs::Metadata) -> u64 {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    mtime ^ meta.len().rotate_left(32)
}

fn load_one(path: &Path, source: &Path) -> Result<RawRow> {
    let meta = std::fs::metadata(path)?;
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (yaml, body) = split_front_matter(&text);
    let front: FrontMatter = if yaml.trim().is_empty() {
        FrontMatter::default()
    } else {
        serde_yaml_ng::from_str(yaml)
            .with_context(|| format!("front matter of {}", path.display()))?
    };
    let rel = path.strip_prefix(source).unwrap_or(path).to_path_buf();
    Ok(RawRow {
        path: path.to_path_buf(),
        rel,
        version: version_of(&meta),
        front,
        body: body.to_string(),
    })
}

/// Does this file start with a `---` front-matter fence?
///
/// This is the page/static discriminator in Jekyll: a file with front matter is
/// rendered and gets a pretty URL; one without is copied verbatim. Cheap enough
/// to run over the whole tree (4 bytes per file).
pub fn peek_front_matter(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 4];
    match f.read(&mut buf) {
        Ok(n) if n >= 4 => &buf[..3] == b"---" && (buf[3] == b'\n' || buf[3] == b'\r'),
        _ => false,
    }
}

/// One file found by the tree walk. `has_front_matter` is filled in by the
/// caller, which knows which rows are binary objects and can skip the peek.
#[derive(Debug)]
pub struct TreeFile {
    pub path: PathBuf,
    pub rel: PathBuf,
    pub version: u64,
    pub size: u64,
    pub has_front_matter: bool,
}

/// The one place `.gitignore` semantics are defined; both the tree walk and the
/// marker scan build on it.
///
/// `.gitignore` is the site's existing, authoritative statement of "this is not
/// content" — every build artifact (`_site*`, `_log*`, `vendor`, `_cache`,
/// `grackle/target`, `.jekyll-cache`, …) is already listed there, and those are
/// exactly the directories that are expensive to walk. Honouring it removes a
/// hand-maintained duplicate that silently rots.
///
/// It is not a *complete* exclusion list: `docker/`, `scripts/`, `CHANGES/`,
/// `Gemfile` and friends are tracked on purpose but still aren't content, so a
/// Jekyll-style `exclude` list stays (DESIGN.md §4c).
pub fn walker(root: &Path, gitignore: bool) -> ignore::WalkBuilder {
    let mut b = ignore::WalkBuilder::new(root);
    b.hidden(false) // dotfile policy is ours: .well-known and markers must survive
        .git_ignore(gitignore)
        .git_exclude(gitignore)
        // A contributor's global gitignore must not change what the site
        // publishes, and neither must a .gitignore above the site root.
        .git_global(false)
        .parents(false)
        // Honour .gitignore even in a checkout that isn't a git repo.
        .require_git(false)
        .follow_links(false);
    b
}

fn is_git_dir(e: &ignore::DirEntry) -> bool {
    e.file_type().is_some_and(|t| t.is_dir()) && e.file_name() == ".git"
}

/// Walk the site root the way Jekyll does: skip dot- and underscore-prefixed
/// entries and anything matching `exclude`, unless `include` re-adds it.
/// `.gitignore` is honoured underneath (see `walker`).
pub fn walk_tree(
    root: &Path,
    exclude: &globset::GlobSet,
    include: &globset::GlobSet,
    gitignore: bool,
) -> Result<Vec<TreeFile>> {
    let mut out = Vec::new();
    let root_owned = root.to_path_buf();
    let inc = include.clone();
    let exc = exclude.clone();

    let mut b = walker(root, gitignore);
    b.filter_entry(move |e| {
        if is_git_dir(e) {
            return false;
        }
        let Ok(rel) = e.path().strip_prefix(&root_owned) else {
            return true;
        };
        if rel.as_os_str().is_empty() {
            return true;
        }
        if inc.is_match(rel) {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        if name.starts_with('.') || name.starts_with('_') {
            return false;
        }
        !exc.is_match(rel)
    });

    for entry in b.build() {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = match entry.path().strip_prefix(root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };
        // filter_entry prunes directories; re-check files that slipped through
        // via an ancestor being allowed.
        if !include.is_match(&rel) && exclude.is_match(&rel) {
            continue;
        }
        let meta = entry.metadata()?;
        out.push(TreeFile {
            has_front_matter: false, // filled in by the caller; see peek_front_matter
            path: entry.path().to_path_buf(),
            rel,
            version: version_of(&meta),
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Walk `source` and hydrate every file with one of `extensions`.
pub fn load_dir(source: &Path, extensions: &[&str]) -> Result<Vec<RawRow>> {
    let paths: Vec<PathBuf> = WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
                .unwrap_or(false)
        })
        .collect();

    // 327 files / 1.6 MB: parallel parse keeps the whole load well inside the
    // 200ms budget even cold.
    paths
        .par_iter()
        .map(|p| load_one(p, source))
        .collect::<Result<Vec<_>>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_front_matter() {
        let (yaml, body) = split_front_matter("---\ntitle: x\n---\nhello\n");
        assert_eq!(yaml, "title: x\n");
        assert_eq!(body, "hello\n");
    }

    #[test]
    fn handles_no_front_matter() {
        let (yaml, body) = split_front_matter("just text");
        assert_eq!(yaml, "");
        assert_eq!(body, "just text");
    }

    #[test]
    fn body_containing_hr_is_not_a_terminator() {
        // A `---` inside the body must not be mistaken for the closing fence
        // once we've already closed.
        let (yaml, body) = split_front_matter("---\na: 1\n---\nx\n\n---\n\ny\n");
        assert_eq!(yaml, "a: 1\n");
        assert_eq!(body, "x\n\n---\n\ny\n");
    }

    #[test]
    fn tags_accepts_seq_and_string() {
        #[derive(Deserialize)]
        struct T {
            #[serde(default, deserialize_with = "string_or_seq")]
            tags: Vec<String>,
        }
        let a: T = serde_yaml_ng::from_str("tags:\n  - x\n  - y\n").unwrap();
        assert_eq!(a.tags, vec!["x", "y"]);
        let b: T = serde_yaml_ng::from_str("tags: x y\n").unwrap();
        assert_eq!(b.tags, vec!["x", "y"]);
        let c: T = serde_yaml_ng::from_str("title: z\n").unwrap();
        assert!(c.tags.is_empty());
    }
}
