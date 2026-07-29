//! Axis a row's route rule spends (q53).

use serde::Serialize;

/// The axis a ROW's route rule spends (q53 step 2) — the rule's template writes
/// a `{theme}` (or `{axis:theme}`) segment, and that is what opts its rows in.
///
/// `[axes.*]` declares values and a field; the URL shape lives where every other
/// URL shape lives. `Row.url` is the CANONICAL member's, so links, `by_url` and
/// every reader that wants "the address of this row" get the right answer
/// without knowing an axis exists.
///
/// The name is all this carries. It used to carry the rule's first template too,
/// which was an arbitrary pick out of a list and the seed of MERGE.md C5's bug —
/// a member's address is LOOKED UP in the routes the build issued
/// (`LinkSpace::member_url`), never rebuilt from a template. Nothing read the
/// field after C5, so F3 dropped it.
#[derive(Debug, Clone, Serialize)]
pub struct RowAxis {
    pub name: String,
}
