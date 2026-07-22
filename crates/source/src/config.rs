use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_root")]
    pub root: PathBuf,
    /// Honour .gitignore when walking (default true). It is the site's existing
    /// declaration of what is not content; see store::walker.
    #[serde(default = "default_true")]
    pub gitignore: bool,
    pub site: Site,
    /// Keyed by table name, resolved from each entry's `name` or its
    /// source directory. Built at load from `declared_collections`.
    #[serde(skip)]
    pub collections: BTreeMap<String, Collection>,
    /// `[[collections]]` — an array, because the table name comes from the
    /// SOURCE DIRECTORY (`_posts` -> `posts`, leading underscore stripped)
    /// and TOML has no keyless table. `name =` overrides where the two
    /// genuinely differ. A rootward source (`.`) has no directory to name
    /// it and falls back to `entries` (q51).
    #[serde(default, rename = "collections")]
    declared_collections: Vec<Collection>,
    /// Sets and routes, merged. One namespace (§5c): `from` names a
    /// collection, a set or a route, so the three cannot collide — checked
    /// in `validate`. Internally this stays one map because the split is a
    /// config-surface distinction, not an engine one: a set is a route
    /// with no path, exactly as before.
    #[serde(skip)]
    pub views: BTreeMap<String, View>,
    /// Queries that never land — no `path`. Composable, embeddable.
    #[serde(default)]
    sets: BTreeMap<String, View>,
    /// Queries that land: every URL the site emits from a query.
    #[serde(default)]
    routes: BTreeMap<String, View>,
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
    /// Build profiles (§4a): a profile is a different PROJECTION of the same
    /// database, never a different database. It may change three things and
    /// no others — which rows the views admit, what URL space the output is
    /// addressed in, and a marker themes can style on. Anything else stays
    /// site config, because a profile that can override any key is a config
    /// merge, and config merges drift (the Jekyll profiles this replaces had
    /// three different opinions about `exclude`).
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileCfg>,
    /// The profile in force, once `apply_profile` has run. `None` is the
    /// default profile: the config exactly as written.
    #[serde(skip)]
    pub profile: Option<String>,
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
    /// The config file itself. Never content: a site's own config is input
    /// to the build, and without this it routes as an ordinary tree row and
    /// gets published — which is how `grackle.toml` ended up on a website.
    /// Excluded by identity rather than by an `exclude` glob, so no site has
    /// to know the trap exists.
    #[serde(skip)]
    pub config_file: PathBuf,
}

/// One profile's overrides. Closed vocabulary, checked at load: an unknown
/// key is a parse error rather than a silently ignored intention.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfileCfg {
    /// Address: the absolute URL this projection is published under, which
    /// canonical links, the feed and the sitemap all read.
    ///
    /// `baseurl` is deliberately NOT here in v1. Today it prefixes assets
    /// and nothing else — routes are generated without it — so a profile
    /// setting it would claim to relocate a projection while leaving every
    /// canonical URL pointing at the real site. Making it a true route
    /// prefix is the punted half of this axis.
    pub url: Option<String>,
    /// A profile publishing to its own URL space usually should not be
    /// indexed — q10 is exactly this case, stated once instead of per page.
    #[serde(default)]
    pub noindex: bool,
    /// Selection: per-query `where` replacements. Queries are the only
    /// selection mechanism, so a profile never changes what LOADS — the
    /// database is identical under every profile, which is what makes two
    /// projections comparable and lets one resident db answer for several.
    ///
    /// Split to match the config surface: relaxing a set patches a QUERY,
    /// relaxing a route patches a LANDING, and which one a profile means is
    /// worth seeing. Merged at load, since the namespace is one.
    #[serde(default)]
    pub sets: BTreeMap<String, ProfileView>,
    #[serde(default)]
    pub routes: BTreeMap<String, ProfileView>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfileView {
    #[serde(rename = "where")]
    pub filter: Option<String>,
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

/// Strict is the DEFAULT (Matt, 2026-07-20): a link that matches no source
/// file or route is a load error naming the file, and a raw URL to routable
/// content is an error telling you the source form to use instead. Loose
/// leaves both untouched, which means a typo ships as a 404.
///
/// It was Loose while the corpus still had 28 raw-URL links to convert;
/// with those gone, defaulting to lenient would be the same silent-drop
/// this codebase keeps closing everywhere else (§4's constraint ethos).
/// `policy = "loose"` remains for importing a corpus that has not been
/// converted yet.
#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinkPolicy {
    Loose,
    #[default]
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
            LocalizedStr::One(s) => s.strip_prefix('@').filter(|rest| !rest.starts_with('@')),
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
    /// Set by a profile, never by the site: a projection published to its
    /// own URL space asks search engines away (q10). Site-wide, so it needs
    /// stating once rather than per row.
    #[serde(skip)]
    pub noindex: bool,
}

