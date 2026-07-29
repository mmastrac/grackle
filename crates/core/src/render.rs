//! Presentation: head facts + serializations.
//!
//!   schema  -> head facts (computed, never branched; §5a)
//!   layout  -> part maps (parts.rs, §5e)
//!   theme   -> fragments + css (themes/<name>/, theme.rs, §5e)
//!   feed/sitemap -> serializations, no look (below)
//!
//! What remains here is what has no theme: the computed `<head>` facts, the
//! `light` tier's minimal wrapper (§5g "Row tiers" — a tier, not the null
//! theme, which takes the full head), and the XML serializations.

use anyhow::{Context, Result};

use crate::config::Config;
use crate::model::Row;
use std::fmt::Write as _;

pub use grackle_model::{Alternate, Tag};

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Typed facts derived from a row's schema. A theme renders the subset it
/// wants; nobody branches on "am I a post" (§5a).
#[derive(Debug, Default)]
pub struct Head {
    pub title: String,
    /// Declared head tags, already evaluated (§4e). Empty values are dropped
    /// before they get here, so emitting is a loop with no decision in it —
    /// the engine no longer knows that one of these is called `robots`, or
    /// `og:title`, or what makes any of them appear.
    pub meta: Vec<(Tag, String, String)>,
    pub jsonld: Option<String>,
    /// q53 axis members: alternative FORMS of this row, each an absolute URL
    /// with an optional `hreflang` (the locale axis) OR an optional media `type`
    /// (a different-format form, e.g. the md twin). A same-format restyle — a
    /// theme member — carries neither: it is the same representation at another
    /// URL, and `rel="canonical"` already names the one that counts.
    ///
    /// Locale hreflang comes from a declared expand (`[html.head.link]
    /// alternate = { from = "axis.locale", … }`); other axis forms still land
    /// here from the engine until they get their own expands.
    pub alternates: Vec<Alternate>,
}

pub struct Site<'a> {
    pub url: &'a str,
    pub title: &'a str,
    pub author: &'a str,
    pub email: Option<&'a str>,
    /// The site icon's published URL, or empty when the tree has none
    /// (§4d). Resolved from the ROUTE set rather than from a filename, so
    /// pinning an icon that lives elsewhere is an ordinary named object
    /// route (§4) and needs no key of its own. Empty is the ordinary case
    /// and every consumer drops its tag, which is §5e's rule 2 again.
    pub icon: &'a str,
}

pub fn head_for_post(p: &Row, site: &Site) -> Head {
    let canonical = format!("{}{}", site.url, p.url);
    let published = p.date.map(xmlschema);
    let jsonld = published.as_ref().map(|ts| {
        let mut j = String::new();
        let _ = write!(
            j,
            r#"{{"@context":"https://schema.org","@type":"BlogPosting","headline":{},"mainEntityOfPage":{{"@type":"WebPage","@id":{}}},"url":{},"datePublished":"{ts}","dateModified":"{ts}","author":{{"@type":"Person","name":{},"url":{}}},"publisher":{{"@type":"Person","name":{}}}"#,
            json_str(p.title.as_deref().unwrap_or_default()),
            json_str(&canonical),
            json_str(&canonical),
            json_str(site.author),
            json_str(&format!("{}/", site.url)),
            json_str(site.author),
        );
        if let Some(d) = p.string("description") {
            let _ = write!(j, r#","description":{}"#, json_str(d));
        }
        j.push('}');
        j
    });
    Head {
        title: p.title.clone().unwrap_or_default(),
        meta: Vec::new(),
        jsonld,
        alternates: Vec::new(),
    }
}

/// The declared head (§4e), compiled once.
///
/// One set, not one per surface, because the environment is the HEAD's
/// vocabulary rather than whatever happens to be underneath it: `title` and
/// `url` are what the head is being built for, `site.*` is config, and the
/// rest is the row. A listing has no `description`, so that name reads Null
/// there, the expression yields empty, and no tag is emitted — the conditional
/// that used to be an `if let Some(d)` in Rust.
#[derive(Debug, Default)]
pub struct Metas(pub Vec<Decl>);

/// Declared `[html.html.attribute]` / `[html.body.attribute]` expressions,
/// compiled once (§4e).
#[derive(Debug, Default)]
pub struct HtmlAttrs {
    pub html: Vec<(String, crate::filter::Text)>,
    pub body: Vec<(String, crate::filter::Text)>,
}

/// One declared head tag: a single expression, or an expand over a pool.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // `Text` is the CEL payload; boxing it would cost a hop on every tag.
pub enum Decl {
    /// One tag whose value is a CEL text expression.
    Single {
        tag: Tag,
        key: String,
        expr: crate::filter::Text,
    },
    /// One tag per member of `from`; attributes are CEL text expressions.
    Expand {
        tag: Tag,
        key: String,
        from: String,
        attrs: Vec<(String, crate::filter::Text)>,
    },
}

/// The row a head expression actually reads: the row itself, plus `site.*`.
///
/// A head says things about the site as often as about the row — the author,
/// the absolute URL a canonical link is built from — and none of that is a
/// row field. Rather than teach the evaluator about config, the environment
/// grows three names and the row answers them.
struct HeadRow<'a, R: crate::filter::Row + ?Sized> {
    row: &'a R,
    site: &'a Site<'a>,
    /// The two the head knows for itself. A listing's title is computed, not a
    /// route column, so without these `og:title` would work on posts and
    /// vanish on listings — which is precisely the silent asymmetry this
    /// design is meant to make impossible.
    title: &'a str,
    url: &'a str,
}

