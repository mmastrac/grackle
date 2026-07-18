use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_root")]
    pub root: PathBuf,
    /// Honour .gitignore when walking (default true). It is the site's existing
    /// declaration of what is not content; see store::walker.
    #[serde(default = "default_true")]
    pub gitignore: bool,
    pub site: Site,
    #[serde(default)]
    pub collections: BTreeMap<String, Collection>,
    #[serde(default)]
    pub views: BTreeMap<String, View>,
    /// Marker filename -> defaults it applies to its directory and below.
    /// The config says what a marker means; the tree says where (DESIGN.md §4b).
    #[serde(default)]
    pub markers: BTreeMap<String, BTreeMap<String, toml::Value>>,
    /// Custom block widgets (§5d): `{% name %}…{% endname %}` expands to the
    /// wrapper template with the markdown body spliced at `{body}`. Adding a
    /// widget is one config entry, no code.
    #[serde(default)]
    pub widgets: BTreeMap<String, String>,
    /// Related-posts ranking policy (§6b). Cosine similarity supplies the
    /// candidates; this shapes them per site.
    #[serde(default)]
    pub related: RelatedCfg,
    /// Script shells (§5g, and yes, the pun): registered shell types backed
    /// by an external command — the experimental bench for serializations
    /// the engine doesn't speak yet (PDF, PostScript, whatever). The command
    /// gets the view's rows as JSON on stdin (schema is TEMP, see §5g) and
    /// its stdout bytes land at the view's route verbatim.
    #[serde(default)]
    pub shells: BTreeMap<String, ShellDef>,
    /// i18n (§6f): the locale axis. Absent = a monolingual site; every row
    /// carries the default locale and nothing changes.
    #[serde(default)]
    pub i18n: I18nCfg,
    /// Enum records (§6f, generalized from tag records at Matt's ask):
    /// `[records.<field>.<id>]` declares the value domain of a grouped
    /// field — tags, courses, any typed field a view groups by. A value
    /// used in front matter needs no entry (id is slug is name), but an
    /// entry can set the route `slug`, per-locale display `name`s (used
    /// by pills AND by `{key}` in grouped titles/crumbs), and an `intro`
    /// — mode-A landing prose for that value's own archive page.
    #[serde(default)]
    pub records: BTreeMap<String, BTreeMap<String, RecordCfg>>,
    /// The retired `[tags.<id>]` spelling — kept only so validate() can
    /// error loudly with the new form instead of ignoring it silently.
    #[serde(default)]
    pub tags: BTreeMap<String, toml::Value>,
    /// Internal-link policy (§6a, Matt's rule): links reference what the
    /// database owns — rows by SOURCE PATH, views by `view:` reference —
    /// because final URLs are derived values (locales, slugs, templates).
    /// `strict` errors on raw internal URLs with the correct form as the
    /// suggestion; `loose` (default) resolves the new forms but leaves raw
    /// URLs alone — the migration posture for a legacy corpus.
    #[serde(default)]
    pub links: LinksCfg,
    #[serde(skip)]
    pub dir: PathBuf,
}

fn default_root() -> PathBuf {
    PathBuf::from(".")
}

fn default_true() -> bool {
    true
}

/// A registered script shell: `sh -c command`, run from the site root.
#[derive(Debug, Deserialize)]
pub struct ShellDef {
    pub command: String,
}

/// The locale axis (§6f). The *path selector* assigns each row its locale
/// at load: `suffix` reads `dal.fr.md`, `prefix` reads `fr/recipes/dal.md`.
/// Everything downstream — rules, globs, route tokens, schema resolution —
/// sees the LOGICAL path (locale stripped), so a translation rides the same
/// rule as its original and lands at the locale-prefixed URL.
#[derive(Debug, Deserialize)]
pub struct I18nCfg {
    #[serde(default = "default_locale")]
    pub default: String,
    /// Non-default locales a path may declare. Empty = i18n off.
    #[serde(default)]
    pub locales: Vec<String>,
    #[serde(default)]
    pub selector: Selector,
    /// Display names for the translations axis (`fr = "Français"`);
    /// a missing entry falls back to the locale code.
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    /// The GLOBAL string map (§6f): the fallback layer of the display-name
    /// hierarchy (inline beats global beats engine built-in). Engine
    /// vocabulary keys (ENGINE_STRINGS) override what the engine emits; any
    /// other key is a shared string for `"@key"` references — and must be
    /// referenced somewhere, so a typo'd engine key can't hide as an
    /// accidental unused string. Values are literal (no reference chains).
    #[serde(default)]
    pub strings: BTreeMap<String, LocalizedStr>,
}

/// The engine's display vocabulary: every string the engine emits into
/// pages, with its built-in default. `[i18n.strings]` may override any of
/// these (per locale or wholesale); nothing else may appear there.
pub const ENGINE_STRINGS: &[(&str, &str)] = &[
    ("home", "Home"),
    ("drafts", "Drafts"),
    ("related", "Related"),
    ("later", "Later post"),
    ("earlier", "Earlier post"),
    ("linked_from", "Linked from"),
    ("translations", "Translations"),
    ("page", "Page {n}"),
];

fn default_locale() -> String {
    "en".to_string()
}