/// A collection's table. Declared here, defined by the database — a `kind`
/// in the TOML deserializes straight into the database's own vocabulary.
pub use grackle_model::Kind;

/// The arrangements a view can ask for. `listing` and `card_list` are routed
/// pages; `link_list` and `card` are what an embedded view renders as;
/// `gallery` is the object one.
pub const LAYOUTS: &[&str] = &["listing", "card_list", "gallery", "link_list", "card"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub kind: Kind,
    /// The table name, when the source directory is the wrong word for it
    /// (`_posts` holding a table called `notes`). Absent, the directory
    /// names the table — one place, not two.
    pub name: Option<String>,
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
    /// `trail` stays where `crumb`/`index` did not: a subdivision chain
    /// renders from a row's group keys, which no URL walk can recover.
    ///
    /// The view whose subdivision chain forms this collection's row trails
    /// (e.g. `monthly_archive` → Home > Blog > 2022 > December > 16).
    pub trail: Option<String>,
    /// The view that owns this collection's tag routes (q32): tag pills
    /// render their URLs from ITS route template, so config can move the
    /// archive and the chrome follows. Optional — a unique tags-grouped
    /// view is found on its own; no tags view at all = unlinked pills.
    pub tags: Option<String>,
    /// The SET that `next`/`previous` step through (q51). "Previous post"
    /// means previous *in a sequence*, and a sequence is a set — so the
    /// reach is declared rather than inherited from whatever the table
    /// happened to be sorted by.
    ///
    /// The point of naming one: a set carries its filter, so
    /// `adjacency = "published"` (`!draft && !hidden`) drops drafts **by
    /// construction**. Today they drop only by accident — a draft is
    /// usually undated, so it falls to the end of the chronological index
    /// and off the ends of the chain. Give a draft a date and it appears
    /// as someone's "later post".
    ///
    /// Unset keeps exactly that accident: every row of the collection, in
    /// the default locale, newest first.
    pub adjacency: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    #[serde(rename = "match")]
    pub pattern: String,
    pub route: Option<String>,
    /// Gate the rule on front-matter presence. This is what separates a Jekyll
    /// *page* (rendered, pretty URL) from a static file (copied verbatim).
    pub front_matter: Option<bool>,
    /// Publish a matching row only when something REFERENCES it (§4).
    ///
    /// The rule's `route` template still computes the row's URL, so a link
    /// can resolve before anything materializes — what is deferred is only
    /// whether a `Route` is emitted. At most one on-demand rule may cover a
    /// path (a load error otherwise); eager rules cascade normally and an
    /// eager match wins, which is what lets a specific `.well-known/**` rule
    /// sit above an on-demand `**/*` catch-all.
    #[serde(default)]
    pub on_demand: Option<bool>,
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
#[serde(deny_unknown_fields)]
pub struct View {
    /// A collection name, another set/route's name, or `*` for the route
    /// set. Spelled `from` — one namespace, so what it names decides
    /// whether this selects or subdivides (§5c); the engine derives that
    /// from the referent rather than taking a second keyword for it.
    #[serde(rename = "from")]
    pub over: String,
    #[serde(rename = "where")]
    pub filter: Option<String>,
    /// Path-glob scoping (§5 audit): globs already exist in rules (§4), so
    /// view scoping reuses them rather than growing the filter language a
    /// path operator. Matched against the row's root-relative path.
    #[serde(rename = "match")]
    pub scope: Option<String>,
    /// Explicit ordering for rows that have no natural one (§5 audit:
    /// posts sort reverse-chronologically by construction; objects don't).
    /// A column name, `-` for descending. Declared rather than defaulted:
    /// `path` is the only order every row is guaranteed to have, so anything
    /// else has to be asked for.
    pub order_by: Option<String>,
    pub group_by: Option<String>,
    pub paginate: Option<usize>,
    /// Where this lands. Present ⇒ it is a `[routes]` entry; absent ⇒ a
    /// `[sets]` entry, which never materializes (§5c's three shapes, now
    /// visible as which section an entry lives in).
    #[serde(rename = "path")]
    pub route: Option<String>,
    /// Several templates for one query — pagination lands on more than one
    /// URL, so the path cannot be the key.
    #[serde(default, rename = "paths")]
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
        RelatedCfg {
            limit: 4,
            min_score: None,
            year_penalty: None,
            max_years: None,
        }
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
    /// Every `match` glob along the chain. **Conjoined, like filters**: a
    /// child narrows within its parent's subtree and can never widen out of
    /// it. `match` is a path predicate that happens to be spelled as a glob
    /// (§5 chose globs over a filter path-operator to avoid growing the
    /// filter language, not because it is a different kind of clause), so it
    /// composes the way a predicate does.
    pub scopes: Vec<String>,
    /// The nearest `order_by` along the chain — nearest wins, like `fields`.
    /// Re-sorting a parent's rows is ordinary; there is nothing to conjoin.
    pub order_by: Option<String>,
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
        Config::load_profile(path, None)
    }

    /// Load, then project through a profile (§4a).
    ///
    /// `dev` is implicit: it needs no declaration, and undeclared it changes
    /// nothing — which is what makes `serve` safe to default to it. Any
    /// other name must be declared, so a typo is a load error naming what
    /// exists rather than a build that silently ships the wrong projection.
    pub fn load_profile(path: &Path, profile: Option<&str>) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg = Config::from_toml(&text)
            .with_context(|| format!("parsing config {}", path.display()))?;
        cfg.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        cfg.config_file = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        cfg.validate()?;
        if let Some(name) = profile {
            cfg.apply_profile(name)?;
        }
        Ok(cfg)
    }

    /// Parse and fold the query sections. The one parse path, so a config
    /// built in a test is the same shape as one read from disk.
    pub fn from_toml(text: &str) -> Result<Config> {
        let mut cfg: Config = toml::from_str(text)?;
        cfg.merge_collections()?;
        cfg.merge_queries()?;
        Ok(cfg)
    }

    /// The table a collection contributes to: its `name`, else its source
    /// directory with any leading underscore stripped. `_posts` is the
    /// `posts` table; `recipes/` is the `recipes` table; a source of `.`
    /// has no directory to name it and is `entries`.
    fn table_name(c: &Collection) -> Result<String> {
        if let Some(n) = &c.name {
            return Ok(n.clone());
        }
        let Some(src) = c.source.as_deref() else {
            anyhow::bail!(
                "a collection with no `source` (objects are matched by \
                 extension, not by directory) has no directory to name it — \
                 give it a `name`."
            );
        };
        let base = Path::new(src)
            .file_name()
            .map(|s| s.to_string_lossy().trim_start_matches('_').to_string())
            .unwrap_or_default();
        Ok(if base.is_empty() {
            "entries".to_string()
        } else {
            base
        })
    }

    /// Key every collection by its resolved name. This names the thing
    /// `from` refers to — it does NOT decide which table rows land in, which
    /// is still `kind` (`_posts` and `_drafts` are two `posts` collections
    /// feeding one corpus, §4, and stay two entries here).
    fn merge_collections(&mut self) -> Result<()> {
        for c in std::mem::take(&mut self.declared_collections) {
            let name = Config::table_name(&c)?;
            let src = c.source.clone().unwrap_or_default();
            if let Some(prev) = self.collections.insert(name.clone(), c) {
                anyhow::bail!(
                    "two collections resolve to the name {name:?} (sources \
                     {:?} and {src:?}) — `from` needs one name per thing, so \
                     give one of them an explicit `name`.",
                    prev.source.unwrap_or_default(),
                );
            }
        }
        Ok(())
    }

    /// Fold `[sets]` and `[routes]` into the one `views` map: a set never
    /// lands, a route always does. The namespace is shared with collections,
    /// so a name may live in exactly one of the three.
    fn merge_queries(&mut self) -> Result<()> {
        let sets = std::mem::take(&mut self.sets);
        let routes = std::mem::take(&mut self.routes);
        for (name, v) in &sets {
            if v.route.is_some() || !v.routes.is_empty() {
                anyhow::bail!(
                    "[sets.{name}] declares a path. A set is a query that \
                     never lands — move it to [routes.{name}]."
                );
            }
        }
        for (name, v) in &routes {
            if v.route.is_none() && v.routes.is_empty() {
                anyhow::bail!(
                    "[routes.{name}] declares no `path`. A route is a query \
                     that lands — give it one, or move it to [sets.{name}]."
                );
            }
        }
        for (name, v) in sets.into_iter().chain(routes) {
            if self.collections.contains_key(&name) {
                anyhow::bail!(
                    "{name:?} names both a collection and a set/route. `from` \
                     resolves against one namespace, so the name must be unique."
                );
            }
            if self.views.insert(name.clone(), v).is_some() {
                anyhow::bail!("{name:?} is declared as both a set and a route.");
            }
        }
        Ok(())
    }

    fn apply_profile(&mut self, name: &str) -> Result<()> {
        let Some(p) = self.profiles.remove(name) else {
            if name == "dev" {
                self.profile = Some(name.to_string());
                return Ok(());
            }
            let mut known: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
            known.push("dev");
            known.sort_unstable();
            anyhow::bail!("unknown profile {name:?} — declared: {}", known.join(", "));
        };
        if let Some(u) = p.url {
            self.site.url = u;
        }
        self.site.noindex = p.noindex;
        for (vname, over) in p.sets.into_iter().chain(p.routes) {
            let v = self
                .views
                .get_mut(&vname)
                .with_context(|| format!("profile {name}: no view named {vname:?}"))?;
            if let Some(f) = over.filter {
                // Parsed here so a bad profile filter fails at load like any
                // other, rather than at the pass that first evaluates it.
                grackle_db::filter::Filter::parse(&f, &grackle_model::row_schema())
                    .or_else(|_| {
                        grackle_db::filter::Filter::parse(&f, &grackle_model::route_schema())
                    })
                    .with_context(|| format!("profile {name}: view {vname}: filter {f:?}"))?;
                v.filter = Some(f);
            }
        }
        self.profile = Some(name.to_string());
        Ok(())
    }

    /// Every load-time config check (split from `load` so tests can run
    /// them on in-memory configs).
    pub(crate) fn validate(&self) -> Result<()> {
        let cfg = self;
        // Zero collections means zero rows, zero routes, and a build that
        // reports success over an empty directory. Always a mistake, and
        // silence is the worst way to report it: the first config a
        // newcomer writes is `[site]` and nothing else.
        if cfg.collections.is_empty() {
            anyhow::bail!(
                "no collections declared — nothing would be built. A site \
                 needs at least one `[[collections]]` saying where its \
                 content lives, e.g.\n\n  \
                 [[collections]]\n  kind = \"posts\"\n  source = \"_posts\"\n\n  \
                   [[collections.rules]]\n  match = \"**\"\n  \
                 route = \"/blog/{{year}}/{{month:02}}/{{slug}}/\""
            );
        }
        for (vname, v) in &cfg.views {
            // §5a: `layout` names the arrangement the engine builds — which
            // parts a view produces, not which fragment dresses them (that
            // is `variant`). A closed vocabulary, because an unknown name
            // used to be inert: it named no fragment, the theme fell back to
            // canonical rendering, and the routed passes discarded the value
            // anyway. Three of grack.com's own layouts were dead that way.
            if let Some(l) = v.layout.as_deref() {
                if !LAYOUTS.contains(&l) {
                    anyhow::bail!(
                        "view {vname}: layout {l:?} is not a layout — expected {}",
                        LAYOUTS.join(", ")
                    );
                }
            }
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
                if v.group_by.as_deref().map(grackle_model::spec_field) != Some("tags") {
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
                        v.group_by.as_deref().map(grackle_model::spec_field) == Some("tags")
                    })
                    .map(|(n, _)| n.as_str())
                    .collect();
                if tag_views.len() > 1 {
                    anyhow::bail!(
                        "multiple views group by tags ({}) — declare which owns \
                         tag routes: [collections.<posts>] tags = \"<view>\"",
                        tag_views.join(", ")
                    );
                }
            }
            if let Some((name, v)) = cfg.tags_view() {
                if let Some(tmpl) = v.route.as_deref() {
                    grackle_db::template::render(tmpl, |k| match k {
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
                let LocalizedStr::PerLocale(m) = s else {
                    return Ok(());
                };
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
        let mut scopes = Vec::new();
        // Nearest wins, and we walk outermost-first, so the first one seen.
        let mut order_by: Option<String> = None;
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
            if let Some(s) = &v.scope {
                scopes.push(s.clone());
            }
            if order_by.is_none() {
                order_by.clone_from(&v.order_by);
            }
            // A collection or `*` terminates the chain.
            let Some(next) = self.views.get(v.over.as_str()) else {
                if v.over != "*" && !self.collections.contains_key(&v.over) {
                    anyhow::bail!(
                        "{name}: `from = {:?}` is neither a collection, a set nor a route",
                        v.over
                    );
                }
                filters.reverse();
                scopes.reverse();
                return Ok(Query {
                    base: v.over.clone(),
                    filters,
                    scopes,
                    order_by,
                });
            };
            if !next.is_query_only() {
                let subdividable = next.group_by.is_some()
                    && next.paginate.is_none()
                    && next.limit.is_none()
                    && next.template.is_none();
                if !subdividable {
                    anyhow::bail!(
                        "{name}: `from = {:?}` names something that is neither a set nor a \
                         grouped route. Only sets and grouped, unpaginated routes may be \
                         composed over (subdivision, §5c); pagination × subdivision is \
                         punted (open question 30).",
                        v.over
                    );
                }
                if v.group_by.is_none() {
                    anyhow::bail!(
                        "{name}: `from = {:?}` names a grouped route, but {name} has no \
                         `group_by`. Composing over a grouped route means subdividing its \
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
        self.record(field, id)
            .and_then(|t| t.slug.as_deref())
            .unwrap_or(id)
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

    /// Content-claimed rows: logical source path → the owning view.
    /// Uniqueness is a validate() invariant, so a map is honest.
    pub fn content_claims(&self) -> BTreeMap<&str, &str> {
        self.views
            .iter()
            .filter_map(|(n, v)| v.content.as_deref().map(|c| (c, n.as_str())))
            .collect()
    }

    /// The view that owns tag routes: the posts collection's declared `tags`
    /// view, else the unique view grouped by tags. Ambiguity without a
    /// declaration is a load error, so None means "no tag archive".
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
            if v.group_by.as_deref().map(grackle_model::spec_field) == Some("tags") {
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
        let url = grackle_db::template::render(tmpl, |k| match k {
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
        let mut c = cfg_unmerged(views);
        c.merge_queries()
            .expect("test config sections should merge");
        c
    }

    /// Parsed, with collections keyed — but queries not yet folded, which is
    /// what the `merge_queries` checks below are about.
    fn cfg_unmerged(views: &str) -> Config {
        let src = format!(
            "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"blog\"\nkind = \"posts\"\nsource = \"_posts\"\n{views}"
        );
        let mut c: Config = toml::from_str(&src).expect("test config should parse");
        c.merge_collections()
            .expect("test collections should resolve");
        c
    }

    /// The error `merge_queries` produces, as a full anyhow chain.
    fn merge_err(views: &str) -> String {
        let mut c = cfg_unmerged(views);
        format!(
            "{:#}",
            c.merge_queries()
                .expect_err("sections should fail to merge")
        )
    }

    /// The load-time error a config produces, as a full anyhow chain.
    fn cfg_err(views: &str) -> String {
        let c = cfg_raw(views);
        format!(
            "{:#}",
            c.validate().expect_err("config should fail validation")
        )
    }

    #[test]
    fn chain_flattens_and_conjoins_filters() {
        let c = cfg(r#"
            [sets.published]
            from = "blog"
            where = "!draft && !hidden"

            [sets.latest]
            from = "published"
            where = "!noindex"
            limit = 3
        "#);
        let q = c.query("latest").unwrap();
        assert_eq!(q.base, "blog");
        // Outermost last, and every link in the chain must hold.
        assert_eq!(q.predicate().unwrap(), "(!draft && !hidden) && (!noindex)");
    }

    #[test]
    fn single_filter_is_not_parenthesised() {
        let c = cfg("[sets.published]\nfrom = \"blog\"\nwhere = \"!draft\"\n");
        assert_eq!(c.query("published").unwrap().predicate().unwrap(), "!draft");
    }

    #[test]
    fn unfiltered_chain_has_no_predicate() {
        let c = cfg("[sets.all]\nfrom = \"blog\"\n");
        assert!(c.query("all").unwrap().predicate().is_none());
    }

    /// The rule that keeps composition from needing inheritance semantics.
    #[test]
    fn composing_over_a_materialized_view_is_an_error() {
        let c = cfg(r#"
            [routes.blog_index]
            from = "blog"
            where = "!draft"
            paginate = 5
            paths = ["/blog/"]

            [sets.latest]
            from = "blog_index"
            limit = 3
        "#);
        let e = c.query("latest").unwrap_err().to_string();
        assert!(
            e.contains("neither a set nor a grouped route"),
            "unexpected error: {e}"
        );
    }

    /// Subdivision (§5c): a grouped view may compose over a grouped view —
    /// the filters flatten straight through it.
    #[test]
    fn grouped_over_grouped_is_subdivision() {
        let c = cfg(r#"
            [routes.yearly]
            from = "blog"
            where = "!draft"
            group_by = "date.year"
            path = "/blog/{year}/"

            [routes.monthly]
            from = "yearly"
            group_by = "date.month"
            path = "/blog/{year}/{month:02}/"
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
            [routes.yearly]
            from = "blog"
            group_by = "date.year"
            path = "/blog/{year}/"

            [sets.latest]
            from = "yearly"
            limit = 3
        "#);
        let e = c.query("latest").unwrap_err().to_string();
        assert!(e.contains("subdividing"), "unexpected error: {e}");
    }

    #[test]
    fn subdividing_a_paginated_view_is_punted() {
        let c = cfg(r#"
            [routes.yearly]
            from = "blog"
            group_by = "date.year"
            paginate = 10
            path = "/blog/{year}/"

            [routes.monthly]
            from = "yearly"
            group_by = "date.month"
            path = "/blog/{year}/{month:02}/"
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
            [sets.published]
            from = "blog"
            [sets.published.fields.summary]
            truncate = { max_blocks = 4 }

            [routes.blog_index]
            from = "published"
            paginate = 5
            paths = ["/blog/"]

            [routes.tag_index]
            from = "published"
            group_by = "tags"
            path = "/blog/tags/{key}/"
            [routes.tag_index.fields.summary]
            truncate = { max_blocks = 1 }
        "#);
        let inherited = c.fields_for("blog_index");
        assert_eq!(inherited["summary"].truncate.unwrap().max_blocks, Some(4));
        let overridden = c.fields_for("tag_index");
        assert_eq!(overridden["summary"].truncate.unwrap().max_blocks, Some(1));
    }

    /// The directory names the table, so it is written once.
    #[test]
    fn a_collection_takes_its_name_from_its_source_directory() {
        let c = Config::from_toml(
            "root = \".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nkind = \"posts\"\nsource = \"_posts\"\n\
             [[collections]]\nkind = \"tree\"\nsource = \"recipes\"\n",
        )
        .unwrap();
        let names: Vec<&str> = c.collections.keys().map(String::as_str).collect();
        assert_eq!(names, vec!["posts", "recipes"]);
    }

    /// A rootward source has no directory to name it (q51).
    #[test]
    fn a_root_collection_is_named_entries() {
        let c = Config::from_toml(
            "root = \".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nkind = \"tree\"\nsource = \".\"\n",
        )
        .unwrap();
        assert!(
            c.collections.contains_key("entries"),
            "{:?}",
            c.collections.keys()
        );
    }

    #[test]
    fn an_explicit_name_overrides_the_directory() {
        let c = Config::from_toml(
            "root = \".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname = \"notes\"\nkind = \"posts\"\nsource = \"_posts\"\n",
        )
        .unwrap();
        assert!(c.collections.contains_key("notes"));
    }

    /// Objects are matched by extension, so no directory names them.
    #[test]
    fn a_sourceless_collection_must_be_named() {
        let e = Config::from_toml(
            "root = \".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nkind = \"objects\"\nextensions = [\"png\"]\n",
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("give it a `name`"), "{e}");
    }

    #[test]
    fn two_collections_may_not_resolve_to_one_name() {
        let e = Config::from_toml(
            "root = \".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nkind = \"posts\"\nsource = \"_posts\"\n\
             [[collections]]\nkind = \"tree\"\nsource = \"posts\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("resolve to the name"), "{e}");
    }

    /// `match` conjoins along the chain: a child narrows within its
    /// parent's subtree. Nearest-wins would let a child silently escape it.
    /// The first config anyone writes is `[site]` and nothing else. It used
    /// to build successfully over an empty directory.
    /// A layout that names no arrangement used to be inert: it matched no
    /// fragment, the theme fell back to canonical rendering, and the routed
    /// passes discarded the name anyway. grack.com carried three of them —
    /// `tag_index`, `yearly_archive`, `monthly_archive` — and swapping all
    /// three for `listing` changed not one byte of the built site.
    #[test]
    fn a_layout_outside_the_vocabulary_is_a_load_error() {
        let src = "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
                   [[collections]]\nkind = \"posts\"\nsource = \"_posts\"\n\
                   filename_formats = [\"{slug}\"]\n\
                   [routes.x]\npath = \"/x/\"\nfrom = \"posts\"\nlayout = \"tag_index\"\n";
        let c = Config::from_toml(src).expect("it parses; validation is the gate");
        let e = format!("{:#}", c.validate().unwrap_err());
        assert!(e.contains("layout \"tag_index\" is not a layout"), "{e}");
        assert!(e.contains("listing, card_list, gallery"), "{e}");
    }

    /// §4a's leak, closed for pages. `FrontMatter` parses `draft` for every
    /// row, but only posts kept it — so `draft: true` on a page was read,
    /// dropped, and the page published into `sitemap.xml`. The flags now
    /// reach the page schema too: before this, a tree set could not say
    /// `!draft && !hidden` at all, because neither field was known.
    #[test]
    fn the_flag_family_is_queryable_on_pages() {
        // Type-checking the filter IS the assertion — `contains_key` on the
        // schema only restates a struct definition.
        let c = cfg("[sets.pages]\nfrom = \"blog\"\nwhere = \"!draft && !hidden && !noindex\"\n");
        let q = c.query("pages").unwrap();
        grackle_db::filter::Filter::parse(&q.predicate().unwrap(), &grackle_model::row_schema())
            .expect("!draft && !hidden should type-check against a page");
    }

    #[test]
    fn a_config_with_no_collections_says_so() {
        let src = "root = \".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n";
        let c = Config::from_toml(src).unwrap();
        let e = c.validate().unwrap_err().to_string();
        assert!(e.contains("no collections declared"), "{e}");
        assert!(
            e.contains("[[collections]]"),
            "the error should show the shape: {e}"
        );
    }

    #[test]
    fn match_conjoins_along_the_chain() {
        let c = cfg("[sets.recipes]\nfrom = \"blog\"\nmatch = \"recipes/**\"\n\
             [sets.desserts]\nfrom = \"recipes\"\nmatch = \"**/sweet/**\"\n");
        assert_eq!(
            c.query("desserts").unwrap().scopes,
            vec!["recipes/**".to_string(), "**/sweet/**".to_string()]
        );
        // The parent keeps only its own.
        assert_eq!(
            c.query("recipes").unwrap().scopes,
            vec!["recipes/**".to_string()]
        );
    }

    /// `order_by` is nearest-wins — re-sorting a parent's rows is ordinary.
    #[test]
    fn order_by_inherits_nearest_wins() {
        let c = cfg("[sets.books]\nfrom = \"blog\"\norder_by = \"-month\"\n\
             [sets.by_title]\nfrom = \"books\"\norder_by = \"title\"\n\
             [sets.newest]\nfrom = \"books\"\nlimit = 1\n");
        assert_eq!(
            c.query("by_title").unwrap().order_by.as_deref(),
            Some("title")
        );
        // Undeclared: inherited from the parent rather than lost.
        assert_eq!(
            c.query("newest").unwrap().order_by.as_deref(),
            Some("-month")
        );
    }

    #[test]
    fn a_set_may_not_declare_a_path() {
        let e = merge_err("[sets.s]\nfrom = \"blog\"\npath = \"/s/\"\n");
        assert!(e.contains("[routes.s]"), "{e}");
    }

    #[test]
    fn a_route_must_declare_a_path() {
        let e = merge_err("[routes.r]\nfrom = \"blog\"\n");
        assert!(e.contains("[sets.r]"), "{e}");
    }

    /// One namespace: `from` resolves against collections, sets and routes
    /// alike, and the lookup used to prefer a view silently.
    #[test]
    fn a_name_may_not_be_both_a_collection_and_a_query() {
        let e = merge_err("[sets.blog]\nfrom = \"blog\"\n");
        assert!(e.contains("one namespace"), "{e}");
    }

    #[test]
    fn a_name_may_not_be_both_a_set_and_a_route() {
        let e =
            merge_err("[sets.x]\nfrom = \"blog\"\n[routes.x]\nfrom = \"blog\"\npath = \"/x/\"\n");
        assert!(e.contains("both a set and a route"), "{e}");
    }

    /// Retired spellings carry no bespoke message any more (two consumers,
    /// both migrated) — but they must still not be SILENTLY IGNORED, which
    /// is what `deny_unknown_fields` buys: a stale key is a parse error
    /// listing what is valid.
    #[test]
    fn an_unknown_config_key_is_a_parse_error() {
        for stale in [
            "[views.published]\nfrom = \"blog\"\n",
            "[sets.s]\nover = \"blog\"\n",
            "[sets.s]\nfrom = \"blog\"\nfilter = \"!draft\"\n",
            "[routes.r]\nfrom = \"blog\"\nroute = \"/r/\"\n",
        ] {
            let src = format!(
                "root = \".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
                 [[collections]]\nkind = \"posts\"\nsource = \"_posts\"\n{stale}"
            );
            let e = Config::from_toml(&src)
                .expect_err("stale spelling should not parse")
                .to_string();
            assert!(e.contains("unknown field"), "{stale} -> {e}");
        }
    }

    #[test]
    fn unknown_over_is_an_error() {
        let c = cfg("[sets.latest]\nfrom = \"pubished\"\nlimit = 3\n");
        let e = c.query("latest").unwrap_err().to_string();
        assert!(
            e.contains("neither a collection, a set nor a route"),
            "unexpected error: {e}"
        );
    }

    #[test]
    fn cyclic_chain_terminates() {
        let c = cfg("[sets.a]\nfrom = \"b\"\n\n[sets.b]\nfrom = \"a\"\n");
        let e = c.query("a").unwrap_err().to_string();
        assert!(e.contains("cyclic"), "unexpected error: {e}");
    }

    /// §6f: the path selector assigns locales; everything else sees the
    /// logical path. Disabled i18n must be a perfect no-op.
    #[test]
    fn i18n_selectors_split_paths() {
        use std::path::Path;
        let mut i = I18nCfg {
            locales: vec!["fr".into()],
            ..Default::default()
        };

        // suffix: dal.fr.md -> (dal.md, fr); dal.md untouched.
        let (l, loc) = i.split(Path::new("recipes/dal.fr.md"));
        assert_eq!(
            (l.to_str().unwrap(), loc.as_str()),
            ("recipes/dal.md", "fr")
        );
        let (l, loc) = i.split(Path::new("recipes/dal.md"));
        assert_eq!(
            (l.to_str().unwrap(), loc.as_str()),
            ("recipes/dal.md", "en")
        );
        // an undeclared locale-looking suffix is just a dotted filename
        let (l, loc) = i.split(Path::new("a/jquery.min.js"));
        assert_eq!(
            (l.to_str().unwrap(), loc.as_str()),
            ("a/jquery.min.js", "en")
        );

        // prefix: fr/recipes/dal.md -> (recipes/dal.md, fr).
        i.selector = Selector::Prefix;
        let (l, loc) = i.split(Path::new("fr/recipes/dal.md"));
        assert_eq!(
            (l.to_str().unwrap(), loc.as_str()),
            ("recipes/dal.md", "fr")
        );
        let (l, loc) = i.split(Path::new("recipes/dal.md"));
        assert_eq!(
            (l.to_str().unwrap(), loc.as_str()),
            ("recipes/dal.md", "en")
        );

        // i18n off: nothing fires, ever.
        let off = I18nCfg::default();
        let (l, loc) = off.split(Path::new("recipes/dal.fr.md"));
        assert_eq!(
            (l.to_str().unwrap(), loc.as_str()),
            ("recipes/dal.fr.md", "en")
        );
    }

    /// §6f display-name hierarchy: inline beats global beats built-in;
    /// "@key" references the global map; "@@" escapes a literal @.
    #[test]
    fn string_hierarchy_resolves() {
        let c = cfg("[sets.a]\nfrom = \"posts\"\ntitle = \"@kitchen\"\n\n\
             [sets.b]\nfrom = \"posts\"\ntitle = \"Inline wins\"\ncrumb = \"@@literal-at\"\n\n\
             [i18n]\nlocales = [\"fr\"]\n\n\
             [i18n.strings]\nkitchen = { en = \"Kitchen\", fr = \"Cuisine\" }\n\
             home = { en = \"Home\", fr = \"Accueil\" }\n");
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
        let e = cfg_err("[sets.a]\nfrom = \"posts\"\ntitle = \"@nope\"\n");
        assert!(e.contains("names no string"), "{e}");
        let e = cfg_err("[i18n.strings]\nhom = \"Home\"\n");
        assert!(e.contains("unused string"), "{e}");
        let e = cfg_err(
            "[sets.a]\nfrom = \"posts\"\ntitle = \"@x\"\n\n[i18n.strings]\nx = \"@y\"\ny = \"z\"\n",
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
        let i = c
            .record("course", "dinner")
            .unwrap()
            .intro
            .as_ref()
            .unwrap();
        assert_eq!(c.i18n.text(i, "en"), "Sure to please!");
    }
}
