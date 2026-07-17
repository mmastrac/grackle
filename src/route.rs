//! Filename formats (key extraction) and route templates (key -> URL).
//!
//! Both are config data, not code: see DESIGN.md §4.

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
        Ok(Self { re: Regex::new(&re)? })
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

/// Render a route template such as `/blog/{year}/{month:02}/{day:02}/{slug}/`.
///
/// `get` resolves a token name to its value. `{name:0N}` zero-pads to width N,
/// which is only meaningful for numeric tokens.
pub fn render(tmpl: &str, get: impl Fn(&str) -> Option<String>) -> Result<String> {
    let mut out = String::with_capacity(tmpl.len() + 16);
    let mut rest = tmpl;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..]
            .find('}')
            .ok_or_else(|| anyhow!("unclosed '{{' in route template {tmpl:?}"))?
            + open;
        let token = &rest[open + 1..close];
        let (name, pad) = match token.split_once(':') {
            Some((n, spec)) => {
                let width: usize = spec
                    .trim_start_matches('0')
                    .parse()
                    .map_err(|_| anyhow!("bad pad spec {spec:?} in route template {tmpl:?}"))?;
                (n, Some(width))
            }
            None => (token, None),
        };
        let value = get(name)
            .ok_or_else(|| anyhow!("route template {tmpl:?} references unknown token {{{name}}}"))?;
        match pad {
            Some(w) if value.len() < w => {
                for _ in 0..(w - value.len()) {
                    out.push('0');
                }
                out.push_str(&value);
            }
            _ => out.push_str(&value),
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Which tokens a template needs. Used to enforce the "undated row routed by a
/// dated template" constraint (DESIGN.md §4) at load time rather than as a 404.
pub fn tokens(tmpl: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = tmpl;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|c| c + open) else {
            break;
        };
        let token = &rest[open + 1..close];
        let name = token.split_once(':').map(|(n, _)| n).unwrap_or(token);
        out.push(name.to_string());
        rest = &rest[close + 1..];
    }
    out
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

    #[test]
    fn renders_route_with_padding() {
        let url = render("/blog/{year}/{month:02}/{day:02}/{slug}/", |k| {
            Some(match k {
                "year" => "2014".into(),
                "month" => "12".into(),
                "day" => "6".into(),
                "slug" => "foo".into(),
                _ => return None,
            })
        })
        .unwrap();
        assert_eq!(url, "/blog/2014/12/06/foo/");
    }

    #[test]
    fn unknown_token_is_an_error() {
        assert!(render("/x/{nope}/", |_| None).is_err());
    }

    #[test]
    fn reports_tokens() {
        assert_eq!(
            tokens("/blog/{year}/{month:02}/{slug}/"),
            vec!["year", "month", "slug"]
        );
    }
}
