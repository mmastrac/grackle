use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Part kinds this site declares, or parts it adds to engine kinds (§5e).
    /// The engine's own kinds are always present; a site adds to them.
    #[serde(default)]
    pub parts: Vec<PartsDecl>,
    /// The config this one inherits (§4d). `"default"` merges the engine's
    /// base config underneath this file; `"none"` is the stock setup, where
    /// the site declares every collection, rule and route itself.
    ///
    /// Same word as `theme.toml`'s, for the same reason: it is the same
    /// operation — a union merge where the child wins — and one word means
    /// one thing to learn.
    #[serde(default = "default_extends")]
    pub extends: String,
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
    /// in `validate`. One map internally because the split is a
    /// config-surface distinction, not an engine one: a set is a route
    /// with no path.
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
    /// Axes: alternative FORMS of a row (q53). Each one publishes its rows at
    /// several URLs, one per value, and is the only mechanism permitted to do
    /// so — §4's "one row, one route" names this as its sole exception.
    #[serde(default)]
    pub axes: BTreeMap<String, Axis>,
    #[serde(default)]
    pub markers: BTreeMap<String, BTreeMap<String, toml::Value>>,
    /// `[html]` (§4e): what the engine puts in the document's head, declared
    /// rather than compiled in. Today one table — `[html.head.meta]`.
    #[serde(default)]
    pub html: HtmlCfg,
    /// `[schema]` (§5b, third axis): typed fields every row of the site has.
    /// `.schema.toml` says *where* a field applies; this says *always*. The
    /// base config uses it for the flag family (§4d/§4e), which are properties
    /// of a row rather than of a directory.
    #[serde(default)]
    pub schema: toml::Table,
    /// Custom block widgets (§5d): `{% name %}…{% endname %}` expands to the
    /// wrapper template with the markdown body spliced at `{body}`. Adding a
    /// widget is one config entry, no code.
    #[serde(default)]
    pub widgets: BTreeMap<String, String>,
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

fn default_extends() -> String {
    "default".to_string()
}

/// The base config, compiled in (§4d) — the same move as `parts.toml` and the
/// base theme, for the same reason: a site can forget to copy a file, and
/// cannot forget the binary.
const BASE: &str = include_str!("../assets/base.toml");

/// Merge the base config underneath a site's own.
///
/// Three shapes, three rules — and every one of them already existed, which is
/// the evidence that config inheritance needed no new law:
///
/// * **`[[collections]]` merge by SOURCE**, and the site's rules **prepend**
///   to the inherited ones. §4's "first writer wins, per key" then resolves
///   them with no extra machinery: a site's rule is nearer, so it writes the
///   route, and the base's `**` catch-all fills whatever is left. Source is
///   the key rather than `name` because source is the physical thing and
///   `name` is a label — a site renaming its posts collection to `notes` is
///   still talking about `_posts/`, and must not end up reading it twice.
/// * **Registries of definitions shadow by name** — `[sets.*]`, `[routes.*]`,
///   `[markers]`, `[widgets]`, `[shells]`, `[profiles]`, `[records.*.*]`,
///   `[i18n.strings.*]`. Your table replaces the base's of that name entire;
///   you never diff into one. Same rule as a theme fragment shadowing the
///   base's file of the same name, and it means you never have to know what
///   the base put in a table to predict what overriding it does.
/// * **Everything else merges per key, child wins** — `[site]`, `[i18n]`,
///   scalars. Same rule as front matter over rule defaults.
///
/// Arrays other than `collections`/`rules` replace wholesale: a list is one
/// authored value, and half-inheriting one is never what was meant.
fn merge_base(site: toml::Value) -> Result<toml::Value> {
    let base: toml::Value =
        toml::from_str(BASE).context("parsing the built-in base config (this is an engine bug)")?;
    let (Some(bt), Some(st)) = (base.as_table(), site.as_table()) else {
        return Ok(site);
    };
    let mut out = bt.clone();
    for (k, sv) in st.clone() {
        let merged = match (k.as_str(), out.remove(&k)) {
            ("collections", Some(bv)) => merge_collection_list(bv, sv),
            // Registries: one level deep, so the named entry is the unit.
            (
                "sets" | "routes" | "markers" | "widgets" | "shells" | "profiles" | "schema",
                Some(bv),
            ) => merge_to_depth(bv, sv, 1),
            // `[records.<field>.<id>]` and `[i18n.strings.<key>]` put the unit
            // one level further down.
            ("records" | "i18n", Some(bv)) => merge_to_depth(bv, sv, 2),
            // `[html.head.meta.<name>]` puts the unit two levels down.
            ("html", Some(bv)) => merge_to_depth(bv, sv, 3),
            ("site", Some(bv)) => merge_to_depth(bv, sv, 1),
            (_, _) => sv,
        };
        out.insert(k, merged);
    }
    Ok(toml::Value::Table(out))
}

