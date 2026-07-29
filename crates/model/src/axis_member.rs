//! One route's membership of an axis (q53).

use serde::Serialize;

/// One route's membership of an axis (q53): which axis, which value, and the
/// row field that value sets while rendering.
///
/// `field` is carried rather than looked up so the render paths need no handle
/// on the config to ask "what does this member wear" — the same reason a
/// group key carries its params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxisMember {
    pub axis: String,
    pub value: String,
    pub field: String,
    /// The first-declared member, which is what `rel="canonical"` names and the
    /// only one a `*` view sees. An alternate is not a duplicate; it is what
    /// `rel="alternate"` is for.
    pub canonical: bool,
}