impl<R: crate::filter::Row + ?Sized> crate::filter::Row for HeadRow<'_, R> {
    fn field(&self, name: &str) -> crate::filter::Value {
        use crate::filter::Value as V;
        match name {
            "site.url" => V::Str(self.site.url.to_string()),
            "site.title" => V::Str(self.site.title.to_string()),
            "site.author" => V::Str(self.site.author.to_string()),
            "site.icon" => V::Str(self.site.icon.to_string()),
            "title" => V::Str(self.title.to_string()),
            "url" => V::Str(self.url.to_string()),
            other => self.row.field(other),
        }
    }
}

/// Attribute expressions share the head vocabulary, plus pairing-axis
/// fallback so an unstamped canonical member still resolves.
struct AttrRow<'a, R: crate::filter::Row + ?Sized> {
    row: &'a R,
    cfg: &'a Config,
    site: &'a Site<'a>,
    title: &'a str,
    url: &'a str,
}

impl<R: crate::filter::Row + ?Sized> crate::filter::Row for AttrRow<'_, R> {
    fn field(&self, name: &str) -> crate::filter::Value {
        use crate::filter::Value as V;
        match name {
            "site.url" => V::Str(self.site.url.to_string()),
            "site.title" => V::Str(self.site.title.to_string()),
            "site.author" => V::Str(self.site.author.to_string()),
            "site.icon" => V::Str(self.site.icon.to_string()),
            "title" => V::Str(self.title.to_string()),
            "url" => V::Str(self.url.to_string()),
            other => {
                let v = self.row.field(other);
                let empty = match &v {
                    V::Null => true,
                    V::Str(s) if s.is_empty() => true,
                    _ => false,
                };
                if empty {
                    let pairing_field = self
                        .cfg
                        .pairing_axis()
                        .map(|(_, a)| a.field.as_str())
                        .or(self.cfg.i18n.axis.as_deref());
                    if pairing_field == Some(other) {
                        return V::Str(self.cfg.pairing_member(self.row));
                    }
                }
                v
            }
        }
    }
}

/// The `site.*` names, added to whichever schema a head expression is
/// checked against.
fn with_site(mut s: crate::filter::Schema) -> crate::filter::Schema {
    for n in ["site.url", "site.title", "site.author", "site.icon"] {
        s.insert(n, crate::filter::Type::Str);
    }
    s
}

pub fn compile_metas(cfg: &Config, declared: &crate::filter::Schema) -> Result<Metas> {
    use crate::config::HeadEntry;
    let mut env = grackle_model::row_schema();
    for (k, t) in declared {
        env.insert(k, *t);
    }
    let env = with_site(env);
    let mut out = Vec::new();
    let h = &cfg.html.head;
    let tables = [
        (Tag::Meta, "[html.head.meta]", &h.meta),
        (Tag::Property, "[html.head.property]", &h.property),
        (Tag::Link, "[html.head.link]", &h.link),
    ];
    for (tag, whose, table) in tables {
        for (key, entry) in table {
            match entry {
                HeadEntry::Expr(src) => {
                    let expr = crate::filter::Text::parse(src, &env)
                        .with_context(|| format!("{whose} {key}"))?;
                    out.push(Decl::Single {
                        tag,
                        key: key.clone(),
                        expr,
                    });
                }
                HeadEntry::Expand(exp) => {
                    anyhow::ensure!(
                        !exp.attrs.is_empty(),
                        "{whose} {key}: an expand needs at least one attribute \
                         expression (e.g. href)"
                    );
                    anyhow::ensure!(
                        exp.attrs.contains_key("href"),
                        "{whose} {key}: an expand needs an href expression"
                    );
                    let mut attrs = Vec::new();
                    for (attr, src) in &exp.attrs {
                        let expr = crate::filter::Text::parse(src, &env)
                            .with_context(|| format!("{whose} {key}.{attr}"))?;
                        attrs.push((attr.clone(), expr));
                    }
                    out.push(Decl::Expand {
                        tag,
                        key: key.clone(),
                        from: exp.from.clone(),
                        attrs,
                    });
                }
            }
        }
    }
    Ok(Metas(out))
}

