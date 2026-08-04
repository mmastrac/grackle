//! Engine asset URL addressing (DESIGN.md q54).
//!
//! One choke point turns an asset's *bytes* plus the site's `[assets]
//! addressing` choice into the URL its referrer names. Today it governs the
//! `<head>` stylesheet — the "global row": one URL, in every page. `stable`
//! keeps the human path (`/css/main.css`) and a revalidating cache, churning
//! nothing; `hashed` mints a content address under `/static` for immutable
//! caching, at the cost of rewriting every page's `<link>` when the bytes
//! move. The sub-asset / pinned / local rows are not knobs (they follow from
//! properties the engine already knows) and are not built here yet.
//!
//! The mechanism the design calls `asset()`: because a hashed URL is a
//! function of the compiled bytes, the sheet must be compiled BEFORE the pages
//! that link it render — the post-order the reference DAG implies. So the
//! stylesheets compile first, their URLs land in [`CssUrls`], and every
//! `<head>` reads the resolved URL from there instead of recomputing the
//! convention.

use anyhow::{bail, Result};
use std::collections::HashMap;

/// The site's addressing choice, parsed from `[assets] addressing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addressing {
    /// `/css/main.css` — human path, revalidating cache, no churn.
    Stable,
    /// `/static/{hash}.css` — content address, immutable cache, churns every
    /// page that links it.
    Hashed,
}

/// The content-addressing variant tag for a compiled stylesheet — part of the
/// hash key, so a rendition of the same bytes can never collide with it (the
/// §6b law, see `strong`).
const CSS_VARIANT: &str = "css-v1";

impl Addressing {
    /// Parse the raw config string. Unknown modes are a build error naming the
    /// vocabulary rather than a silent fall-through to `stable`.
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "stable" => Ok(Addressing::Stable),
            "hashed" => Ok(Addressing::Hashed),
            other => bail!(
                "[assets] addressing = {other:?} is not a mode; expected \"stable\" or \"hashed\""
            ),
        }
    }

    /// The URL a compiled stylesheet is emitted at and linked by, given the
    /// theme's stable convention URL and its compiled bytes. `stable` returns
    /// the convention untouched; `hashed` replaces it with the content
    /// address (baseurl-prefixed like the convention).
    pub fn css_url(self, baseurl: &str, stable: &str, bytes: &[u8]) -> String {
        match self {
            Addressing::Stable => stable.to_string(),
            Addressing::Hashed => {
                format!(
                    "{baseurl}{}",
                    grackle_source::strong::address(bytes, CSS_VARIANT, "css")
                )
            }
        }
    }
}

/// The resolved stylesheet URL of each theme, built once after the sheets
/// compile and read at render time — so every `<head>` links the exact URL the
/// bytes were emitted at, hash and all. Keyed by theme name, with the default
/// theme canonicalized to `None` (the convention treats `None` and
/// `Some("default")` alike).
#[derive(Debug, Default)]
pub struct CssUrls {
    map: HashMap<Option<String>, String>,
}

/// `Some("default")` and `None` are one theme; collapse them so a lookup by
/// either finds the same URL.
fn key(theme: Option<&str>) -> Option<String> {
    match theme {
        None | Some("default") => None,
        Some(n) => Some(n.to_string()),
    }
}

impl CssUrls {
    pub fn insert(&mut self, theme: Option<&str>, url: String) {
        self.map.insert(key(theme), url);
    }

    /// The URL for a theme's sheet. Falls back to the stable convention if a
    /// theme somehow never compiled one — it always does, but keeping this
    /// total beats a panic during render.
    pub fn of(&self, baseurl: &str, theme: Option<&str>) -> String {
        self.map
            .get(&key(theme))
            .cloned()
            .unwrap_or_else(|| crate::theme::css_url(baseurl, theme))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_is_the_convention_untouched() {
        let a = Addressing::parse("stable").unwrap();
        assert_eq!(a.css_url("", "/css/main.css", b"body{}"), "/css/main.css");
        // The bytes do not enter a stable URL, so an edit does not move it.
        assert_eq!(a.css_url("", "/css/main.css", b"other"), "/css/main.css");
    }

    #[test]
    fn hashed_is_the_content_address_and_moves_with_the_bytes() {
        let a = Addressing::parse("hashed").unwrap();
        let one = a.css_url("", "/css/main.css", b"body{}");
        assert!(
            one.starts_with("/static/") && one.ends_with(".css"),
            "{one}"
        );
        // A one-byte change is a different URL — this IS the churn q54 weighs.
        assert_ne!(one, a.css_url("", "/css/main.css", b"body{ }"));
        // Same bytes, same URL, regardless of the convention it replaces.
        assert_eq!(one, a.css_url("", "/css/other.css", b"body{}"));
    }

    #[test]
    fn baseurl_prefixes_the_hashed_address() {
        let a = Addressing::parse("hashed").unwrap();
        assert!(a.css_url("/blog", "/x", b"z").starts_with("/blog/static/"));
    }

    #[test]
    fn an_unknown_mode_is_a_build_error_naming_the_vocabulary() {
        let e = format!("{:#}", Addressing::parse("buster").unwrap_err());
        assert!(e.contains("stable") && e.contains("hashed"), "{e}");
    }

    #[test]
    fn the_default_theme_is_none_or_default() {
        let mut u = CssUrls::default();
        u.insert(None, "/static/abc.css".into());
        assert_eq!(u.of("", None), "/static/abc.css");
        assert_eq!(u.of("", Some("default")), "/static/abc.css");
    }
}
