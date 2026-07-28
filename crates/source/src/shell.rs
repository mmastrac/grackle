//! The shell axis (IO.md §4): one vocabulary, one validator, two families.
//!
//! A shell is a function from content to final bytes. Until IO.md I2 the
//! engine held two vocabularies behind an artificial wall — a ROW tier ladder
//! (`none`/`light`/`html`, checked in `load::cascade`) and a VIEW
//! serialization set (`atom`/`sitemap`/`search` plus `[shells.*]` script
//! shells, checked in `Config::check`). Nothing joined them, so `shell` meant
//! two things in one word and neither checker knew the other's values.
//!
//! They were never two axes. They are one axis split by **arity**:
//!
//! - **map shells** consume ONE output and emit one file each: [`MAP`].
//! - **fold shells** sit on a query over outputs, consume the collection, and
//!   emit one artifact: [`FOLD`], plus every registered script shell.
//!
//! Arity is a hard contract, and it is what the two checks below enforce: a
//! row (or a per-member route) is one output, so it takes a map shell; a view
//! is a query, so it takes a fold shell. Identity is the SOFT contract — an
//! identity-less file under `html` becomes a degenerate row (IO.md §1, Matt
//! 2026-07-27), and since I7c that softness is stated here too, as
//! [`renders`]: the one law that says whether a row is a document at all.
//!
//! The retired spellings (`none` → `raw`, `light` → `light_html`) get no
//! teaching error: MERGE.md §4 makes retired spellings hard cutoffs, and no
//! site ships. They are simply not in the vocabulary, and the error naming the
//! knowns is the only thing a typo gets.

use anyhow::{bail, Result};
use std::path::Path;

/// Map shells: one output in, one file out. Legal on a row, and on a
/// per-member route (an axis over `shell` is one row serialized several ways —
/// each member is still one output).
///
/// `raw` is the transparent one: it emits the output verbatim, wrapper-free,
/// and is what today's static passthrough and object byte-copies are.
/// `light_html` is the html shell with no theme root merged.
pub const MAP: &[&str] = &["raw", "html", "light_html"];

/// Fold shells the engine ships. A site adds more by registering
/// `[shells.<name>] command = "…"`, which is why every fold check takes the
/// registered names beside these.
pub const FOLD: &[&str] = &["atom", "sitemap", "search"];

/// What a view route with no `shell =` leaves through (IO.md §3: `shell` is
/// "the serialization it left through", and a listing leaves through HTML).
///
/// It is a MAP shell name on a FOLD-shaped declaration, and that is not a
/// contradiction: an undeclared view materializes one HTML document per
/// route — `paginate` and `group_by` fan it out — so the arity of what it
/// EMITS is one file per output, the map arity. The declaration slot is
/// reserved for folds because that is the only thing a view can say that the
/// route set does not already answer.
pub const VIEW_DEFAULT: &str = "html";

/// The **document family**: the map shells that wrap an output in an HTML
/// document. A subset of [`MAP`] — `raw` is the transparent one and wraps
/// nothing — and the set [`renders`] reads.
///
/// It is a named set rather than two literals because the law below is a
/// statement about a FAMILY: the future `md` shell will have to decide whether
/// it joins, and a `matches!(s, "html" | "light_html")` at the one call site
/// would let it join by omission.
pub const DOCUMENT: &[&str] = &["html", "light_html"];

pub fn is_map(name: &str) -> bool {
    MAP.contains(&name)
}

pub fn is_document(name: &str) -> bool {
    DOCUMENT.contains(&name)
}