/// Compile `[html.html.attribute]` / `[html.body.attribute]` (§4e).
pub fn compile_attrs(cfg: &Config, declared: &crate::filter::Schema) -> Result<HtmlAttrs> {
    let mut env = grackle_model::row_schema();
    for (k, t) in declared {
        env.insert(k, *t);
    }
    let env = with_site(env);
    let compile = |whose: &str, table: &std::collections::BTreeMap<String, String>| {
        table
            .iter()
            .map(|(key, src)| {
                let expr = crate::filter::Text::parse(src, &env)
                    .with_context(|| format!("{whose} {key}"))?;
                Ok((key.clone(), expr))
            })
            .collect::<Result<Vec<_>>>()
    };
    Ok(HtmlAttrs {
        html: compile("[html.html.attribute]", &cfg.html.html.attribute)?,
        body: compile("[html.body.attribute]", &cfg.html.body.attribute)?,
    })
}

/// Evaluate a compiled attribute table, dropping empties. The pairing-axis
/// field (or the aspirational `[i18n] axis` name when that axis is not yet
/// declared) resolves through [`Config::pairing_member`] so a canonical /
/// monolingual page still yields `lang="en"` for `lang = 'locale'`.
pub fn eval_attrs(
    attrs: &[(String, crate::filter::Text)],
    cfg: &Config,
    row: &(impl crate::filter::Row + ?Sized),
    site: &Site,
    title: &str,
    url: &str,
) -> Vec<(String, String)> {
    let env = AttrRow {
        row,
        cfg,
        site,
        title,
        url,
    };
    attrs
        .iter()
        .filter_map(|(key, expr)| {
            let v = expr.eval(&env);
            (!v.is_empty()).then(|| (key.clone(), v))
        })
        .collect()
}

/// Evaluate the single-expression metas, dropping the empty ones — §5e's
/// rule 2 one layer up: an empty value emits no tag.
pub fn eval_metas(
    metas: &Metas,
    row: &impl crate::filter::Row,
    site: &Site,
    title: &str,
    url: &str,
) -> Vec<(Tag, String, String)> {
    let env = HeadRow {
        row,
        site,
        title,
        url,
    };
    metas
        .0
        .iter()
        .filter_map(|d| match d {
            Decl::Single { tag, key, expr } => {
                let v = expr.eval(&env);
                (!v.is_empty()).then(|| (*tag, key.clone(), v))
            }
            Decl::Expand { .. } => None,
        })
        .collect()
}

/// Evaluate expand declarations: `from = "axis.<name>"` resolves a member pool
/// per expand via `pool_for`. Attributes become an [`Alternate`]: `href`
/// required, `hreflang` / `type` optional. A pool of fewer than two members
/// emits nothing — the monolingual case.
pub fn eval_expands<'a>(
    metas: &Metas,
    site: &Site,
    title: &str,
    cfg: &crate::config::Config,
    mut pool_for: impl FnMut(&str) -> Vec<ExpandMember<'a>>,
) -> Vec<Alternate> {
    let mut out = Vec::new();
    for d in &metas.0 {
        let Decl::Expand { from, attrs, .. } = d else {
            continue;
        };
        let Some(axis_name) = from.strip_prefix("axis.") else {
            continue;
        };
        let Some(axis) = cfg.axes.get(axis_name) else {
            continue;
        };
        let field = axis.field.as_str();
        let members = pool_for(axis_name);
        if members.len() < 2 {
            continue;
        }
        for m in &members {
            let row = MemberRow {
                inner: m.row,
                field,
                member: m.member.as_str(),
            };
            let env = HeadRow {
                row: &row,
                site,
                title,
                url: m.url,
            };
            let mut href = String::new();
            let mut hreflang = None;
            let mut media_type = None;
            for (attr, expr) in attrs {
                let v = expr.eval(&env);
                if v.is_empty() {
                    continue;
                }
                match attr.as_str() {
                    "href" => href = v,
                    "hreflang" => hreflang = Some(v),
                    "type" => media_type = Some(v),
                    _ => {}
                }
            }
            if href.is_empty() {
                continue;
            }
            out.push(Alternate {
                href,
                hreflang,
                media_type,
            });
        }
    }
    out
}

