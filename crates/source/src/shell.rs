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
//! 2026-07-27) and is I7's business, not this module's. Nothing here looks at
//! front matter.
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

pub fn is_map(name: &str) -> bool {
    MAP.contains(&name)
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
