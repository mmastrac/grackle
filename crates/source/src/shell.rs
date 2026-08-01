//! Shell axis (IO.md §4): map shells wrap one output; fold shells serialize a query.

use anyhow::{bail, Result};
use std::path::Path;

/// Map shells: one output in, one file out.
pub const MAP: &[&str] = &["raw", "html", "light_html"];

/// Built-in fold shells. Script shells register via `[shells.*]`.
pub const FOLD: &[&str] = &["atom", "sitemap", "search"];

/// Default when a view omits `shell =` (IO.md §3).
pub const VIEW_DEFAULT: &str = "html";

/// Map shells that wrap HTML documents. Named set so future shells must opt in explicitly.
pub const DOCUMENT: &[&str] = &["html", "light_html"];

pub fn is_map(name: &str) -> bool {
    MAP.contains(&name)
}

pub fn is_document(name: &str) -> bool {
    DOCUMENT.contains(&name)
}

/// Row renders iff it has a front-matter block or a document shell (IO.md §1, I7c).
/// Takes the block, not identity: sidecars grant identity without a block (IO.md I8).
pub fn renders(has_block: bool, shell: Option<&str>) -> bool {
    has_block || shell.is_some_and(is_document)
}

/// Identity-less row that still renders via a document shell (IO.md §1). Returns the shell for the warning.
pub fn degenerate(has_identity: bool, shell: Option<&str>) -> Option<&str> {
    shell.filter(|s| !has_identity && is_document(s))
}

pub fn is_fold(name: &str) -> bool {
    FOLD.contains(&name)
}

fn list(names: &[&str]) -> String {
    names.join(", ")
}

fn eats(name: &str) -> &'static str {
    match name {
        "atom" => "a feed's worth of entries",
        "sitemap" => "every published URL",
        "search" => "the searchable rows",
        _ => "a collection of outputs",
    }
}

pub fn check_row(name: &str, whose: &Path) -> Result<()> {
    if is_map(name) {
        return Ok(());
    }
    if is_fold(name) {
        bail!(
            "{}: shell = \"{name}\" is a fold shell — it eats {} and emits one \
             artifact, so it belongs on a view (`[routes.<name>] shell = \
             \"{name}\"`). A row is ONE output and takes a map shell: {} \
             (IO.md §4)",
            whose.display(),
            eats(name),
            list(MAP),
        );
    }
    bail!(
        "{}: shell = \"{name}\" is not a shell — a row takes {} (IO.md §4)",
        whose.display(),
        list(MAP),
    );
}

/// Per-member route shell check: axis values are map shells only.
pub fn check_axis_value(name: &str, axis: &str) -> Result<()> {
    if is_map(name) {
        return Ok(());
    }
    let why = if is_fold(name) {
        format!(
            " — that is a fold shell, and it eats {} rather than one document",
            eats(name)
        )
    } else {
        String::new()
    };
    bail!(
        "axis {axis:?} spends the `shell` field, so its values are the shells \
         its members leave through: \"{name}\" is not a map shell{why}. \
         Map shells: {} (IO.md §4)",
        list(MAP),
    );
}

/// The check a VIEW's `shell =` takes. `registered` is `[shells.*]`.
pub fn check_view(name: &str, view: &str, registered: &[&str]) -> Result<()> {
    if is_fold(name) || registered.contains(&name) {
        return Ok(());
    }
    let folds = format!(
        "fold shell: {}{} (IO.md §4)",
        list(FOLD),
        match registered.is_empty() {
            true => String::new(),
            false => format!("; registered script shells: {}", list(registered)),
        }
    );
    if is_map(name) {
        bail!(
            "view {view}: shell = \"{name}\" is a map shell — it wraps ONE \
             output, and a view's shell serializes the whole collection its \
             query selects. Leave it out for the HTML listing; a view takes a \
             {folds}"
        );
    }
    bail!("view {view}: unknown shell {name:?} — a view takes a {folds}");
}