/// One member of an expand pool.
pub struct ExpandMember<'a> {
    pub row: &'a dyn crate::filter::Row,
    pub url: &'a str,
    /// Axis member code — Route fields omit the default, so the pool carries
    /// it explicitly for expressions like `hreflang = 'locale'`.
    pub member: String,
}

/// Injects the expand axis field over an inner row so expressions see it even
/// when the underlying Route left the default unstamped.
struct MemberRow<'a> {
    inner: &'a dyn crate::filter::Row,
    field: &'a str,
    member: &'a str,
}

impl crate::filter::Row for MemberRow<'_> {
    fn field(&self, name: &str) -> crate::filter::Value {
        if name == self.field {
            return crate::filter::Value::Str(self.member.to_owned());
        }
        self.inner.field(name)
    }
}

impl Head {
    /// A head with nothing but a title — the base a filter builds over.
    fn empty(title: String) -> Head {
        Head {
            title,
            meta: Vec::new(),
            jsonld: None,
            alternates: Vec::new(),
        }
    }
}

/// A head with nothing computed: everything but the title, the hreflang list
/// and JSON-LD is declared now (§4e), so a listing's head IS its title.
pub fn head_simple(title: &str, _url: &str, _site: &Site) -> Head {
    Head::empty(title.to_string())
}

/// Title + evaluated metas for a listing/landing/page.
pub fn head_for(
    title: &str,
    url: &str,
    site: &Site,
    metas: &Metas,
    row: &impl crate::filter::Row,
) -> Head {
    let mut head = head_simple(title, url, site);
    head.meta = eval_metas(metas, row, site, title, url);
    head
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Default,
    Light,
}

/// §5g: the engine-owned ROOT HTML SHELL every theme inherits — doctype,
/// `<html>` stamped with the root kind and any subtheme tokens, `<head>`
/// from the computed facts, `<body>` from the theme's body chrome. A theme
/// may ship a document-shaped `root.html` (IO.md §6), but what it ships is
/// chrome and `<style>`: the skeleton is the engine's, and a fragmentless
/// (null) theme still produces a valid document.
pub fn root_shell(
    head: &str,
    html_attrs: &[(String, String)],
    body_attrs: &[(String, String)],
    subtheme: Option<&str>,
    profile: Option<&str>,
    axis: &[grackle_model::AxisMember],
    body: &str,
) -> String {
    let sub = subtheme
        .map(|s| format!(" data-subtheme=\"{}\"", esc(s)))
        .unwrap_or_default();
    let prof = profile
        .map(|p| format!(" data-profile=\"{}\"", esc(p)))
        .unwrap_or_default();
    // §4a / q53: axis members stamped as data-axis-* (select) and --axis-*
    // custom props (inherit); one style= holding every property.
    let ax = {
        let mut attrs = String::new();
        let mut styles = String::new();
        for a in axis {
            let name = esc(&a
                .axis
                .replace(|c: char| !c.is_alphanumeric() && c != '-', "-"));
            let value = esc(&a.value);
            let _ = write!(attrs, " data-axis-{name}=\"{value}\"");
            if !styles.is_empty() {
                styles.push(';');
            }
            let _ = write!(styles, "--axis-{name}:&quot;{value}&quot;");
        }
        if styles.is_empty() {
            attrs
        } else {
            format!("{attrs} style=\"{styles}\"")
        }
    };
    // Declared attributes (§4e): `[html.html.attribute]` / `[html.body.attribute]`.
    // Engine stamps (`data-kind`, subtheme, profile, axis) stay beside them.
    let declared = |attrs: &[(String, String)]| -> String {
        attrs
            .iter()
            .map(|(k, v)| format!(" {}=\"{}\"", esc(k), esc(v)))
            .collect()
    };
    let html_a = declared(html_attrs);
    let body_a = declared(body_attrs);
    format!(
        "<!doctype html>\n<html data-kind=\"root\"{html_a}{sub}{prof}{ax}>\n<head>{head}</head>\n<body{body_a}>\n{}\n</body>\n</html>\n",
        body.trim_end()
    )
}

