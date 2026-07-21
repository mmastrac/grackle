//! Filename formats: key extraction from a file stem.
//!
//! Config data, not code (DESIGN.md §4). The routing half — key -> URL —
//! is the database's, in `grackle_db::route`.

use anyhow::{anyhow, bail, Result};
use regex::Regex;

/// A compiled `filename_formats` entry, e.g. `{year}-{month}-{day}-{slug}`.
#[derive(Debug)]
pub struct FilenameFormat {
    re: Regex,
}

/// What a filename format yields.
#[derive(Debug, Clone)]
pub struct FileKey {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub slug: String,
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

    /// Parse a file stem (basename without extension).
    pub fn parse(&self, stem: &str) -> Option<FileKey> {
        let c = self.re.captures(stem)?;
        Some(FileKey {
            year: c.name("year")?.as_str().parse().ok()?,
            month: c.name("month")?.as_str().parse().ok()?,
            day: c.name("day")?.as_str().parse().ok()?,
            slug: c.name("slug")?.as_str().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_standard_post_filename() {
        let f = FilenameFormat::compile("{year}-{month}-{day}-{slug}").unwrap();
        let k = f.parse("2014-12-06-the-next-decade-part-2").unwrap();
        assert_eq!((k.year, k.month, k.day), (2014, 12, 6));
        assert_eq!(k.slug, "the-next-decade-part-2");
    }

    #[test]
    fn legacy_format_does_not_match_standard_names() {
        // Guards ordering: the MM-DD-YYYY form must not swallow YYYY-MM-DD names.
        let legacy = FilenameFormat::compile("{month}-{day}-{year}-{slug}").unwrap();
        assert!(legacy.parse("2014-12-06-the-next-decade").is_none());
    }
}
