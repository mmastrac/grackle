//! FsStore: hydrate stat/version and front matter only.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// YAML block and TOML sidecar deserialize into the same struct.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct FrontMatter {
    pub title: Option<String>,
    /// Cut point in the render chain. `root` skips document furniture.
    pub slot: Option<String>,
    pub permalink: Option<String>,
    /// Declared position within a section tree. Unset sorts last.
    pub order: Option<i64>,
    /// Which theme renders this row.
    /// Cascades from rules, so a subtree changes look with one rule.
    pub theme: Option<String>,
    /// Which shell wraps this row: `none` emits the body with
    /// no skeleton at all, so an imported artifact can carry front matter
    /// without being nested inside a second document. Cascades like
    /// `theme`.
    pub shell: Option<String>,
    /// Everything else: captured for schema validation. `draft`,
    /// `hidden`, `noindex`, `toc`, `tags`, `description`, and `date` arrive
    /// here, declared fields the base config ships, not names this struct knows.
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_yaml_ng::Value>,
}

pub fn read_body(path: &Path) -> Result<String> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(split_front_matter(&text).2.to_string())
}

/// Which parser a front-matter fence selects: `---` is YAML (the native form),
/// `+++` is TOML (the form Zola/Hugo write). Both deserialize into the one
/// `FrontMatter` struct, the same struct TOML sidecars already use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FmFmt {
    Yaml,
    Toml,
}

/// Split `---\nyaml\n---\nbody` or `+++\ntoml\n+++\nbody`, returning the fence's
/// format alongside the block and the body. The close fence must match the
/// open. No fence means all body, `(None, "", text)`.
pub fn split_front_matter(text: &str) -> (Option<FmFmt>, &str, &str) {
    let (fence, fmt) = match text.as_bytes() {
        [b'-', b'-', b'-', ..] => ("---", FmFmt::Yaml),
        [b'+', b'+', b'+', ..] => ("+++", FmFmt::Toml),
        _ => return (None, "", text),
    };
    let rest = &text[fence.len()..];
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == fence {
            let block = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return (Some(fmt), block, body);
        }
        offset += line.len();
    }
    (None, "", text)
}

/// mtime xor length; shared with sidecar scan.
pub(crate) fn version_of(meta: &std::fs::Metadata) -> u64 {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    mtime ^ meta.len().rotate_left(32)
}

pub fn peek_front_matter(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 4];
    match f.read(&mut buf) {
        Ok(n) if n >= 4 => opens_front_matter(buf),
        _ => false,
    }
}

fn opens_front_matter(text: impl AsRef<[u8]>) -> bool {
    matches!(
        text.as_ref(),
        [b'-', b'-', b'-', b'\n' | b'\r', ..] | [b'+', b'+', b'+', b'\n' | b'\r', ..]
    )
}

#[derive(Debug)]
pub struct TreeFile {
    pub path: PathBuf,
    pub rel: PathBuf,
    pub version: u64,
    pub size: u64,
    pub has_front_matter: bool,
}

/// `.gitignore` semantics for tree and marker walks.
pub fn walker(root: &Path, gitignore: bool) -> ignore::WalkBuilder {
    let mut b = ignore::WalkBuilder::new(root);
    b.hidden(false) // dotfile policy is ours: .well-known and markers must survive
        .git_ignore(gitignore)
        .git_exclude(gitignore)
        .git_global(false)
        .parents(false)
        .require_git(false)
        .follow_links(false);
    b
}

fn is_git_dir(e: &ignore::DirEntry) -> bool {
    e.file_type().is_some_and(|t| t.is_dir()) && e.file_name() == ".git"
}

/// Site-root `themes/` is engine vocabulary by position.
pub const THEMES: &str = "themes";

/// Declared not-content layer: `exclude` with `include` re-adding.
#[derive(Clone)]
pub struct NotContent {
    exclude: globset::GlobSet,
    include: globset::GlobSet,
}

impl NotContent {
    pub fn new(exclude: globset::GlobSet, include: globset::GlobSet) -> NotContent {
        NotContent { exclude, include }
    }

    pub fn included(&self, rel: &Path) -> bool {
        self.include.is_match(rel)
    }

    /// Directory twin of `included`; subtree patterns match contents, not the dir.
    pub fn included_dir(&self, rel: &Path) -> bool {
        self.include.is_match(rel) || self.include.is_match(rel.join(""))
    }

    fn keeps(&self, rel: &Path) -> bool {
        rel.as_os_str().is_empty() || self.include.is_match(rel) || !self.exclude.is_match(rel)
    }

