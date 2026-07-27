//! Filename formats: key extraction from a file stem.
//!
//! Config data, not code (DESIGN.md §4). The routing half — key -> URL —
//! is the database's, in `grackle_db::route`.
//!
//! A format is declared per RULE, falling back to its collection's own list
//! (IO.md I6): the extractor is one half of the route-token supply, and the
//! other half — the path tokens — has always been the rule's.

use anyhow::{anyhow, bail, Result};
use regex::Regex;

/// A compiled `filename_formats` entry, e.g. `{year}-{month}-{day}-{slug}`.
#[derive(Debug)]
pub struct FilenameFormat {
    re: Regex,
}

/// What a filename format yields: **whatever the format named**, and nothing
/// else. Every field is optional because a format need not name every token —
/// `{slug}` alone is a legal format (grack.com's `_drafts` writes exactly
/// that) and used to match NOTHING, since a key was only ever built when all
/// four captures were present. That silence was survivable only by accident:
/// the slug fell back to the whole stem, which is what `{slug}` captures
/// anyway. A format with a literal (`notes-{slug}`) would have fallen back to
/// the same whole stem — the prefix silently kept — which is the failure this
/// shape refuses.
#[derive(Debug, Clone, Default)]
pub struct FileKey {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub slug: Option<String>,
}

impl FileKey {
    /// The date the format named, when it named all three parts. A partial
    /// date is no date: the row's `date` is a day, and two thirds of one
    /// cannot be one.
    pub fn ymd(&self) -> Option<(i32, u32, u32)> {
        Some((self.year?, self.month?, self.day?))
    }
}

impl FilenameFormat {
    pub fn compile(fmt: &str) -> Result<Self> {
        let mut re = String::from("^");
        let mut rest = fmt;
        while let Some(open) = rest.find('{') {
            re.push_str(&regex::escape(&rest[..open]));
            let close = rest[open..]
                .find('}')
                .ok_or_else(|| anyhow!("unclosed '{{' in filename format {fmt:?}"))?
                + open;
            let token = &rest[open + 1..close];
            re.push_str(match token {
                "year" => r"(?P<year>\d{4})",
                "month" => r"(?P<month>\d{1,2})",
                "day" => r"(?P<day>\d{1,2})",
                "slug" => r"(?P<slug>.+)",
                other => bail!("unknown token {{{other}}} in filename format {fmt:?}"),
            });
            rest = &rest[close + 1..];
        }
        re.push_str(&regex::escape(rest));
        re.push('$');
        Ok(Self {
            re: Regex::new(&re)?,
        })
    }

    /// Parse a file stem (basename without extension). `None` is "this format
    /// does not describe this name"; a match yields only the tokens the format
    /// actually named.
    pub fn parse(&self, stem: &str) -> Option<FileKey> {
        let c = self.re.captures(stem)?;
        // A named group that matched must parse — the pattern for each is
        // digits — so a parse failure is a bug rather than a non-match.
        Some(FileKey {
            year: c.name("year").and_then(|m| m.as_str().parse().ok()),
            month: c.name("month").and_then(|m| m.as_str().parse().ok()),
            day: c.name("day").and_then(|m| m.as_str().parse().ok()),
            slug: c.name("slug").map(|m| m.as_str().to_string()),
        })
    }
}

/// The first format that describes this stem, in declared order.
pub fn extract(formats: &[FilenameFormat], stem: &str) -> Option<FileKey> {
    formats.iter().find_map(|f| f.parse(stem))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_standard_post_filename() {
        let f = FilenameFormat::compile("{year}-{month}-{day}-{slug}").unwrap();
        let k = f.parse("2014-12-06-the-next-decade-part-2").unwrap();
        assert_eq!(k.ymd(), Some((2014, 12, 6)));
        assert_eq!(k.slug.as_deref(), Some("the-next-decade-part-2"));
    }

    #[test]
    fn legacy_format_does_not_match_standard_names() {
        // Guards ordering: the MM-DD-YYYY form must not swallow YYYY-MM-DD names.
        let legacy = FilenameFormat::compile("{month}-{day}-{year}-{slug}").unwrap();
        assert!(legacy.parse("2014-12-06-the-next-decade").is_none());
    }

    /// A format yields what it names, and a partial date is not a date —
    /// which is what lets `{slug}` be an extractor rather than a no-op.
    #[test]
    fn a_format_yields_only_the_tokens_it_names() {
        let slug_only = FilenameFormat::compile("{slug}").unwrap();
        let k = slug_only.parse("caret").unwrap();
        assert_eq!(k.slug.as_deref(), Some("caret"));
        assert_eq!(k.ymd(), None);

        // The literal is honoured, which is the half the all-or-nothing key
        // could not express: pre-I6 this returned `None` and the caller kept
        // the whole stem, prefix included.
        let prefixed = FilenameFormat::compile("notes-{slug}").unwrap();
        assert_eq!(
            prefixed.parse("notes-hello").unwrap().slug.as_deref(),
            Some("hello")
        );
        assert!(prefixed.parse("hello").is_none());

        let yearly = FilenameFormat::compile("{year}-{slug}").unwrap();
        let k = yearly.parse("2020-hello").unwrap();
        assert_eq!(k.year, Some(2020));
        assert_eq!(k.ymd(), None);
    }
}
