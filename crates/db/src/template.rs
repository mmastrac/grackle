//! A small string templater: `{name}` substitution with optional zero-padding.
//!
//! Deliberately not a language. There are no conditionals, no loops and no
//! expressions — a template names values and a caller supplies them. That is
//! enough for every template this engine has (URLs, titles, breadcrumbs) and
//! it keeps a typo in one a load error rather than a rendering surprise.
//!
//! `{{` and `}}` are literal braces. Every other `{` opens a token and must
//! be closed.

use anyhow::{anyhow, bail, Result};

/// What a scan yields. An escaped brace arrives as `Literal`, which is what
/// keeps it out of the token stream.
enum Piece<'a> {
    Literal(&'a str),
    Token { name: &'a str, pad: Option<usize> },
}

/// One scan of a template, shared by `render` and `tokens` so the two can
/// never disagree about what a template contains.
fn scan(tmpl: &str, mut emit: impl FnMut(Piece<'_>) -> Result<()>) -> Result<()> {
    let mut rest = tmpl;
    while let Some(open) = rest.find(['{', '}']) {
        emit(Piece::Literal(&rest[..open]))?;
        let brace = &rest[open..open + 1];
        // A doubled brace is that brace, literally.
        if rest[open + 1..].starts_with(brace) {
            emit(Piece::Literal(brace))?;
            rest = &rest[open + 2..];
            continue;
        }
        if brace == "}" {
            bail!("unmatched '}}' in template {tmpl:?} (write '}}}}' for a literal one)");
        }
        let close = rest[open..]
            .find('}')
            .ok_or_else(|| anyhow!("unclosed '{{' in template {tmpl:?}"))?
            + open;
        let body = &rest[open + 1..close];
        let (name, pad) = match body.split_once(':') {
            Some((n, spec)) => {
                let width: usize = spec
                    .parse()
                    .map_err(|_| anyhow!("bad pad width {spec:?} in template {tmpl:?}"))?;
                (n, Some(width))
            }
            None => (body, None),
        };
        if name.is_empty() {
            bail!("empty token {{}} in template {tmpl:?}");
        }
        emit(Piece::Token { name, pad })?;
        rest = &rest[close + 1..];
    }
    emit(Piece::Literal(rest))
}

/// Render a template such as `/blog/{year}/{month:02}/{day:02}/{slug}/`.
///
/// `get` resolves a token name to its value; `None` is an error naming the
/// token, because a template that silently drops an unresolved name produces
/// a plausible-looking wrong answer. `{name:N}` left-pads with zeros to width
/// N, which is only meaningful for numeric values.
pub fn render(tmpl: &str, get: impl Fn(&str) -> Option<String>) -> Result<String> {
    let mut out = String::with_capacity(tmpl.len() + 16);
    scan(tmpl, |piece| {
        match piece {
            Piece::Literal(text) => out.push_str(text),
            Piece::Token { name, pad } => {
                let value = get(name).ok_or_else(|| {
                    anyhow!("template {tmpl:?} references unknown token {{{name}}}")
                })?;
                for _ in value.len()..pad.unwrap_or(0) {
                    out.push('0');
                }
                out.push_str(&value);
            }
        }
        Ok(())
    })?;
    Ok(out)
}

/// Which tokens a template names, in order and with duplicates kept.
///
/// Fails on exactly what `render` fails on, so "does this template parse" and
/// "what does it need" are one question. Callers use it to check a template
/// against what they can supply before rendering — which is how an undated
/// row routed by a dated template becomes a load error rather than a 404.
pub fn tokens(tmpl: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    scan(tmpl, |piece| {
        if let Piece::Token { name, .. } = piece {
            out.push(name.to_string());
        }
        Ok(())
    })?;
    Ok(out)
}

/// Resolve a token from a list of name/value pairs — the `get` most callers
/// want, when their values already exist as pairs rather than as a match.
pub fn param(params: &[(String, String)], k: &str) -> Option<String> {
    params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(kv: &[(&str, &str)]) -> Vec<(String, String)> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn renders_with_padding() {
        let p = pairs(&[
            ("year", "2014"),
            ("month", "12"),
            ("day", "6"),
            ("slug", "foo"),
        ]);
        let url = render("/blog/{year}/{month:02}/{day:02}/{slug}/", |k| param(&p, k)).unwrap();
        assert_eq!(url, "/blog/2014/12/06/foo/");
    }

    /// Padding never truncates: a value wider than the pad wins.
    #[test]
    fn padding_is_a_minimum_not_a_width() {
        let p = pairs(&[("n", "12345")]);
        assert_eq!(render("{n:02}", |k| param(&p, k)).unwrap(), "12345");
    }

    #[test]
    fn unknown_token_is_an_error() {
        let e = render("/x/{nope}/", |_| None).unwrap_err().to_string();
        assert!(e.contains("unknown token {nope}"), "{e}");
    }

    #[test]
    fn reports_tokens_in_order_with_duplicates() {
        assert_eq!(
            tokens("/{a}/{b:02}/{a}").unwrap(),
            vec!["a", "b", "a"],
            "a caller checking what it can supply needs every mention"
        );
    }

    #[test]
    fn doubled_braces_are_literal() {
        assert_eq!(render("{{{x}}}", |_| Some("v".into())).unwrap(), "{v}");
        assert_eq!(render("{{}}", |_| None).unwrap(), "{}");
        assert!(
            tokens("{{x}}").unwrap().is_empty(),
            "an escaped brace names no token"
        );
    }

    /// `tokens` and `render` reject the same templates — the point of one
    /// scan behind both.
    #[test]
    fn malformed_templates_fail_the_same_way_in_both() {
        for bad in ["{unclosed", "closed}", "{}", "{x:notanumber}"] {
            assert!(render(bad, |_| Some("v".into())).is_err(), "render {bad:?}");
            assert!(tokens(bad).is_err(), "tokens {bad:?}");
        }
    }
}
