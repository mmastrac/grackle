//! Body markup without engine policy.
//!
//! Markdown AST work and the liquid-shaped tag *scan* live here. What a tag
//! *means* (views, thumbs, link resolution) is supplied by callbacks from the
//! engine — so this module never names `SiteDb`, `Config`, or a theme.

pub mod scan;

/// Link vs embed — the only distinction the markdown/HTML rewriters need.
/// Policy for what each form means lives in [`crate::links`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cite {
    /// `[text](x)`, `<a href>` — an address a human may bookmark.
    Link,
    /// `![alt](x)`, `<img src>`, `<iframe src>` — bytes the page pulls in.
    Embed,
}