/// Per-key merge down `depth` levels of tables; below that the site's value
/// replaces the base's whole. Depth 1 = "the named entry is the unit".
fn merge_to_depth(base: toml::Value, site: toml::Value, depth: usize) -> toml::Value {
    let (Some(bt), Some(st)) = (base.as_table(), site.as_table()) else {
        return site;
    };
    if depth == 0 {
        return site;
    }
    let mut out = bt.clone();
    for (k, sv) in st.clone() {
        let merged = match out.remove(&k) {
            Some(bv) => merge_to_depth(bv, sv, depth - 1),
            None => sv,
        };
        out.insert(k, merged);
    }
    toml::Value::Table(out)
}

/// What identifies a collection across the merge: its source directory, else
/// its name (objects have no source — they are matched by extension).
fn collection_key(c: &toml::Value) -> Option<String> {
    let t = c.as_table()?;
    if let Some(s) = t.get("source").and_then(|v| v.as_str()) {
        let s = s.trim_end_matches('/');
        return Some(format!("source:{}", if s.is_empty() { "." } else { s }));
    }
    t.get("name")
        .and_then(|v| v.as_str())
        .map(|n| format!("name:{n}"))
}

fn merge_collection_list(base: toml::Value, site: toml::Value) -> toml::Value {
    let (Some(ba), Some(sa)) = (base.as_array(), site.as_array()) else {
        return site;
    };
    let mut out = ba.clone();
    for sc in sa {
        match collection_key(sc).and_then(|k| {
            out.iter()
                .position(|bc| collection_key(bc).as_deref() == Some(k.as_str()))
        }) {
            Some(i) => out[i] = merge_collection(out[i].clone(), sc.clone()),
            None => out.push(sc.clone()),
        }
    }
    toml::Value::Array(out)
}

fn merge_collection(base: toml::Value, site: toml::Value) -> toml::Value {
    let (Some(bt), Some(st)) = (base.as_table(), site.as_table()) else {
        return site;
    };
    let mut out = bt.clone();
    for (k, sv) in st.clone() {
        let merged = match (k.as_str(), out.remove(&k)) {
            // The one place order carries meaning: the site's rules go FIRST,
            // which is all "specific rules before the catch-all" ever meant.
            ("rules", Some(bv)) => match (bv.as_array(), sv.as_array()) {
                (Some(b), Some(s)) => {
                    let mut r = s.clone();
                    r.extend(b.iter().cloned());
                    toml::Value::Array(r)
                }
                _ => sv,
            },
            ("relations" | "schema", Some(bv)) => merge_to_depth(bv, sv, 1),
            (_, _) => sv,
        };
        out.insert(k, merged);
    }
    toml::Value::Table(out)
}

/// `index.{md,html}` -> `["index.md", "index.html"]`. One group, which is all
/// `default_content` has ever needed; anything else is a literal path.
fn brace_alternatives(pat: &str) -> Vec<String> {
    let (Some(open), Some(close)) = (pat.find('{'), pat.find('}')) else {
        return vec![pat.to_string()];
    };
    if close < open {
        return vec![pat.to_string()];
    }
    let (head, tail) = (&pat[..open], &pat[close + 1..]);
    pat[open + 1..close]
        .split(',')
        .map(|alt| format!("{head}{}{tail}", alt.trim()))
        .collect()
}

fn default_root() -> PathBuf {
    PathBuf::from(".")
}

/// Whether a `content`/`default_content` string carries `{token}` placeholders
/// — a template resolved per route against its group params and axis members
/// (§5c), rather than a literal logical path. A literal claim is settled at
/// load; a templated one only once the routes exist, so the loader routes the
/// two differently. A malformed template counts as templated, so its error
/// surfaces where templates render rather than as a mysterious "names no row".
///
/// A `{a,b}` brace-alternative (`index.{md,html}`) is NOT a template token: a
/// token names ONE field, an alternative carries a comma. So a literal
/// `default_content` with alternatives is still settled at load, unchanged.
pub(crate) fn is_templated(s: &str) -> bool {
    grackle_db::template::tokens(s).map_or(true, |t| t.iter().any(|tok| !tok.contains(',')))
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
    // Titles the BASE CONFIG's routes wear (§4d). They live here rather than
    // as literals in `base.toml` so that an inherited route localizes like
    // every other engine string — a site retitles `/blog/` in one place, in
    // every language, without restating the route.
    ("blog", "Blog"),
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

/// A part-map kind a site declares, or parts it adds to an engine kind (§5e).
///
/// Raw on purpose: the typed vocabulary lives in the binary crate beside the
/// binder that enforces it, and `source` sits below that. This carries the
/// author's words up to be typed and checked there.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct PartsDecl {
    pub kind: String,
    /// `[name, type]` pairs, in declaration order. Types are `text`, `url`,
    /// `html`, `flag`, `stream:<kind>` or `map:<kind>`.
    #[serde(default)]
    pub parts: Vec<(String, String)>,
}

