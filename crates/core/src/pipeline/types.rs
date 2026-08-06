//! Shared pipeline types.

use std::collections::BTreeMap;

use crate::markdown::Doc;

/// The rendered site, keyed by URL (`/blog/`, `/atom.xml`, `/css/main.css`,
/// `/static/{hash}.jpg`, …). A directory URL ends in `/` and, on disk, becomes
/// that directory's `index.html`.
pub type SiteOutput = BTreeMap<String, Vec<u8>>;

#[derive(Default)]
pub struct Stats {
    pub posts: usize,
    pub pages: usize,
    pub listings: usize,
    pub copied: usize,
    pub css: usize,
    pub serialized: usize,
    /// Distinct derived thumbnails published under `/static/`.
    pub thumbs: usize,
    /// Rows published because something referenced them.
    pub on_demand: usize,
    pub skipped: Vec<String>,
    /// Posts whose embeddings are missing or stale. The caller decides
    /// when to run the model: `build` before rendering, `serve` in the
    /// background with a re-render on completion.
    pub embed_pending: Vec<crate::embed::Pending>,
    pub search_bytes: usize,
    /// Stylesheets that failed to compile. Collected rather than thrown so
    /// the two callers can disagree: `serve` prints and keeps the loop
    /// alive, `build` refuses to publish. A site whose CSS silently failed
    /// looks deployable and is wrong, which is the one outcome worth
    /// failing a build over.
    pub css_errors: Vec<String>,
    /// CSS complaints that are not failures, today, the one about a
    /// `_tokens.scss` nothing imports. Printed as they happen (`serve` shows
    /// them on every rebuild); collected here only so a test can assert on
    /// SILENCE, which is what a warning that lies is fixed into. Nothing
    /// reads this to decide anything.
    pub css_warnings: Vec<String>,
}

/// A rendered page body: the expanded fragment plus its Doc (markdown
/// pages) for outline extraction. Computed BEFORE any page is themed so
/// the link graph can scan every body first.
pub struct PageBody {
    pub(crate) frag: String,
    pub(crate) doc: Option<Doc>,
    /// An unimplemented construct survived expansion; the page is skipped.
    pub(crate) skipped: bool,
}

/// One backlink: `(source title, source url, source date)`. The date rides
/// along so a backlink list can be read in date order.
pub(crate) type Backlink = (String, String, Option<chrono::NaiveDate>);