/// View with no `from` (IO.md §4, I3). Only engine fold shells may omit `from`; script shells need a pool.
pub fn check_absent_from(shell: Option<&str>, view: &str, registered: &[&str]) -> Result<()> {
    if shell.is_some_and(is_fold) {
        return Ok(());
    }
    if let Some(s) = shell.filter(|s| registered.contains(s)) {
        bail!(
            "view {view}: shell = {s:?} is a script shell and has no `from`, \
             so it would be handed no rows at all. A script shell eats the row \
             projection its view selects — the payload's `rows` — and only the \
             engine's own folds ({}) read every output without a `from` \
             (IO.md §4). Name a pool: `from = \"<collection or set>\"`.",
            list(FOLD),
        );
    }
    bail!(
        "view {view}: no `from` — a listing has to say what it lists. Only a \
         FOLD shell reads every output without one (IO.md §4), and this one \
         leaves through {}, which wraps one output at a time. Name a pool \
         (`from = \"<collection or set>\"`), or declare a fold shell: {}",
        match shell {
            Some(s) => format!("{s:?}"),
            None => format!("the HTML listing ({VIEW_DEFAULT:?}, the default)"),
        },
        list(FOLD),
    )
}

/// Built-in names shadow registered script shells.
pub fn check_registered_name(name: &str) -> Result<()> {
    if is_map(name) || is_fold(name) {
        bail!(
            "[shells.{name}]: \"{name}\" is a built-in shell (map shells: {}; \
             fold shells: {}) — a script shell needs a name of its own, or the \
             built-in answers first and the command never runs (IO.md §4)",
            list(MAP),
            list(FOLD),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two families are disjoint, and the vocabulary is the union. A name
    /// in both would make `check_row` and `check_view` disagree about which
    /// error a typo gets.
    #[test]
    fn the_families_do_not_overlap() {
        for m in MAP {
            assert!(!is_fold(m), "{m} is in both families");
        }
        assert!(is_map(VIEW_DEFAULT), "the view default is a map shell");
    }

    /// The document family is the html half of [`MAP`]: `raw` wraps nothing,
    /// and a fold shell is not a row's to wear at all. Spelled as a
    /// partition so that adding a map shell (the future `md`) has to decide
    /// which side it falls on rather than defaulting to one.
    #[test]
    fn the_document_family_is_the_non_raw_map_shells() {
        for name in DOCUMENT {
            assert!(is_map(name), "{name} is not even a map shell");
        }
        assert!(!is_document("raw"), "raw wraps nothing");
        for f in FOLD {
            assert!(!is_document(f), "{f} is a fold");
        }
    }

    /// **The law** (I7c), in all four corners. The two that are not simply
    /// "identity decides" are the ones the corpus writes: a front-mattered
    /// `raw` row renders (field-notes' `demos/pane.html`), and an
    /// identity-less `html` row renders as the degenerate case (grack.com's
    /// caret draft).
    #[test]
    fn a_row_renders_iff_it_has_identity_or_a_document_shell() {
        assert!(renders(true, None));
        assert!(renders(true, Some("raw")));
        assert!(renders(false, Some("html")));
        assert!(renders(false, Some("light_html")));
        assert!(!renders(false, Some("raw")));
        assert!(!renders(false, None));
    }

    /// The second clause on its own, and the three ways not to be it: identity
    /// (an ordinary document), `raw` without identity (an ordinary byte row),
    /// and no shell at all.
    #[test]
    fn only_an_identity_less_document_shell_is_degenerate() {
        assert_eq!(degenerate(false, Some("html")), Some("html"));
        assert_eq!(degenerate(false, Some("light_html")), Some("light_html"));
        assert_eq!(degenerate(true, Some("html")), None);
        assert_eq!(degenerate(false, Some("raw")), None);
        assert_eq!(degenerate(false, None), None);
    }

    /// The retired spellings are hard cutoffs (MERGE.md §4): out of the
    /// vocabulary entirely, in both families.
    #[test]
    fn the_retired_spellings_are_gone() {
        for old in ["none", "light"] {
            assert!(!is_map(old) && !is_fold(old), "{old} still parses");
            assert!(check_row(old, Path::new("x.md")).is_err());
            assert!(check_view(old, "v", &[]).is_err());
        }
    }
}
