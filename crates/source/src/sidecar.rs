//! Sidecar files: TOML identity beside a companion file.
//! Read on the declaration walk so `*.toml` excludes do not drop them.

use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::store::FrontMatter;

const SUFFIX: &str = ".toml";

/// Lexical companion path; existence is the caller's check.
fn companion(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(SUFFIX)?;
    // `.toml` alone speaks for nothing; `.schema.toml` speaks for `.schema`.
    (!stem.is_empty()).then(|| path.with_file_name(stem))
}

pub struct Sidecar {
    /// Absolute path for schema errors (the file the author edits).
    pub path: PathBuf,
    pub front: FrontMatter,
    /// Folded into the row's `version` when identity lives here.
    pub version: u64,
}

#[derive(Default)]
pub struct Sidecars {
    by_file: HashMap<PathBuf, Sidecar>,
    /// Sidecar paths the content walk must not route.
    files: HashSet<PathBuf>,
    pub found: usize,
}

impl Sidecars {
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

    pub fn get(&self, rel: &Path) -> Option<&Sidecar> {
        self.by_file.get(rel)
    }

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