/// The `light` head (§5e step 4): title and robots, nothing else — the
/// minimal facts subset, wrapped by the same root shell as everything.
pub fn light_head(head: &Head) -> String {
    // The `light` tier keeps the `[html.head.meta]` declarations and drops the
    // rest (§5g). The line is the ELEMENT, not a list of blessed names: a
    // `<meta name=…>` is a fact about the document, while Open Graph and a
    // canonical link are apparatus for describing it to other systems, and an
    // imported artifact wearing minimal chrome does not want the apparatus.
    //
    // The alternative was the engine deciding that `robots` is the one tag
    // `light` keeps — which is the §4e smell wearing a tier for a hat.
    let kept = Head {
        meta: head
            .meta
            .iter()
            .filter(|(tag, _, _)| *tag == Tag::Meta)
            .cloned()
            .collect(),
        ..Head::empty(head.title.clone())
    };
    format!(
        "\n\t<title>{}</title>{}\n\t<meta charset=\"utf-8\">\n",
        esc(&head.title),
        meta_tags(&kept)
    )
}

/// The declared metas, in declaration order. One loop, no names.
fn meta_tags(head: &Head) -> String {
    let mut out = String::new();
    for (tag, key, value) in &head.meta {
        let _ = match tag {
            Tag::Meta => write!(
                out,
                "\n\t<meta name=\"{}\" content=\"{}\">",
                esc(key),
                esc(value)
            ),
            Tag::Property => write!(
                out,
                "\n\t<meta property=\"{}\" content=\"{}\">",
                esc(key),
                esc(value)
            ),
            Tag::Link => write!(
                out,
                "\n\t<link rel=\"{}\" href=\"{}\">",
                esc(key),
                esc(value)
            ),
        };
    }
    out
}

/// The computed head facts as the shell's `head` part (§5a: a theme renders
/// the subset it wants; today's default takes them all). This fills
/// `<head data-slot="head">` in the theme's shell fragment. `css` is the
/// rendering theme's stylesheet URL — themes are per row (§5a), so the
/// link is too.
pub fn head_html(head: &Head, css: &str) -> String {
    let mut h = String::with_capacity(2048);
    let _ = write!(h, "\n\t<title>{}</title>", esc(&head.title));
    h.push_str(&meta_tags(head));
    // q53: the axes in the head, each an alternate FORM of this row. Locale
    // hreflang comes from a declared expand; other axis forms still land here
    // from the engine. NOT a single declared tag: a variable-length LIST.
    for a in &head.alternates {
        h.push_str("\n\t<link rel=\"alternate\"");
        if let Some(lang) = &a.hreflang {
            let _ = write!(h, " hreflang=\"{}\"", esc(lang));
        }
        if let Some(t) = &a.media_type {
            let _ = write!(h, " type=\"{}\"", esc(t));
        }
        let _ = write!(h, " href=\"{}\">", esc(&a.href));
    }
    if let Some(j) = &head.jsonld {
        let _ = write!(
            h,
            "\n\t<script type=\"application/ld+json\">\n\t{j}\n\t</script>"
        );
    }
    h.push_str("\n\t<meta charset=\"utf-8\">");
    let _ = write!(
        h,
        "\n\t<link href='{css}' rel='stylesheet' type='text/css'>"
    );
    h.push('\n');
    h
}

/// A date as Atom/sitemap `date_to_xmlschema`: `2026-06-25T00:00:00+00:00`.
pub fn xmlschema(d: chrono::NaiveDate) -> String {
    format!("{}T00:00:00+00:00", d.format("%Y-%m-%d"))
}

