//! Expansion of the liquid-shaped constructs that appear in post bodies and
//! tree pages.
//!
//! A targeted expander, not a liquid implementation. The *scan* is generic
//! ([`crate::markup::scan`]); this module is the engine handlers — `{% image %}`,
//! `{% view %}`, `{% include %}`, `{{ site.baseurl }}`.

use crate::markup::scan;
use crate::model::SiteDb;
use crate::render::Site;
use anyhow::{bail, Context, Result};
use grackle_model::{Ask, Rendition};
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
    /// One demand — a source path and the rendition it asked for — to the
    /// output the pre-pass materialized for it (§6b, IO.md §4a): the published
    /// URL, and the dimensions that let the tag emit `width`/`height` so a body
    /// image reserves its space (q26). Keyed by the whole ASK, because two asks
    /// for one image are two outputs. When absent, `{% image %}` falls back to
    /// linking the original at full size.
    pub thumbs: Option<&'a crate::thumbs::Renditions>,
    /// The theme, for embedded views ({% view %}) that render through
    /// fragments. None disables embedding.
    pub theme: Option<&'a crate::theme::Theme>,
    /// Custom widgets by name. None disables them.
    pub widgets: Option<&'a std::collections::BTreeMap<String, crate::config::WidgetDef>>,
    /// Site config — archive pills and `{% view %}` member fill need it.
    pub cfg: Option<&'a crate::config::Config>,
    /// IO.md §4a: the address book, for the affordances this expander
    /// generates. None leaves `{% image %}` with the source path it was given,
    /// which is what every caller that has no `LinkSpace` (the unit tests) is
    /// asking for.
    pub links: Option<&'a crate::links::LinkSpace>,
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
            cfg: None,
            links: None,
            embed: None,
        }
    }
}

/// What one `{% image %}` demands — defined in [`grackle_model::ImageTag`].
pub use grackle_model::ImageTag;

/// Parse `{% image [left|right|inline] SRC [width=N] %}`.
///
/// `Err` carries the complaint without file context, because the two callers
/// have different ones: `image` names the source file it is rendering, and the
/// pre-pass has no message to give (the render pass will reach the same tag and
/// say so properly).
fn image_tag(arg: &str) -> Result<ImageTag, String> {
    let mut parts = arg.split_whitespace();
    let first = parts.next().unwrap_or_default();
    let (mode, src) = match first {
        "left" => ("floatleft", parts.next().unwrap_or_default()),
        "right" => ("floatright", parts.next().unwrap_or_default()),
        "inline" => ("inline", parts.next().unwrap_or_default()),
        other => ("standard", other),
    };
    if src.is_empty() {
        return Err("{% image %} with no source".into());
    }
    // The ask. Absent, a citation gets the engine's default rendition, which
    // is what every one of the corpus's 194 uses does and why they all keep
    // their addresses.
    let mut rendition = Rendition::THUMB;
    for tok in parts {
        // A bare trailing token used to be ignored silently; an ask is a
        // parameter and a misspelt parameter must not read as "no ask" —
        // that would publish a different rendition than the author wrote.
        let Some((key, value)) = tok.split_once('=') else {
            return Err(format!(
                "{{% image %}}: {tok:?} is not a parameter — the form is `width=N`"
            ));
        };
        match key {
            "width" => {
                let w: u32 = value
                    .parse()
                    .map_err(|_| format!("{{% image %}}: width={value:?} is not a pixel count"))?;
                if w == 0 {
                    return Err("{% image %}: width=0 asks for nothing".into());
                }
                rendition = Rendition::width(w);
            }
            other => {
                return Err(format!(
                    "{{% image %}}: unknown parameter {other:?} — known: width"
                ))
            }
        }
    }
    Ok(ImageTag {
        mode,
        src: src.to_string(),
        rendition,
    })
}

/// Every rendition one body demands, for the pre-pass (build.rs): the source
/// and the parameters each `{% image %}` asked for.
///
/// Mirrors `expand`'s tag scan so it sees exactly the tags rendering will.
/// Unparseable tags are skipped rather than reported — `image` reaches the same
/// tag with the file name in hand and bails there.
pub fn image_asks(body: &str) -> Vec<Ask> {
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
            if let Ok(t) = image_tag(arg.trim()) {
                out.push(t.ask());
            }
        }
        rest = &after[end + 2..];
    }
    out
}

