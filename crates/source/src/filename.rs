//! File patterns (`file = [...]`): capture tokens and spent axes from a path.
//! Tokens: `{slug}`/`{stem}`, `{field.year|month|day}`, `{axis:NAME}`.

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct FilePattern {
    /// Axis tokens stripped; filled for logical path.
    logical: String,
    re: Regex,
    /// Axis name -> regex capture name (`_axis_locale`).
    axes: Vec<(String, String)>,
    /// Regex group name -> capture key (`date.year`, `slug`, ...).
    caps: Vec<(String, String)>,
}

impl FilePattern {
    pub fn spent_axes(&self) -> impl Iterator<Item = &str> + '_ {
        self.axes.iter().map(|(n, _)| n.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct FileMatch {
    pub captures: BTreeMap<String, String>,
    pub axes: BTreeMap<String, String>,
    pub logical_stem: String,
}

impl FileMatch {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.captures.get(name).map(String::as_str)
    }

    /// Prefer `date.*`, else any `*.part`.
    pub fn date_part(&self, part: &str) -> Option<&str> {
        let keyed = format!("date.{part}");
        self.get(&keyed).or_else(|| {
            self.captures.iter().find_map(|(k, v)| {
                k.strip_suffix(part)
                    .filter(|rest| rest.ends_with('.'))
                    .map(|_| v.as_str())
            })
        })
    }
}

/// Non-canonical members a spent `{axis:N}` may capture.
pub type AxisValues<'a> = BTreeMap<&'a str, &'a [String]>;

impl FilePattern {
    pub fn compile(fmt: &str, axes: &AxisValues<'_>) -> Result<Self> {
        let mut re = String::from("^");
        let mut logical = String::new();
        let mut rest = fmt;
        let mut axis_caps = Vec::new();
        let mut caps = Vec::new();
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
            let (key, class) = match token {
                "slug" => ("slug", "slug"),
                "stem" => ("stem", "stem"),
                "year" | "month" | "day" => bail!(
                    "file pattern {fmt:?}: {{{token}}} is not a token — write \
                     {{date.{token}}} (or {{field.{token}}} for another date field)"
                ),
                other => match other.split_once('.') {
                    Some((field, part)) if !field.is_empty() => match part {
                        "year" | "month" | "day" => (other, part),
                        _ => bail!(
                            "file pattern {fmt:?}: unknown date part {{{other}}} — \
                             use year, month, or day"
                        ),
                    },
                    _ => bail!("unknown token {{{other}}} in file pattern {fmt:?}"),
                },
            };
            let group = format!("_c{}", caps.len());
            logical.push('{');
            logical.push_str(key);
            logical.push('}');
            re.push_str("(?P<");
            re.push_str(&group);
            re.push('>');
            re.push_str(match class {
                "year" => r"\d{4}",
                "month" | "day" => r"\d{1,2}",
                "slug" => r"[^/]+",
                "stem" => r".+",
                _ => unreachable!(),
            });
            re.push(')');
            caps.push((group, key.to_string()));
            rest = &rest[close + 1..];
        }
        re.push_str(&regex::escape(rest));
        logical.push_str(rest);
        re.push('$');
        Ok(Self {
            logical,
            re: Regex::new(&re)?,
            axes: axis_caps,
            caps,
        })
    }

    pub fn parse(&self, subject: &str) -> Option<FileMatch> {
        let c = self.re.captures(subject)?;
        let mut captures = BTreeMap::new();
        for (group, key) in &self.caps {
            if let Some(m) = c.name(group) {
                captures.insert(key.clone(), m.as_str().to_string());
            }
        }
        let mut axes = BTreeMap::new();
        for (name, cap) in &self.axes {
            if let Some(m) = c.name(cap) {
                axes.insert(name.clone(), m.as_str().to_string());
            }
        }
        let logical_stem = if let Some(s) = captures.get("stem") {
            s.clone()
        } else {
            fill_logical(&self.logical, &captures)
        };
        Some(FileMatch {
            captures,
            axes,
            logical_stem,
        })
    }
}

