//! The base theme, compiled into the binary (THEME.md §5).
//!
//! Themes replace fragments by name; undeclared kinds keep the base. Sites
//! with no `themes/` *are* this theme. Fragments exist because `canonical()`
//! cannot pair label+url without them; the sheet selects only on engine
//! vocabulary so it lands on base, partial themes, and the null fallback.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// `(fragment name, source)`. Explicit rather than a directory walk: the list
/// is the manifest, and a file that goes missing is a compile error.
const FRAGMENTS: &[(&str, &str)] = &[
    ("root", include_str!("../../assets/base/root.html")),
    ("row", include_str!("../../assets/base/row.html")),
    // Default member face: titled preview with optional date/tags/note/body.
    (
        "row--card",
        include_str!("../../assets/base/row--card.html"),
    ),
    // A row that IS its picture (hero + gallery members).
    (
        "row--figure",
        include_str!("../../assets/base/row--figure.html"),
    ),
    (
        "row--link",
        include_str!("../../assets/base/row--link.html"),
    ),
    // Smaller kinds (crumb, tag, page_link, outline_entry, pagination,
    // relation, neighbor, axis, axis_member) live inline under root/row
    // (THEME.md §5).
];

/// Dev-only: read the base from `assets/base/` on disk instead of from the
/// bytes compiled in. `serve` turns this on when the source tree it was
/// built from still exists, which restores the edit-reload loop for the one
/// directory that gets edited most while the floor settles — without it, a
/// one-character change to the base needs a `cargo build` before the preview
/// moves. Off in every released binary, because `option_env!` is `None` for
/// anything not built from source and the directory check fails anyway.
///
/// It leaks each re-read, deliberately: the signatures downstream are
/// `&'static str` (as `PartMap` keys already are), the leak is bounded by
/// how many times a human saves a file, and the alternative is threading
/// lifetimes through the whole theme loader for a dev convenience.
static DEV_SOURCE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Enable the disk path if this binary was built from a source tree that is
/// still present. Returns whether it took effect, so `serve` can say so.
pub fn use_source_tree_if_present() -> bool {
    let dir = option_env!("CARGO_MANIFEST_DIR")
        .map(|d| Path::new(d).join("assets/base"))
        .filter(|d| d.is_dir());
    let on = dir.is_some();
    let _ = DEV_SOURCE.set(dir);
    on
}

/// The live base directory, when the dev path is on — `serve` watches it, so
/// that editing the floor rebuilds the site the same way editing a theme does.
pub fn dev_source() -> Option<&'static Path> {
    DEV_SOURCE.get()?.as_deref()
}

fn from_disk(file: &str) -> Option<&'static str> {
    let dir = DEV_SOURCE.get()?.as_ref()?;
    let text = std::fs::read_to_string(dir.join(file)).ok()?;
    Some(Box::leak(text.into_boxed_str()))
}

pub fn fragments() -> &'static [(&'static str, &'static str)] {
    if DEV_SOURCE.get().is_some_and(Option::is_some) {
        let live: Vec<(&'static str, &'static str)> = FRAGMENTS
            .iter()
            .map(|(n, embedded)| (*n, from_disk(&format!("{n}.html")).unwrap_or(embedded)))
            .collect();
        return Box::leak(live.into_boxed_slice());
    }
    FRAGMENTS
}

/// The base stylesheet's partials, keyed the way `@import "x"` names them, so
/// the base's own `theme.scss` resolves its imports with no disk underneath.
const PARTIALS: &[(&str, &str)] = &[
    ("tokens", include_str!("../../assets/base/_tokens.scss")),
    ("base", include_str!("../../assets/base/_base.scss")),
    ("search", include_str!("../../assets/base/_search.scss")),
    // The ladder: imported by the base's own sheet, always. Exposed here
    // too so a theme that wants it inside its OWN layer can re-import it.
    ("type", include_str!("../../assets/base/_type.scss")),
    // Opt-in: NOT imported by the base's own sheet. A theme takes the code
    // panel, blockquote rule and table borders with `@import "skin";`.
    ("skin", include_str!("../../assets/base/_skin.scss")),
];