/// `expand_urls: site.url` (expand_urls.rb): make root-relative `href`/`src`
/// absolute. Protocol-relative `//host` is left alone (the `[^/>]` guard).
fn expand_urls(html: &str, url: &str) -> String {
    let re = regex::Regex::new(r#"(\s+(?:href|src)\s*=\s*["'])(/[^/>][^"'>]*)"#).unwrap();
    re.replace_all(html, |c: &regex::Captures| {
        format!("{}{}{}", &c[1], url, &c[2])
    })
    .into_owned()
}

/// `feed_images` (feed_images.rb): float images get `align`/`width` so feed
/// readers, which ignore our CSS, still flow text around them. The plugin
/// selects `a.floatright > img`; `{% image right %}` is the only thing that
/// emits that shape (see tags::image), so a targeted rewrite matches it.
fn feed_images(html: &str) -> String {
    // q26 puts width/height on every body image. The feed wants its own fixed
    // width for floats, and two `width` attributes is invalid markup in which
    // the FIRST one wins — so the page's 640 would silently beat the feed's
    // 200. Strip ours from the matched tag before injecting the feed's.
    let dims = regex::Regex::new(r#"\s*(?:width|height)='[^']*'"#).unwrap();
    let inject = |html: String, class: &str, align: &str| -> String {
        let re = regex::Regex::new(&format!(
            r#"(<a class='image {class}'[^>]*><img )([^>]*?)>"#
        ))
        .unwrap();
        re.replace_all(&html, |c: &regex::Captures| {
            format!(
                r#"{}{} align="{align}" width="200">"#,
                &c[1],
                dims.replace_all(&c[2], "").trim_end()
            )
        })
        .into_owned()
    };
    let s = inject(html.to_string(), "floatright", "right");
    inject(s, "floatleft", "left")
}

/// Escape a `]]>` that would otherwise close the surrounding CDATA section.
fn cdata_escape(s: &str) -> String {
    s.replace("]]>", "]]]]><![CDATA[>")
}

/// The Atom feed (atom.xml). `updated` is the build timestamp already in
/// xmlschema form; entries are `(post, rendered_body)`, newest first.
/// `self_path` is the feed route's own URL (§6f: locale-parallel feeds —
/// /atom.xml and /fr/atom.xml — each claim their own self link).
pub fn feed(site: &Site, self_path: &str, updated: &str, entries: &[(&Row, &str)]) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    s.push_str("\t<feed xmlns=\"http://www.w3.org/2005/Atom\">\n");
    let _ = writeln!(
        s,
        "\t<title><![CDATA[{}]]></title>",
        cdata_escape(site.title)
    );
    let _ = writeln!(
        s,
        "\t<link href=\"{u}{self_path}\" rel=\"self\"/>",
        u = site.url
    );
    let _ = writeln!(s, "\t<link href=\"{u}/\"/>", u = site.url);
    // §4d: this used to name grack.com's own favicon path, in every site's
    // feed — `examples/minimal` shipped a link to a file it does not have.
    // Both elements take the site icon absolutely, and a site without one
    // emits neither: the same "empty means absent" the head runs on.
    if !site.icon.is_empty() {
        let _ = writeln!(s, "\t<icon>{u}{i}</icon>", u = site.url, i = site.icon);
        let _ = writeln!(s, "\t<logo>{u}{i}</logo>", u = site.url, i = site.icon);
    }
    let _ = writeln!(s, "\t<updated>{updated}</updated>");
    let _ = writeln!(s, "\t<id>{u}/</id>", u = site.url);
    s.push_str("\t<author>\n");
    let _ = writeln!(s, "\t\t<name><![CDATA[{}]]></name>", site.author);
    if let Some(email) = site.email {
        let _ = writeln!(s, "\t\t<email><![CDATA[{email}]]></email>");
    }
    s.push_str("\t</author>\n");
    s.push_str("\t<generator uri=\"http://jekyllrb.com/\">Jekyll</generator>\n");
    for (p, body) in entries {
        let content = cdata_escape(&feed_images(&expand_urls(body, site.url)));
        let updated = p.date.map(xmlschema).unwrap_or_default();
        s.push_str("\t<entry>\n");
        let _ = writeln!(
            s,
            "\t\t<title type=\"html\"><![CDATA[{}]]></title>",
            cdata_escape(p.title.as_deref().unwrap_or_default())
        );
        let _ = writeln!(s, "\t\t<link href=\"{}{}\"/>", site.url, p.url);
        let _ = writeln!(s, "\t\t<updated>{updated}</updated>");
        let _ = writeln!(
            s,
            "\t\t<id>{}{}</id>",
            site.url,
            p.url.trim_end_matches('/')
        );
        let _ = writeln!(
            s,
            "\t\t<content type=\"html\"><![CDATA[{content}]]></content>"
        );
        s.push_str("\t</entry>\n");
    }
    s.push_str("</feed>\n");
    s
}

#[cfg(test)]
mod feed_tests {
    use super::*;

    #[test]
    fn expand_urls_makes_root_relative_absolute() {
        let out = expand_urls(
            r#"<a href="/blog/x/"><img src="/a.png"></a>"#,
            "https://grack.com",
        );
        assert_eq!(
            out,
            r#"<a href="https://grack.com/blog/x/"><img src="https://grack.com/a.png"></a>"#
        );
    }

    #[test]
    fn expand_urls_leaves_absolute_and_protocol_relative_alone() {
        // Already absolute, protocol-relative, and fragment refs must not move.
        let src = r##"<a href="https://x.com/y"><img src="//cdn/z.png"><a href="#top">"##;
        assert_eq!(expand_urls(src, "https://grack.com"), src);
    }

    #[test]
    fn feed_images_injects_align_and_width() {
        let src = "<a class='image floatright' href='https://grack.com/a.jpg'><img src='https://grack.com/a.jpg' alt=''></a>";
        let out = feed_images(src);
        assert!(
            out.contains(r#"alt='' align="right" width="200">"#),
            "{out}"
        );
    }

    #[test]
    fn feed_images_ignores_non_float_images() {
        let src = "<a class='image standard' href='x'><img src='x' alt=''></a>";
        assert_eq!(feed_images(src), src);
    }

    #[test]
    fn cdata_escape_splits_terminator() {
        assert_eq!(cdata_escape("a]]>b"), "a]]]]><![CDATA[>b");
    }

    #[test]
    fn xmlschema_is_utc_midnight() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 6, 25).unwrap();
        assert_eq!(xmlschema(d), "2026-06-25T00:00:00+00:00");
    }
}