/// **The rendering law** (IO.md §1 and §4, I7c): a row renders iff it carries
/// identity, or a rule sent it through a document shell.
///
/// Both halves are load-bearing and neither implies the other, which is why the
/// law is a disjunction rather than either clause on its own:
///
/// - **`front_mattered` alone is not enough to say `raw`.** A front-mattered
///   file wearing `shell = "raw"` is a document the pipeline renders and the
///   `raw` shell then emits verbatim — `examples/field-notes`' `demos/pane.html`
///   is the live one. A law spelled "the shell decides" would byte-copy it, and
///   what a byte copy of a front-mattered file ships is the `---` block.
/// - **`shell` alone is not enough to say "no identity, no document".** That
///   second clause IS the degenerate row (IO.md §1): an identity-less file that
///   rules send through `html`/`light_html` renders anyway — a warning, never an
///   error, with its title implied from its slug. `_drafts/caret/…` on grack.com
///   is the corpus's one.
///
/// An identity-less file routed `raw` is the ordinary byte row, and gets no
/// warning: that is the normal case, not a degenerate one.
pub fn renders(front_mattered: bool, shell: Option<&str>) -> bool {
    front_mattered || shell.is_some_and(is_document)
}

/// The law's second clause standing alone: the row that renders WITHOUT
/// identity — IO.md §1's **degenerate row**. Hands back the document shell that
/// made it one, so the warning can name it.
///
/// A predicate would have been enough for the branch and not for the message,
/// and the message is most of what a warning is. It also removes the corner
/// where a caller has to default a shell name it can prove is present.
pub fn degenerate(front_mattered: bool, shell: Option<&str>) -> Option<&str> {
    shell.filter(|s| !front_mattered && is_document(s))
}

pub fn is_fold(name: &str) -> bool {
    FOLD.contains(&name)
}

fn list(names: &[&str]) -> String {
    names.join(", ")
}

/// What a fold shell eats, for the error a row wearing one gets (IO.md §4's
/// sentence: "a row wearing `shell = atom` is a load error naming what atom
/// eats").
fn eats(name: &str) -> &'static str {
    match name {
        "atom" => "a feed's worth of entries",
        "sitemap" => "every published URL",
        "search" => "the searchable rows",
        _ => "a collection of outputs",
    }
}

/// The check a ROW's `shell` takes, wherever the cascade produced it — front
/// matter, a marker, or a rule default.
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

/// The check a per-member route's shell takes: an `[axes.*]` whose `field` is
/// `shell` declares the serializations its members leave through, and a member
/// is one output, so the values are map shells.
///
/// Without this the axis values would reach `build.rs` unchecked and a retired
/// or misfamilied one would render the WRONG TIER in silence — the exact
/// failure `load::cascade`'s check has always existed to stop, on the one path
/// that never went through it.
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

/// The check a view with **no `from`** takes (IO.md §4, I3).
///
/// "A fold shell with no `from` reads all outputs" is §4's sentence, and it is
/// the successor to the retired `from = "*"` — a respelling, not a new power:
/// the pool a star view read is the pool this reads.
///
/// Absent-`from` is legal exactly where the shell is a fold, because the two
/// families answer the question differently. A fold sits on a query over
/// outputs, so "all of them" is a query it can serialize. A map shell wraps
/// ONE output, so a view wearing one is a listing, and a listing has to say
/// what it lists — including the undeclared view, whose shell is
/// [`VIEW_DEFAULT`].
///
/// `registered` is `[shells.*]`, as in [`check_view`]: a script shell is a
/// fold by arity — but only the ENGINE's folds may be `from`-less, which is
/// the narrowing IR1(a) lands. The engine's folds read the output pool
/// themselves (`resolve_pool_folds` fills `route_members`); a script shell is
/// fed the row projection its view selected, so a `from`-less one is handed
/// `rows: []` and publishes an empty payload at a URL the author asked a query
/// for — the same silent-empty disease one family over. IO.md §4 gives a
/// script shell a `pulls = "inputs" | "outputs"` declaration later; until it
/// lands, a script shell eats rows and has to be told which ones.
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

/// A registered script shell may not take a name the engine already owns.
///
/// It would be a shell nobody could reach: `check_view` answers from the
/// built-in vocabulary first, so `[shells.atom]` would be shadowed and
/// `[shells.html]` would be rejected as a map shell — either way the command
/// never runs and nothing says so.
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