/// `{% image [left|right|inline] path [width=N] %}`
///
/// The anchor links the full-size original; the `<img>` shows the rendition
/// (§6b, §4a) the pre-pass materialized for this exact ask, else the original.
/// The mode maps to a float class, which is the contract the theme styles
/// against (§5a).
fn image(arg: &str, cx: &Ctx) -> Result<String> {
    let t = image_tag(arg).map_err(|e| anyhow::anyhow!("{}: {e}", cx.source))?;
    let (mode, src) = (t.mode, t.src.as_str());
    // The lookup is keyed by the WHOLE ask, so a page asking 256 and a page
    // asking 512 of one image reach two different outputs (IO.md §4a: an
    // image's rendition set is the union of its consumers' asks).
    let thumb = cx.thumbs.and_then(|m| m.get(&t.ask()));
    // IO.md §4a's lightbox chain, as far as I11 takes it. The `<a>` is an
    // EXPANSION AFFORDANCE — markup this expander generates, not a link a
    // human wrote — so it is entitled to the strong address, which is exactly
    // what the design's worked example says the full-size expansion uses. A
    // routed asset keeps its canonical address here, byte for byte, because a
    // routed output wins; only a row no rule routed reaches for the hash.
    //
    // The THIRD URL still waits, and I12 measured why rather than inheriting
    // the note: the example wants a download link at the canonical route
    // BESIDE the expansion, and that is a second element on 194 corpus tags —
    // a byte change on every post that shows a picture, which is Matt's call
    // and not a consequence of renditions. What renditions owed was the
    // parameters an affordance carries, and those landed: the ask above.
    let full = match cx.links.and_then(|s| s.strong_of_source(src)) {
        // No `baseurl` prefix, deliberately: a strong address is a published
        // address like `Row.url`, and `Row.url` carries none either. The two
        // agree, which is what lets the citation scan resolve this back to its
        // input through `by_strong`.
        Some(strong) => strong.to_string(),
        None => format!("{}/{}", cx.baseurl, src),
    };
    let img_src = match thumb {
        Some(t) => t.url.clone(),
        None => full.clone(),
    };
    // q26: dimensions on the element that ships, so the browser reserves the
    // box before the bytes land. Emitted only when the thumb pass measured
    // them — an unmeasured image gets no attributes rather than guessed ones.
    let dims = match thumb.and_then(|t| t.dims) {
        Some((w, h)) => format!(" width='{w}' height='{h}'"),
        None => String::new(),
    };
    Ok(format!(
        "<a class='image {mode}' href='{full}'><img src='{img_src}' alt=''{dims}></a>"
    ))
}

/// `{% view latest %}` -> a routeless view, rendered by its declared layout.
///
/// The seam between the database and the page (DESIGN.md §5c). Fences the
/// spliced output in marker comments so the link graph can tell an
/// arrangement's links from a human's citations (§6g Problem 2: the homepage's
/// recent-posts block is not a citation of every post it lists).
fn view(name: &str, cx: &Ctx) -> Result<String> {
    let html = view_inner(name, cx)?;
    Ok(format!("<!--grackle:view-->{html}<!--/grackle:view-->"))
}

