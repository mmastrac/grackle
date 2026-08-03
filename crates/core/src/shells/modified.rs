//! The last-modified instant of a row, and of a set of rows.
//!
//! One source the sitemap's per-URL `<lastmod>` and the feed's `<updated>`
//! both read, so the two agree. It is `git.lastmod` (the last commit to touch
//! the file) when `metadata = ["git"]` filled it, else the published `date`.
//! Either keeps a build a pure function of its inputs, with no wall-clock.

use chrono::NaiveDate;
use grackle_db::filter::{Row as _, Value};

use crate::model::{parse_date_str, Row};

/// A row's last-modified date, or `None` if it carries no date. Git's
/// commit date (last *edit*) beats the front-matter `date` (publication) when
/// the provider supplied it.
pub(crate) fn of(p: &Row) -> Option<NaiveDate> {
    if let Value::Str(s) = p.field("git.lastmod") {
        if let Some(d) = parse_date_str(&s) {
            return Some(d);
        }
    }
    p.as_date("date")
}

/// The newest last-modified across a set of rows. `None` when none carries a date.
pub(crate) fn latest<'a>(rows: impl IntoIterator<Item = &'a Row>) -> Option<NaiveDate> {
    rows.into_iter().filter_map(of).max()
}
