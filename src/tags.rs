//! Expansion of the liquid-shaped constructs that appear in post bodies and
//! tree pages.
//!
//! A targeted expander, not a liquid implementation. Post bodies use exactly
//! one tag — `{% image %}` (194 uses / 68 posts) — plus `{{ site.baseurl }}`
//! and its `| prepend:` form (12). `{% post_url %}` was the second until the
//! q51 merge retired it: it was a foreign key into `posts.by_name`, and that
//! index was the only thing requiring a post's `rel` to be collection-relative
//! while a page's is root-relative. Its 51 uses are now ordinary file-relative
//! links, resolved by `links::resolve` like every other source link.
//!
//! `{% view %}` and `{% include %}` are grackle's own, added for `/` (§5c).
//! Each is a whole recognised construct rather than a step toward a template
//! language: `{% include %}` refuses parameters, and `{% view %}` dispatches to
//! a layout kind rather than exposing rows for a template to iterate.
//!
//! Anything unrecognised is emitted verbatim, so an unimplemented construct
//! shows up in the output instead of silently evaluating to nothing.

use crate::db::SiteDb;
use crate::render::Site;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Ctx<'a> {
    pub db: &'a SiteDb,
    pub baseurl: &'a str,
    /// Source path, for error messages that name the file.
    pub source: String,
    /// Where `{% include %}` resolves names. None disables the tag.
    pub includes: Option<PathBuf>,
    pub site: Option<&'a Site<'a>>,
    /// `{% image %}` source path -> the thumbnail's published URL (§6b). When
    /// absent, `{% image %}` falls back to linking the original at full size.
    pub thumbs: Option<&'a HashMap<String, String>>,
    /// The theme, for embedded views ({% view %}) that render through
    /// fragments. None disables embedding.
    pub theme: Option<&'a crate::theme::Theme>,
    /// Custom block widgets: `name → wrapper template with a {body} hole`
    /// (§5d). None disables paired tags.
    pub widgets: Option<&'a std::collections::BTreeMap<String, String>>,
    /// q45: the landing's route-aware self-embed — when a claimed row
    /// places `{% view <owner> %}`, the owning view is not looked up (it
    /// is materialized, not embeddable): THIS route's already-rendered
    /// slice substitutes instead.
    pub embed: Option<(&'a str, &'a str)>,
}

impl<'a> Ctx<'a> {
    pub fn new(db: &'a SiteDb, baseurl: &'a str, source: impl Into<String>) -> Self {
        Ctx {
            db,
            baseurl,
            source: source.into(),
            includes: None,
            site: None,
            thumbs: None,
            theme: None,
            widgets: None,
            embed: None,
        }
    }
}

/// The source path in `{% image [left|right|inline] SRC %}`, mode stripped.
/// The one place the mode-or-source parse lives, shared by rendering and the
/// thumbnail pre-pass so the two cannot disagree on what a source is.
fn image_src(arg: &str) -> Option<&str> {
    let mut parts = arg.split_whitespace();
    let first = parts.next()?;
    let src = match first {
        "left" | "right" | "inline" => parts.next()?,
        other => other,
    };
    (!src.is_empty()).then_some(src)
}

/// Every `{% image %}` source in a body, for the thumbnail pre-pass (build.rs).
/// Mirrors `expand`'s tag scan so it sees exactly the tags rendering will.
pub fn image_sources(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("{%") {
        let after = &rest[i + 2..];
        let Some(end) = after.find("%}") else { break };
        let inner = after[..end].trim();
        if let Some(arg) = inner
            .strip_prefix("image")
            .filter(|a| a.starts_with(char::is_whitespace))
        {
            if let Some(src) = image_src(arg.trim()) {
                out.push(src.to_string());
            }
        }
        rest = &after[end + 2..];
    }
    out
}

/// `{% image [left|right|inline] path %}`
///
/// The anchor links the full-size original; the `<img>` shows the thumbnail
/// (§6b) when the pre-pass generated one, else the original. The mode maps to a
/// float class, which is the contract the theme styles against (§5a).
fn image(arg: &str, cx: &Ctx) -> Result<String> {
    let mut parts = arg.split_whitespace();
    let first = parts.next().unwrap_or_default();
    let (mode, src) = match first {
        "left" => ("floatleft", parts.next().unwrap_or_default()),
        "right" => ("floatright", parts.next().unwrap_or_default()),
        "inline" => ("inline", parts.next().unwrap_or_default()),
        other => ("standard", other),
    };
    if src.is_empty() {
        bail!("{}: {{% image %}} with no source", cx.source);
    }
    let img_src = match cx.thumbs.and_then(|t| t.get(src)) {
        Some(url) => url.clone(),
        None => format!("{}/{}", cx.baseurl, src),
    };
    Ok(format!(
        "<a class='image {mode}' href='{b}/{src}'><img src='{img_src}' alt=''></a>",
        b = cx.baseurl,
    ))
}