/// The splice itself. The view owns the query and names a face (`layout`, or
/// `variant` when set); this looks the rows up and concatenates member faces
/// (THEME.md §3). The host theme must ship `row--{face}`.
fn view_inner(name: &str, cx: &Ctx) -> Result<String> {
    let mut name = name.trim();
    let mut layout_override: Option<&str> = None;
    if let Some((n, lay)) = name.split_once('|') {
        name = n.trim();
        layout_override = Some(lay.trim());
    }
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
    let layout = layout_override.or(v.layout.as_deref()).ok_or_else(|| {
        anyhow::anyhow!(
            "{}: view {name} is query-only (no layout), so it cannot be embedded",
            cx.source
        )
    })?;
    let none = crate::thumbs::Renditions::new();
    let thumbs = cx.thumbs.unwrap_or(&none);
    let Some(cfg) = cx.cfg else {
        bail!(
            "{}: {{% view {name} %}} needs a config context for member fill",
            cx.source
        );
    };
    let resolve_asset = |src: &str| -> String {
        crate::thumbs::default_of(thumbs, src)
            .map(|t| t.url.clone())
            .unwrap_or_else(|| crate::build::asset_url(cx.baseurl, src))
    };
    let mut items = Vec::new();
    for k in &v.members {
        let Some(p) = cx.db.rows.get(k) else {
            continue;
        };
        items.push(crate::parts::from_row(
            cfg,
            theme.schemas(),
            &resolve_asset,
            p,
            crate::parts::Presentation::default(),
            crate::parts::FillOpts {
                view: Some(name),
                thumbs: Some(thumbs),
                ..Default::default()
            },
        )?);
    }
    crate::assemble::chain::member_faces(theme, layout, v.variant.as_deref(), &items)
        .with_context(|| format!("{}: view {name}", cx.source))
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

pub fn expand(body: &str, cx: &Ctx) -> Result<String> {
    let mut used = std::collections::BTreeSet::new();
    expand_used(body, cx, &mut used)
}

/// Desugar `$$…$$` display math to `{% math %}…{% endmath %}` so the `math`
/// widget renders it (and pulls in its head). `$$` inside a fenced code block
/// is left alone. A `$$` outside code with no `math` widget defined is an
/// error, since the author asked for math the site cannot render.
pub fn desugar_math(body: &str, source: &str, has_math_widget: bool) -> Result<String> {
    if !body.contains("$$") {
        return Ok(body.to_string());
    }
    let mut out = String::with_capacity(body.len() + 64);
    let mut fence: Option<char> = None;
    let mut replaced = false;
    for line in body.split_inclusive('\n') {
        let t = line.trim_start();
        let opener = t
            .starts_with("```")
            .then_some('`')
            .or_else(|| t.starts_with("~~~").then_some('~'));
        match (fence, opener) {
            (Some(c), Some(o)) if c == o => {
                fence = None;
                out.push_str(line);
            }
            (Some(_), _) => out.push_str(line),
            (None, Some(o)) => {
                fence = Some(o);
                out.push_str(line);
            }
            (None, None) => replace_math(line, &mut out, &mut replaced),
        }
    }
    if replaced && !has_math_widget {
        bail!("{source}: `$$` math needs a `math` widget — declare one under [widgets]");
    }
    Ok(out)
}

/// Replace each `$$…$$` pair in one code-free span. An unpaired `$$` is left
/// verbatim.
fn replace_math(text: &str, out: &mut String, replaced: &mut bool) {
    let mut rest = text;
    while let Some(open) = rest.find("$$") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("$$") else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str("{% math %}");
        out.push_str(&after[..close]);
        out.push_str("{% endmath %}");
        *replaced = true;
        rest = &after[close + 2..];
    }
    out.push_str(rest);
}

