//! Presentation: head facts + the light-tier wrapper.
//!
//!   schema  -> head facts (computed, never branched; §5a)
//!   layout  -> part maps (parts.rs, §5e)
//!   theme   -> fragments + css (themes/<name>/, theme.rs, §5e)
//!
//! Fold serializations (atom, sitemap, search) live in [`crate::shells`].
//! What remains here is what has no theme: the computed `<head>` facts and
//! the `light` tier's minimal wrapper (§5g "Row tiers" — a tier, not the
//! null theme, which takes the full head).

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

/// Typed facts derived from a row's schema. A theme renders the subset it
/// wants; nobody branches on "am I a post" (§5a).
#[derive(Debug, Default)]
pub struct Head {
    pub title: String,
    /// Declared head tags, already evaluated (§4e). Empty values are dropped
    /// before they get here, so emitting is a loop with no decision in it —
    /// the engine no longer knows that one of these is called `robots`, or
    /// `og:title`, or what makes any of them appear.
    pub meta: Vec<MetaItem>,
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
    /// Head fragments pulled in by the widgets the body used (script/style/link),
    /// deduped, emitted verbatim.
    pub injected: Vec<String>,
}

/// One evaluated `[html.head.*]` tag (§4e). `value` is `content`/`href`;
/// `attrs` carries any extras from a table-form entry (`sizes`, `type`, …).
#[derive(Debug, Clone)]
pub struct MetaItem {
    pub tag: Tag,
    pub key: String,
    pub value: String,
    pub attrs: Vec<(String, String)>,
}

