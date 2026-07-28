//! FsStore: the filesystem as storage engine.
//!
//! Hydrates stat/version and front matter only. The body is split out but
//! never parsed or rendered here — see DESIGN.md §2 for the stages.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

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
    /// Everything else: captured for schema validation (§5b). `draft`,
    /// `hidden` and `noindex` arrive here now — they are declared fields the
    /// base config ships (§4e), not names this struct knows.
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
        Ok(n) if n >= 4 => opens_front_matter(buf),
        _ => false,
    }
}

/// The same question asked of bytes already in hand.
///
/// ONE definition, deliberately: two spellings of "does this file carry front
/// matter" would be two answers the day one of them learned about `\r\n` and
/// the other did not. Since IO.md I7d there is also only one CALLER — the one
/// walk peeks every file it may render, where the posts loader used to read
/// whole files and ask this of the text.
fn opens_front_matter(text: impl AsRef<[u8]>) -> bool {
    matches!(text.as_ref(), [b'-', b'-', b'-', b'\n' | b'\r', ..])
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

/// The *declared* not-content layer of DESIGN.md §4c — a collection's
/// `exclude`, with `include` re-adding ahead of it — compiled once and read
/// by every walk of the site root.
///
/// It is one value rather than two globsets passed around because the walks
/// must reach the same verdict: the vocabulary walk carrying its own idea of
/// what lies outside the site is how `cover`, declared under grack.com's
/// excluded `grackle/**`, became part of grack.com's field vocabulary
/// (MERGE.md R1 — q34's disease, one rung up).
///
/// One value is not by itself one verdict, though: the walks ask it different
/// questions. `keeps` answers for a single path, which is all the tree walk
/// needs — it post-checks every file it emits. A walk that decides by pruning
/// directories has no second chance at the files inside, so it must ask
/// `keeps_dir` (MERGE.md R2).
#[derive(Clone)]
pub struct NotContent {
    exclude: globset::GlobSet,
    include: globset::GlobSet,
}

impl NotContent {
    pub fn new(exclude: globset::GlobSet, include: globset::GlobSet) -> NotContent {
        NotContent { exclude, include }
    }

    /// Does an `include` pattern name `rel` explicitly?
    ///
    /// `include` is the escape hatch, and `keeps` gives it first say — so a
    /// *positional* not-content rule (IO.md I7b's site-root `themes/`) has to
    /// ask the same question the same way, or the hatch would open for the
    /// declared layer and not for the engine's own.
    pub fn included(&self, rel: &Path) -> bool {
        self.include.is_match(rel)
    }

    /// Is `rel` (root-relative) still inside the site?
    fn keeps(&self, rel: &Path) -> bool {
        rel.as_os_str().is_empty() || self.include.is_match(rel) || !self.exclude.is_match(rel)
    }

    /// Is the directory `rel` — and everything under it — still inside the
    /// site? A subtree pattern names the contents, not the directory:
    /// `embedded/**` matches `embedded/x`, never `embedded`, so a walk that
    /// prunes on `keeps` alone steps one level into every excluded subtree and
    /// reads whatever sits directly there (MERGE.md R2).
    ///
    /// The second question is the same path with a trailing separator — the
    /// empty child, `embedded/`. A subtree pattern matches it; a file-shaped
    /// one cannot (`*.toml` matches `a/b.toml`, never `a/`), which is what
    /// keeps R1's narrowing: grack.com excludes `*.toml` and must not thereby
    /// prune the directory a `.schema.toml` lives in.
    pub fn keeps_dir(&self, rel: &Path) -> bool {
        // `Path::join("")` is that empty child: "embedded" -> "embedded/".
        let below = rel.join("");
        rel.as_os_str().is_empty()
            || self.include.is_match(rel)
            || self.include.is_match(&below)
            || !(self.exclude.is_match(rel) || self.exclude.is_match(&below))
    }
}

/// A walk of the site root for files that are not content but *declare*
/// things about it: marker files (§4b) and the `.schema.toml` / `.section`
/// vocabulary walk (§5b, §6e).
///
/// It honours `.gitignore` and the declared `exclude`, but NOT the
/// dot/underscore skip — these walks are looking for dotfiles, and markers
/// live under `_posts`, so that skip would hide the very thing being sought.
///
/// `exclude` is applied to **directories only**, which makes the reachable
/// directory set a superset of the tree walk's. Pruning the subtree is what
/// these walks need from it: an embedded site under an excluded directory
/// (grack.com's `grackle/**`) must not put its declarations into its host's
/// vocabulary. A file-shaped pattern is a statement about *content* —
/// grack.com's `exclude` lists `*.toml` — and must not silently unspeak a
/// declaration, which is the same silent loss in the other direction.
///
/// Directory-only means `keeps_dir`, not `keeps`: the subtree's own root has
/// to be pruned with it, or the walk steps one level in and reads the
/// declaration sitting directly there (MERGE.md R2).
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
            Ok(rel) => not.keeps_dir(rel),
            Err(_) => true,
        }
    });
    b
}

