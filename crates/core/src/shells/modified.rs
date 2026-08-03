//! The last-modified instant of a row, and of a set of rows.
//!
//! One source the sitemap's per-URL `<lastmod>` and the feed's `<updated>`
//! both read: the row's `date`. That keeps a build a pure function of its
//! inputs, with no wall-clock. A git commit-date source would extend this one
//! module, and both shells follow.

use chrono::NaiveDate;

use crate::model::Row;

/// A row's last-modified date, or `None` if it carries no date.
pub(crate) fn of(p: &Row) -> Option<NaiveDate> {
    p.as_date("date")
}

/// The newest last-modified across a set of rows. `None` when none carries a date.
pub(crate) fn latest<'a>(rows: impl IntoIterator<Item = &'a Row>) -> Option<NaiveDate> {
    rows.into_iter().filter_map(of).max()
}