impl Default for I18nCfg {
    fn default() -> Self {
        I18nCfg {
            default: default_locale(),
            locales: Vec::new(),
            selector: Selector::default(),
            names: BTreeMap::new(),
            strings: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Selector {
    #[default]
    Suffix,
    Prefix,
}

impl I18nCfg {
    pub fn enabled(&self) -> bool {
        !self.locales.is_empty()
    }

    /// Split a collection-relative path into (logical path, locale).
    pub fn split(&self, rel: &Path) -> (PathBuf, String) {
        if self.enabled() {
            match self.selector {
                Selector::Suffix => {
                    if let Some(stem) = rel.file_stem().and_then(|s| s.to_str()) {
                        if let Some((base, loc)) = stem.rsplit_once('.') {
                            if loc != self.default && self.locales.iter().any(|l| l == loc) {
                                let fname = match rel.extension().and_then(|e| e.to_str()) {
                                    Some(ext) => format!("{base}.{ext}"),
                                    None => base.to_string(),
                                };
                                return (rel.with_file_name(fname), loc.to_string());
                            }
                        }
                    }
                }
                Selector::Prefix => {
                    let mut it = rel.iter();
                    if let Some(first) = it.next().and_then(|c| c.to_str()) {
                        if first != self.default && self.locales.iter().any(|l| l == first) {
                            return (it.as_path().to_path_buf(), first.to_string());
                        }
                    }
                }
            }
        }
        (rel.to_path_buf(), self.default.clone())
    }

    /// The label a locale wears in the translations axis.
    pub fn name_of<'a>(&'a self, locale: &'a str) -> &'a str {
        self.names.get(locale).map(String::as_str).unwrap_or(locale)
    }

    /// A named string (§6f), for a locale: the global `[i18n.strings]`
    /// entry if declared, else the engine built-in. This is the FALLBACK
    /// half of the hierarchy; `text` adds the inline-beats-global half.
    pub fn string<'a>(&'a self, key: &str, locale: &str) -> &'a str {
        if let Some(s) = self.strings.get(key) {
            return s.get(locale, &self.default);
        }
        ENGINE_STRINGS
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
            .unwrap_or("")
    }

    /// Resolve a display-name site (§6f): an inline value wins outright;
    /// an `"@key"` reference falls back to the global map (which itself
    /// falls back to engine built-ins). Load validation guarantees every
    /// reference resolves.
    pub fn text<'a>(&'a self, s: &'a LocalizedStr, locale: &str) -> &'a str {
        match s.reference() {
            Some(key) => self.string(key, locale),
            None => s.get(locale, &self.default),
        }
    }
}

/// Internal-link policy (§6a).
#[derive(Debug, Deserialize, Default)]
pub struct LinksCfg {
    #[serde(default)]
    pub policy: LinkPolicy,
}

#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinkPolicy {
    #[default]
    Loose,
    Strict,
}

/// One enum record: the full schema of one value of a grouped field.
/// `slug` and `name` default to the id; `intro` is absent by default.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordCfg {
    pub slug: Option<String>,
    pub name: Option<LocalizedStr>,
    pub intro: Option<LocalizedStr>,
}

/// THE shape for display names (§6f): any human-facing string the config
/// authors is either a bare string, a per-locale map, or a REFERENCE
/// (`"@key"`) into the global `[i18n.strings]` map. The hierarchy is
/// inline beats global beats engine built-in: write a value at the site
/// to be surgical, name a shared string to say one thing everywhere.
/// Validated at load: per-locale maps name only declared locales and
/// include the default locale (resolution is total); references must
/// resolve; `"@@…"` escapes a literal leading `@`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum LocalizedStr {
    One(String),
    PerLocale(BTreeMap<String, String>),
}

impl LocalizedStr {
    /// The `"@key"` reference form, if this is one.
    pub fn reference(&self) -> Option<&str> {
        match self {
            LocalizedStr::One(s) => {
                s.strip_prefix('@').filter(|rest| !rest.starts_with('@'))
            }
            LocalizedStr::PerLocale(_) => None,
        }
    }