/// `[html]` (§4e): the parts of the document head that are a site's decision
/// rather than the engine's.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HtmlCfg {
    #[serde(default)]
    pub head: HeadCfg,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HeadCfg {
    /// `<meta name="KEY" content="…">`, where the content is a §5f text
    /// expression over the row (or, for a listing, the route). An empty
    /// result emits no tag — §5e's "an empty part deletes its element", one
    /// layer up.
    ///
    /// This is where `noindex` stopped being engine vocabulary: the engine
    /// used to read `Row.noindex` and decide to emit a robots meta. Now it
    /// evaluates whatever the config declares and knows none of the names.
    #[serde(default)]
    pub meta: BTreeMap<String, String>,
    /// `<meta property="KEY" content="…">`. A separate table because the
    /// ATTRIBUTE is different, not the mechanism: Open Graph and the
    /// `article:*` family are `property=`, and folding them into `meta` would
    /// mean the engine deciding which name takes which attribute — the exact
    /// kind of knowledge §4e is removing.
    #[serde(default)]
    pub property: BTreeMap<String, String>,
    /// `<link rel="KEY" href="…">`. Same shape one element over.
    #[serde(default)]
    pub link: BTreeMap<String, String>,
}

/// Internal-link policy (§6a).
#[derive(Debug, Deserialize, Default)]
pub struct LinksCfg {
    #[serde(default)]
    pub policy: LinkPolicy,
}

/// Strict is the DEFAULT: a link that matches no source file or route is a
/// load error naming the file, and a raw URL to routable content is an error
/// telling you the source form to use instead. Loose leaves both untouched,
/// which means a typo ships as a 404 — kept only for importing a corpus
/// whose links have not been converted yet.
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
#[derive(Debug, Clone, Deserialize)]
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
    /// The site's default theme — the root of the per-row cascade (§5a:
    /// front matter → rule default → here → the `default` directory → the
    /// base theme). A full spec, so `"ledger:dark"` sets the site's subtheme
    /// tokens too; a row that names its own theme states its own tokens.
    ///
    /// Absent is not "no theme": it means the `default` directory if there is
    /// one, and the base theme otherwise — which is why this stays `Option`
    /// rather than defaulting to `"default"`. A name no theme directory
    /// answers to is a load error listing the knowns (`Themes::load_all`).
    pub theme: Option<String>,
    /// Set by a profile, never by the site: a projection published to its
    /// own URL space asks search engines away (q10). Site-wide, so it needs
    /// stating once rather than per row.
    #[serde(skip)]
    pub noindex: bool,
}

/// A collection's table. Declared here, defined by the database — a `kind`
/// in the TOML deserializes straight into the database's own vocabulary.
pub use grackle_model::Kind;

/// The arrangements a view can ask for. `listing` is the routed one — a
/// gallery and a card list are listings whose previews hold pictures, told
/// apart by `variant`, not by layout. `link_list` and `card` are what an
/// embedded view renders as.
pub const LAYOUTS: &[&str] = &["listing", "link_list", "card"];

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
    /// The view whose subdivision chain forms this collection's row trails
    /// (e.g. `monthly_archive` → Home > Blog > 2022 > December > 16).
    /// Declared, not derived: the chain renders from a row's group keys,
    /// which no URL walk can recover.
    pub trail: Option<String>,
    /// The view that owns this collection's tag routes (q32): tag pills
    /// render their URLs from ITS route template, so config can move the
    /// archive and the chrome follows. Optional — a unique tags-grouped
    /// view is found on its own; no tags view at all = unlinked pills.
    pub tags: Option<String>,
    /// This collection's neighbour queries (§6g, q52). Each `[collections.
    /// relations.NAME]` is a small row-relative query — `over` (candidate
    /// pool), `where` (a predicate over the two-row `self`/`candidate`
    /// environment), `rank` (a score, bigger wins), `limit` — that produces
    /// one labelled group in a document's body. A collection declaring none
    /// inherits the four engine defaults (`earlier`, `later`, `related`,
    /// `linked_from`); declaring one overrides that NAME alone.
    #[serde(default)]
    pub relations: BTreeMap<String, RelationCfg>,
    /// `[collections.<name>.schema]` (§5b): typed fields every row of THIS
    /// collection has, whichever source it came from. `.schema.toml` is
    /// positional and a collection may have several sources, so this is the
    /// only place "every post has a `series`" can be said once.
    #[serde(default)]
    pub schema: toml::Table,
}