/// Is `rel` on the way to, or inside, one of the declared scope sources?
///
/// The punch-through (IO.md I7d). The dot/underscore skip below is Jekyll's
/// and it survives — `_tools/`, `_hidden/`, `_includes/` are not content on
/// any site and nothing declares them — but a scope that says
/// `source = "_posts"` has declared that directory to be content, in the one
/// key that means that. So the skip asks the sources first, and DESIGN.md
/// §9b's "six underscore directories need explicit excludes" obstacle is
/// amended rather than paid: the five that no scope names stay out for free.
///
/// Both directions of containment matter and for different reasons: a source
/// itself (`rel == "_posts"`) and everything under it are admitted, and so is
/// every directory on the way DOWN to a nested source (`_a` for a
/// `source = "_a/_b"`), because a pruning walk that refuses the parent never
/// reaches the child. Whole components throughout — `_drafts_temp` is not
/// under `_drafts`, which a string prefix would get wrong.
fn punches_through(rel: &Path, sources: &[PathBuf]) -> bool {
    sources
        .iter()
        .any(|s| rel.starts_with(s) || s.starts_with(rel))
}

/// Walk the site root the way Jekyll does: skip dot- and underscore-prefixed
/// entries and anything matching `exclude`, unless `include` re-adds it — or
/// unless a collection declared the path as its `source` (see
/// [`punches_through`]).
///
/// **The one walk** (IO.md I7d). Every row the site loads comes from here:
/// what a scope's rules claim decides which table it lands in, and the walk
/// itself has no opinion about tables.
///
/// `.gitignore` is honoured underneath (see `walker`).
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
        // filter_entry prunes directories; re-check files that slipped through
        // via an ancestor being allowed. Unlike the declaration walk, a file
        // pattern does exclude a file here: this walk is the one deciding what
        // is content.
        if !not.keeps(&rel) {
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

// `load_dir` — the posts loader's own walk of one `source`, hydrating every
// `.md`/`.markdown` under it — is GONE (IO.md I7d). There is one walk, and
// the extension filter that lived in its argument list is a rule glob now
// (`match = "**/*.{md,markdown}"`), which is I7a's move one scope over: what a
// scope claims is what its rules say, in the one mechanism that also says
// where a claimed row lands. What that filter did by accident — a `.png`
// beside a draft is not a post, and not anything else either — is now the
// stated law: **a scope owns its source** (`load::walk_site`).

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
        // `embedded/**` matches the contents, not the directory — so `keeps`
        // says yes to `embedded` itself and a pruning walk steps one level in
        // (MERGE.md R2). `keeps_dir` is the one that closes it.
        let not = not_content(&["embedded/**"], &[]);
        assert!(not.keeps(Path::new("embedded")));
        assert!(!not.keeps_dir(Path::new("embedded")));
        assert!(!not.keeps_dir(Path::new("embedded/books")));
        assert!(not.keeps_dir(Path::new("books")));
        assert!(not.keeps_dir(Path::new("")));
    }

    #[test]
    fn a_file_shaped_pattern_still_does_not_prune_a_directory() {
        // R1's deliberate narrowing: grack.com excludes `*.toml`, which is a
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

    /// The punch-through compares whole COMPONENTS, both ways (IO.md I7d).
    ///
    /// grack.com is the site that makes each half matter: it has `_drafts` and
    /// `_drafts_temp` side by side, so a string prefix would walk the second
    /// because the first is declared — and the downward direction is what lets
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