    /// Exact locale, else the default locale's entry (validated present;
    /// the empty-string fallback is unreachable on a loaded config).
    /// Reference-blind — resolution with the global map is `I18nCfg::text`.
    pub fn get<'a>(&'a self, locale: &str, default: &str) -> &'a str {
        match self {
            // "@@literal" -> "@literal"
            LocalizedStr::One(s) => s.strip_prefix('@').unwrap_or(s),
            LocalizedStr::PerLocale(m) => m
                .get(locale)
                .or_else(|| m.get(default))
                .map(String::as_str)
                .unwrap_or(""),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Site {
    pub url: String,
    #[serde(default)]
    pub baseurl: String,
    pub title: String,
    pub author: String,
    /// The feed's `<author><email>`; omitted from the feed when absent.
    pub email: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Posts,
    Tree,
    Objects,
}

#[derive(Debug, Deserialize)]
pub struct Collection {
    pub kind: Kind,
    pub source: Option<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    /// §6a bubble+bucket asset resolution names this directory; declared
    /// ahead of the code that consumes it (deferred with the q26 pass).
    #[allow(dead_code)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub filename_formats: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// The collection's crumb identity: what it contributes to breadcrumb
    /// trails (§5c provenance — the chain roots at the collection), and
    /// where that crumb links. Per-locale maps carry the lang axis (§6f).
    pub crumb: Option<LocalizedStr>,
    pub index: Option<String>,
    /// The view whose subdivision chain forms this collection's row trails
    /// (e.g. `monthly_archive` → Home > Blog > 2022 > December > 16).
    pub trail: Option<String>,
    /// The view that owns this collection's tag routes (q32): tag pills
    /// render their URLs from ITS route template, so config can move the
    /// archive and the chrome follows. Optional — a unique tags-grouped
    /// view is found on its own; no tags view at all = unlinked pills.
    pub tags: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    #[serde(rename = "match")]
    pub pattern: String,
    pub route: Option<String>,
    /// Gate the rule on front-matter presence. This is what separates a Jekyll
    /// *page* (rendered, pretty URL) from a static file (copied verbatim).
    pub front_matter: Option<bool>,
    #[serde(default)]
    pub defaults: BTreeMap<String, toml::Value>,
}

/// A view is a *query* plus, optionally, a *materialization*.
///
/// The two were welded together until the home page needed the top 3 posts
/// with no route of its own (DESIGN.md §5c). Splitting them gives three shapes:
///
///   * query only (no route, no layout) — a named set, e.g. `published`
///   * query + layout, no route         — embeddable, e.g. `latest`
///   * query + layout + route(s)        — materialized, e.g. `blog_index`
#[derive(Debug, Deserialize)]
pub struct View {
    /// A collection name, another view's name, or `*` for the route set.
    pub over: String,
    pub filter: Option<String>,
    /// Path-glob scoping (§5 audit): globs already exist in rules (§4), so
    /// view scoping reuses them rather than growing the filter language a
    /// path operator. Matched against the row's root-relative path.
    #[serde(rename = "match")]
    pub scope: Option<String>,
    /// Explicit ordering for rows that have no natural one (§5 audit:
    /// posts sort reverse-chronologically by construction; objects don't).
    /// `"name"` is the only value so far. Declared, not defaulted — the
    /// corpus's zero-padding making lexical order correct is luck.
    pub order_by: Option<String>,
    pub group_by: Option<String>,
    pub paginate: Option<usize>,
    pub route: Option<String>,
    #[serde(default)]
    pub routes: Vec<String>,
    pub layout: Option<String>,
    /// Fragment variant (q24): the theme renders this view through
    /// `{kind}--{variant}.html` when it ships one, falling back to the
    /// kind's base fragment. How `/books/` gets cards while `/blog/`
    /// stays textual, both being listings.
    pub variant: Option<String>,
    /// Fill the listing's `featured` slot with the first row (q36) — the
    /// book-of-the-month shape. Most listings leave it off.
    #[serde(default)]
    pub featured: bool,
    /// §6f locale-parallel materialization, DEFAULT-ON: a materializing
    /// row-query view partitions per declared locale (each locale's rows,
    /// locale-prefixed routes, titles resolved per locale; a locale with
    /// no rows materializes nothing). `"default"` opts out; `"*"` states
    /// the default explicitly. Star views never multiply (filter on
    /// `locale`); embedded views follow their embedding page (pending).
    pub locales: Option<String>,
    /// The view's outermost serialization (Matt, 2026-07): `"atom"` and
    /// `"sitemap"` are built-in XML shells, `"search"` is the postcard
    /// index /search.js consumes — the feed is not a special pass, it is
    /// the same rows in a different wrapper, and the searchable set is a
    /// query like any other (§5g). Absent = the HTML shell (theme). The
    /// full generalization is q44.
    pub shell: Option<String>,
    pub limit: Option<usize>,
    pub template: Option<String>,
    /// Listing title, as a template over the route's group params
    /// (`"{year} {month_name}"`, `"Posts Tagged “{key}”"`). Same placeholder
    /// language as routes, same load-time discipline.
    pub title: Option<LocalizedStr>,
    /// What this view contributes to descendants' breadcrumb trails.
    /// Defaults to `title`. Per-locale maps carry the lang axis (§6f).
    pub crumb: Option<LocalizedStr>,
    /// q45 mode A: prose the view owns — rendered as markdown through the
    /// locale-aware link resolver into the listing layout's `intro` slot.
    /// The theme owns the arrangement; the slot collapses when absent.
    pub intro: Option<LocalizedStr>,
    /// q45 mode B: the root-relative source path of a row this landing
    /// CLAIMS. The row becomes the whole body and must place
    /// `{% view <this view> %}` itself — the author owns the arrangement.
    /// A claimed row loses its standalone route and leaves every query.
    pub content: Option<String>,
    /// Computed fields (§6d): columns this view adds to its rows, each
    /// defined by a deriver. Views composed `over` this one inherit them —
    /// fields flow with rows through query composition the way filters do —
    /// and redeclaring a name overrides (nearest wins). The field named
    /// `summary` is what listing previews consume.
    #[serde(default)]
    pub fields: BTreeMap<String, Field>,
}

/// One computed field: exactly one deriver names how the value is computed
/// from the row. `deny_unknown_fields` makes an unknown deriver a parse
/// error naming the known ones.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    /// The row's content blocks, kept until a budget runs out (block
    /// granularity, at least one block; `max_chars` counts visible text).
    /// Carries a `truncated` fact for the theme's ★.
    pub truncate: Option<Truncate>,
}

/// `[related]`: how embedding similarity becomes a related-posts list.
/// `year_penalty` subtracts per year of date distance (a soft prior toward
/// contemporaries); `max_years` is a hard cap; `min_score` drops weak
/// matches after adjustment — a 2004 post is probably not relevant on this
/// blog, but might be on another, so all of it is per-site policy.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RelatedCfg {
    pub limit: usize,
    pub min_score: Option<f32>,
    pub year_penalty: Option<f32>,
    pub max_years: Option<i32>,
}

