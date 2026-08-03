//! Liquid-shaped tag/var scanner.
//!
//! Walks `{% … %}` and `{{ … }}`, expands registered widgets, and asks the
//! caller what each recognised tag or variable becomes. Unknown constructs
//! stay verbatim.

use anyhow::{bail, Result};
use grackle_source::config::WidgetDef;
use std::collections::{BTreeMap, BTreeSet};

/// A tag's leading name and the remainder (its arguments), e.g. `image` and
/// `right a/b.png` or `youtube` and `id="x"`.
fn split_name(inner: &str) -> (&str, &str) {
    match inner.split_once(char::is_whitespace) {
        Some((n, a)) => (n, a.trim()),
        None => (inner, ""),
    }
}

/// Parse `key="value" key2='value2'` widget arguments. Values are quoted so
/// they may hold spaces; the quote may be single or double.
fn parse_args(mut rest: &str) -> Result<Vec<(String, String)>, String> {
    let mut args = Vec::new();
    rest = rest.trim_start();
    while !rest.is_empty() {
        let eq = rest
            .find('=')
            .ok_or_else(|| format!("expected name=\"value\", got {rest:?}"))?;
        let key = rest[..eq].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(format!("{key:?} is not an argument name"));
        }
        let after = rest[eq + 1..].trim_start();
        let quote = match after.chars().next() {
            Some(q @ ('"' | '\'')) => q,
            _ => return Err(format!("argument `{key}` needs a quoted value")),
        };
        let val = &after[1..];
        let end = val
            .find(quote)
            .ok_or_else(|| format!("unterminated value for `{key}`"))?;
        args.push((key.to_string(), val[..end].to_string()));
        rest = val[end + 1..].trim_start();
    }
    Ok(args)
}

/// Fill a widget template: `{body}` takes the expanded body verbatim (trusted
/// HTML), each `{name}` takes its argument escaped (author text).
fn fill(tmpl: &str, args: &[(String, String)], body: Option<&str>) -> String {
    let mut s = tmpl.to_string();
    if let Some(b) = body {
        s = s.replace("{body}", b);
    }
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), &esc(v));
    }
    s
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

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
/// - `widgets` — `{% name arg="v" %}`, filling `{name}` holes; a `{body}` hole
///   makes it paired (`… {% endname %}`), its absence self-closing.
pub fn expand(
    body: &str,
    source: &str,
    widgets: Option<&BTreeMap<String, WidgetDef>>,
    used: &mut BTreeSet<String>,
    mut on_tag: impl FnMut(&str, &str) -> Result<Option<String>>,
    mut on_var: impl FnMut(&str) -> Option<String>,
) -> Result<String> {
    // Dyn so nested widget bodies don't stack `impl FnMut` layers forever.
    expand_inner(body, source, widgets, used, &mut on_tag, &mut on_var)
}

fn expand_inner(
    body: &str,
    source: &str,
    widgets: Option<&BTreeMap<String, WidgetDef>>,
    used: &mut BTreeSet<String>,
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
            let (name, args_str) = split_name(&inner);
            if let Some(def) = widgets.and_then(|w| w.get(name)) {
                used.insert(name.to_string());
                let tmpl = def.template.as_str();
                let args = parse_args(args_str)
                    .map_err(|e| anyhow::anyhow!("{source}: {{% {name} %}}: {e}"))?;
                // `{body}` in the template makes the widget paired; without it
                // the widget is self-closing and takes no body.
                if tmpl.contains("{body}") {
                    let after = &rest[next + end + close.len()..];
                    let Some((body_end, resume)) = find_end_tag(after, name) else {
                        bail!("{source}: {{% {name} %}} has no matching {{% end{name} %}}");
                    };
                    let nested = expand_inner(
                        after[..body_end].trim(),
                        source,
                        widgets,
                        used,
                        on_tag,
                        on_var,
                    )?;
                    out.push_str(&fill(tmpl, &args, Some(&nested)));
                    rest = &after[resume..];
                } else {
                    out.push_str(&fill(tmpl, &args, None));
                    rest = &rest[next + end + close.len()..];
                }
                continue;
            }
        }

        let replacement = if is_tag {
            let (name, arg) = split_name(&inner);
            on_tag(name, arg)?
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

    /// Expand with no engine tag/var handlers, discarding the used set.
    fn go(body: &str, w: Option<&BTreeMap<String, WidgetDef>>) -> String {
        expand(
            body,
            "t",
            w,
            &mut BTreeSet::new(),
            |_, _| Ok(None),
            |_| None,
        )
        .unwrap()
    }

    fn widgets(pairs: &[(&str, &str)]) -> BTreeMap<String, WidgetDef> {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    WidgetDef {
                        template: v.to_string(),
                        head: None,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn unknown_stays_verbatim() {
        assert_eq!(
            go("a {% foo %} b {{ bar }} c", None),
            "a {% foo %} b {{ bar }} c"
        );
    }

    #[test]
    fn tag_and_var_handlers_fire() {
        let out = expand(
            "{% hi there %} / {{ x }}",
            "t",
            None,
            &mut BTreeSet::new(),
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

    #[test]
    fn paired_widget_takes_args_and_body() {
        let w = widgets(&[(
            "figure",
            "<figure><img src=\"{src}\" alt=\"{caption}\">{body}</figure>",
        )]);
        assert_eq!(
            go(
                r#"{% figure src="cat.png" caption="A cat" %}A caption{% endfigure %}"#,
                Some(&w)
            ),
            r#"<figure><img src="cat.png" alt="A cat">A caption</figure>"#
        );
    }

    #[test]
    fn self_closing_widget_has_no_body_and_no_end_tag() {
        let w = widgets(&[("youtube", r#"<iframe src="https://x/{id}"></iframe>"#)]);
        assert_eq!(
            go(r#"a {% youtube id="abc" %} b"#, Some(&w)),
            r#"a <iframe src="https://x/abc"></iframe> b"#
        );
    }

    #[test]
    fn arg_values_are_html_escaped() {
        let w = widgets(&[("q", "<b>{x}</b>")]);
        assert_eq!(
            go(r#"{% q x="<script>&" %}"#, Some(&w)),
            "<b>&lt;script&gt;&amp;</b>"
        );
    }

    #[test]
    fn bodyless_argless_widget_still_works() {
        let w = widgets(&[("hr", "<hr class=fancy>")]);
        assert_eq!(go("{% hr %}", Some(&w)), "<hr class=fancy>");
    }

    #[test]
    fn used_records_the_widgets_that_fired() {
        let w = widgets(&[("a", "A{body}"), ("b", "B"), ("c", "C")]);
        let mut used = BTreeSet::new();
        expand(
            "{% a %}x{% enda %} {% b %}",
            "t",
            Some(&w),
            &mut used,
            |_, _| Ok(None),
            |_| None,
        )
        .unwrap();
        assert_eq!(used.iter().cloned().collect::<Vec<_>>(), ["a", "b"]);
    }
}