fn fill_logical(template: &str, captures: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}').unwrap() + open;
        let token = &rest[open + 1..close];
        if let Some(v) = captures.get(token) {
            out.push_str(v);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// `(field, "YYYY-MM-DD")` per field with a year capture; missing month/day = `01`.
pub fn dates_from_captures(
    captures: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut parts: BTreeMap<String, (Option<&str>, Option<&str>, Option<&str>)> = BTreeMap::new();
    for (key, raw) in captures {
        let Some((field, part)) = key.split_once('.') else {
            continue;
        };
        let slot = parts.entry(field.to_string()).or_default();
        match part {
            "year" => slot.0 = Some(raw.as_str()),
            "month" => slot.1 = Some(raw.as_str()),
            "day" => slot.2 = Some(raw.as_str()),
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    for (field, (y, m, d)) in parts {
        let Some(y) = y else {
            bail!(
                "file pattern captured {{{field}.month}}/{{{field}.day}} without {{{field}.year}}"
            );
        };
        let y: i32 = y
            .parse()
            .with_context(|| format!("{{{field}.year}}: {y:?} is not a number"))?;
        let m: u32 = m.unwrap_or("1").parse().with_context(|| {
            format!("{{{field}.month}}: {:?} is not a number", m.unwrap_or("1"))
        })?;
        let d: u32 = d
            .unwrap_or("1")
            .parse()
            .with_context(|| format!("{{{field}.day}}: {:?} is not a number", d.unwrap_or("1")))?;
        let date = chrono::NaiveDate::from_ymd_opt(y, m, d).with_context(|| {
            format!("{{{field}.*}}: {y}-{m:02}-{d:02} is not a real calendar day")
        })?;
        out.insert(field, date.format("%Y-%m-%d").to_string());
    }
    Ok(out)
}

/// First matching pattern (full path key, then basename).
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
        let vals: &'static [String] = Box::leak(vec!["en".into(), "fr".into()].into_boxed_slice());
        BTreeMap::from([("locale", vals)])
    }

    #[test]
    fn parses_standard_post_filename() {
        let axes = BTreeMap::new();
        let f = FilePattern::compile("{date.year}-{date.month}-{date.day}-{slug}", &axes).unwrap();
        let k = f.parse("2014-12-06-the-next-decade-part-2").unwrap();
        assert_eq!(k.get("date.year"), Some("2014"));
        assert_eq!(k.get("date.month"), Some("12"));
        assert_eq!(k.get("date.day"), Some("06"));
        assert_eq!(k.get("slug"), Some("the-next-decade-part-2"));
        assert_eq!(k.logical_stem, "2014-12-06-the-next-decade-part-2");
        assert_eq!(
            dates_from_captures(&k.captures)
                .unwrap()
                .get("date")
                .map(String::as_str),
            Some("2014-12-06")
        );
    }

    #[test]
    fn bare_year_token_is_rejected() {
        let axes = BTreeMap::new();
        let e = FilePattern::compile("{year}-{month}-{day}-{slug}", &axes)
            .unwrap_err()
            .to_string();
        assert!(e.contains("{date.year}"), "{e}");
    }

    #[test]
    fn legacy_format_does_not_match_standard_names() {
        let axes = BTreeMap::new();
        let legacy =
            FilePattern::compile("{date.month}-{date.day}-{date.year}-{slug}", &axes).unwrap();
        assert!(legacy.parse("2014-12-06-the-next-decade").is_none());
    }

    #[test]
    fn a_format_yields_only_the_tokens_it_names() {
        let axes = BTreeMap::new();
        let slug_only = FilePattern::compile("{slug}", &axes).unwrap();
        let k = slug_only.parse("caret").unwrap();
        assert_eq!(k.get("slug"), Some("caret"));
        assert!(k.get("date.year").is_none());

        let with_lit = FilePattern::compile("notes-{slug}", &axes).unwrap();
        let k = with_lit.parse("notes-caret").unwrap();
        assert_eq!(k.get("slug"), Some("caret"));
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
            FilePattern::compile(
                "{date.year}-{date.month}-{date.day}-{slug}.{axis:locale}",
                &axes,
            )
            .unwrap(),
            FilePattern::compile("{date.year}-{date.month}-{date.day}-{slug}", &axes).unwrap(),
        ];
        let fr = extract(&patterns, "2026-01-01-hello.fr").unwrap();
        assert_eq!(fr.logical_stem, "2026-01-01-hello");
        assert_eq!(fr.get("slug"), Some("hello"));
        assert_eq!(fr.axes.get("locale").map(String::as_str), Some("fr"));
    }

    #[test]
    fn published_field_components() {
        let axes = BTreeMap::new();
        let f = FilePattern::compile("{published.year}-{published.month}-{slug}", &axes).unwrap();
        let k = f.parse("2020-3-hello").unwrap();
        assert_eq!(
            dates_from_captures(&k.captures)
                .unwrap()
                .get("published")
                .map(String::as_str),
            Some("2020-03-01")
        );
    }
}