impl Default for RelatedCfg {
    fn default() -> Self {
        RelatedCfg { limit: 4, min_score: None, year_penalty: None, max_years: None }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Truncate {
    pub max_blocks: Option<usize>,
    pub max_chars: Option<usize>,
}

impl View {
    /// A view with nothing but a query — nothing to inherit ambiguously, which
    /// is why it is the only thing `over` may name.
    pub fn is_query_only(&self) -> bool {
        self.route.is_none()
            && self.routes.is_empty()
            && self.layout.is_none()
            && self.template.is_none()
            && self.paginate.is_none()
            && self.limit.is_none()
            && self.group_by.is_none()
    }

    /// Whether this view materializes routes of its own.
    pub fn is_materialized(&self) -> bool {
        self.route.is_some() || !self.routes.is_empty()
    }
}

/// A view's query, with the `over` chain flattened.
#[derive(Debug)]
pub struct Query {
    /// Collection name, or `*`.
    pub base: String,
    /// Every filter along the chain, outermost view last. All must hold.
    pub filters: Vec<String>,
}

impl Query {
    /// The conjunction of the chain, or None when nothing filters.
    pub fn predicate(&self) -> Option<String> {
        match self.filters.len() {
            0 => None,
            1 => Some(self.filters[0].clone()),
            _ => Some(
                self.filters
                    .iter()
                    .map(|f| format!("({f})"))
                    .collect::<Vec<_>>()
                    .join(" && "),
            ),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text)
            .with_context(|| format!("parsing config {}", path.display()))?;
        cfg.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        cfg.validate()?;
        Ok(cfg)
    }

    /// Every load-time config check (split from `load` so tests can run
    /// them on in-memory configs).
    fn validate(&self) -> Result<()> {
        let cfg = self;
        for (vname, v) in &cfg.views {
            for (fname, f) in &v.fields {
                if f.truncate.is_none() {
                    anyhow::bail!(
                        "view {vname}: field {fname:?} declares no deriver \
                         (have: truncate)"
                    );
                }
            }
        }
        for (name, tmpl) in &cfg.widgets {
            if !tmpl.contains("{body}") {
                anyhow::bail!(
                    "widget {name:?}: wrapper template has no {{body}} hole, \
                     so the author's markdown would be dropped"
                );
            }
        }
        // q32: the tag-route owner must be resolvable and renderable at
        // load — tag pills render URLs from its route template, and a
        // template that can't render from a tag key would 404 the chrome.
        {
            let declared = cfg
                .collections
                .values()
                .find(|c| c.kind == Kind::Posts)
                .and_then(|c| c.tags.as_deref());
            if let Some(name) = declared {
                let Some(v) = cfg.views.get(name) else {
                    anyhow::bail!("collection tags view {name:?} is not a declared view");
                };
                if v.group_by.as_deref().map(crate::views::spec_field) != Some("tags") {
                    anyhow::bail!("collection tags view {name:?} is not grouped by tags");
                }
                if v.route.is_none() {
                    anyhow::bail!("collection tags view {name:?} has no route");
                }
            } else {
                let tag_views: Vec<&str> = cfg
                    .views
                    .iter()
                    .filter(|(_, v)| {
                        v.group_by.as_deref().map(crate::views::spec_field) == Some("tags")
                    })
                    .map(|(n, _)| n.as_str())
                    .collect();
                if tag_views.len() > 1 {
                    anyhow::bail!(
                        "multiple views group by tags ({}) — declare which owns tag                          routes: [collections.<posts>] tags = \"<view>\"",
                        tag_views.join(", ")
                    );
                }
            }
            if let Some((name, v)) = cfg.tags_view() {
                if let Some(tmpl) = v.route.as_deref() {
                    crate::route::render(tmpl, |k| match k {
                        "key" | "tags" => Some("probe".to_string()),
                        _ => None,
                    })
                    .with_context(|| {
                        format!("view {name}: tag route template needs more than {{key}}")
                    })?;
                }
            }
        }
        // §6f: every LocalizedStr in the config obeys ONE rule — a
        // per-locale map may only name declared locales, and must include
        // the default locale so resolution is total. A typo'd locale key
        // is a load error, not a silently unused name.
        {
            let check = |what: &str, s: &LocalizedStr| -> Result<()> {
                let LocalizedStr::PerLocale(m) = s else { return Ok(()) };
                for loc in m.keys() {
                    if *loc != cfg.i18n.default && !cfg.i18n.locales.iter().any(|l| l == loc) {
                        anyhow::bail!(
                            "{what}: declares locale {loc:?}, which is neither the \
                             default ({:?}) nor in i18n.locales {:?}",
                            cfg.i18n.default,
                            cfg.i18n.locales
                        );
                    }
                }
                if !m.contains_key(&cfg.i18n.default) {
                    anyhow::bail!(
                        "{what}: a per-locale name must include the default locale ({:?})",
                        cfg.i18n.default
                    );
                }
                Ok(())
            };
            if let Some(id) = cfg.tags.keys().next() {
                anyhow::bail!(
                    "[tags.{id}]: tag records generalized to enum records — \
                     declare [records.tags.{id}] instead (§6f)"
                );
            }
            for (field, recs) in &cfg.records {
                for (id, t) in recs {
                    if let Some(n) = &t.name {
                        check(&format!("record {field}.{id}: name"), n)?;
                    }
                    if let Some(i) = &t.intro {
                        check(&format!("record {field}.{id}: intro"), i)?;
                    }
                }
            }
            for (name, v) in &cfg.views {
                if let Some(t) = &v.title {
                    check(&format!("view {name}: title"), t)?;
                }
                if let Some(c) = &v.crumb {
                    check(&format!("view {name}: crumb"), c)?;
                }
                if let Some(i) = &v.intro {
                    check(&format!("view {name}: intro"), i)?;
                }
            }
            for (name, c) in &cfg.collections {
                if let Some(cr) = &c.crumb {
                    check(&format!("collection {name}: crumb"), cr)?;
                }
            }
            // The global map: same locale rule; values are literal (a
            // reference chain would make resolution non-total).
            for (key, s) in &cfg.i18n.strings {
                check(&format!("i18n.strings.{key}"), s)?;
                if s.reference().is_some() {
                    anyhow::bail!(
                        "i18n.strings.{key}: a global string may not itself be a \
                         reference (no chains)"
                    );
                }
            }
            // References must resolve, and every non-engine global string
            // must be referenced — an unused key is a load error, which is
            // what catches a typo'd engine-vocabulary override ("hom") now
            // that user keys are legal.
            let mut referenced: Vec<&str> = Vec::new();
            {
                let mut refs: Vec<(String, &LocalizedStr)> = Vec::new();
                for (field, recs) in &cfg.records {
                    for (id, t) in recs {
                        if let Some(n) = &t.name {
                            refs.push((format!("record {field}.{id}: name"), n));
                        }
                        if let Some(i) = &t.intro {
                            refs.push((format!("record {field}.{id}: intro"), i));
                        }
                    }
                }
                for (name, v) in &cfg.views {
                    if let Some(t) = &v.title {
                        refs.push((format!("view {name}: title"), t));
                    }
                    if let Some(c) = &v.crumb {
                        refs.push((format!("view {name}: crumb"), c));
                    }
                    if let Some(i) = &v.intro {
                        refs.push((format!("view {name}: intro"), i));
                    }
                }
                for (name, c) in &cfg.collections {
                    if let Some(cr) = &c.crumb {
                        refs.push((format!("collection {name}: crumb"), cr));
                    }
                }
                for (what, s) in refs {
                    let Some(key) = s.reference() else { continue };
                    let known = cfg.i18n.strings.contains_key(key)
                        || ENGINE_STRINGS.iter().any(|(k, _)| *k == key);
                    if !known {
                        let mut knowns: Vec<&str> =
                            ENGINE_STRINGS.iter().map(|(k, _)| *k).collect();
                        knowns.extend(cfg.i18n.strings.keys().map(String::as_str));
                        knowns.sort_unstable();
                        knowns.dedup();
                        anyhow::bail!(
                            "{what}: reference @{key} names no string (knowns: {})",
                            knowns.join(", ")
                        );
                    }
                    referenced.push(key);
                }
            }
            for key in cfg.i18n.strings.keys() {
                if !ENGINE_STRINGS.iter().any(|(k, _)| k == key)
                    && !referenced.iter().any(|r| r == key)
                {
                    anyhow::bail!(
                        "i18n.strings.{key}: unused string — nothing references \
                         @{key}, and it is not engine vocabulary (a typo'd engine \
                         key would look exactly like this)"
                    );
                }
            }
        }
        // q45: a landing's prose is a slot text OR a claimed row, never
        // both (the engine would have to guess the arrangement); either
        // form belongs to a view that materializes routes; and a row may
        // serve exactly one landing.
        {
            let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
            for (vname, v) in &cfg.views {
                if v.intro.is_some() && v.content.is_some() {
                    anyhow::bail!(
                        "view {vname}: declares both intro and content — the \
                         slot text and a claimed row are exclusive (q45): the \
                         theme owns the arrangement, or the row does"
                    );
                }
                if (v.intro.is_some() || v.content.is_some()) && !v.is_materialized() {
                    anyhow::bail!(
                        "view {vname}: intro/content on a view with no route — \
                         a landing materializes somewhere"
                    );
                }
                if (v.intro.is_some() || v.content.is_some()) && v.over == "*" {
                    anyhow::bail!(
                        "view {vname}: star views serialize the route set and \
                         have no landing to give prose to"
                    );
                }
                if let Some(c) = v.content.as_deref() {
                    if let Some(other) = claimed.insert(c, vname) {
                        anyhow::bail!(
                            "row {c:?} is claimed as content by two views \
                             ({other} and {vname}) — a row serves one landing"
                        );
                    }
                }
            }
        }
        for (vname, v) in &cfg.views {
            if let Some(l) = v.locales.as_deref() {
                if !matches!(l, "*" | "default") {
                    anyhow::bail!(
                        "view {vname}: locales must be \"*\" (every declared \
                         locale — the default) or \"default\" (opt out of \
                         locale-parallel materialization, §6f)"
                    );
                }
                if v.over == "*" {
                    anyhow::bail!(
                        "view {vname}: star views serialize the whole route \
                         set and never materialize per locale — filter on \
                         `locale` instead (§6f)"
                    );
                }
            }
            if let Some(s) = v.shell.as_deref() {
                if !matches!(s, "atom" | "sitemap" | "search") && !cfg.shells.contains_key(s) {
                    let registered: Vec<&str> = cfg.shells.keys().map(|k| k.as_str()).collect();
                    anyhow::bail!(
                        "view {vname}: unknown shell {s:?} (built-in shells: atom, sitemap, search; registered script shells: [{}])",
                        registered.join(", ")
                    );
                }
            }
        }
        Ok(())
    }

    /// Flatten a view's `over` chain into a base collection plus every filter
    /// along the way.
    ///
    /// `over` may name a **query-only** view (nothing to inherit ambiguously)
    /// or a **grouped, unpaginated** view — subdivision (§5c): the composer
    /// refines the parent's partition, so it must itself be grouped, and the
    /// parent's route/layout are *not* inherited (the child declares its own).
    /// Composing over a paginated view is punted (open question 30): a
    /// pageable year with months on its root raises a URL-namespace question
    /// we haven't answered.
    pub fn query(&self, name: &str) -> Result<Query> {
        let mut filters = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        let mut cur = name;
        loop {
            let v = self
                .views
                .get(cur)
                .with_context(|| format!("view {name}: `over` names unknown view {cur:?}"))?;
            if seen.contains(&cur) {
                anyhow::bail!("view {name}: `over` chain is cyclic at {cur:?}");
            }
            seen.push(cur);
            if let Some(f) = &v.filter {
                filters.push(f.clone());
            }
            // A collection or `*` terminates the chain.
            let Some(next) = self.views.get(v.over.as_str()) else {
                if v.over != "*" && !self.collections.contains_key(&v.over) {
                    anyhow::bail!(
                        "view {name}: `over = {:?}` is neither a collection nor a view",
                        v.over
                    );
                }
                filters.reverse();
                return Ok(Query {
                    base: v.over.clone(),
                    filters,
                });
            };
            if !next.is_query_only() {
                let subdividable = next.group_by.is_some()
                    && next.paginate.is_none()
                    && next.limit.is_none()
                    && next.template.is_none();
                if !subdividable {
                    anyhow::bail!(
                        "view {name}: `over = {:?}` names a view that is neither query-only \
                         nor grouped. Only query-only views and grouped unpaginated views \
                         (subdivision, §5c) may be composed over; pagination × subdivision \
                         is punted (open question 30).",
                        v.over
                    );
                }
                if v.group_by.is_none() {
                    anyhow::bail!(
                        "view {name}: `over = {:?}` names a grouped view, but {name} has no \
                         `group_by`. Composing over a grouped view means subdividing its \
                         partition (§5c), so the composer must be grouped too.",
                        v.over
                    );
                }
            }
            cur = &v.over;
        }
    }

    /// The `over` chain from `name` down to its base, nearest view first.
    /// The one chain walker — everything derived from composition
    /// (`fields_for`, `group_specs`, `grouped_chain`) reads this. Assumes the
    /// chain is acyclic, which `query()` validated at load.
    pub fn chain<'a: 'b, 'b>(&'a self, name: &'b str) -> Vec<(&'b str, &'a View)> {
        let mut out = Vec::new();
        let mut cur = name;
        while let Some(v) = self.views.get(cur) {
            out.push((cur, v));
            cur = &v.over;
        }
        out
    }

    /// The `group_by` specs governing a view, outermost ancestor first. This
    /// is subdivision (§5c): a grouped view `over` a grouped view refines the
    /// parent's partition, so the parent's spec applies before the child's.
    pub fn group_specs(&self, name: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .chain(name)
            .iter()
            .filter_map(|(_, v)| v.group_by.clone())
            .collect();
        v.reverse();
        v
    }

    /// The grouped views forming a view's subdivision chain, outermost first
    /// — the provenance axis breadcrumb trails walk (§5c).
    pub fn grouped_chain(&self, name: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .chain(name)
            .iter()
            .filter(|(_, v)| v.group_by.is_some())
            .map(|(n, _)| n.to_string())
            .collect();
        v.reverse();
        v
    }

    /// The computed-field set a view's rows carry: the union along the
    /// `over` chain, nearest declaration winning per name — fields compose
    /// exactly as filters do (§5c). Declaring `fields.summary` once on a
    /// shared query view (`published`) covers every listing composed over
    /// it; a view wanting different budgets redeclares the field.
    pub fn fields_for(&self, view: &str) -> BTreeMap<&str, &Field> {
        let mut out: BTreeMap<&str, &Field> = BTreeMap::new();
        for (_, v) in self.chain(view) {
            for (name, f) in &v.fields {
                out.entry(name.as_str()).or_insert(f);
            }
        }
        out
    }

    /// Site root, resolved relative to the config file's directory.
    pub fn root(&self) -> PathBuf {
        let joined = self.dir.join(&self.root);
        std::fs::canonicalize(&joined).unwrap_or(joined)
    }

    /// The enum record for one value of a grouped field, if declared.
    pub fn record(&self, field: &str, id: &str) -> Option<&RecordCfg> {
        self.records.get(field)?.get(id)
    }

    /// The display name a field value wears for a locale (§6f): the
    /// record's name through the standard hierarchy (inline / "@ref" /
    /// global), else the id itself.
    pub fn record_name<'a>(&'a self, field: &str, id: &'a str, locale: &str) -> &'a str {
        match self.record(field, id).and_then(|r| r.name.as_ref()) {
            Some(n) => self.i18n.text(n, locale),
            None => id,
        }
    }

    /// The slug a field value wears in routes (§6f). Defaults to the id —
    /// URLs are the only surface slugs touch; keys, params and titles
    /// keep the id.
    pub fn record_slug<'a>(&'a self, field: &str, id: &'a str) -> &'a str {
        self.record(field, id).and_then(|t| t.slug.as_deref()).unwrap_or(id)
    }

    /// The slug a tag uses in routes (§6f). Defaults to the id.
    pub fn tag_slug<'a>(&'a self, id: &'a str) -> &'a str {
        self.record_slug("tags", id)
    }

    /// The display name a tag wears for a locale — `record_name` for the
    /// `tags` field, kept named because pills call it everywhere.
    pub fn tag_name<'a>(&'a self, id: &'a str, locale: &str) -> &'a str {
        self.record_name("tags", id, locale)
    }

    /// The view that owns tag routes (q32): the posts collection's declared
    /// `tags` view, else the unique view grouped by tags. Validation
    /// guarantees a declared name resolves; ambiguity without a declaration
    /// is a load error, so None here means "this site has no tag archive".
    /// q45: content-claimed rows — logical source path → the owning view.
    /// Uniqueness is a validate() invariant, so a map is honest.
    pub fn content_claims(&self) -> BTreeMap<&str, &str> {
        self.views
            .iter()
            .filter_map(|(n, v)| v.content.as_deref().map(|c| (c, n.as_str())))
            .collect()
    }

    pub fn tags_view(&self) -> Option<(&str, &View)> {
        if let Some(name) = self
            .collections
            .values()
            .find(|c| c.kind == Kind::Posts)
            .and_then(|c| c.tags.as_deref())
        {
            return self.views.get(name).map(|v| (name, v));
        }
        let mut found = None;
        for (name, v) in &self.views {
            if v.group_by.as_deref().map(crate::views::spec_field) == Some("tags") {
                if found.is_some() {
                    return None; // ambiguous — validation already errored
                }
                found = Some((name.as_str(), v));
            }
        }
        found
    }

    /// A tag's archive URL for a row's locale (q32 + §6f): the owning
    /// view's route template rendered with the tag's slug, locale-prefixed
    /// when that view materializes per locale. None = no tag archive
    /// exists, and the pill renders unlinked.
    pub fn tag_url(&self, id: &str, locale: &str) -> Option<String> {
        let (_, v) = self.tags_view()?;
        let tmpl = v.route.as_deref()?;
        let url = crate::route::render(tmpl, |k| match k {
            "key" | "tags" => Some(self.tag_slug(id).to_string()),
            _ => None,
        })
        .ok()?;
        if locale != self.i18n.default && v.locales.as_deref() != Some("default") {
            return Some(format!("/{locale}{url}"));
        }
        Some(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(views: &str) -> Config {
        let c = cfg_raw(views);
        c.validate().expect("test config should validate");
        c
    }

    fn cfg_raw(views: &str) -> Config {
        let src = format!(
            "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [collections.blog]\nkind = \"posts\"\nsource = \"_posts\"\n{views}"
        );
        toml::from_str(&src).expect("test config should parse")
    }

    /// The load-time error a config produces, as a full anyhow chain.
    fn cfg_err(views: &str) -> String {
        let c = cfg_raw(views);
        format!("{:#}", c.validate().expect_err("config should fail validation"))
    }

    #[test]
    fn chain_flattens_and_conjoins_filters() {
        let c = cfg(r#"
            [views.published]
            over = "blog"
            filter = "!draft && !hidden"

            [views.latest]
            over = "published"
            filter = "!noindex"
            limit = 3
        "#);
        let q = c.query("latest").unwrap();
        assert_eq!(q.base, "blog");
        // Outermost last, and every link in the chain must hold.
        assert_eq!(
            q.predicate().unwrap(),
            "(!draft && !hidden) && (!noindex)"
        );
    }

    #[test]
    fn single_filter_is_not_parenthesised() {
        let c = cfg("[views.published]\nover = \"blog\"\nfilter = \"!draft\"\n");
        assert_eq!(c.query("published").unwrap().predicate().unwrap(), "!draft");
    }

    #[test]
    fn unfiltered_chain_has_no_predicate() {
        let c = cfg("[views.all]\nover = \"blog\"\n");
        assert!(c.query("all").unwrap().predicate().is_none());
    }

    /// The rule that keeps composition from needing inheritance semantics.
    #[test]
    fn composing_over_a_materialized_view_is_an_error() {
        let c = cfg(r#"
            [views.blog_index]
            over = "blog"
            filter = "!draft"
            paginate = 5
            routes = ["/blog/"]

            [views.latest]
            over = "blog_index"
            limit = 3
        "#);
        let e = c.query("latest").unwrap_err().to_string();
        assert!(e.contains("query-only"), "unexpected error: {e}");
    }

    /// Subdivision (§5c): a grouped view may compose over a grouped view —
    /// the filters flatten straight through it.
    #[test]
    fn grouped_over_grouped_is_subdivision() {
        let c = cfg(r#"
            [views.yearly]
            over = "blog"
            filter = "!draft"
            group_by = "date.year"
            route = "/blog/{year}/"

            [views.monthly]
            over = "yearly"
            group_by = "date.month"
            route = "/blog/{year}/{month:02}/"
        "#);
        let q = c.query("monthly").unwrap();
        assert_eq!(q.base, "blog");
        assert_eq!(q.predicate().unwrap(), "!draft");
    }

    /// Only subdivision is defined: a non-grouped view over a grouped one
    /// has no meaning (yet), and pagination × subdivision is punted (q30).
    #[test]
    fn non_grouped_over_grouped_is_an_error() {
        let c = cfg(r#"
            [views.yearly]
            over = "blog"
            group_by = "date.year"
            route = "/blog/{year}/"

            [views.latest]
            over = "yearly"
            limit = 3
        "#);
        let e = c.query("latest").unwrap_err().to_string();
        assert!(e.contains("subdividing"), "unexpected error: {e}");
    }

    #[test]
    fn subdividing_a_paginated_view_is_punted() {
        let c = cfg(r#"
            [views.yearly]
            over = "blog"
            group_by = "date.year"
            paginate = 10
            route = "/blog/{year}/"

            [views.monthly]
            over = "yearly"
            group_by = "date.month"
            route = "/blog/{year}/{month:02}/"
        "#);
        let e = c.query("monthly").unwrap_err().to_string();
        assert!(e.contains("punted"), "unexpected error: {e}");
    }

    /// Computed fields flow with rows through composition (§6d): declared
    /// once on a shared query view, visible to everything over it; nearest
    /// redeclaration wins.
    #[test]
    fn fields_inherit_along_over_nearest_wins() {
        let c = cfg(r#"
            [views.published]
            over = "blog"
            [views.published.fields.summary]
            truncate = { max_blocks = 4 }

            [views.blog_index]
            over = "published"
            paginate = 5
            routes = ["/blog/"]

            [views.tag_index]
            over = "published"
            group_by = "tags"
            route = "/blog/tags/{key}/"
            [views.tag_index.fields.summary]
            truncate = { max_blocks = 1 }
        "#);
        let inherited = c.fields_for("blog_index");
        assert_eq!(inherited["summary"].truncate.unwrap().max_blocks, Some(4));
        let overridden = c.fields_for("tag_index");
        assert_eq!(overridden["summary"].truncate.unwrap().max_blocks, Some(1));
    }

    #[test]
    fn unknown_over_is_an_error() {
        let c = cfg("[views.latest]\nover = \"pubished\"\nlimit = 3\n");
        let e = c.query("latest").unwrap_err().to_string();
        assert!(
            e.contains("neither a collection nor a view"),
            "unexpected error: {e}"
        );
    }

    #[test]
    fn cyclic_chain_terminates() {
        let c = cfg("[views.a]\nover = \"b\"\n\n[views.b]\nover = \"a\"\n");
        let e = c.query("a").unwrap_err().to_string();
        assert!(e.contains("cyclic"), "unexpected error: {e}");
    }

    /// §6f: the path selector assigns locales; everything else sees the
    /// logical path. Disabled i18n must be a perfect no-op.
    #[test]
    fn i18n_selectors_split_paths() {
        use std::path::Path;
        let mut i = I18nCfg { locales: vec!["fr".into()], ..Default::default() };

        // suffix: dal.fr.md -> (dal.md, fr); dal.md untouched.
        let (l, loc) = i.split(Path::new("recipes/dal.fr.md"));
        assert_eq!((l.to_str().unwrap(), loc.as_str()), ("recipes/dal.md", "fr"));
        let (l, loc) = i.split(Path::new("recipes/dal.md"));
        assert_eq!((l.to_str().unwrap(), loc.as_str()), ("recipes/dal.md", "en"));
        // an undeclared locale-looking suffix is just a dotted filename
        let (l, loc) = i.split(Path::new("a/jquery.min.js"));
        assert_eq!((l.to_str().unwrap(), loc.as_str()), ("a/jquery.min.js", "en"));

        // prefix: fr/recipes/dal.md -> (recipes/dal.md, fr).
        i.selector = Selector::Prefix;
        let (l, loc) = i.split(Path::new("fr/recipes/dal.md"));
        assert_eq!((l.to_str().unwrap(), loc.as_str()), ("recipes/dal.md", "fr"));
        let (l, loc) = i.split(Path::new("recipes/dal.md"));
        assert_eq!((l.to_str().unwrap(), loc.as_str()), ("recipes/dal.md", "en"));

        // i18n off: nothing fires, ever.
        let off = I18nCfg::default();
        let (l, loc) = off.split(Path::new("recipes/dal.fr.md"));
        assert_eq!((l.to_str().unwrap(), loc.as_str()), ("recipes/dal.fr.md", "en"));
    }

    /// §6f display-name hierarchy: inline beats global beats built-in;
    /// "@key" references the global map; "@@" escapes a literal @.
    #[test]
    fn string_hierarchy_resolves() {
        let c = cfg(
            "[views.a]\nover = \"posts\"\ntitle = \"@kitchen\"\n\n\
             [views.b]\nover = \"posts\"\ntitle = \"Inline wins\"\ncrumb = \"@@literal-at\"\n\n\
             [i18n]\nlocales = [\"fr\"]\n\n\
             [i18n.strings]\nkitchen = { en = \"Kitchen\", fr = \"Cuisine\" }\n\
             home = { en = \"Home\", fr = \"Accueil\" }\n",
        );
        let t = c.views["a"].title.as_ref().unwrap();
        assert_eq!(c.i18n.text(t, "en"), "Kitchen");
        assert_eq!(c.i18n.text(t, "fr"), "Cuisine");
        let t = c.views["b"].title.as_ref().unwrap();
        assert_eq!(c.i18n.text(t, "fr"), "Inline wins");
        let t = c.views["b"].crumb.as_ref().unwrap();
        assert_eq!(c.i18n.text(t, "en"), "@literal-at");
        // Global overrides the engine built-in; absent key keeps it.
        assert_eq!(c.i18n.string("home", "fr"), "Accueil");
        assert_eq!(c.i18n.string("related", "fr"), "Related");
    }

    /// §6f: a dangling reference and an unused global string are both load
    /// errors — the latter is what catches a typo'd engine-key override.
    #[test]
    fn string_hierarchy_fails_loud() {
        let e = cfg_err("[views.a]\nover = \"posts\"\ntitle = \"@nope\"\n");
        assert!(e.contains("names no string"), "{e}");
        let e = cfg_err("[i18n.strings]\nhom = \"Home\"\n");
        assert!(e.contains("unused string"), "{e}");
        let e = cfg_err(
            "[views.a]\nover = \"posts\"\ntitle = \"@x\"\n\n[i18n.strings]\nx = \"@y\"\ny = \"z\"\n",
        );
        assert!(e.contains("no chains"), "{e}");
    }

    /// §6f enum records: slug and display names default to the id; a
    /// per-locale name falls back default-locale, then id. The `intro`
    /// rides the same record; the retired [tags.x] spelling errors with
    /// the new form.
    #[test]
    fn enum_records_default_to_id() {
        let c = cfg(
            "[records.tags.contes]\nslug = \"fairy-tales\"\nname = { en = \"Fairy tales\", fr = \"Contes\" }\n\n\
             [records.course.dinner]\nintro = \"Sure to please!\"\n\n[i18n]\nlocales = [\"fr\"]\n",
        );
        assert_eq!(c.tag_slug("contes"), "fairy-tales");
        assert_eq!(c.tag_slug("rust"), "rust");
        assert_eq!(c.tag_name("contes", "fr"), "Contes");
        assert_eq!(c.tag_name("contes", "en"), "Fairy tales");
        assert_eq!(c.tag_name("contes", "de"), "Fairy tales");
        assert_eq!(c.tag_name("rust", "fr"), "rust");
        assert_eq!(c.record_slug("course", "dinner"), "dinner");
        assert_eq!(c.record_name("course", "dinner", "fr"), "dinner");
        let i = c.record("course", "dinner").unwrap().intro.as_ref().unwrap();
        assert_eq!(c.i18n.text(i, "en"), "Sure to please!");

        let e = cfg_err("[tags.contes]\nslug = \"x\"\n");
        assert!(e.contains("records.tags.contes"), "{e}");
    }
}