    /// Pruning walks need the directory question: `embedded/**` matches `embedded/x`, not `embedded`.
    pub fn keeps_dir(&self, rel: &Path) -> bool {
        let below = rel.join("");
        rel.as_os_str().is_empty()
            || self.include.is_match(rel)
            || self.include.is_match(&below)
            || !(self.exclude.is_match(rel) || self.exclude.is_match(&below))
    }
}

/// Declaration walk: markers, `.schema.toml`, sidecars. No dot/underscore skip; exclude prunes directories only.
pub fn walker_declarations(root: &Path, not: &NotContent, gitignore: bool) -> ignore::WalkBuilder {
    let root_owned = root.to_path_buf();
    let not = not.clone();
    let mut b = walker(root, gitignore);
    b.filter_entry(move |e| {
        if is_git_dir(e) {
            return false;
        }
        if !e.file_type().is_some_and(|t| t.is_dir()) {
            return true;
        }
        match e.path().strip_prefix(&root_owned) {
            Ok(rel) if rel == Path::new(THEMES) && !not.included_dir(rel) => false,
            Ok(rel) => not.keeps_dir(rel),
            Err(_) => true,
        }
    });
    b
}

/// Collection `source` punches through Jekyll's dot/underscore skip.
fn punches_through(rel: &Path, sources: &[PathBuf]) -> bool {
    sources
        .iter()
        .any(|s| rel.starts_with(s) || s.starts_with(rel))
}

