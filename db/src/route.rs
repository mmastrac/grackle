//! Route templates: key -> URL. Config data, not code (DESIGN.md §4).

use anyhow::{anyhow, Result};

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
        let value = get(name).ok_or_else(|| {
            anyhow!("route template {tmpl:?} references unknown token {{{name}}}")
        })?;
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

/// Group-key params as a template `get`: what route/`title`/`crumb` templates
/// resolve their tokens from (§5c).
pub fn param(params: &[(String, String)], k: &str) -> Option<String> {
    params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
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