/// Expand, recording into `used` which widgets fired so a caller can pull in
/// their head fragments.
pub fn expand_used(
    body: &str,
    cx: &Ctx,
    used: &mut std::collections::BTreeSet<String>,
) -> Result<String> {
    scan::expand(
        body,
        &cx.source,
        cx.widgets,
        used,
        |name, arg| match name {
            "image" => Ok(Some(image(arg, cx)?)),
            "view" => Ok(Some(view(arg, cx)?)),
            "include" => Ok(Some(include(arg, cx)?)),
            _ => Ok(None),
        },
        |inner| {
            if inner == "site.baseurl" {
                Some(cx.baseurl.to_string())
            } else {
                prepend_baseurl(inner, cx)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
        map.insert(
            Ask::thumb("a/b.png"),
            thumb("/static/deadbeef.jpg", Some((640, 480))),
        );
        let c = Ctx {
            thumbs: Some(&map),
            ..Ctx::new(&db, "", "t")
        };
        let out = expand("{% image right a/b.png %}", &c).unwrap();
        // Thumbnail in the <img>, full-size original still in the <a href>.
        assert!(out.contains("<img src='/static/deadbeef.jpg'"), "{out}");
        assert!(out.contains("href='/a/b.png'"), "{out}");
        // q26: measured dimensions ride the element that ships.
        assert!(out.contains("width='640' height='480'"), "{out}");
    }

    /// A `Thumb` standing in for the rendition pass's output.
    fn thumb(url: &str, dims: Option<(u32, u32)>) -> crate::thumbs::Thumb {
        crate::thumbs::Thumb {
            cache_path: std::path::PathBuf::new(),
            address: url.to_string(),
            url: url.to_string(),
            dims,
            rendition: Rendition::THUMB,
        }
    }

    #[test]
    fn image_omits_dimensions_when_the_thumb_pass_could_not_measure() {
        let db = SiteDb::default();
        let mut map = HashMap::new();
        map.insert(Ask::thumb("a/b.png"), thumb("/static/deadbeef.jpg", None));
        let c = Ctx {
            thumbs: Some(&map),
            ..Ctx::new(&db, "", "t")
        };
        let out = expand("{% image a/b.png %}", &c).unwrap();
        // Unmeasured means no attributes, never guessed ones.
        assert!(!out.contains("width="), "{out}");
        assert!(!out.contains("height="), "{out}");
    }

    #[test]
    fn image_asks_finds_every_tag_and_strips_mode() {
        let body = "x {% image a.png %} y {% image left dir/b.jpg %} z {% image inline c.gif %}";
        assert_eq!(
            image_asks(body),
            vec![
                Ask::thumb("a.png"),
                Ask::thumb("dir/b.jpg"),
                Ask::thumb("c.gif"),
            ]
        );
    }

    /// **The citing edge carries the parameters** (IO.md §4a): the ask is
    /// written where the citation is, and two pages asking different widths of
    /// one image are two demands rather than one.
    ///
    /// The pre-pass and the renderer read the SAME parse, which is what keeps
    /// them from materializing one rendition and embedding another.
    #[test]
    fn an_image_tag_carries_its_ask() {
        assert_eq!(
            image_asks("{% image right cover.png width=256 %}"),
            vec![Ask::new("cover.png", Rendition::width(256))]
        );
        // Two asks of one source: a set of two, which is the demand union.
        assert_eq!(
            image_asks("{% image a.png width=256 %} {% image a.png width=512 %}").len(),
            2
        );

        // …and the renderer reaches the output made for its own ask.
        let db = SiteDb::default();
        let mut map = HashMap::new();
        map.insert(
            Ask::new("a.png", Rendition::width(256)),
            thumb("/static/small.jpg", None),
        );
        map.insert(Ask::thumb("a.png"), thumb("/static/big.jpg", None));
        let c = Ctx {
            thumbs: Some(&map),
            ..Ctx::new(&db, "", "t")
        };
        assert!(expand("{% image a.png width=256 %}", &c)
            .unwrap()
            .contains("src='/static/small.jpg'"));
        assert!(expand("{% image a.png %}", &c)
            .unwrap()
            .contains("src='/static/big.jpg'"));
    }

    /// A misspelt ask must not read as "no ask": that would silently publish
    /// and embed a rendition the author did not write. The trailing token used
    /// to be ignored, and this is the refusal that replaced the silence.
    ///
    /// Mutation, red and restored: return `Ok` from the `split_once('=')` arm
    /// (ignore the token, as before) → all three cases render a default
    /// thumbnail without complaint.
    #[test]
    fn an_unparseable_ask_is_an_error_naming_the_file() {
        let db = SiteDb::default();
        let c = ctx(&db);
        for (tag, want) in [
            ("{% image a.png 256 %}", "not a parameter"),
            ("{% image a.png height=90 %}", "unknown parameter"),
            ("{% image a.png width=wide %}", "not a pixel count"),
            ("{% image a.png width=0 %}", "asks for nothing"),
        ] {
            let e = expand(tag, &c).unwrap_err().to_string();
            assert!(e.contains(want), "{tag}: {e}");
            assert!(e.contains("test.md"), "{tag}: {e}");
        }
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

    fn widgets() -> BTreeMap<String, crate::config::WidgetDef> {
        let mut w = BTreeMap::new();
        w.insert(
            "callout".to_string(),
            crate::config::WidgetDef {
                template: "<callout>\n<div>\n\n{body}\n\n</div>\n</callout>\n".to_string(),
                head: None,
            },
        );
        w
    }

    #[test]
    fn dollar_math_desugars_to_the_math_widget() {
        let out = desugar_math("see $$a+b$$ now", "t", true).unwrap();
        assert_eq!(out, "see {% math %}a+b{% endmath %} now");
    }

    #[test]
    fn math_inside_a_code_fence_is_left_alone() {
        let src = "before\n```\n$$x$$\n```\nafter $$y$$";
        let out = desugar_math(src, "t", true).unwrap();
        assert!(out.contains("```\n$$x$$\n```"), "{out}");
        assert!(out.contains("after {% math %}y{% endmath %}"), "{out}");
    }

    #[test]
    fn math_without_a_math_widget_is_an_error() {
        let e = desugar_math("a $$b$$ c", "post.md", false)
            .unwrap_err()
            .to_string();
        assert!(e.contains("post.md") && e.contains("widget"), "{e}");
    }

    #[test]
    fn an_unpaired_dollar_dollar_is_left_verbatim() {
        assert_eq!(desugar_math("a $$ b", "t", true).unwrap(), "a $$ b");
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