/// One declared relation (§6g). A neighbour list expressed as a query over
/// the two-row environment, so "related" and "previous post" stop being five
/// hardcoded axes and become config a site can move, retune or invent.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationCfg {
    /// The candidate pool: a set name, a collection, or a derived relation
    /// name (`linked_from`, `ancestors`, …). Absent = the collection's
    /// published set — "the shape every site's published set has" (§6g open
    /// sub-question), resolved at load.
    #[serde(rename = "over")]
    pub over: Option<String>,
    /// A boolean over `self`/`candidate` (qualified fields) and relation
    /// names (`!(candidate in earlier)`). Absent = every candidate.
    #[serde(rename = "where")]
    pub filter: Option<String>,
    /// A path glob scoping which `self` rows carry this relation — and, when
    /// the pool spans a subtree with its own `.schema.toml`, the schema
    /// `self.*`/`candidate.*` type-check against (§6g: `same_course` needs
    /// `self.course`, a recipes-only field).
    #[serde(rename = "match")]
    pub scope: Option<String>,
    /// The score, bigger wins (§6g slice 2). Absent = the built-in embedding
    /// order, so a relation that only filters need not restate ranking.
    pub rank: Option<String>,
    /// Drop candidates scoring below this after `rank` — grack.com's
    /// `min_score`, applied to the *adjusted* score, which is why it is its
    /// own key rather than a clause inside `where`.
    pub min_rank: Option<f64>,
    /// The window size. Defaults to a handful; `earlier`/`later` set 1.
    pub limit: Option<usize>,
    /// The group's heading, an `@ref` into `[i18n.strings]` (defaulting to
    /// `@NAME`). A per-locale map carries the language axis, like `title`.
    pub label: Option<LocalizedStr>,
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
/// The split gives three shapes (DESIGN.md §5c):
///
///   * query only (no route, no layout) — a named set, e.g. `published`
///   * query + layout, no route         — embeddable, e.g. `latest`
///   * query + layout + route(s)        — materialized, e.g. `blog_index`
/// An axis: alternative forms of one row (q53).
///
/// A relation points at *other rows* and needs a reach; an axis points at
/// *other forms of this row* and does not, because the row determines its own
/// members. Mechanically: **one row, several routes, keyed by a value.**
///
/// ```toml
/// [axes.theme]
/// values = ["ledger", "atlas"]   # the members; order fixes the canonical one
/// field  = "theme"               # the row field each member sets
/// prefix = "/{value}"            # what its URL wears
/// match  = "notes/**"            # which rows multiply (all of them, absent)
/// ```
///
/// The one thing an axis may not be is implicit: every value, the field it
/// sets and the URL shape are declared, because an axis multiplies the URL
/// space and §4's constraint exists to make that deliberate.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axis {
    /// The members, in order. The first is CANONICAL: it is what
    /// `<link rel="canonical">` names, what a link to the row resolves to, and
    /// the only one a `*` view (sitemap, search) sees. The rest are alternates,
    /// which is what `rel="alternate"` means and why they are not duplicates.
    pub values: Vec<String>,
    /// The row field each member sets while rendering. `theme` renders one
    /// corpus six ways; the field is named rather than assumed so the mechanism
    /// is not a theme feature wearing a general name.
    pub field: String,
}

impl Axis {
    /// The canonical member — the first declared.
    pub fn canonical(&self) -> Option<&str> {
        self.values.first().map(String::as_str)
    }

}

/// What a view ranges over (§5c). One name — a collection, another view, or
/// `*` — or a union of collections.
///
/// The union exists because `from` a collection SCOPES to that collection, and
/// §4 deliberately lets several sources feed one table: `_posts` and `_drafts`
/// are two collections of one corpus. Before scoping, `from = "posts"` quietly
/// meant every posts collection, so the union was a thing the engine kept and
/// the config could not say. Now the config says it.
///
/// A union may name only COLLECTIONS, and they must share a kind. Unioning a
/// set with a set is a query operation this does not attempt, and unioning
/// across kinds would ask one filter to type-check against two vocabularies.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum From {
    One(String),
    Union(Vec<String>),
}

impl From {
    /// The route set (`from = "*"`), which ranges over routes rather than rows.
    pub fn is_star(&self) -> bool {
        matches!(self, From::One(s) if s == "*")
    }

    /// The single name this composes over, or `None` for a union — which
    /// terminates a chain rather than continuing it.
    pub fn single(&self) -> Option<&str> {
        match self {
            From::One(s) => Some(s.as_str()),
            From::Union(_) => None,
        }
    }

    pub fn names(&self) -> &[String] {
        match self {
            From::One(s) => std::slice::from_ref(s),
            From::Union(v) => v.as_slice(),
        }
    }

    /// How it was written, for diagnostics.
    pub fn display(&self) -> String {
        match self {
            From::One(s) => format!("{s:?}"),
            From::Union(v) => format!("{v:?}"),
        }
    }
}