impl MetaItem {
    fn simple(tag: Tag, key: String, value: String) -> Self {
        Self {
            tag,
            key,
            value,
            attrs: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
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

impl<'a> Site<'a> {
    /// Same site facts with a locale-resolved display title (§6f).
    pub fn with_title(self, title: &'a str) -> Self {
        Self { title, ..self }
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
pub struct Metas {
    pub tags: Vec<Decl>,
    /// Compiled `[html.head.jsonld]` tree. Empty map ⇒ no script ever.
    pub jsonld: std::collections::BTreeMap<String, JsonLdNode>,
}

/// One node of a compiled JSON-LD tree.
#[derive(Debug)]
pub enum JsonLdNode {
    Expr(crate::filter::Text),
    Object(std::collections::BTreeMap<String, JsonLdNode>),
}

/// Declared `[html.html.attribute]` / `[html.body.attribute]` expressions,
/// compiled once (§4e).
#[derive(Debug, Default)]
pub struct HtmlAttrs {
    pub html: Vec<(String, crate::filter::Text)>,
    pub body: Vec<(String, crate::filter::Text)>,
}

/// One declared head tag: a single expression, a multi-attr table, or an
/// expand over a pool.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // `Text` is the CEL payload; boxing it would cost a hop on every tag.
pub enum Decl {
    /// One tag whose value is a CEL text expression (`content` / `href`).
    Single {
        tag: Tag,
        key: String,
        expr: crate::filter::Text,
    },
    /// One tag with several CEL attributes (`href` + `sizes` + …).
    Attrs {
        tag: Tag,
        key: String,
        attrs: Vec<(String, crate::filter::Text)>,
    },
    /// One tag per member of `from`; attributes are CEL text expressions.
    Expand {
        tag: Tag,
        key: String,
        from: String,
        attrs: Vec<(String, crate::filter::Text)>,
    },
}

/// The primary attribute a table-form entry must carry, by element.
fn primary_attr(tag: Tag) -> &'static str {
    match tag {
        Tag::Link => "href",
        Tag::Meta | Tag::Property => "content",
    }
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
    let mut tags = Vec::new();
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
                    tags.push(Decl::Single {
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
                    tags.push(Decl::Expand {
                        tag,
                        key: key.clone(),
                        from: exp.from.clone(),
                        attrs,
                    });
                }
                HeadEntry::Attrs(map) => {
                    let primary = primary_attr(tag);
                    anyhow::ensure!(
                        map.contains_key(primary),
                        "{whose} {key}: a table entry needs a {primary} expression"
                    );
                    anyhow::ensure!(
                        !map.is_empty(),
                        "{whose} {key}: a table entry needs at least one attribute"
                    );
                    let mut attrs = Vec::new();
                    for (attr, src) in map {
                        let expr = crate::filter::Text::parse(src, &env)
                            .with_context(|| format!("{whose} {key}.{attr}"))?;
                        attrs.push((attr.clone(), expr));
                    }
                    tags.push(Decl::Attrs {
                        tag,
                        key: key.clone(),
                        attrs,
                    });
                }
            }
        }
    }
    let jsonld = compile_jsonld_map(&h.jsonld, &env, "[html.head.jsonld]")?;
    Ok(Metas { tags, jsonld })
}

fn compile_jsonld_map(
    map: &std::collections::BTreeMap<String, crate::config::JsonLdValue>,
    env: &crate::filter::Schema,
    whose: &str,
) -> Result<std::collections::BTreeMap<String, JsonLdNode>> {
    let mut out = std::collections::BTreeMap::new();
    for (key, val) in map {
        let path = format!("{whose}.{key}");
        out.insert(key.clone(), compile_jsonld_value(val, env, &path)?);
    }
    Ok(out)
}

fn compile_jsonld_value(
    val: &crate::config::JsonLdValue,
    env: &crate::filter::Schema,
    whose: &str,
) -> Result<JsonLdNode> {
    use crate::config::JsonLdValue;
    match val {
        JsonLdValue::Expr(src) => {
            let expr = crate::filter::Text::parse(src, env).with_context(|| whose.to_string())?;
            Ok(JsonLdNode::Expr(expr))
        }
        JsonLdValue::Object(map) => Ok(JsonLdNode::Object(compile_jsonld_map(map, env, whose)?)),
    }
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

/// Evaluate the single-expression and table-form metas, dropping the empty
/// ones — §5e's rule 2 one layer up: an empty primary value emits no tag.
pub fn eval_metas(
    metas: &Metas,
    row: &impl crate::filter::Row,
    site: &Site,
    title: &str,
    url: &str,
) -> Vec<MetaItem> {
    let env = HeadRow {
        row,
        site,
        title,
        url,
    };
    metas
        .tags
        .iter()
        .filter_map(|d| match d {
            Decl::Single { tag, key, expr } => {
                let v = expr.eval(&env);
                (!v.is_empty()).then(|| MetaItem::simple(*tag, key.clone(), v))
            }
            Decl::Attrs { tag, key, attrs } => {
                let primary = primary_attr(*tag);
                let mut value = String::new();
                let mut extra = Vec::new();
                for (attr, expr) in attrs {
                    let v = expr.eval(&env);
                    if v.is_empty() {
                        continue;
                    }
                    if attr == primary {
                        value = v;
                    } else {
                        extra.push((attr.clone(), v));
                    }
                }
                (!value.is_empty()).then(|| MetaItem {
                    tag: *tag,
                    key: key.clone(),
                    value,
                    attrs: extra,
                })
            }
            Decl::Expand { .. } => None,
        })
        .collect()
}

/// Evaluate `[html.head.jsonld]` into a JSON document. Requires a non-empty
/// `@type` after eval; empty leaves (and objects that collapse to nothing)
/// are omitted.
pub fn eval_jsonld(
    metas: &Metas,
    row: &impl crate::filter::Row,
    site: &Site,
    title: &str,
    url: &str,
) -> Option<String> {
    if metas.jsonld.is_empty() {
        return None;
    }
    let env = HeadRow {
        row,
        site,
        title,
        url,
    };
    let obj = eval_jsonld_object(&metas.jsonld, &env)?;
    let ty = obj.get("@type").and_then(|v| v.as_str()).unwrap_or("");
    if ty.is_empty() {
        return None;
    }
    Some(serde_json::Value::Object(obj).to_string())
}

fn eval_jsonld_object(
    map: &std::collections::BTreeMap<String, JsonLdNode>,
    env: &impl crate::filter::Row,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut out = serde_json::Map::new();
    for (key, node) in map {
        if let Some(v) = eval_jsonld_node(node, env) {
            out.insert(key.clone(), v);
        }
    }
    (!out.is_empty()).then_some(out)
}

fn eval_jsonld_node(node: &JsonLdNode, env: &impl crate::filter::Row) -> Option<serde_json::Value> {
    match node {
        JsonLdNode::Expr(expr) => {
            let s = expr.eval(env);
            (!s.is_empty()).then(|| serde_json::Value::String(s))
        }
        JsonLdNode::Object(map) => eval_jsonld_object(map, env).map(serde_json::Value::Object),
    }
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
    for d in &metas.tags {
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
            injected: Vec::new(),
        }
    }
}

/// A head with nothing computed: everything but the title, the hreflang list
/// and JSON-LD is declared now (§4e), so a listing's head IS its title.
pub fn head_simple(title: &str, _url: &str, _site: &Site) -> Head {
    Head::empty(title.to_string())
}

/// Title + evaluated metas (and JSON-LD) for a listing/landing/page.
pub fn head_for(
    title: &str,
    url: &str,
    site: &Site,
    metas: &Metas,
    row: &impl crate::filter::Row,
) -> Head {
    let mut head = head_simple(title, url, site);
    head.meta = eval_metas(metas, row, site, title, url);
    head.jsonld = eval_jsonld(metas, row, site, title, url);
    head
}

/// Post head: same declared tables as every other surface.
pub fn head_for_post(p: &Row, site: &Site, metas: &Metas) -> Head {
    head_for(
        p.title.as_deref().unwrap_or_default(),
        &p.url,
        site,
        metas,
        p,
    )
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
            .filter(|m| m.tag == Tag::Meta)
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
    for m in &head.meta {
        let extras = m
            .attrs
            .iter()
            .map(|(k, v)| format!(" {}=\"{}\"", esc(k), esc(v)))
            .collect::<String>();
        let _ = match m.tag {
            Tag::Meta => write!(
                out,
                "\n\t<meta name=\"{}\" content=\"{}\"{extras}>",
                esc(&m.key),
                esc(&m.value),
            ),
            Tag::Property => write!(
                out,
                "\n\t<meta property=\"{}\" content=\"{}\"{extras}>",
                esc(&m.key),
                esc(&m.value),
            ),
            Tag::Link => write!(
                out,
                "\n\t<link rel=\"{}\" href=\"{}\"{extras}>",
                esc(&m.key),
                esc(&m.value),
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
pub fn head_html(head: &Head, css: &str, include: &[String]) -> String {
    let mut h = String::with_capacity(2048);
    let _ = write!(h, "\n\t<title>{}</title>", esc(&head.title));
    h.push_str(&meta_tags(head));
    // `[html.head] include`: verbatim site fragments (§4e escape hatch), high
    // in the head where an analytics tag wants to be. Emitted as written.
    for frag in include {
        let _ = write!(h, "\n\t{frag}");
    }
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
    for frag in &head.injected {
        let _ = write!(h, "\n\t{frag}");
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

    fn eval(m: &Metas, r: &grackle_model::Row) -> Vec<MetaItem> {
        eval_metas(m, r, &site(), "Hello", "/u/")
    }

    fn pairs(items: &[MetaItem]) -> Vec<(Tag, String, String)> {
        items
            .iter()
            .map(|m| (m.tag, m.key.clone(), m.value.clone()))
            .collect()
    }

    #[test]
    fn a_declared_meta_is_emitted_only_when_its_expression_says_so() {
        let c = cfg("robots = 'noindex ? \"noindex,follow\" : \"\"'\n");
        let m = compile_metas(&c, &declared()).unwrap();

        assert_eq!(
            pairs(&eval(&m, &row(true))),
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
            .map(|m| (m.key, m.value))
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
            MetaItem::simple(Tag::Meta, "a".into(), "one".into()),
            MetaItem::simple(Tag::Property, "b".into(), "a \"quoted\" & escaped".into()),
            MetaItem::simple(Tag::Link, "canonical".into(), "https://e.com/".into()),
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

    #[test]
    fn a_link_table_carries_sizes_and_type() {
        let c = Config::from_toml(
            "extends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [html.head.link]\n\
             \"apple-touch-icon\" = { href = 'site.icon', sizes = '\"180x180\"', type = '\"image/png\"' }\n",
        )
        .unwrap();
        let m = compile_metas(&c, &declared()).unwrap();
        let site = Site {
            url: "https://e.com",
            title: "T",
            author: "Ada",
            email: None,
            icon: "/icon.png",
        };
        let items = eval_metas(&m, &row(false), &site, "Hello", "/u/");
        assert_eq!(items.len(), 1, "{items:?}");
        let item = &items[0];
        assert_eq!(item.tag, Tag::Link);
        assert_eq!(item.key, "apple-touch-icon");
        assert_eq!(item.value, "/icon.png");
        let attrs: std::collections::BTreeMap<_, _> = item.attrs.iter().cloned().collect();
        assert_eq!(attrs.get("sizes").map(String::as_str), Some("180x180"));
        assert_eq!(attrs.get("type").map(String::as_str), Some("image/png"));
        let out = meta_tags(&Head {
            meta: items,
            ..Head::empty("T".into())
        });
        assert!(
            out.contains("rel=\"apple-touch-icon\"")
                && out.contains("href=\"/icon.png\"")
                && out.contains("sizes=\"180x180\"")
                && out.contains("type=\"image/png\""),
            "{out}"
        );
    }

    #[test]
    fn head_include_is_emitted_verbatim_and_empty_adds_nothing() {
        let head = Head::empty("T".into());
        let frag = "<script async src=\"https://x/g.js?id=G-1\"></script>";
        let with = head_html(&head, "/c.css", std::slice::from_ref(&frag.to_string()));
        assert!(with.contains(frag), "{with}");
        // Trusted and verbatim: the `<` is NOT escaped into an entity.
        assert!(!with.contains("&lt;script"), "{with}");
        // Absent include changes nothing.
        let without = head_html(&head, "/c.css", &[]);
        assert_eq!(without, with.replace(&format!("\n\t{frag}"), ""));
    }

    #[test]
    fn jsonld_emits_only_when_type_is_non_empty() {
        let c = Config::from_toml(
            r#"extends = "none"
               [site]
               url = "https://e.com"
               title = "t"
               author = "a"
               [schema]
               date = { type = "date" }
               [html.head.jsonld]
               "@context" = '"https://schema.org"'
               "@type" = 'date ? "BlogPosting" : ""'
               headline = 'title'
               [html.head.jsonld.author]
               "@type" = '"Person"'
               name = 'site.author'
            "#,
        )
        .unwrap();
        let mut decl = crate::filter::Schema::new();
        decl.insert("date", crate::filter::Type::Str);
        let m = compile_metas(&c, &decl).unwrap();
        let mut dated = grackle_model::Row::default();
        dated.fields.insert(
            "date".into(),
            crate::filter::Value::Str("2026-01-02".into()),
        );
        let j = eval_jsonld(&m, &dated, &site(), "Hello", "/p/").unwrap();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["@type"], "BlogPosting");
        assert_eq!(v["headline"], "Hello");
        assert_eq!(v["author"]["name"], "Ada");
        assert!(eval_jsonld(&m, &row(false), &site(), "Hello", "/p/").is_none());
    }
}