/// The XML sitemap (sitemap.xml). Entries are `(absolute loc, optional
/// lastmod)`, already in the order they should appear.
pub fn sitemap(entries: &[(String, Option<String>)]) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<urlset xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://www.sitemaps.org/schemas/sitemap/0.9 http://www.sitemaps.org/schemas/sitemap/0.9/sitemap.xsd\" xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    for (loc, lastmod) in entries {
        s.push_str("<url>\n");
        let _ = writeln!(s, "<loc>{loc}</loc>");
        if let Some(lm) = lastmod {
            let _ = writeln!(s, "<lastmod>{lm}</lastmod>");
        }
        s.push_str("</url>\n");
    }
    s.push_str("</urlset>\n");
    s
}

#[cfg(test)]
mod meta_tests {
    use super::*;
    use crate::config::Config;

    fn cfg(meta: &str) -> Config {
        Config::from_toml(&format!(
            "extends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [html.head.meta]\n{meta}"
        ))
        .expect("config parses")
    }

    fn declared() -> crate::filter::Schema {
        let mut s = crate::filter::Schema::new();
        s.insert("noindex", crate::filter::Type::Bool);
        s
    }

    fn row(noindex: bool) -> grackle_model::Row {
        let mut r = grackle_model::Row::default();
        if noindex {
            r.fields
                .insert("noindex".into(), crate::filter::Value::Bool(true));
        }
        r
    }

