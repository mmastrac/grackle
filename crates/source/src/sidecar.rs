//! Sidecar files: identity for a file that cannot carry a block (IO.md I8).
//!
//! IO.md §1 says identity comes from front matter — "a literal block, or a
//! sidecar file" — and §3 says what the second spelling buys: **sidecars split
//! identity from parsing**. A `.png` with a sidecar is a governed row (declared
//! fields, schema-validated, a title, a place in the link graph) whose bytes are
//! never parsed. Which facts a file has and what the pipeline does with it are
//! separate questions with separate answers.
//!
//! **The spelling is the pair, not the name.** `photo.png.toml` beside
//! `photo.png` is that file's sidecar; a `x.toml` with no `x` beside it is an
//! ordinary file, content like any other. Identifying a sidecar by its
//! *companion* rather than by a name pattern is what keeps `Cargo.toml`,
//! `netlify.toml` and `.schema.toml` out of the mechanism without a list of
//! exceptions to maintain — and DESIGN.md q49's rider ("what this must NOT do:
//! infer page-vs-component from absence") is the reason it is a pair test and
//! not a heuristic over the stem.
//!
//! **A sidecar is a declaration, so it is read on the declaration walk** —
//! `store::walker_declarations`, beside markers and `.schema.toml`. That
//! placement is a decision (IO.md IR6's world) and it is load-bearing: a
//! file-shaped `exclude` pattern is a statement about *content*, and grack.com's
//! `exclude` lists `*.toml`. Reading sidecars on the content walk would let that
//! one line silently unspeak every sidecar on the site — MERGE.md R1's narrowing,
//! at the newest declaration family.

use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::store::FrontMatter;

/// What a sidecar's name adds to the file it speaks for.
const SUFFIX: &str = ".toml";

/// The file `path` is a sidecar FOR, if its name has the sidecar shape.
///
/// Purely lexical — whether that file exists is the caller's question, and it
/// is the one that decides whether this is a sidecar at all.
fn companion(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(SUFFIX)?;
    // `.toml` alone speaks for nothing; `.schema.toml` speaks for `.schema`,
    // which no site has, so the pair test declines it without naming it.
    (!stem.is_empty()).then(|| path.with_file_name(stem))
}

/// One sidecar, parsed.
pub struct Sidecar {
    /// The sidecar file itself — absolute, so a schema error names the file the
    /// author has to edit rather than the file it speaks for.
    pub path: PathBuf,
    /// Exactly what a `---` block would have said (see [`FrontMatter`]).
    pub front: FrontMatter,
    /// The sidecar's own change stamp, folded into its row's `version` so that
    /// editing a sidecar invalidates the row it governs. A row's identity lives
    /// in this file; a `version` that ignored it would report a row unchanged
    /// after its title changed.
    pub version: u64,
}

/// Every sidecar the declaration walk found, keyed by the file it speaks for.
#[derive(Default)]
pub struct Sidecars {
    /// Root-relative content path -> its sidecar.
    by_file: HashMap<PathBuf, Sidecar>,
    /// Root-relative sidecar paths, so the content walk can refuse to route
    /// them — the statement `Markers::is_marker` makes one declaration family
    /// over. Positional in the same sense: what makes this file not content is
    /// where it sits (beside the file it declares), not what it is called.
    files: HashSet<PathBuf>,
    pub found: usize,
}

impl Sidecars {
    /// Offer one file found by the declaration walk. Returns whether it was a
    /// sidecar.
    ///
    /// `path` is absolute (the walk's), `rel` root-relative.
    pub fn offer(&mut self, path: &Path, rel: &Path) -> Result<bool> {
        let (Some(abs), Some(companion)) = (companion(path), companion(rel)) else {
            return Ok(false);
        };
        if !abs.is_file() {
            return Ok(false);
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let front: FrontMatter =
            toml::from_str(&text).with_context(|| format!("sidecar {}", rel.display()))?;
        let meta =
            std::fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
        self.files.insert(rel.to_path_buf());
        self.by_file.insert(
            companion,
            Sidecar {
                path: path.to_path_buf(),
                front,
                version: crate::store::version_of(&meta),
            },
        );
        self.found += 1;
        Ok(true)
    }

    /// The sidecar speaking for a row, given its **root-relative** path.
    pub fn get(&self, rel: &Path) -> Option<&Sidecar> {
        self.by_file.get(rel)
    }

    /// Is this file a sidecar — i.e. a declaration rather than content?
    pub fn is_sidecar(&self, rel: &Path) -> bool {
        self.files.contains(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lexical half of the pair rule. The existence half is what the
    /// fixtures test; this is what says which names are even candidates.
    #[test]
    fn a_sidecar_speaks_for_the_name_it_extends() {
        assert_eq!(
            companion(Path::new("a/photo.png.toml")),
            Some(PathBuf::from("a/photo.png"))
        );
        assert_eq!(
            companion(Path::new("notes.html.toml")),
            Some(PathBuf::from("notes.html"))
        );
        // The two `.toml` files every site has, and the one the engine ships:
        // each names a file that does not exist, so the pair test declines
        // them without an exception list.
        assert_eq!(
            companion(Path::new("grackle.toml")),
            Some(PathBuf::from("grackle"))
        );
        assert_eq!(
            companion(Path::new("a/.schema.toml")),
            Some(PathBuf::from("a/.schema"))
        );
        // Not a candidate at all.
        assert_eq!(companion(Path::new("photo.png")), None);
        assert_eq!(companion(Path::new("a/.toml")), None);
    }

    /// A sidecar's payload is a front-matter block in TOML, and this is the
    /// claim that makes it so: the same struct, every named field, plus
    /// `extra` for the declared ones.
    #[test]
    fn a_sidecar_deserializes_the_front_matter_struct() {
        let front: FrontMatter = toml::from_str(
            "title = \"A photo\"\nshell = \"raw\"\ntags = [\"x\", \"y\"]\n\
             date = \"2020-01-02\"\nalt = \"a cat\"\nwidth_hint = 3\n",
        )
        .expect("a sidecar is TOML in the shape of front matter");
        assert_eq!(front.title.as_deref(), Some("A photo"));
        assert_eq!(front.shell.as_deref(), Some("raw"));
        assert_eq!(
            front.extra.get("date").and_then(|v| v.as_str()),
            Some("2020-01-02")
        );
        // tags travels in `extra` with the other declared fields; schema
        // validate coerces the list.
        assert_eq!(front.extra.len(), 4);
        assert!(front.extra.contains_key("tags"));
        assert_eq!(
            front.extra.get("alt").and_then(|v| v.as_str()),
            Some("a cat")
        );
        assert_eq!(
            front.extra.get("width_hint").and_then(|v| v.as_i64()),
            Some(3)
        );
    }
}