/// `axis = "theme"` or `axis = ["locale", "theme"]` → a Vec, empty when absent.
/// One axis is the common case; a list declares the cartesian product (q53).
/// Same string-or-list shape `from` takes, kept as a free deserializer because
/// the field wants a plain `Vec<String>` rather than a queryable referent.
fn one_or_many_string<'de, D>(d: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// A collection name, another set/route's name, `*` for the route set, or
    /// a LIST of collections to union. Spelled `from` — one namespace, so what
    /// it names decides whether this selects, subdivides (§5c) or unions; the
    /// engine derives that from the referent rather than taking a keyword for
    /// each.
    #[serde(rename = "from")]
    pub over: From,
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
    /// The axes this route is materialized across (q53): `axis = "theme"` for
    /// one, `axis = ["locale", "theme"]` for the cartesian product. One route
    /// per member-tuple, each rendering through its members' fields.
    ///
    /// The route's path allocates the URL space with a `{<axis name>}` segment
    /// per axis — the route decides where each segment goes, because that is
    /// where every other part of the URL is already decided. A view route could
    /// not be axis-multiplied before, which is why a gallery of six themes
    /// needed six copies of every landing; two axes over one view could not
    /// compose, which is the edge this closes.
    #[serde(default, deserialize_with = "one_or_many_string")]
    pub axis: Vec<String>,
    /// Which theme dresses this view, `name[:tokens]` like a row's (§5a).
    ///
    /// A listing otherwise takes the theme its member rows agree on, which
    /// makes the theme a property of the CONTENT — so the only way to render
    /// one query under two looks was to keep two copies of the rows. Declared
    /// here it is a property of the route, and N routes over one set may each
    /// wear their own. Nearest wins: the view beats member unanimity, which
    /// beats `[site] theme`.
    pub theme: Option<String>,
    /// Fill the listing's `featured` slot with the first row (q36) — the
    /// book-of-the-month shape. Most listings leave it off.
    #[serde(default)]
    pub featured: bool,
    /// Ask search engines away from this route. Declared, because the rule
    /// is editorial: tag pages and date archives are the same query language
    /// as the blog index and differ only in whether they are worth indexing.
    #[serde(default)]
    pub noindex: bool,
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
    /// q45 mode B, offered rather than demanded: the source path of a row this
    /// landing claims **if that row exists**. A brace group takes the first
    /// that does — `index.{md,html}`.
    ///
    /// The difference from `content` is entirely in the absence: a missing
    /// `content` row is an error, a missing `default_content` row just leaves
    /// the route a plain landing. That is what lets the base config ship `/`
    /// (§4d). A site with an `index.md` has that row own its homepage; a site
    /// without one gets the listing; neither had to say anything. The engine
    /// still never guesses the ARRANGEMENT (§5h) — both outcomes are declared
    /// here, and which one applies is a fact about the tree.
    pub default_content: Option<String>,
    /// True when this view came from the base config rather than the site's
    /// own file (§4d). It buys one rule: **an inherited route with nothing to
    /// show does not materialize.** A site with no `_posts/` never asked for
    /// an empty `/blog/` or a feed with no entries, and the base may not mint
    /// URLs the author did not ask for. A route the SITE declared still
    /// materializes empty — it asked.
    #[serde(skip)]
    pub inherited: bool,
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
    /// The collections this ranges over, or the single name `*`. More than one
    /// is a union (§5c), and every member shares a kind — checked where the
    /// chain terminates, so a materializer can read the kind off the first.
    pub base: Vec<String>,
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
        cfg.resolve_default_content();
        cfg.validate()?;
        if let Some(name) = profile {
            cfg.apply_profile(name)?;
        }
        Ok(cfg)
    }

    /// Parse and fold the query sections. The one parse path, so a config
    /// built in a test is the same shape as one read from disk — including
    /// the §4d base merge, which is why a test wanting isolation says
    /// `extends = "none"` rather than reaching for a second entry point.
    pub fn from_toml(text: &str) -> Result<Config> {
        let value: toml::Value = toml::from_str(text)?;
        let extends = value
            .get("extends")
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        // Whose view is whose, recorded before the merge blurs the two.
        let declared: Vec<String> = ["sets", "routes"]
            .iter()
            .filter_map(|k| value.get(k)?.as_table())
            .flat_map(|t| t.keys().cloned())
            .collect();
        let value = match extends {
            "none" => value,
            "default" => merge_base(value)?,
            other => anyhow::bail!(
                "extends = {other:?} — the only values are \"default\" (inherit \
                 the engine's base config, §4d) and \"none\" (declare \
                 everything yourself)."
            ),
        };
        let mut cfg: Config = match value.try_into() {
            Ok(c) => c,
            Err(e) => {
                // Deserializing from a merged Value loses TOML spans, and a
                // typo in the site's own file is the common failure. So
                // re-parse the site's text alone: if THAT is what's wrong, its
                // error carries the line number and is the actionable one.
                toml::from_str::<Config>(text)?;
                return Err(anyhow::Error::new(e));
            }
        };
        cfg.merge_collections()?;
        cfg.merge_queries()?;
        for (name, v) in cfg.views.iter_mut() {
            v.inherited = !declared.contains(name);
        }
        Ok(cfg)
    }

    /// Settle every `default_content` offer against the tree (§4d). A
    /// filesystem question, so it happens here rather than in `from_toml`,
    /// which has no directory to resolve against.
    ///
    /// Three outcomes, and each leaves exactly one thing at the URL:
    ///
    /// * **No such row** — the route lands on its own, as an ordinary landing.
    /// * **The row exists and places `{% view <name> %}`** — it accepts the
    ///   offer, and the claim is an ordinary q45 mode B claim from there on.
    /// * **The row exists and declines** — it wants the URL to itself, and it
    ///   already has its own route there, so the offered route stands down.
    ///
    /// The third case is what keeps this safe to inherit. A site whose
    /// homepage is a hand-built page has said nothing about `[routes.home]`
    /// and must not have its rendering changed by a route it never wrote.
    fn resolve_default_content(&mut self) {
        let root = self.root();
        for (name, v) in self.views.iter_mut() {
            let Some(pat) = v.default_content.as_deref() else {
                continue;
            };
            // A templated offer resolves per route once the group keys exist, so
            // the filesystem question this settles is answered post-materialize.
            if is_templated(pat) {
                continue;
            }
            let Some(found) = brace_alternatives(pat)
                .into_iter()
                .find(|c| root.join(c).exists())
            else {
                continue;
            };
            let tag = format!("{{% view {name} %}}");
            let accepted = std::fs::read_to_string(root.join(&found))
                .map(|t| t.contains(&tag))
                .unwrap_or(false);
            if accepted {
                v.content = Some(found);
            } else {
                v.route = None;
                v.routes.clear();
            }
        }
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
        for (name, v) in sets.iter().chain(&routes) {
            // Checked here rather than in validate() because it is a question
            // about the config's shape alone — `resolve_default_content` has
            // folded one into the other by the time validate runs.
            if v.content.is_some() && v.default_content.is_some() {
                anyhow::bail!(
                    "view {name}: declares both content and default_content — \
                     one claims a row unconditionally, the other only if it \
                     exists. Pick which."
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

    /// The fields CONFIG declares, as a filter schema. Not the whole declared
    /// set — `.schema.toml` is read during the tree walk, which has not
    /// happened yet wherever this is used.
    fn config_declared_schema(&self) -> grackle_db::filter::Schema {
        let mut s = grackle_db::filter::Schema::new();
        let tables =
            std::iter::once(&self.schema).chain(self.collections.values().map(|c| &c.schema));
        for t in tables {
            for (name, v) in t {
                let ty = v
                    .as_table()
                    .and_then(|t| t.get("type"))
                    .and_then(|t| t.as_str())
                    .and_then(crate::schema::FieldType::parse);
                if let Some(ty) = ty {
                    s.insert(grackle_model::intern(name.clone()), ty.filter_type());
                }
            }
        }
        s
    }

    fn apply_profile(&mut self, name: &str) -> Result<()> {
        let self_declared = self.config_declared_schema();
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
        if p.noindex {
            // q10: a projection in its own URL space asks search engines away,
            // site-wide. It used to set a bool the head pass read by name;
            // now it overrides the declaration, which is the same thing said
            // in the site's own vocabulary (§4e).
            self.html
                .head
                .meta
                .insert("robots".to_string(), "\"noindex,follow\"".to_string());
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
                //
                // The vocabulary is the built-ins plus whatever CONFIG
                // declares (§4e) — this runs before the tree walk, so a
                // profile filter naming a positional `.schema.toml` field
                // still parses at the pass that evaluates it, just not here.
                grackle_db::filter::Filter::parse(&f, &grackle_model::row_schema())
                    .or_else(|_| {
                        grackle_db::filter::Filter::parse(
                            &f,
                            &grackle_model::route_schema(&self_declared),
                        )
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
        // Zero collections builds an empty site and reports success.
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
            // is `variant`). Closed vocabulary: an unknown name would
            // otherwise be inert, falling back to canonical rendering.
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
                    grackle_db::template::render(tmpl, |tok| {
                        match grackle_db::template::classify(tok) {
                            (None | Some("group"), "key" | "tags") => Some("probe".to_string()),
                            _ => None,
                        }
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
                // Relation labels (§6g) are `@refs` too, so a custom label
                // (`same_course`) can name a `[i18n.strings]` entry — and a
                // dangling one is caught here, like every other reference.
                for (cname, c) in &cfg.collections {
                    for (rname, r) in &c.relations {
                        if let Some(l) = &r.label {
                            refs.push((format!("collection {cname}: relation {rname} label"), l));
                        }
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
                if (v.intro.is_some() || v.content.is_some()) && v.over.is_star() {
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
                if v.over.is_star() {
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
            // A collection, a union, or `*` terminates the chain.
            let next = v.over.single().and_then(|s| self.views.get(s));
            let Some(next) = next else {
                self.check_base(name, &v.over)?;
                filters.reverse();
                scopes.reverse();
                return Ok(Query {
                    base: v.over.names().to_vec(),
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
                        v.over.display()
                    );
                }
                if v.group_by.is_none() {
                    anyhow::bail!(
                        "{name}: `from = {:?}` names a grouped route, but {name} has no \
                         `group_by`. Composing over a grouped route means subdividing its \
                         partition (§5c), so the composer must be grouped too.",
                        v.over.display()
                    );
                }
            }
            cur = v.over.single().expect("a union terminates the chain above");
        }
    }

    /// What a terminated chain is allowed to name (§5c).
    ///
    /// One name may be a collection or `*`. A union may name only collections,
    /// and they must share a kind: the members decide the vocabulary a `where`
    /// type-checks against and whether the rows are parsed, so two kinds in one
    /// union is a query with two answers to both questions.
    fn check_base(&self, name: &str, over: &From) -> Result<()> {
        if over.is_star() {
            return Ok(());
        }
        let mut kinds: Vec<(&str, Kind)> = Vec::new();
        for member in over.names() {
            let Some(c) = self.collections.get(member) else {
                if matches!(over, From::Union(_)) {
                    anyhow::bail!(
                        "{name}: `from` unions {member:?}, which is not a collection. A union \
                         ranges over collections; to narrow a set, compose over it with `from = \
                         {member:?}` and a `where`."
                    );
                }
                anyhow::bail!(
                    "{name}: `from = {}` is neither a collection, a set nor a route",
                    over.display()
                );
            };
            kinds.push((member.as_str(), c.kind));
        }
        if let Some((first, k)) = kinds.first() {
            if let Some((other, k2)) = kinds.iter().find(|(_, x)| x != k) {
                anyhow::bail!(
                    "{name}: `from` unions collections of two kinds — {first:?} is {k:?} and \
                     {other:?} is {k2:?}. A union's members share a vocabulary, so they share a \
                     kind."
                );
            }
        }
        if over.names().is_empty() {
            anyhow::bail!("{name}: `from = []` names nothing to range over.");
        }
        Ok(())
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
            let Some(n) = v.over.single() else { break };
            cur = n;
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
            // A templated `content` resolves to a different row per route, so its
            // claims are settled post-materialization (see `load`), not here.
            .filter(|(c, _)| !is_templated(c))
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
        let url = grackle_db::template::render(tmpl, |tok| {
            match grackle_db::template::classify(tok) {
                (None | Some("group"), "key" | "tags") => Some(self.tag_slug(id).to_string()),
                _ => None,
            }
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
            "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
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
        assert_eq!(q.base, ["blog"]);
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
        assert_eq!(q.base, ["blog"]);
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
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
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
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
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
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname = \"notes\"\nkind = \"posts\"\nsource = \"_posts\"\n",
        )
        .unwrap();
        assert!(c.collections.contains_key("notes"));
    }

    /// Objects are matched by extension, so no directory names them.
    #[test]
    fn a_sourceless_collection_must_be_named() {
        let e = Config::from_toml(
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nkind = \"objects\"\nextensions = [\"png\"]\n",
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("give it a `name`"), "{e}");
    }

    #[test]
    fn two_collections_may_not_resolve_to_one_name() {
        let e = Config::from_toml(
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nkind = \"posts\"\nsource = \"_posts\"\n\
             [[collections]]\nkind = \"tree\"\nsource = \"posts\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("resolve to the name"), "{e}");
    }

    /// A layout outside the vocabulary would otherwise be inert — no
    /// fragment matches, the theme silently falls back to canonical
    /// rendering.
    #[test]
    fn a_layout_outside_the_vocabulary_is_a_load_error() {
        let src = "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
                   [[collections]]\nkind = \"posts\"\nsource = \"_posts\"\n\
                   filename_formats = [\"{slug}\"]\n\
                   [routes.x]\npath = \"/x/\"\nfrom = \"posts\"\nlayout = \"tag_index\"\n";
        let c = Config::from_toml(src).expect("it parses; validation is the gate");
        let e = format!("{:#}", c.validate().unwrap_err());
        assert!(e.contains("layout \"tag_index\" is not a layout"), "{e}");
        assert!(e.contains("listing, link_list, card"), "{e}");
    }

    /// noindex was once hardcoded as `view != "blog_index"`, making every
    /// other listing noindex by accident. It is editorial, so it is
    /// declared; an undeclared listing is indexed.
    #[test]
    fn noindex_is_a_view_declaration_defaulting_to_indexed() {
        let head = "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
                    [[collections]]\nkind = \"posts\"\nsource = \"_posts\"\n\
                    filename_formats = [\"{slug}\"]\n";
        let c = Config::from_toml(&format!(
            "{head}[routes.blog_index]\npath = \"/blog/\"\nfrom = \"posts\"\nlayout = \"listing\"\n\
             [routes.tag_index]\npath = \"/t/\"\nfrom = \"posts\"\nlayout = \"listing\"\n\
             noindex = true\n"
        ))
        .unwrap();
        assert!(!c.views["blog_index"].noindex);
        assert!(c.views["tag_index"].noindex);
    }

    /// §4a: the flag family reaches the page schema, not just posts —
    /// `draft: true` on a page was once read, dropped, and published.
    ///
    /// §4e moved the flags out of `row_schema()` and into declared schema, so
    /// the vocabulary the filter type-checks against is the SITE's now. That
    /// the assertion still reads the same way is the point: nothing about
    /// what a page can be asked changed, only who says so.
    #[test]
    fn the_flag_family_is_queryable_on_pages() {
        let c = cfg("[sets.pages]\nfrom = \"blog\"\nwhere = \"!draft && !hidden && !noindex\"\n");
        let q = c.query("pages").unwrap();
        let mut schema = grackle_model::row_schema();
        for f in ["draft", "hidden", "noindex"] {
            schema.insert(f, grackle_db::filter::Type::Bool);
        }
        // Type-checking the filter IS the assertion.
        grackle_db::filter::Filter::parse(&q.predicate().unwrap(), &schema)
            .expect("!draft && !hidden should type-check against a page");
    }

    // ---------------------------------------------------------------- §4d

    /// The site's rules go FIRST, which is the whole mechanism: §4's
    /// first-writer-wins then hands the route to the site and lets the base's
    /// catch-all fill whatever is left. Mutation-checked by reversing the
    /// concatenation, which puts the base's `/blog/...` route first.
    #[test]
    fn a_sites_rules_prepend_to_the_inherited_ones() {
        let c = Config::from_toml(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nkind=\"posts\"\nsource=\"_posts\"\n\
             [[collections.rules]]\nmatch=\"**\"\nroute=\"/writing/{slug}/\"\n",
        )
        .unwrap();
        let rules = &c.collections["posts"].rules;
        assert_eq!(rules[0].route.as_deref(), Some("/writing/{slug}/"));
        assert_eq!(
            rules.len(),
            2,
            "the base's catch-all should still be there, below"
        );
        // Not restated, so it comes from the base.
        assert_eq!(
            c.collections["posts"].filename_formats,
            vec!["{year}-{month}-{day}-{slug}".to_string()]
        );
    }

    /// Collections are matched by SOURCE, not by name — a site renaming its
    /// posts collection is still talking about `_posts/`, and two collections
    /// over one directory would read every post twice.
    #[test]
    fn a_renamed_collection_replaces_the_inherited_one_over_the_same_source() {
        let c = Config::from_toml(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"notes\"\nkind=\"posts\"\nsource=\"_posts\"\n",
        )
        .unwrap();
        assert!(c.collections.contains_key("notes"));
        assert!(
            !c.collections.contains_key("posts"),
            "`_posts` would be read twice: {:?}",
            c.collections.keys()
        );
    }

    /// A registry entry is the unit: your `[routes.feed]` replaces the base's
    /// whole, so you never have to know what the base put in one.
    #[test]
    fn a_named_route_shadows_the_inherited_one_entire() {
        let c = Config::from_toml(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [routes.feed]\npath=\"/feed.xml\"\nfrom=\"published\"\nshell=\"atom\"\n",
        )
        .unwrap();
        let feed = &c.views["feed"];
        assert_eq!(feed.route.as_deref(), Some("/feed.xml"));
        assert_eq!(feed.limit, None, "the base's limit = 20 must not leak in");
        // Untouched neighbours survive.
        assert!(c.views.contains_key("blog_index"));
    }

    /// `[site]` is a settings bag, not a registry: you set the two keys you
    /// care about and keep the rest.
    #[test]
    fn site_keys_merge_one_at_a_time() {
        let c = Config::from_toml("[site]\ntitle = \"Mine\"\n").unwrap();
        assert_eq!(c.site.title, "Mine");
        assert_eq!(c.site.url, "http://localhost:8080", "inherited");
    }

    /// Which views the SITE declared, recorded before the merge blurs it —
    /// the flag that keeps an inherited route from minting an empty URL.
    #[test]
    fn declared_views_are_told_apart_from_inherited_ones() {
        let c = Config::from_toml(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [routes.feed]\npath=\"/feed.xml\"\nfrom=\"published\"\nshell=\"atom\"\n",
        )
        .unwrap();
        assert!(!c.views["feed"].inherited, "the site wrote this one");
        assert!(c.views["blog_index"].inherited);
    }

    #[test]
    fn an_unknown_extends_names_the_two_that_exist() {
        let e = Config::from_toml(
            "extends = \"ledger\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("\"default\"") && e.contains("\"none\""), "{e}");
    }

    #[test]
    fn brace_alternatives_expand_in_order() {
        assert_eq!(
            brace_alternatives("index.{md,html}"),
            ["index.md", "index.html"]
        );
        assert_eq!(brace_alternatives("index.md"), ["index.md"]);
    }

    /// A view may not both demand a row and offer to take one.
    #[test]
    fn content_and_default_content_are_exclusive() {
        let e = Config::from_toml(
            "extends=\"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nkind=\"tree\"\nsource=\".\"\n\
             [routes.r]\npath=\"/r/\"\nfrom=\"entries\"\ncontent=\"a.md\"\n\
             default_content=\"b.md\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("Pick which"), "{e}");
    }

    /// A `[site]`-only config used to build successfully over an empty tree.
    #[test]
    fn a_config_with_no_collections_says_so() {
        let src =
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n";
        let c = Config::from_toml(src).unwrap();
        let e = c.validate().unwrap_err().to_string();
        assert!(e.contains("no collections declared"), "{e}");
        assert!(
            e.contains("[[collections]]"),
            "the error should show the shape: {e}"
        );
    }

    /// `match` conjoins: a child narrows within its parent's subtree.
    /// Nearest-wins would let a child silently escape it.
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

    /// Retired spellings must not be silently ignored: `deny_unknown_fields`
    /// makes a stale key a parse error listing what is valid.
    #[test]
    fn an_unknown_config_key_is_a_parse_error() {
        for stale in [
            "[views.published]\nfrom = \"blog\"\n",
            "[sets.s]\nover = \"blog\"\n",
            "[sets.s]\nfrom = \"blog\"\nfilter = \"!draft\"\n",
            "[routes.r]\nfrom = \"blog\"\nroute = \"/r/\"\n",
        ] {
            let src = format!(
                "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
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

        // i18n off (locales empty): the selector never fires.
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
