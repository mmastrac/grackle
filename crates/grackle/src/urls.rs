//! URL-set parity against a reference build (§4).
//!
//! §4 has always called URL parity "a hard requirement (`grackle diff`)", but
//! `diff` compares post *bodies* — it extracts `<article>` and never looks at
//! the URL set at all. The requirement had no instrument. This is it.
//!
//! The asymmetry between the two answers is the whole point, and it is why
//! this is not a set difference printed twice:
//!
//! - A **missing** URL is a link that used to resolve and now 404s. On a
//!   20-year corpus whose canonical/indexing work depends on the URL set not
//!   shifting, that is the failure this check exists to prevent, and it exits
//!   non-zero.
//! - An **extra** URL is usually just content published since the reference
//!   was built. It is reported, never fatal.
//!
//! The reference is a directory of built output — `_site-prod`, or a tree
//! rsynced down from the live server. Both are the same shape, which is what
//! lets this check outlive the Jekyll build that produced the first one.
//!
//! **Derived assets are exempt, per q12.** Thumbnails moved from Jekyll's
//! `/_thumbs/{md5}-600-600` to content-addressed `/static/{hash}.{ext}`, and
//! that was a deliberate decision: URL parity stays hard for *pages* and is
//! waived for derived output. Without the waiver this check reports 262
//! missing URLs on a correct build, which is the fastest way to teach someone
//! to ignore it.

use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;

/// Map a built file to the URL a server would answer it at.
///
/// `index.html` is the directory itself (`blog/2022/index.html` → `/blog/2022/`
/// — the pretty-URL convention both builds use); everything else is its own
/// path. Returns `None` for files that are not addressable content.
fn url_of(rel: &Path) -> Option<String> {
    let s = rel.to_str()?.replace('\\', "/");
    if s.starts_with('.') || s.contains("/.") {
        return None;
    }
    Some(match s.strip_suffix("index.html") {
        // "index.html" at the root is "/", not "".
        Some(dir) => format!("/{dir}"),
        None => format!("/{s}"),
    })
}

/// Is this URL exempt from parity — a derived asset (q12) rather than a page?
pub fn exempt(url: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| url.starts_with(p.as_str()))
}

/// Filter a URL set to the addressable, parity-bearing ones. Both sides go
/// through this, so an exemption or a dotfile can never count as a difference
/// on one side only.
pub fn parity_set<I: IntoIterator<Item = String>>(urls: I, exempt_prefixes: &[String]) -> BTreeSet<String> {
    urls.into_iter()
        .filter(|u| {
            !exempt(u, exempt_prefixes) && !u.rsplit('/').next().is_some_and(|f| f.starts_with('.'))
        })
        .collect()
}

/// Every URL a directory of built output would serve.
pub fn urls_in_dir(root: &Path) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for e in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !e.file_type().is_file() {
            continue;
        }
        let rel = e
            .path()
            .strip_prefix(root)
            .with_context(|| format!("{} is not under {}", e.path().display(), root.display()))?;
        if let Some(u) = url_of(rel) {
            out.insert(u);
        }
    }
    Ok(out)
}

/// What the comparison found. Kept as data so the caller owns presentation and
/// the exit code.
pub struct Parity {
    pub missing: Vec<String>,
    pub extra: Vec<String>,
    pub shared: usize,
}

impl Parity {
    pub fn compare(ours: &BTreeSet<String>, reference: &BTreeSet<String>) -> Parity {
        Parity {
            missing: reference.difference(ours).cloned().collect(),
            extra: ours.difference(reference).cloned().collect(),
            shared: ours.intersection(reference).count(),
        }
    }

    /// Missing URLs are the failure; extras are news.
    pub fn ok(&self) -> bool {
        self.missing.is_empty()
    }

    pub fn report(&self, limit: usize) {
        println!("  shared    {}", self.shared);
        println!("  missing   {} (in the reference, not in ours)", self.missing.len());
        for u in self.missing.iter().take(limit) {
            println!("              {u}");
        }
        if self.missing.len() > limit {
            println!("              … and {} more", self.missing.len() - limit);
        }
        println!("  extra     {} (ours, not in the reference)", self.extra.len());
        for u in self.extra.iter().take(limit) {
            println!("              {u}");
        }
        if self.extra.len() > limit {
            println!("              … and {} more", self.extra.len() - limit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_index_is_its_directory() {
        assert_eq!(url_of(Path::new("blog/2022/index.html")).unwrap(), "/blog/2022/");
        assert_eq!(url_of(Path::new("index.html")).unwrap(), "/");
        assert_eq!(url_of(Path::new("atom.xml")).unwrap(), "/atom.xml");
        assert_eq!(
            url_of(Path::new("code/legacy/nnet/nnet")).unwrap(),
            "/code/legacy/nnet/nnet"
        );
    }

    /// Dotfiles are build residue, not addressable content — `.htaccess` and
    /// friends would otherwise read as URLs that one side is always missing.
    #[test]
    fn dotfiles_are_not_urls() {
        assert!(url_of(Path::new(".htaccess")).is_none());
        assert!(url_of(Path::new("_thumbs/.htaccess")).is_none());
    }

    /// q12: the thumbnail scheme changed on purpose, so neither side's
    /// derived assets may count as a parity difference.
    #[test]
    fn derived_assets_are_exempt_on_both_sides() {
        let ex = vec!["/static/".to_string(), "/_thumbs/".to_string()];
        let ours = parity_set(
            ["/a/".to_string(), "/static/abc.jpg".to_string()],
            &ex,
        );
        let reference = parity_set(
            ["/a/".to_string(), "/_thumbs/def-600-600".to_string()],
            &ex,
        );
        let p = Parity::compare(&ours, &reference);
        assert!(p.missing.is_empty() && p.extra.is_empty(), "thumbs leaked into parity");
        assert_eq!(p.shared, 1);
    }

    /// A dotfile on either side is build residue, not a URL.
    #[test]
    fn dotfiles_drop_from_both_sides() {
        let s = parity_set(["/.htaccess".to_string(), "/a/".to_string()], &[]);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn missing_fails_and_extra_does_not() {
        let reference = set(&["/a/", "/b/"]);

        let dropped = Parity::compare(&set(&["/a/"]), &reference);
        assert_eq!(dropped.missing, vec!["/b/"]);
        assert!(!dropped.ok(), "a dropped URL must fail the check");

        let added = Parity::compare(&set(&["/a/", "/b/", "/c/"]), &reference);
        assert_eq!(added.extra, vec!["/c/"]);
        assert!(added.ok(), "new content must not fail the check");
        assert_eq!(added.shared, 2);
    }
}