    /// §4e: the engine emits what the config declared, and knows neither the
    /// name `robots` nor what makes it appear. Mutation-checked by inverting
    /// the conditional, which swaps both assertions.
    fn site() -> Site<'static> {
        Site {
            url: "https://e.com",
            title: "T",
            author: "Ada",
            email: None,
            icon: "",
        }
    }

    fn eval(m: &Metas, r: &grackle_model::Row) -> Vec<(Tag, String, String)> {
        eval_metas(m, r, &site(), "Hello", "/u/")
    }

    #[test]
    fn a_declared_meta_is_emitted_only_when_its_expression_says_so() {
        let c = cfg("robots = 'noindex ? \"noindex,follow\" : \"\"'\n");
        let m = compile_metas(&c, &declared()).unwrap();

        assert_eq!(
            eval(&m, &row(true)),
            vec![(
                Tag::Meta,
                "robots".to_string(),
                "noindex,follow".to_string()
            )]
        );
        // The empty branch emits nothing — §5e's rule 2, one layer up.
        assert!(eval(&m, &row(false)).is_empty());
    }

    /// The head's own vocabulary: `title` and `url` are what the head is being
    /// built FOR, so they answer on a listing too — without them `og:title`
    /// would work on posts and silently vanish on listings.
    #[test]
    fn the_environment_carries_the_head_and_the_site() {
        let c = Config::from_toml(
            "extends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [html.head.property]\n\"og:title\" = 'title'\n\"og:url\" = 'site.url + url'\n\
             [html.head.meta]\nauthor = 'site.author'\n",
        )
        .unwrap();
        let m = compile_metas(&c, &declared()).unwrap();
        let got: Vec<(String, String)> = eval(&m, &row(false))
            .into_iter()
            .map(|(_, k, v)| (k, v))
            .collect();
        assert!(
            got.contains(&("og:title".into(), "Hello".into())),
            "{got:?}"
        );
        assert!(
            got.contains(&("og:url".into(), "https://e.com/u/".into())),
            "string `+` concatenates: {got:?}"
        );
        assert!(got.contains(&("author".into(), "Ada".into())), "{got:?}");
    }

    /// A row-only name reads Null elsewhere, which yields empty, which emits
    /// An empty / missing description yields no tag.
    #[test]
    fn a_name_the_row_cannot_answer_emits_nothing() {
        let c = Config::from_toml(
            "extends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [schema]\ndescription = { type = \"string\" }\n\
             [html.head.meta]\ndescription = 'description'\n",
        )
        .expect("config parses");
        let mut decl = declared();
        decl.insert("description", crate::filter::Type::Str);
        let m = compile_metas(&c, &decl).unwrap();
        assert!(eval(&m, &row(false)).is_empty());
    }

    #[test]
    fn an_undeclared_name_is_a_load_error_naming_its_table() {
        let c = cfg("x = 'nope'\n");
        let e = format!("{:#}", compile_metas(&c, &declared()).unwrap_err());
        assert!(e.contains("[html.head.meta] x"), "{e}");
        assert!(e.contains("unknown field `nope`"), "{e}");
    }

    #[test]
    fn an_expand_emits_one_alternate_per_pool_member() {
        let c = Config::from_toml(
            "extends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [schema]\nlocale = { type = \"string\" }\n\
             [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\
             [html.head.link]\nalternate = { from = \"axis.locale\", hreflang = 'locale', href = 'site.url + url' }\n",
        )
        .unwrap();
        let mut decl = declared();
        decl.insert("locale", crate::filter::Type::Str);
        let m = compile_metas(&c, &decl).unwrap();
        let mut en = grackle_model::Row::default();
        en.fields
            .insert("locale".into(), crate::filter::Value::Str("en".into()));
        en.url = "/a/".into();
        let mut fr = grackle_model::Row::default();
        fr.fields
            .insert("locale".into(), crate::filter::Value::Str("fr".into()));
        fr.url = "/fr/a/".into();
        let alts = eval_expands(&m, &site(), "T", &c, |_| {
            vec![
                ExpandMember {
                    row: &en,
                    url: "/a/",
                    member: "en".into(),
                },
                ExpandMember {
                    row: &fr,
                    url: "/fr/a/",
                    member: "fr".into(),
                },
            ]
        });
        let got: Vec<_> = alts
            .iter()
            .map(|a| (a.hreflang.clone(), a.href.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some("en".into()), "https://e.com/a/".into()),
                (Some("fr".into()), "https://e.com/fr/a/".into()),
            ]
        );
        assert!(eval_expands(&m, &site(), "T", &c, |_| {
            vec![ExpandMember {
                row: &en,
                url: "/a/",
                member: "en".into(),
            }]
        })
        .is_empty());
    }

    #[test]
    fn the_tags_escape_and_render_in_declaration_order() {
        let mut head = head_simple(
            "T",
            "/u/",
            &Site {
                url: "https://e.com",
                title: "t",
                author: "a",
                email: None,
                icon: "",
            },
        );
        head.meta = vec![
            (Tag::Meta, "a".into(), "one".into()),
            (Tag::Property, "b".into(), "a \"quoted\" & escaped".into()),
            (Tag::Link, "canonical".into(), "https://e.com/".into()),
        ];
        let out = meta_tags(&head);
        assert!(out.contains("<meta name=\"a\" content=\"one\">"), "{out}");
        // Open Graph is `property=`, not `name=` — a different attribute, which
        // is why it is a different table rather than a special case.
        assert!(out.contains("<meta property=\"b\""), "{out}");
        assert!(out.contains("<link rel=\"canonical\" href="), "{out}");
        assert!(out.find("name=\"a\"") < out.find("property=\"b\""));
        assert!(out.contains("&quot;quoted&quot;"), "{out}");
        assert!(out.contains("&amp;"), "{out}");
    }
}