/// Content tree walk. Rules decide tables; the walk has no opinion.
pub fn walk_tree(
    root: &Path,
    not: &NotContent,
    gitignore: bool,
    sources: &[PathBuf],
) -> Result<Vec<TreeFile>> {
    let mut out = Vec::new();
    let root_owned = root.to_path_buf();
    let owned = not.clone();
    let sources = sources.to_vec();

    let mut b = walker(root, gitignore);
    b.filter_entry(move |e| {
        if is_git_dir(e) {
            return false;
        }
        let Ok(rel) = e.path().strip_prefix(&root_owned) else {
            return true;
        };
        if rel.as_os_str().is_empty() || owned.include.is_match(rel) {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        if (name.starts_with('.') || name.starts_with('_')) && !punches_through(rel, &sources) {
            return false;
        }
        owned.keeps(rel)
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
        if !not.keeps(&rel) {
            continue;
        }
        let meta = entry.metadata()?;
        out.push(TreeFile {
            has_front_matter: false,
            path: entry.path().to_path_buf(),
            rel,
            version: version_of(&meta),
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn not_content(exclude: &[&str], include: &[&str]) -> NotContent {
        fn set(pats: &[&str]) -> globset::GlobSet {
            let mut b = globset::GlobSetBuilder::new();
            for p in pats {
                b.add(globset::Glob::new(p).unwrap());
            }
            b.build().unwrap()
        }
        NotContent::new(set(exclude), set(include))
    }

    #[test]
    fn an_excluded_subtree_is_pruned_at_its_own_root() {
        // `embedded/**` matches the contents, not the directory, so `keeps`
        // says yes to `embedded` itself and a pruning walk steps one level in
        // `keeps_dir` is the one that closes it.
        let not = not_content(&["embedded/**"], &[]);
        assert!(not.keeps(Path::new("embedded")));
        assert!(!not.keeps_dir(Path::new("embedded")));
        assert!(!not.keeps_dir(Path::new("embedded/books")));
        assert!(not.keeps_dir(Path::new("books")));
        assert!(not.keeps_dir(Path::new("")));
    }

    #[test]
    fn a_file_shaped_pattern_still_does_not_prune_a_directory() {
        // A deliberate narrowing: grack.com excludes `*.toml`, which is a
        // statement about content. It must not unspeak the `.schema.toml`
        // declarations the vocabulary walk exists to find, and it must not
        // prune the directories they live in.
        let not = not_content(&["*.toml", "Gemfile*", "*.sh"], &[]);
        assert!(not.keeps_dir(Path::new("posts")));
        assert!(not.keeps_dir(Path::new("posts/2026")));
        // The same set does still exclude the files themselves for the tree
        // walk, which is where a content statement belongs.
        assert!(!not.keeps(Path::new("posts/thing.toml")));
    }

    #[test]
    fn include_re_adds_at_the_same_granularity() {
        // Both questions are asked of `include` first, so a subtree-shaped
        // re-add keeps the subtree's root walkable.
        let not = not_content(&["vendor/**"], &["vendor/**"]);
        assert!(not.keeps_dir(Path::new("vendor")));
        assert!(not.keeps_dir(Path::new("vendor/keep")));
    }

    /// The positional layer's hatch asks the DIRECTORY question.
    ///
    /// `included` is the file question the content filter asks, and it is
    /// the wrong one for a pruning walk: the declaration walks prune `themes`
    /// itself, and `themes/**`, the one spelling a site would write, does
    /// not match `themes`. Same asymmetry `keeps_dir` exists for, so the
    /// same idiom answers it.
    ///
    /// Mutation: drop the empty-child clause from `included_dir` and the hatch
    /// opens for nothing a site would type.
    #[test]
    fn the_positional_hatch_is_asked_of_the_directory() {
        let not = not_content(&[], &["themes/**"]);
        assert!(!not.included(Path::new("themes")));
        assert!(not.included_dir(Path::new("themes")));
        // A file-shaped `include` cannot open a directory, which is the other
        // half of the same asymmetry.
        let not = not_content(&[], &["*.toml"]);
        assert!(!not.included_dir(Path::new("themes")));
    }

    /// The punch-through compares whole COMPONENTS, both ways.
    ///
    /// grack.com is the site that makes each half matter: it has `_drafts` and
    /// `_drafts_temp` side by side, so a string prefix would walk the second
    /// because the first is declared, and the downward direction is what lets
    /// a pruning walk reach a nested source at all, since refusing `_a` never
    /// gets you to `_a/_b`.
    ///
    /// Mutation: `to_string_lossy().starts_with()` instead of
    /// `Path::starts_with` and `_drafts_temp` joins the walk; drop the second
    /// clause and a `source = "_a/_b"` is unreachable.
    #[test]
    fn the_punch_through_names_whole_components_in_both_directions() {
        let sources = vec![PathBuf::from("_drafts"), PathBuf::from("_a/_b")];
        assert!(punches_through(Path::new("_drafts"), &sources));
        assert!(punches_through(Path::new("_drafts/caret/x.md"), &sources));
        assert!(!punches_through(Path::new("_drafts_temp"), &sources));
        assert!(!punches_through(Path::new("_drafts_temp/x.md"), &sources));
        // On the way down to a nested source, and no further.
        assert!(punches_through(Path::new("_a"), &sources));
        assert!(punches_through(Path::new("_a/_b/x.md"), &sources));
        assert!(!punches_through(Path::new("_a2"), &sources));
        // And a site that declares no source punches nothing through.
        assert!(!punches_through(Path::new("_posts"), &[]));
    }

    #[test]
    fn splits_front_matter() {
        let (fmt, yaml, body) = split_front_matter("---\ntitle: x\n---\nhello\n");
        assert_eq!(fmt, Some(FmFmt::Yaml));
        assert_eq!(yaml, "title: x\n");
        assert_eq!(body, "hello\n");
    }

    #[test]
    fn splits_toml_front_matter() {
        // `+++` opens a TOML block (the Zola/Hugo form). Close fence matches.
        let (fmt, toml, body) = split_front_matter("+++\ntitle = \"x\"\n+++\nhello\n");
        assert_eq!(fmt, Some(FmFmt::Toml));
        assert_eq!(toml, "title = \"x\"\n");
        assert_eq!(body, "hello\n");
    }

    #[test]
    fn a_toml_block_needs_a_toml_close() {
        // A `+++` open closed by `---` is not a block: the fences must match,
        // so the whole thing is body (no valid front matter).
        let (fmt, block, body) = split_front_matter("+++\ntitle = \"x\"\n---\nhello\n");
        assert_eq!(fmt, None);
        assert_eq!(block, "");
        assert_eq!(body, "+++\ntitle = \"x\"\n---\nhello\n");
    }

    #[test]
    fn handles_no_front_matter() {
        let (fmt, block, body) = split_front_matter("just text");
        assert_eq!(fmt, None);
        assert_eq!(block, "");
        assert_eq!(body, "just text");
    }

    #[test]
    fn body_containing_hr_is_not_a_terminator() {
        // A `---` inside the body must not be mistaken for the closing fence
        // once we've already closed.
        let (fmt, yaml, body) = split_front_matter("---\na: 1\n---\nx\n\n---\n\ny\n");
        assert_eq!(fmt, Some(FmFmt::Yaml));
        assert_eq!(yaml, "a: 1\n");
        assert_eq!(body, "x\n\n---\n\ny\n");
    }
}