/// `{% view latest %}` -> a routeless view, rendered by its declared layout.
///
/// The seam between the database and the page (DESIGN.md §5c). The view owns
/// the query and names a layout kind; this only looks the rows up and
/// dispatches. Nothing here knows what "latest" means — change the filter in
/// `grackle.toml` and this code does not move.
fn view(name: &str, cx: &Ctx) -> Result<String> {
    use crate::config::Kind;
    let name = name.trim();
    // q45: inside a landing's claimed content, the owning view embeds as
    // this route's slice — page 2 renders page 2's rows, /fr/ the French
    // partition.
    if let Some((owner, html)) = cx.embed {
        if owner == name {
            return Ok(html.to_string());
        }
    }
    let Some(v) = cx.db.views.get(name) else {
        bail!(
            "{}: {{% view {name} %}} matches no routeless view. \
             A view with a route is materialized, not embedded.",
            cx.source
        );
    };
    let theme = cx.theme.ok_or_else(|| {
        anyhow::anyhow!("{}: {{% view {name} %}} needs a theme context", cx.source)
    })?;
    match v.layout.as_deref() {
        // Bare titled links — posts and pages embed alike.
        Some("link_list") => {
            let pairs: Vec<(String, String)> = match v.table {
                Kind::Posts => v
                    .members
                    .iter()
                    .map(|&i| {
                        let p = &cx.db.rows[i];
                        (p.title.clone().unwrap_or_default(), p.url.clone())
                    })
                    .collect(),
                Kind::Tree => v
                    .members
                    .iter()
                    .map(|&i| {
                        let p = &cx.db.rows[i];
                        (p.title.clone().unwrap_or_default(), p.url.clone())
                    })
                    .collect(),
                Kind::Objects => bail!(
                    "{}: view {name} ranges over objects, which link_list cannot show",
                    cx.source
                ),
            };
            Ok(theme.fragments.render(&crate::parts::link_list(&pairs)))
        }
        // One featured row as a card — the homepage's book of the month.
        Some("card") => {
            if v.table != Kind::Tree {
                bail!(
                    "{}: view {name}: card embedding is for tree rows",
                    cx.source
                );
            }
            let Some(&i) = v.members.first() else {
                return Ok(String::new());
            };
            let p = &cx.db.rows[i];
            let src = p
                .hero_source()
                .and_then(|s| cx.thumbs.and_then(|t| t.get(s)))
                .cloned();
            let c = crate::parts::CardRow {
                title: p.title.clone().unwrap_or_default(),
                url: p.url.clone(),
                src,
                dims: None,
                note: p.description.clone(),
            };
            Ok(theme
                .fragments
                .render_with(&crate::parts::card(&c), v.variant.as_deref()))
        }
        Some(other) => bail!(
            "{}: view {name} has layout {other:?}, which is not embeddable",
            cx.source
        ),
        None => bail!(
            "{}: view {name} is query-only (no layout), so it cannot be embedded",
            cx.source
        ),
    }
}

/// `{% include social.html %}` — parameterless only.
///
/// The layouts use the parameterised form (`{% include article.html
/// margin_html=... %}`), and supporting that is the first step to writing a
/// template engine. So arguments are a hard error rather than a quiet
/// half-implementation (§5c).
fn include(arg: &str, cx: &Ctx) -> Result<String> {
    let arg = arg.trim();
    if arg.contains('=') {
        bail!(
            "{}: {{% include {arg} %}} passes parameters, which are deliberately \
             unsupported — parameterless includes only",
            cx.source
        );
    }
    let Some(dir) = &cx.includes else {
        bail!(
            "{}: {{% include {arg} %}} but no includes directory is configured",
            cx.source
        );
    };
    let path = dir.join(arg);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("{}: {{% include {arg} %}} -> {}", cx.source, path.display()))?;
    // Includes are expanded in their own right, so a partial may use the same
    // tags a page can. Depth is bounded by the filesystem, not by a counter:
    // an include cycle would recurse, which no partial in the corpus does.
    let inner = Ctx {
        source: path.display().to_string(),
        ..cx.clone()
    };
    expand(&text, &inner)
}

