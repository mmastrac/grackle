//! Liquid-shaped tag/var scanner.
//!
//! Walks `{% … %}` and `{{ … }}`, expands registered widgets, and asks the
//! caller what each recognised tag or variable becomes. Unknown constructs
//! stay verbatim.

use anyhow::{bail, Result};
use std::collections::BTreeMap;

/// Find `{% endNAME %}` after an opening paired tag; returns (body_end, resume).
fn find_end_tag(s: &str, name: &str) -> Option<(usize, usize)> {
    let want = format!("end{name}");
    let mut idx = 0;
    while let Some(start) = s[idx..].find("{%") {
        let start = idx + start;
        let close = s[start..].find("%}")? + start;
        if s[start + 2..close].trim() == want {
            return Some((start, close + 2));
        }
        idx = close + 2;
    }
    None
}

/// Expand liquid-shaped constructs with caller-supplied handlers.
///
/// - `on_tag(name, arg)` — `{% name arg %}`; `Ok(None)` leaves the tag verbatim.
/// - `on_var(inner)` — `{{ inner }}`; `None` leaves the var verbatim.
/// - `widgets` — paired tags `{% name %}…{% endname %}` with a `{body}` hole.
pub fn expand(
    body: &str,
    source: &str,
    widgets: Option<&BTreeMap<String, String>>,
    mut on_tag: impl FnMut(&str, &str) -> Result<Option<String>>,
    mut on_var: impl FnMut(&str) -> Option<String>,
) -> Result<String> {
    // Dyn so nested widget bodies don't stack `impl FnMut` layers forever.
    expand_inner(body, source, widgets, &mut on_tag, &mut on_var)
}

fn expand_inner(
    body: &str,
    source: &str,
    widgets: Option<&BTreeMap<String, String>>,
    on_tag: &mut dyn FnMut(&str, &str) -> Result<Option<String>>,
    on_var: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<String> {
    let mut out = String::with_capacity(body.len() + 256);
    let mut rest = body;

    loop {
        let tag = rest.find("{%");
        let var = rest.find("{{");
        let next = match (tag, var) {
            (None, None) => break,
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (Some(a), Some(b)) => a.min(b),
        };
        let is_tag = rest[next..].starts_with("{%");
        let close = if is_tag { "%}" } else { "}}" };
        let Some(end) = rest[next..].find(close) else {
            break;
        };
        let inner = rest[next + 2..next + end].trim().to_string();
        out.push_str(&rest[..next]);

        if is_tag {
            if let Some(tmpl) = widgets.and_then(|w| w.get(inner.as_str())) {
                let after = &rest[next + end + close.len()..];
                let Some((body_end, resume)) = find_end_tag(after, &inner) else {
                    bail!("{source}: {{% {inner} %}} has no matching {{% end{inner} %}}");
                };
                let nested =
                    expand_inner(after[..body_end].trim(), source, widgets, on_tag, on_var)?;
                out.push_str(&tmpl.replace("{body}", &nested));
                rest = &after[resume..];
                continue;
            }
        }

        let replacement = if is_tag {
            match inner.split_once(char::is_whitespace) {
                Some((name, arg)) => on_tag(name, arg.trim())?,
                None => on_tag(inner.as_str(), "")?,
            }
        } else {
            on_var(&inner)
        };

        match replacement {
            Some(r) => out.push_str(&r),
            None => out.push_str(&rest[next..next + end + close.len()]),
        }
        rest = &rest[next + end + close.len()..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_stays_verbatim() {
        let out = expand(
            "a {% foo %} b {{ bar }} c",
            "t",
            None,
            |_, _| Ok(None),
            |_| None,
        )
        .unwrap();
        assert_eq!(out, "a {% foo %} b {{ bar }} c");
    }

    #[test]
    fn tag_and_var_handlers_fire() {
        let out = expand(
            "{% hi there %} / {{ x }}",
            "t",
            None,
            |name, arg| {
                assert_eq!(name, "hi");
                Ok(Some(format!("<{arg}>")))
            },
            |inner| {
                assert_eq!(inner, "x");
                Some("X".into())
            },
        )
        .unwrap();
        assert_eq!(out, "<there> / X");
    }
}