const SHEET: &str = include_str!("../../assets/base/theme.scss");

/// A partial by the name an `@import` would use — how a THEME reaches the
/// base's partials (`@import "base"` in a theme with no `_base.scss` of its
/// own resolves here, so a theme need not carry a reset it did not write).
pub fn partial(name: &str) -> Option<&'static str> {
    if let Some(live) = from_disk(&format!("_{name}.scss")) {
        return Some(live);
    }
    PARTIALS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, src)| *src)
}

/// Compiled base stylesheet (`@layer base`). Ladder always; skins (`with_skin`)
/// opt-in — measured inert under grack.com's own type sheet, skins are not.
pub fn css(with_skin: bool) -> &'static str {
    static NO_SKIN: OnceLock<String> = OnceLock::new();
    static WITH_SKIN: OnceLock<String> = OnceLock::new();
    let compile = || {
        let sheet = from_disk("theme.scss").unwrap_or(SHEET);
        let src = if with_skin {
            sheet.replace("@import \"type\";", "@import \"type\";\n@import \"skin\";")
        } else {
            sheet.to_string()
        };
        match grass::from_string(inline(&src), &grass::Options::default()) {
            Ok(css) => css,
            // An engine asset that does not compile is a build-time mistake,
            // not a site's problem — but a panic here would take down every
            // build, so it degrades to an unstyled page and says why.
            Err(e) => {
                eprintln!("grackle: the base stylesheet failed to compile: {e}");
                String::new()
            }
        }
    };
    // Memoized normally — the base cannot change under a running build. Under
    // the dev source tree it can, so recompile and leak: hot reload is the
    // whole point of that mode.
    if DEV_SOURCE.get().is_some_and(Option::is_some) {
        return Box::leak(compile().into_boxed_str());
    }
    let slot = if with_skin { &WITH_SKIN } else { &NO_SKIN };
    slot.get_or_init(compile)
}

/// Resolve the base sheet's own `@import "x"` lines against `PARTIALS`.
/// Deliberately the same shape as `build::inline_imports`, minus the disk.
fn inline(src: &str) -> String {
    let mut out = String::with_capacity(src.len() * 2);
    for line in src.lines() {
        let name = line
            .trim()
            .strip_prefix("@import")
            .map(|r| r.trim().trim_end_matches(';').trim())
            .filter(|r| r.starts_with('"') && r.ends_with('"'))
            .map(|r| r.trim_matches('"'))
            .and_then(partial);
        match name {
            Some(text) => {
                out.push_str(text);
                out.push('\n');
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The base is an engine asset, so its failure mode is a compile error or
    /// a silent empty sheet — neither of which a site build would report.
    #[test]
    fn the_base_stylesheet_compiles() {
        for with_skin in [false, true] {
            let css = css(with_skin);
            assert!(!css.is_empty(), "base stylesheet compiled to nothing");
            assert!(
                css.contains("--measure"),
                "base stylesheet should bind the token vocabulary: {}",
                &css[..css.len().min(200)]
            );
            // The ladder is unconditional; only the decorative half waits
            // to be asked for. That asymmetry is the whole split.
            assert!(
                css.contains("text-wrap: balance"),
                "the heading ladder always ships"
            );
            assert_eq!(
                css.contains("border-left: calc(var(--border) * 3)"),
                with_skin,
                "the skins ship iff asked for (with_skin={with_skin})"
            );
        }
    }

    /// Every fragment the base ships must name a real kind and parse. Loading
    /// it is the assertion; `Fragments::load` errors on anything malformed.
    #[test]
    fn the_base_fragments_load() {
        let schemas = crate::parts::Schemas::engine_only();
        let sources: Vec<_> = FRAGMENTS
            .iter()
            .map(|(n, s)| (n.to_string(), s.to_string(), format!("<base>/{n}.html")))
            .collect();
        crate::assemble::Fragments::load(sources, &schemas).expect("base fragments load");
    }
}