/// `{{ '/blog' | prepend: site.baseurl }}` -> `/blog`.
///
/// The other half of `site.baseurl`: 12 uses across the corpus, all of this
/// exact shape. Recognised as a whole rather than by implementing liquid's
/// filter pipeline.
fn prepend_baseurl(inner: &str, cx: &Ctx) -> Option<String> {
    let (lit, rest) = inner.split_once('|')?;
    let lit = lit.trim();
    let lit = lit.strip_prefix('\'')?.strip_suffix('\'')?;
    let rest = rest.trim();
    if rest != "prepend: site.baseurl" {
        return None;
    }
    Some(format!("{}{lit}", cx.baseurl))
}

/// Expand the known tags. Anything else is left alone rather than guessed at.
/// Find `{% end<name> %}` in `s`: returns (body end, index after the end
/// tag). Tokenized the same way as the main scan, so spacing inside the tag
/// doesn't matter.
fn find_end_tag(s: &str, name: &str) -> Option<(usize, usize)> {
    let want = format!("end{name}");
    let mut idx = 0;
    while let Some(i) = s[idx..].find("{%") {
        let start = idx + i;
        let close = s[start..].find("%}")? + start;
        if s[start + 2..close].trim() == want {
            return Some((start, close + 2));
        }
        idx = close + 2;
    }
    None
}

pub fn expand(body: &str, cx: &Ctx) -> Result<String> {
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

        // Paired widget tags (§5d custom widgets): `{% name %}` opens a
        // markdown body closed by `{% endname %}`. The body is expanded in
        // its own right (images work inside a callout), then spliced into
        // the widget's wrapper template at `{body}` — a named expansion with
        // a body, not control flow. Registered widgets with no end tag are
        // an author error, loudly; unregistered names stay verbatim below.
        if is_tag {
            if let Some(tmpl) = cx.widgets.and_then(|w| w.get(inner.as_str())) {
                let after = &rest[next + end + close.len()..];
                let Some((body_end, resume)) = find_end_tag(after, &inner) else {
                    bail!(
                        "{}: {{% {inner} %}} has no matching {{% end{inner} %}}",
                        cx.source
                    );
                };
                let body = expand(after[..body_end].trim(), cx)?;
                out.push_str(&tmpl.replace("{body}", &body));
                rest = &after[resume..];
                continue;
            }
        }

        let replacement = if is_tag {
            match inner.split_once(char::is_whitespace) {
                Some(("image", arg)) => Some(image(arg.trim(), cx)?),
                Some(("view", arg)) => Some(view(arg, cx)?),
                Some(("include", arg)) => Some(include(arg, cx)?),
                _ => None,
            }
        } else if inner == "site.baseurl" {
            Some(cx.baseurl.to_string())
        } else {
            prepend_baseurl(&inner, cx)
        };

        match replacement {
            Some(r) => out.push_str(&r),
            // Unknown construct: emit verbatim rather than silently dropping it.
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

    fn ctx(db: &SiteDb) -> Ctx<'_> {
        Ctx::new(db, "", "test.md")
    }

    #[test]
    fn passes_through_plain_text() {
        let db = SiteDb::default();
        assert_eq!(expand("hello world", &ctx(&db)).unwrap(), "hello world");
    }

    #[test]
    fn expands_baseurl() {
        let db = SiteDb::default();
        let c = Ctx::new(&db, "/pre", "t");
        assert_eq!(
            expand("a {{ site.baseurl }}/x b", &c).unwrap(),
            "a /pre/x b"
        );
    }

    #[test]
    fn image_modes_map_to_classes() {
        let db = SiteDb::default();
        let c = ctx(&db);
        assert!(expand("{% image right a/b.png %}", &c)
            .unwrap()
            .contains("class='image floatright'"));
        assert!(expand("{% image left a/b.png %}", &c)
            .unwrap()
            .contains("class='image floatleft'"));
        assert!(expand("{% image a/b.png %}", &c)
            .unwrap()
            .contains("class='image standard'"));
        // With no thumbnail map, the img falls back to the full-size original.
        assert!(expand("{% image a/b.png %}", &c)
            .unwrap()
            .contains("src='/a/b.png'"));
    }

    #[test]
    fn image_uses_thumbnail_url_when_present() {
        let db = SiteDb::default();
        let mut map = HashMap::new();
        map.insert("a/b.png".to_string(), "/static/deadbeef.jpg".to_string());
        let c = Ctx {
            thumbs: Some(&map),
            ..Ctx::new(&db, "", "t")
        };
        let out = expand("{% image right a/b.png %}", &c).unwrap();
        // Thumbnail in the <img>, full-size original still in the <a href>.
        assert!(out.contains("<img src='/static/deadbeef.jpg'"), "{out}");
        assert!(out.contains("href='/a/b.png'"), "{out}");
    }

    #[test]
    fn image_sources_finds_every_tag_and_strips_mode() {
        let body = "x {% image a.png %} y {% image left dir/b.jpg %} z {% image inline c.gif %}";
        assert_eq!(image_sources(body), vec!["a.png", "dir/b.jpg", "c.gif"]);
    }

    #[test]
    fn unknown_tags_survive_rather_than_vanish() {
        let db = SiteDb::default();
        let s = expand("x {% highlight ruby %} y", &ctx(&db)).unwrap();
        assert!(s.contains("{% highlight ruby %}"), "{s}");
    }

    #[test]
    fn prepend_baseurl_form_expands() {
        let db = SiteDb::default();
        let c = Ctx::new(&db, "/pre", "t");
        assert_eq!(
            expand(
                "<a href=\"{{ '/blog' | prepend: site.baseurl }}\">x</a>",
                &c
            )
            .unwrap(),
            "<a href=\"/pre/blog\">x</a>"
        );
    }

    /// An unrecognised filter pipeline must survive, not render half-right.
    #[test]
    fn other_filter_pipelines_are_left_alone() {
        let db = SiteDb::default();
        let s = expand("{{ page.title | escape }}", &ctx(&db)).unwrap();
        assert!(s.contains("{{ page.title | escape }}"), "{s}");
    }

    #[test]
    fn view_must_name_a_routeless_view() {
        let db = SiteDb::default();
        let e = expand("{% view nope %}", &ctx(&db))
            .unwrap_err()
            .to_string();
        assert!(e.contains("matches no routeless view"), "{e}");
        assert!(e.contains("test.md"), "{e}");
    }

    /// The slippery slope guard: parameters are the first step to a template
    /// engine, so they fail loudly rather than being half-supported.
    #[test]
    fn parameterised_include_is_a_hard_error() {
        let db = SiteDb::default();
        let c = ctx(&db);
        let e = expand("{% include article.html margin_html=x %}", &c)
            .unwrap_err()
            .to_string();
        assert!(e.contains("deliberately"), "{e}");
    }

    #[test]
    fn include_without_a_configured_dir_is_an_error() {
        let db = SiteDb::default();
        let e = expand("{% include social.html %}", &ctx(&db))
            .unwrap_err()
            .to_string();
        assert!(e.contains("no includes directory"), "{e}");
    }
}

