//! File patterns: extract identity (and spent axes) from a path.
//!
//! Config data, not code (DESIGN.md §4). The routing half — key -> URL —
//! is the database's, in `grackle_db::route`.
//!
//! A pattern is declared per RULE as `file = [...]`, falling back to its
//! collection's own list. Same list law as `route = [...]`: try in order;
//! the first match wins. The subject is the collection-relative path without
//! its extension (`recipes/dal.fr`, `fr/recipes/dal`). `{axis:NAME}` spends
//! a declared axis as a suffix (`{stem}.{axis:locale}`) or a directory
//! prefix (`{axis:locale}/{stem}`); a shorter pattern without that token is
//! the canonical member. Date tokens (`{year}`, …) are the same matcher —
//! there is one extractor, not two.

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use std::collections::BTreeMap;

/// A compiled `file` entry, e.g. `{year}-{month}-{day}-{slug}.{axis:locale}`
/// or `{axis:locale}/{stem}`.
#[derive(Debug)]
pub struct FilePattern {
    /// Pattern text with `{axis:…}` segments removed — filled to build the
    /// logical path (no extension) after a match.
    logical: String,
    re: Regex,
    /// Axis name -> regex capture name (`_axis_locale`).
    axes: Vec<(String, String)>,
}

impl FilePattern {
    /// Axes this pattern spends into the stem.
    pub fn spent_axes(&self) -> impl Iterator<Item = &str> + '_ {
        self.axes.iter().map(|(n, _)| n.as_str())
    }
}

/// What a file pattern yields: date/slug tokens, spent axis values, and the
/// path (no extension) everything downstream treats as identity.
#[derive(Debug, Clone, Default)]
pub struct FileMatch {
    pub key: FileKey,
    /// Axis name -> member value. Absent axes default to canonical at the
    /// call site.
    pub axes: BTreeMap<String, String>,
    /// Logical path without extension; may contain `/` after a prefix strip.
    pub logical_stem: String,
}

/// What a filename format yields: **whatever the format named**, and nothing
/// else. Every field is optional because a format need not name every token —
/// `{slug}` alone is a legal format (grack.com's `_drafts` writes exactly
/// that).
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

/// Non-canonical members of a declared axis — the values a spent `{axis:N}`
/// may capture. Canonical is the pattern that omits the token.
pub type AxisValues<'a> = BTreeMap<&'a str, &'a [String]>;

impl FilePattern {
    pub fn compile(fmt: &str, axes: &AxisValues<'_>) -> Result<Self> {
        let mut re = String::from("^");
        let mut logical = String::new();
        let mut rest = fmt;
        let mut axis_caps = Vec::new();
        while let Some(open) = rest.find('{') {
            let lit = &rest[..open];
            re.push_str(&regex::escape(lit));
            logical.push_str(lit);
            let close = rest[open..]
                .find('}')
                .ok_or_else(|| anyhow!("unclosed '{{' in file pattern {fmt:?}"))?
                + open;
            let token = &rest[open + 1..close];
            if let Some(name) = token.strip_prefix("axis:") {
                let values = axes.get(name).with_context(|| {
                    format!(
                        "file pattern {fmt:?}: {{axis:{name}}} names no declared \
                         axis — declare [axes.{name}] or drop the token"
                    )
                })?;
                // Non-canonical only: the bare pattern is the canonical member.
                let alts: Vec<&str> = values.iter().skip(1).map(String::as_str).collect();
                if alts.is_empty() {
                    bail!(
                        "file pattern {fmt:?}: {{axis:{name}}} needs a non-canonical \
                         member on [axes.{name}] — a one-value axis has nothing to spend"
                    );
                }
                let cap = format!("_axis_{name}");
                re.push_str("(?P<");
                re.push_str(&cap);
                re.push('>');
                re.push_str(&alts.join("|"));
                re.push(')');
                axis_caps.push((name.to_string(), cap));
                // Logical path drops the `.` or `/` that only attached the
                // axis; the regex still needs that separator.
                let mut next = &rest[close + 1..];
                if logical.ends_with('.') || logical.ends_with('/') {
                    logical.pop();
                } else if next.starts_with('.') || next.starts_with('/') {
                    re.push_str(&regex::escape(&next[..1]));
                    next = &next[1..];
                }
                rest = next;
                continue;
            }
            logical.push('{');
            logical.push_str(token);
            logical.push('}');
            re.push_str(match token {
                "year" => r"(?P<year>\d{4})",
                "month" => r"(?P<month>\d{1,2})",
                "day" => r"(?P<day>\d{1,2})",
                // One path segment: drafts and dated names. `{stem}` may span
                // `/` so a prefix pattern can keep the rest of the path.
                "slug" => r"(?P<slug>[^/]+)",
                "stem" => r"(?P<stem>.+)",
                other => bail!("unknown token {{{other}}} in file pattern {fmt:?}"),
            });
            rest = &rest[close + 1..];
        }
        re.push_str(&regex::escape(rest));
        logical.push_str(rest);
        re.push('$');
        Ok(Self {
            logical,
            re: Regex::new(&re)?,
            axes: axis_caps,
        })
    }