#[cfg(test)]
mod widget_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn widgets() -> BTreeMap<String, String> {
        let mut w = BTreeMap::new();
        w.insert(
            "callout".to_string(),
            "<callout>\n<div>\n\n{body}\n\n</div>\n</callout>\n".to_string(),
        );
        w
    }

    #[test]
    fn widget_splices_body_into_wrapper() {
        let db = SiteDb::default();
        let w = widgets();
        let cx = Ctx {
            widgets: Some(&w),
            ..Ctx::new(&db, "", "t")
        };
        let out = expand("a\n\n{% callout %}\n**bold**\n{% endcallout %}\n\nb", &cx).unwrap();
        assert_eq!(
            out,
            "a\n\n<callout>\n<div>\n\n**bold**\n\n</div>\n</callout>\n\n\nb"
        );
    }

    #[test]
    fn widget_body_is_expanded_recursively() {
        let db = SiteDb::default();
        let w = widgets();
        let cx = Ctx {
            widgets: Some(&w),
            ..Ctx::new(&db, "/b", "t")
        };
        let out = expand(
            "{% callout %}\nsee {{ site.baseurl }}/x\n{% endcallout %}",
            &cx,
        )
        .unwrap();
        assert!(out.contains("see /b/x"), "{out}");
    }

    #[test]
    fn unterminated_widget_is_an_error_naming_the_source() {
        let db = SiteDb::default();
        let w = widgets();
        let cx = Ctx {
            widgets: Some(&w),
            ..Ctx::new(&db, "", "post.md")
        };
        let e = expand("{% callout %}\nnever closed", &cx)
            .unwrap_err()
            .to_string();
        assert!(e.contains("post.md"), "{e}");
        assert!(e.contains("endcallout"), "{e}");
    }

    #[test]
    fn unregistered_paired_tag_stays_verbatim() {
        let db = SiteDb::default();
        let cx = Ctx::new(&db, "", "t");
        let src = "{% callout %}\nx\n{% endcallout %}";
        assert_eq!(expand(src, &cx).unwrap(), src);
    }
}