    pub fn parse(&self, subject: &str) -> Option<FileMatch> {
        let c = self.re.captures(subject)?;
        let key = FileKey {
            year: c.name("year").and_then(|m| m.as_str().parse().ok()),
            month: c.name("month").and_then(|m| m.as_str().parse().ok()),
            day: c.name("day").and_then(|m| m.as_str().parse().ok()),
            slug: c.name("slug").map(|m| m.as_str().to_string()),
        };
        let mut axes = BTreeMap::new();
        for (name, cap) in &self.axes {
            if let Some(m) = c.name(cap) {
                axes.insert(name.clone(), m.as_str().to_string());
            }
        }
        let logical_stem = if let Some(s) = c.name("stem") {
            s.as_str().to_string()
        } else {
            fill_logical(&self.logical, &c)
        };
        Some(FileMatch {
            key,
            axes,
            logical_stem,
        })
    }
}

fn fill_logical(template: &str, c: &regex::Captures<'_>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}').unwrap() + open;
        let token = &rest[open + 1..close];
        if let Some(m) = c.name(token) {
            out.push_str(m.as_str());
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// The first pattern that describes this path key, in declared order.
///
/// `path_key` is the collection-relative path with its extension removed
/// (`recipes/dal.fr`, `fr/recipes/dal`, `1998/1998-08-15-hello`). Each
/// pattern is tried against the whole key first, then against the final
/// filename — so a prefix (`{axis:locale}/{stem}`) and a nested path share
/// one matcher, while dated posts may still live in year subdirectories.
pub fn extract(patterns: &[FilePattern], path_key: &str) -> Option<FileMatch> {
    let name = path_key.rsplit('/').next().unwrap_or(path_key);
    patterns.iter().find_map(|f| {
        f.parse(path_key).or_else(|| {
            if name != path_key {
                f.parse(name)
            } else {
                None
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locale_axis() -> BTreeMap<&'static str, &'static [String]> {
        // leaked for test statics — fine in tests
        let vals: &'static [String] = Box::leak(vec!["en".into(), "fr".into()].into_boxed_slice());
        BTreeMap::from([("locale", vals)])
    }

    #[test]
    fn parses_standard_post_filename() {
        let axes = BTreeMap::new();
        let f = FilePattern::compile("{year}-{month}-{day}-{slug}", &axes).unwrap();
        let k = f.parse("2014-12-06-the-next-decade-part-2").unwrap();
        assert_eq!(k.key.ymd(), Some((2014, 12, 6)));
        assert_eq!(k.key.slug.as_deref(), Some("the-next-decade-part-2"));
        assert_eq!(k.logical_stem, "2014-12-06-the-next-decade-part-2");
    }

    #[test]
    fn legacy_format_does_not_match_standard_names() {
        let axes = BTreeMap::new();
        let legacy = FilePattern::compile("{month}-{day}-{year}-{slug}", &axes).unwrap();
        assert!(legacy.parse("2014-12-06-the-next-decade").is_none());
    }

    #[test]
    fn a_format_yields_only_the_tokens_it_names() {
        let axes = BTreeMap::new();
        let slug_only = FilePattern::compile("{slug}", &axes).unwrap();
        let k = slug_only.parse("caret").unwrap();
        assert_eq!(k.key.slug.as_deref(), Some("caret"));
        assert_eq!(k.key.ymd(), None);

        let with_lit = FilePattern::compile("notes-{slug}", &axes).unwrap();
        let k = with_lit.parse("notes-caret").unwrap();
        assert_eq!(k.key.slug.as_deref(), Some("caret"));
        assert_eq!(k.logical_stem, "notes-caret");
    }

    #[test]
    fn axis_suffix_strips_to_logical_stem() {
        let axes = locale_axis();
        let patterns = [
            FilePattern::compile("{stem}.{axis:locale}", &axes).unwrap(),
            FilePattern::compile("{stem}", &axes).unwrap(),
        ];
        let fr = extract(&patterns, "dal.fr").unwrap();
        assert_eq!(fr.logical_stem, "dal");
        assert_eq!(fr.axes.get("locale").map(String::as_str), Some("fr"));

        let en = extract(&patterns, "dal").unwrap();
        assert_eq!(en.logical_stem, "dal");
        assert!(en.axes.is_empty());
    }

    #[test]
    fn dated_axis_suffix() {
        let axes = locale_axis();
        let patterns = [
            FilePattern::compile("{year}-{month}-{day}-{slug}.{axis:locale}", &axes).unwrap(),
            FilePattern::compile("{year}-{month}-{day}-{slug}", &axes).unwrap(),
        ];
        let fr = extract(&patterns, "2026-01-01-hello.fr").unwrap();
        assert_eq!(fr.logical_stem, "2026-01-01-hello");
        assert_eq!(fr.key.slug.as_deref(), Some("hello"));
        assert_eq!(fr.axes.get("locale").map(String::as_str), Some("fr"));

        let en = extract(&patterns, "2026-01-01-hello").unwrap();
        assert_eq!(en.logical_stem, "2026-01-01-hello");
        assert!(en.axes.is_empty());
    }

    #[test]
    fn unknown_axis_is_a_load_error() {
        let axes = BTreeMap::new();
        let e = FilePattern::compile("{stem}.{axis:locale}", &axes)
            .unwrap_err()
            .to_string();
        assert!(e.contains("no declared axis"), "{e}");
    }

    #[test]
    fn axis_prefix_strips_to_logical_path() {
        let axes = locale_axis();
        let patterns = [
            FilePattern::compile("{axis:locale}/{stem}", &axes).unwrap(),
            FilePattern::compile("{stem}", &axes).unwrap(),
        ];
        let fr = extract(&patterns, "fr/recipes/dal").unwrap();
        assert_eq!(fr.logical_stem, "recipes/dal");
        assert_eq!(fr.axes.get("locale").map(String::as_str), Some("fr"));

        let en = extract(&patterns, "recipes/dal").unwrap();
        assert_eq!(en.logical_stem, "recipes/dal");
        assert!(en.axes.is_empty());
    }

    #[test]
    fn suffix_on_a_nested_path() {
        let axes = locale_axis();
        let patterns = [
            FilePattern::compile("{stem}.{axis:locale}", &axes).unwrap(),
            FilePattern::compile("{stem}", &axes).unwrap(),
        ];
        let fr = extract(&patterns, "recipes/dal.fr").unwrap();
        assert_eq!(fr.logical_stem, "recipes/dal");
        assert_eq!(fr.axes.get("locale").map(String::as_str), Some("fr"));
    }

    #[test]
    fn dated_in_a_subdirectory() {
        let axes = BTreeMap::new();
        let patterns = [FilePattern::compile("{year}-{month}-{day}-{slug}", &axes).unwrap()];
        let k = extract(&patterns, "1998/1998-08-15-hello").unwrap();
        assert_eq!(k.key.ymd(), Some((1998, 8, 15)));
        assert_eq!(k.key.slug.as_deref(), Some("hello"));
    }
}
