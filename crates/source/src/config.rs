use crate::effective::{collection_seg, index_seg, Prov, Trace};
use crate::markers::MarkerDef;
use crate::shape::{annotated, field, Law, Shape, Shaped};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
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
    /// Axes: alternative FORMS of a row (q53). Each one publishes its rows at
    /// several URLs, one per value, and is the only mechanism permitted to do
    /// so — §4's "one row, one route" names this as its sole exception.
    #[serde(default)]
    pub axes: BTreeMap<String, Axis>,
    /// Marker filename -> what that marker MEANS: the defaults it applies to
    /// its directory and below. The config says what a marker means; the tree
    /// says where (DESIGN.md §4b). The payload is a [`MarkerDef`] rather than
    /// a bare table because it is a definition under a user-chosen name, and
    /// definitions are atoms — see the newtype for why the merge needs the
    /// type to say so.
    #[serde(default)]
    pub markers: BTreeMap<String, MarkerDef>,
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
    /// `[embeds]` (IO.md §4a, I11): the embed policy — what an EMBEDDED
    /// citation of an asset no rule routed resolves to.
    ///
    /// The base ships it on, so a site needs no entry. The table exists for
    /// the two answers a site can give that the default cannot: **off** (an
    /// unrouted asset is then a load error, because nothing would address it),
    /// and **a subset** (the policy publishes only what its globs admit, and
    /// the rest is that same error).
    #[serde(default)]
    pub embeds: EmbedsCfg,
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
    /// `strict` (the default) errors on raw internal URLs with the correct
    /// form as the suggestion; `loose` resolves the new forms but leaves raw
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
    /// The selected profile's rung-0 fields, once `apply_profile` has run
    /// (§2, MERGE.md E1) — `[profiles.NAME.force]`, lifted out of the profile
    /// so the loader can reach it without knowing about profiles.
    ///
    /// Empty is the ordinary case and the whole cost of the feature on a site
    /// that declares none: `schema::force` iterates it once per row.
    #[serde(skip)]
    pub forced: BTreeMap<String, toml::Value>,
}

/// One profile: a fenced config **overlay** plus rung 0's veto block
/// (§4a, MERGE.md E2).
///
/// The body is a PARTIAL CONFIG and is kept here as TOML because that is what
/// it is merged as. `[profiles.NAME.<path>]` merges over the merged base+site
/// config through the same `merge_table` + [`Config::shape`] every other table
/// goes through, with the profile as the NEARER writer; the result is
/// deserialized into a `Config` again, so `deny_unknown_fields` type-checks
/// every path a profile can write and nothing here restates the config
/// surface. The bag/definition distinction falls out of the shape rather than
/// out of an annotation: `[profiles.p.site] title` patches one key of a bag,
/// while `[profiles.p.sets.published]` replaces the whole definition — you
/// never inherit half of one (Law 2).
///
/// Two keys are the engine's rather than the overlay's. `force` is reserved to
/// rung 0 (MERGE.md E1) and is lifted out before the merge; and [`PROJECTABLE`]
/// fences the rest, because a profile never changes what LOADS.
#[derive(Debug, Deserialize, Default)]
#[serde(transparent)]
pub struct ProfileCfg {
    body: toml::Table,
}

/// `[profiles.NAME.force]` — rung 0's veto block (MERGE.md E1), reserved and
/// never part of the overlay. Named once because the fence, the split and the
/// error messages all have to agree about the spelling.
const FORCE: &str = "force";

impl ProfileCfg {
    /// The profile's body as written: the overlay plus `force`.
    pub(crate) fn body(&self) -> &toml::Table {
        &self.body
    }
}

/// A profile's body split into the two things it is: the config OVERLAY, and
/// rung 0's forced fields.
///
/// One function because the split happens twice and must not drift — once at
/// load for every declared profile (typing the forced values, [`Config::
/// check_profiles`]) and once for the selected one, on the raw TOML, before
/// there is a `Config` for the overlay to be merged into.
fn split_profile(pname: &str, body: &toml::Table) -> Result<(toml::Table, toml::Table)> {
    let mut overlay = body.clone();
    let force = match overlay.remove(FORCE) {
        None => toml::Table::new(),
        Some(toml::Value::Table(t)) => t,
        Some(other) => anyhow::bail!(
            "profile {pname}: [profiles.{pname}.{FORCE}] is a table of \
             schema-declared field names to values (rung 0, §2), not {}",
            other.type_str()
        ),
    };
    Ok((overlay, force))
}

fn default_extends() -> String {
    "default".to_string()
}

/// The base config, compiled in (§4d) — the same move as `parts.toml` and the
/// base theme, for the same reason: a site can forget to copy a file, and
/// cannot forget the binary.
const BASE: &str = include_str!("../assets/base.toml");

// ------------------------------------------------------- the merge surface
//
// What follows is the description the merge dispatches on. `shape.rs` holds
// Law 2 and the vocabulary of laws; this holds the shape of THIS config, and
// `merge_table` reads each key's law off it — there is no law table, and a
// depth is nowhere written down. The two functions below are the compiler's
// half of that: they never run, and a field added to `Config` or `Collection`
// stops the build in one of them until the description names it.

/// The compiler's half of the merge surface. The merge runs on `toml::Value`,
/// before there is a [`Config`] to descend, so nothing else holds the
/// description to the struct: this pattern does. A new field stops the build
/// here until [`Config::shape`] names it, rather than falling through to
/// wholesale replace — which is how `[axes]` came to be merged by a law
/// nobody chose.
///
/// It pins the FIELDS; `the_shape_covers_the_config_surface` pins their TOML
/// SPELLINGS, which is what the merge dispatches on.
#[allow(dead_code)]
fn every_config_key_has_a_law(c: Config) {
    let Config {
        extends: _,
        root: _,
        gitignore: _,
        site: _,
        declared_collections: _,
        sets: _,
        routes: _,
        axes: _,
        markers: _,
        html: _,
        schema: _,
        widgets: _,
        shells: _,
        i18n: _,
        embeds: _,
        records: _,
        profiles: _,
        links: _,
        // Not config surface: `#[serde(skip)]`, derived at load from the keys
        // above, so no TOML key of a site's ever reaches them.
        collections: _,
        views: _,
        profile: _,
        dir: _,
        config_file: _,
        forced: _,
    } = c;
}

/// The site config's shape — the merge surface itself, since `merge_base`
/// reads every key's law off this list (`law_of`). Every depth in §3 table A
/// is a fact about a type named here; the fields are in declaration order so
/// the list can be diffed against `Config` above.
impl Shaped for Config {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("extends", |c: &Config| &c.extends),
            field("root", |c: &Config| &c.root),
            field("gitignore", |c: &Config| &c.gitignore),
            field("site", |c: &Config| &c.site),
            // The one serde rename on the surface. The merge keys on TOML
            // names, so this list is in TOML's name space, not Rust's.
            //
            // And §1's annotation, half of it: identity is physical, so two
            // configs writing `_posts` are writing one collection however
            // each of them names it. Structurally this is a `Vec` — an atom.
            annotated(
                "collections",
                |c: &Config| &c.declared_collections,
                Law::Collections,
            ),
            field("sets", |c: &Config| &c.sets),
            field("routes", |c: &Config| &c.routes),
            field("axes", |c: &Config| &c.axes),
            field("markers", |c: &Config| &c.markers),
            field("html", |c: &Config| &c.html),
            field("schema", |c: &Config| &c.schema),
            field("widgets", |c: &Config| &c.widgets),
            field("shells", |c: &Config| &c.shells),
            field("i18n", |c: &Config| &c.i18n),
            field("embeds", |c: &Config| &c.embeds),
            field("records", |c: &Config| &c.records),
            field("profiles", |c: &Config| &c.profiles),
            field("links", |c: &Config| &c.links),
            // The `#[serde(skip)]` fields have no TOML name and so no shape:
            // `collections`, `views`, `profile`, `dir`, `config_file`,
            // `forced`.
        ])
    }
}

// ------------------------------------------------------------------ the fence
//
// §4a's iron law, made checkable (MERGE.md E2). It lives here, beside the field
// list, for the reason the two annotations do: it is a DECISION no structure
// can imply — nothing about the type of `[collections]` says a profile may not
// write it — and a decision about a key belongs where a reader meets the key.
//
// The two lists are exhaustive over the config surface, and
// `the_fence_classifies_every_top_level_key` holds them to it: a key added to
// `Config` has to be put on one side or the other, which is the same
// compile-then-test discipline `every_config_key_has_a_law` applies one
// paragraph up. Being exhaustive is also what lets the error tell "you may not
// project this" apart from "this is not a config key at all".

/// What a profile MAY write: the surfaces that decide what a projection says
/// and which rows its queries admit.
const PROJECTABLE: &[&str] = &[
    "site", "html", "sets", "routes", "i18n", "records", "widgets", "shells", "axes",
];

/// What a profile may NEVER write: everything that decides what LOADS.
///
/// The database is identical under every profile — that is what makes two
/// projections comparable, and what lets one resident db answer for several
/// (§4a). `profiles` is here for a second reason: a profile does not contain
/// profiles, so the overlay is one layer and not a ladder.
const NOT_PROJECTABLE: &[&str] = &[
    "collections",
    // The embed policy decides ADDRESSES, and an address is a load fact: turn
    // the policy off in a projection and half the assets stop having one,
    // which is a different database rather than a different view of it.
    "embeds",
    "schema",
    "markers",
    "root",
    "gitignore",
    "extends",
    "links",
    "profiles",
];

/// The fence, applied to one top-level key of one profile's body.
///
/// `force` passes because it is not overlay at all — it is rung 0, lifted out
/// by [`split_profile`] before the merge ever sees the table.
fn fence(pname: &str, key: &str) -> Result<()> {
    if key == FORCE || PROJECTABLE.contains(&key) {
        return Ok(());
    }
    let projectable = PROJECTABLE.join(", ");
    // The two spellings the closed profile vocabulary used to have. Both are
    // live in shipped configs and in DESIGN.md, and the fence's own sentence
    // would leave a reader to guess the new form, so each names it.
    if key == "noindex" {
        anyhow::bail!(
            "profile {pname}: `noindex` is no longer a profile key — it \
             overwrote [html.head.meta] robots with a constant, which \
             silently replaced a site's own expression. Force the FIELD \
             instead, and the expression evaluates it:\n  \
             [profiles.{pname}.{FORCE}]\n  noindex = true"
        );
    }
    if key == "url" {
        anyhow::bail!(
            "profile {pname}: `url` is no longer a profile key of its own — a \
             profile is a config OVERLAY now (§4a), so it writes the site's own \
             key at the site's own path:\n  \
             [profiles.{pname}.site]\n  url = \"https://drafts.example.com\""
        );
    }
    if key == "profiles" {
        anyhow::bail!(
            "profile {pname}: [profiles.{pname}.profiles] — a profile never \
             contains profiles. A projection is one overlay over the config, \
             not a ladder of them (§4a). A profile may write: {projectable}."
        );
    }
    if NOT_PROJECTABLE.contains(&key) {
        anyhow::bail!(
            "profile {pname}: [profiles.{pname}.{key}] — a profile never changes \
             what loads; the database is identical under every profile (§4a). \
             {key:?} decides what the engine reads and how it is typed, so it is \
             site config: every projection of this site sees the same rows, and \
             a profile chooses among them.\n  \
             A profile may write: {projectable}."
        );
    }
    anyhow::bail!(
        "profile {pname}: [profiles.{pname}.{key}] names no config key — a \
         profile's body is a partial config (§4a), so its top-level keys are \
         the config's own.\n  A profile may write: {projectable}."
    )
}

impl Shaped for Collection {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("name", |c: &Collection| &c.name),
            field("source", |c: &Collection| &c.source),
            field("filename_formats", |c: &Collection| &c.filename_formats),
            field("exclude", |c: &Collection| &c.exclude),
            field("include", |c: &Collection| &c.include),
            // The other half of §1's annotation: the site's rules go FIRST,
            // which is Law 1 expressed in list order — nearer writer, earlier
            // in the file, first to claim a key.
            annotated("rules", |c: &Collection| &c.rules, Law::Prepend),
            field("trail", |c: &Collection| &c.trail),
            field("tags", |c: &Collection| &c.tags),
            field("relations", |c: &Collection| &c.relations),
            field("schema", |c: &Collection| &c.schema),
            // `inherited` is `#[serde(skip)]`: it has no TOML name and so no
            // shape, like `Config`'s five below.
        ])
    }
}

impl Shaped for Site {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("url", |s: &Site| &s.url),
            field("baseurl", |s: &Site| &s.baseurl),
            field("title", |s: &Site| &s.title),
            field("author", |s: &Site| &s.author),
            field("email", |s: &Site| &s.email),
            field("theme", |s: &Site| &s.theme),
            // `noindex` is `#[serde(skip)]`: a profile's to set, never the
            // site's to write, so it is not on the merge surface.
        ])
    }
}

impl Shaped for HtmlCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![field("head", |h: &HtmlCfg| &h.head)])
    }
}

impl Shaped for HeadCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("meta", |h: &HeadCfg| &h.meta),
            field("property", |h: &HeadCfg| &h.property),
            field("link", |h: &HeadCfg| &h.link),
        ])
    }
}

impl Shaped for I18nCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("default", |i: &I18nCfg| &i.default),
            field("locales", |i: &I18nCfg| &i.locales),
            field("selector", |i: &I18nCfg| &i.selector),
            field("names", |i: &I18nCfg| &i.names),
            field("strings", |i: &I18nCfg| &i.strings),
        ])
    }
}

impl Shaped for EmbedsCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("enabled", |e: &EmbedsCfg| &e.enabled),
            // TOML's name, not Rust's: the merge keys on what a site writes.
            field("match", |e: &EmbedsCfg| &e.patterns),
        ])
    }
}

impl Shaped for LinksCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![field("policy", |l: &LinksCfg| &l.policy)])
    }
}

/// Enums: atoms wherever they sit. These three are spelled as STRINGS, so a
/// descent that reached one would hand it straight back — there is nothing in
/// them a merge could take apart.
macro_rules! enums_are_atoms {
    ($($t:ty),* $(,)?) => { $(impl Shaped for $t {
        fn shape() -> Shape { Shape::Atom }
    })* };
}

enums_are_atoms![Selector, LinkPolicy];

/// `LocalizedStr` is the atom spelled as a TABLE. `{ en = "Home", fr =
/// "Accueil" }` is one value with one authority — §3 table D's "the atom is
/// the `LocalizedStr`" — and composing two of them per locale would build a
/// string nobody wrote.
///
/// It merges exactly as the three above do; what [`Shape::TableAtom`] buys is
/// the tripwire, since a descent CAN take this one apart. Today every
/// `LocalizedStr` in the config sits at the bottom of its table's deepest path
/// (`[i18n.strings.*]`, `[records.*.*]`'s own fields) rather than beside it —
/// `a_nested_struct_ends_at_one_depth` is what holds that, and
/// `a_localized_string_beside_a_map_would_be_split` is what it would say.
impl Shaped for LocalizedStr {
    fn shape() -> Shape {
        Shape::TableAtom
    }
}

/// The definitions: structs that only ever appear under a USER-chosen name,
/// where Law 2 stops. Their fields are left undescribed on purpose — see
/// [`Shape::definition`]; `a_definition_never_sits_under_an_engine_name`
/// is what keeps that honest.
macro_rules! definitions {
    ($($t:ty),* $(,)?) => { $(impl Shaped for $t {
        fn shape() -> Shape { Shape::definition() }
    })* };
}

definitions![
    View,
    Axis,
    ShellDef,
    ProfileCfg,
    RecordCfg,
    RelationCfg,
    MarkerDef,
];

/// The law for `key`, read off the shape of the field that owns it — Law 2
/// applied to a type (`Shape::law`), or §1's annotation where the field
/// carries one. Nothing is assigned here; retype a field and its law follows.
///
/// A key no field claims is a typo on its way to `deny_unknown_fields`; until
/// it gets there it merges as it always has, whole.
fn law_of(shape: &Shape, key: &str) -> Law {
    shape
        .fields()
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(Law::Atom, |(_, s)| s.law())
}

/// One key's merge, its law now known. A key the base never wrote is the
/// site's whole under every law, so `base` is always a value both sides hold.
fn merge_by(
    law: Law,
    base: toml::Value,
    site: toml::Value,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    match law {
        Law::Atom => {
            t.record(path, t.near_over());
            site
        }
        Law::Descend(n) => merge_to_depth(base, site, n, path, t),
        Law::Collections => merge_collection_list(base, site, path, t),
        Law::Prepend => prepend(base, site, path, t),
    }
}

/// The site's array in front of the base's. Either side not an array leaves
/// the site's value whole — there is nothing to interleave.
fn prepend(
    base: toml::Value,
    site: toml::Value,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    let (Some(b), Some(s)) = (base.as_array(), site.as_array()) else {
        t.record(path, t.near_over());
        return site;
    };
    let mut out = s.clone();
    out.extend(b.iter().cloned());
    // Nearness IS the list order here, so the provenance is the index: the
    // site's rules occupy the front, the base's the tail.
    if t.on() {
        for i in 0..out.len() {
            path.push(index_seg(i));
            match (i < s.len(), t.far()) {
                (true, _) => t.record(path, t.near()),
                (false, Some(far)) => t.record(path, far),
                (false, None) => {}
            }
            path.pop();
        }
    }
    toml::Value::Array(out)
}

/// The keys that have a value even when neither writer wrote one, and what
/// that value is. Rung 6 one notch further in than `base.toml`: a serde
/// default is still the engine speaking.
///
/// Not a second copy of the defaults — these ARE the functions each field's
/// `#[serde(default = "…")]` names, called. A field given a new default
/// changes here with nothing to remember; a NEW defaulted scalar has to be
/// added, which is what `every_defaulted_scalar_is_printed`
/// (`crates/grackle/tests/base_config.rs`) is for: it reads the
/// `#[serde(default = "…")]` fields off [`Config`]'s own text and requires
/// each one in the empty site's effective config.
fn engine_defaults() -> Vec<(&'static str, toml::Value)> {
    vec![
        ("extends", default_extends().into()),
        ("root", default_root().display().to_string().into()),
        ("gitignore", default_true().into()),
    ]
}

/// Merge the base config underneath a site's own (§4d). Every rule this
/// applies already existed somewhere in the system, which is the evidence that
/// config inheritance needed no new law; [`Config::shape`] is the whole of it.
fn merge_base(site: toml::Value) -> Result<toml::Value> {
    merge_base_traced(site, &mut Trace::off())
}

/// The same merge with a recorder attached — the only entry point
/// `--effective` has, so what it prints is what the load path did. See
/// [`crate::effective`].
fn merge_base_traced(site: toml::Value, t: &mut Trace) -> Result<toml::Value> {
    let base: toml::Value =
        toml::from_str(BASE).context("parsing the built-in base config (this is an engine bug)")?;
    Ok(merge_table(
        base,
        site,
        &Config::shape(),
        &mut Vec::new(),
        t,
    ))
}

/// Project a merged config through one profile (§4a, MERGE.md E2): the
/// profile's body, minus rung 0, merged over the config with the profile as the
/// NEARER writer.
///
/// It is the same `merge_table` the base merge runs, on the same shape, so
/// nothing here decides how a key merges: `[site]` is a bag and patches per
/// key, a `[sets.*]` entry is a definition and replaces whole, and a profile
/// that means to relax one clause of a set restates the set. The projected
/// table is then deserialized like any other, which is where a profile's paths
/// are validated — `deny_unknown_fields`, for free.
///
/// Returns the projected table, rung 0's block, and the view names the overlay
/// wrote — the last so that a `where` a profile supplied can be attributed to
/// it downstream (MERGE.md C6a).
fn project(
    merged: toml::Value,
    name: &str,
    t: &mut Trace,
) -> Result<(toml::Value, toml::Table, Vec<String>)> {
    let declared = merged.get("profiles").and_then(|p| p.as_table());
    let body = match declared.and_then(|p| p.get(name)) {
        Some(b) => b.clone(),
        // `dev` is implicit (§4a): it needs no declaration, and undeclared it
        // changes nothing — which is what makes it safe for `serve` to default
        // to. Any other name must be declared, so a typo is a load error naming
        // what exists rather than a build that ships the wrong projection.
        None if name == "dev" => return Ok((merged, toml::Table::new(), Vec::new())),
        None => {
            let mut known: Vec<&str> = declared
                .map(|p| p.keys().map(String::as_str).collect())
                .unwrap_or_default();
            known.push("dev");
            known.sort_unstable();
            anyhow::bail!("unknown profile {name:?} — declared: {}", known.join(", "));
        }
    };
    let Some(body) = body.as_table() else {
        anyhow::bail!(
            "profile {name}: [profiles.{name}] is a partial config — a table of \
             the config's own keys (§4a) — not {}",
            body.type_str()
        );
    };
    for key in body.keys() {
        fence(name, key)?;
    }
    let (overlay, force) = split_profile(name, body)?;
    // Which views the overlay wrote. `sets` and `routes` are two sections of
    // one namespace (`merge_queries`), and this is read before that fold, so
    // both are asked.
    let patched: Vec<String> = ["sets", "routes"]
        .iter()
        .filter_map(|k| overlay.get(*k)?.as_table())
        .flat_map(|tbl| tbl.keys().cloned())
        .collect();
    t.layer(Prov::Profile);
    let projected = merge_table(
        merged,
        toml::Value::Table(overlay),
        &Config::shape(),
        &mut Vec::new(),
        t,
    );
    Ok((projected, force, patched))
}

// ------------------------------------------------------------- the recorder
//
// Provenance is not derived by comparing two configs afterwards: it is written
// down by the merge, as it decides. What follows records the decisions the
// merge does NOT make key by key — a subtree one side never wrote, which the
// merge passes through untouched and which is therefore where the base is most
// invisible and most worth naming.

/// Record `prov` for every atom under `path`, descending exactly as far as the
/// merge would have descended had both sides written here.
fn note_key(t: &mut Trace, path: &mut Vec<String>, law: Law, v: &toml::Value, prov: Prov) {
    if !t.on() {
        return;
    }
    match law {
        Law::Atom => t.record(path, prov),
        Law::Descend(n) => note_depth(t, path, n, v, prov),
        Law::Collections => match v.as_array() {
            Some(a) => {
                for e in a {
                    path.push(collection_seg(e));
                    note_table(t, path, &Collection::shape(), e, prov);
                    path.pop();
                }
            }
            None => t.record(path, prov),
        },
        Law::Prepend => match v.as_array() {
            Some(a) => {
                for i in 0..a.len() {
                    path.push(index_seg(i));
                    t.record(path, prov);
                    path.pop();
                }
            }
            None => t.record(path, prov),
        },
    }
}

/// `note_key` for a whole table whose keys have laws of their own — the
/// config itself, or one `[[collections]]` entry.
fn note_table(t: &mut Trace, path: &mut Vec<String>, shape: &Shape, v: &toml::Value, prov: Prov) {
    let Some(tbl) = v.as_table().filter(|tbl| !tbl.is_empty()) else {
        t.record(path, prov);
        return;
    };
    for (k, kv) in tbl {
        path.push(k.clone());
        note_key(t, path, law_of(shape, k), kv, prov);
        path.pop();
    }
}

/// The depth half of Law 2, walked for recording rather than for merging: an
/// atom sits at `depth` levels down, or wherever the tables run out first.
fn note_depth(t: &mut Trace, path: &mut Vec<String>, depth: usize, v: &toml::Value, prov: Prov) {
    match v.as_table().filter(|tbl| depth > 0 && !tbl.is_empty()) {
        None => t.record(path, prov),
        Some(tbl) => {
            for (k, kv) in tbl {
                path.push(k.clone());
                note_depth(t, path, depth - 1, kv, prov);
                path.pop();
            }
        }
    }
}

/// One table merged over another, each key by its law. The shared body of the
/// two merges — the config's and one collection's — so that a law means the
/// same thing at either level, and a test can drive the dispatch with a base
/// of its own rather than restating the loop.
///
/// `shape` is the type structure of the struct this table deserializes into,
/// and it is the ONLY thing consulted: a key's law is a fact about its
/// field's type (MERGE.md B2), so there is no table here to keep in step with
/// `Config` and no depth for anyone to assign.
/// `path` and `t` are the recorder (MERGE.md B3). The load path passes
/// `Trace::off()`, which reduces the whole apparatus to one bool test per key;
/// `--effective` passes a recording one and prints what it wrote. There is no
/// second traversal and no after-the-fact diff of the two configs, so the
/// provenance cannot disagree with the merge that produced it.
fn merge_table(
    base: toml::Value,
    site: toml::Value,
    shape: &Shape,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    let (Some(bt), Some(st)) = (base.as_table(), site.as_table()) else {
        t.record(path, t.near_over());
        return site;
    };
    let mut out = bt.clone();
    for (k, sv) in st.clone() {
        path.push(k.clone());
        let law = law_of(shape, &k);
        let merged = match out.remove(&k) {
            Some(bv) => merge_by(law, bv, sv, path, t),
            None => {
                note_key(t, path, law, &sv, t.near());
                sv
            }
        };
        path.pop();
        out.insert(k, merged);
    }
    if let Some(far) = t.far().filter(|_| t.on()) {
        for (k, bv) in bt.iter().filter(|(k, _)| !st.contains_key(*k)) {
            path.push(k.clone());
            note_key(t, path, law_of(shape, k), bv, far);
            path.pop();
        }
    }
    toml::Value::Table(out)
}

/// Per-key merge down `depth` levels of tables; below that the site's value
/// replaces the base's whole. Depth 1 = "the named entry is the unit".
fn merge_to_depth(
    base: toml::Value,
    site: toml::Value,
    depth: usize,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    let (Some(bt), Some(st)) = (base.as_table(), site.as_table()) else {
        t.record(path, t.near_over());
        return site;
    };
    if depth == 0 {
        t.record(path, t.near_over());
        return site;
    }
    let mut out = bt.clone();
    for (k, sv) in st.clone() {
        path.push(k.clone());
        let merged = match out.remove(&k) {
            Some(bv) => merge_to_depth(bv, sv, depth - 1, path, t),
            None => {
                note_depth(t, path, depth - 1, &sv, t.near());
                sv
            }
        };
        path.pop();
        out.insert(k, merged);
    }
    if let Some(far) = t.far().filter(|_| t.on()) {
        for (k, bv) in bt.iter().filter(|(k, _)| !st.contains_key(*k)) {
            path.push(k.clone());
            note_depth(t, path, depth - 1, bv, far);
            path.pop();
        }
    }
    toml::Value::Table(out)
}

/// What identifies a collection across the merge: its source directory, else
/// its name (objects have no source — their own rules pick them out of the
/// tree walk).
pub(crate) fn collection_key(c: &toml::Value) -> Option<String> {
    let t = c.as_table()?;
    identity(
        t.get("source").and_then(|v| v.as_str()),
        t.get("name").and_then(|v| v.as_str()),
    )
}

/// [`collection_key`]'s rule, stated once so the TOML side (which runs during
/// the merge) and the typed side (which runs after it, to say whose rules are
/// whose) cannot come to different verdicts about which entry is which.
fn identity(source: Option<&str>, name: Option<&str>) -> Option<String> {
    if let Some(s) = source {
        let s = s.trim_end_matches('/');
        return Some(format!("source:{}", if s.is_empty() { "." } else { s }));
    }
    name.map(|n| format!("name:{n}"))
}

/// How an identity error names one collection: the table name `from` would
/// use, plus the thing that actually identifies it across the merge. Written
/// once so two entries in one message are described the same way — the whole
/// point of such a message is that the reader can tell them apart.
fn describe_collection(name: &str, c: &Collection) -> String {
    match c.source.as_deref() {
        Some(s) => format!("{name:?} at `source = {s:?}`"),
        // Objects have no source, and since IO.md I7a no `extensions` list
        // either — what picks their rows out of the walk is their rules, so
        // the rules are what a reader has left to recognise them by. Listed
        // rather than counted: two objects collections in one config differ
        // in what their globs claim, and that is the difference the reader
        // has to see to know which entry is the one they meant.
        None => match c.rules.as_slice() {
            [] => format!("{name:?} (no `source`, no rules)"),
            rules => format!(
                "{name:?} (no `source`; rules {})",
                rules
                    .iter()
                    .map(|r| format!("{:?}", r.pattern))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    }
}

fn merge_collection_list(
    base: toml::Value,
    site: toml::Value,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    let (Some(ba), Some(sa)) = (base.as_array(), site.as_array()) else {
        t.record(path, t.near_over());
        return site;
    };
    let mut out = ba.clone();
    // Which of the base's entries the site came and met. The rest are the
    // collections a site inherits without knowing they exist.
    let mut paired = vec![false; ba.len()];
    for sc in sa {
        path.push(collection_seg(sc));
        match collection_key(sc).and_then(|k| {
            out.iter()
                .position(|bc| collection_key(bc).as_deref() == Some(k.as_str()))
        }) {
            Some(i) => {
                paired[i] = true;
                out[i] = merge_collection(out[i].clone(), sc.clone(), path, t);
            }
            None => {
                note_table(t, path, &Collection::shape(), sc, t.near());
                out.push(sc.clone());
            }
        }
        path.pop();
    }
    if let Some(far) = t.far().filter(|_| t.on()) {
        for bc in ba.iter().zip(&paired).filter(|(_, m)| !**m).map(|(c, _)| c) {
            path.push(collection_seg(bc));
            note_table(t, path, &Collection::shape(), bc, far);
            path.pop();
        }
    }
    toml::Value::Array(out)
}

fn merge_collection(
    base: toml::Value,
    site: toml::Value,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    merge_table(base, site, &Collection::shape(), path, t)
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
#[serde(deny_unknown_fields)]
pub struct ShellDef {
    pub command: String,
}

/// `[embeds]`: the embed policy (IO.md §4a, I11).
///
/// A rule saying [`Rule::embed`] declines to route its rows; this says what
/// happens to them. On (the default, shipped by the base), such a row gets a
/// `strong_url` under `/static/` and publishes when something embeds it. Off,
/// or outside `match`, it gets no address at all — which is a load error
/// naming the asset, because a claimed row that lands nowhere and can be
/// reached by nothing is the config forgetting, not the config deciding.
///
/// **The prefix is not a key.** `/static/` is one directory the engine owns
/// and has published derived assets under since §6b; making it configurable
/// would be a second name for a place two mints already share, and neither
/// mint asked.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedsCfg {
    /// Publish embed-addressed rows at all. Off, a rule that declines to route
    /// is a rule with no answer, and every row it claims is a load error.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The subset the policy admits, as path globs over the row's
    /// root-relative path. Empty — the default — is every row a rule marked,
    /// which is the honest default because the RULE already selected by shape:
    /// this key is for a site that wants to narrow that selection site-wide
    /// without editing rules it inherited.
    #[serde(default, rename = "match", deserialize_with = "one_or_many_string")]
    pub patterns: Vec<String>,
}

impl Default for EmbedsCfg {
    fn default() -> Self {
        EmbedsCfg {
            enabled: true,
            patterns: Vec::new(),
        }
    }
}

impl EmbedsCfg {
    /// The `match` globs, compiled once per load. `None` is "no subset
    /// declared", which admits everything — distinct from an empty `GlobSet`,
    /// which admits nothing.
    ///
    /// Case-insensitive, for the reason every rule glob is (IO.md I7a): the
    /// shift key is not part of a file's kind.
    pub fn compiled(&self) -> Result<Option<globset::GlobSet>> {
        if self.patterns.is_empty() {
            return Ok(None);
        }
        let mut b = globset::GlobSetBuilder::new();
        for p in &self.patterns {
            b.add(
                globset::GlobBuilder::new(p)
                    .case_insensitive(true)
                    .build()
                    .with_context(|| format!("bad [embeds] match glob {p:?}"))?,
            );
        }
        Ok(Some(b.build()?))
    }
}

/// The locale axis (§6f). The *path selector* assigns each row its locale
/// at load: `suffix` reads `dal.fr.md`, `prefix` reads `fr/recipes/dal.md`.
/// Everything downstream — rules, globs, route tokens, schema resolution —
/// sees the LOGICAL path (locale stripped), so a translation rides the same
/// rule as its original and lands at the locale-prefixed URL.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct I18nCfg {
    #[serde(default = "default_locale")]
    pub default: String,
    /// Non-default locales a path may declare. Empty = i18n off.
    #[serde(default)]
    pub locales: Vec<String>,
    #[serde(default)]
    pub selector: Selector,
    /// Display names for the translations axis (`fr = "Français"`);
    /// a missing entry falls back to the locale code. Keyed by LOCALE, so
    /// every key must be the default locale or one of `locales` — a name for
    /// an undeclared locale labels nothing, and is a load error (C4a).
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
    ///
    /// **The profile's record of itself, not the mechanism** (MERGE.md E1).
    /// What reaches a page is `[profiles.NAME.force] noindex`, written onto
    /// every row and every route at rung 0 and read by the site's own
    /// `[html.head.meta] robots` expression; this bool is what that projection
    /// says about itself, mirrored here from the forced value. Nothing in the
    /// engine reads it today — `data-profile` is stamped from
    /// [`Config::profile`] — so it is a surface for a theme or a future pass
    /// rather than a live one; see MERGE.md §7.
    #[serde(skip)]
    pub noindex: bool,
}

/// The arrangements a view can ask for. `listing` is the routed one — a
/// gallery and a card list are listings whose previews hold pictures, told
/// apart by `variant`, not by layout. `link_list` and `card` are what an
/// embedded view renders as.
pub const LAYOUTS: &[&str] = &["listing", "link_list", "card"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    /// The table name, when the source directory is the wrong word for it
    /// (`_posts` holding a table called `notes`). Absent, the directory
    /// names the table — one place, not two.
    pub name: Option<String>,
    /// The directory this collection reads — **and the scope's ROLE**, now
    /// that `kind` is gone (see [`Collection::is_posts`]). Three cases:
    ///
    /// - **Absent** — the objects scope. It reads no directory of its own; it
    ///   is picked out of the one site walk by whatever its rules claim.
    /// - **`"."`** — the tree scope, the site root. `load::walk_site` walks
    ///   `cfg.root()` whatever a collection wrote, so the tree's `source` says
    ///   nothing but "I am the root"; to give the root table another name, use
    ///   `name`, not a directory that is not read.
    /// - **`"_posts"`** and the like — a posts scope, load-bearing three times
    ///   over since IO.md I7d: it is the subtree walked, the specificity that
    ///   ORDERS the scope in the one rule sequence, and the subtree that scope
    ///   OWNS (a file under it that no rule of it claims is not content). It
    ///   also punches through the dot/underscore skip, which is how `_posts`
    ///   is walked at all.
    pub source: Option<String>,
    // No `extensions`. Membership in an objects scope is what its RULES say
    // (IO.md I7a): a `match` glob naming the extensions
    // (`**/*.{png,jpg,…}`) does the job the list did, in the one mechanism
    // that already decides where a row lands. Hard cutoff — the key is gone
    // and `deny_unknown_fields` names it at the line that wrote it. Rule
    // globs compile case-INSENSITIVE, which is what keeps `after-theme-hack
    // .PNG` an object now that a glob rather than a lowercased extension
    // scan is doing the claiming.
    // No `bucket`. §6a's bubble+bucket bare-name resolution is specced and
    // PARKED (MERGE.md F1, §7 q1): the key was declared ahead of the code that
    // would consume it, and nothing ever consumed it, so it went rather than
    // stayed as configuration that configures nothing. `deny_unknown_fields`
    // above is what makes a leftover declaration say so. The design is
    // unchanged and comes back with page bundles (§5b).
    /// The extractor's **default for this collection's rules** (§4, IO.md I6).
    /// The key that reads a file's stem lives on a [`Rule`] now, because the
    /// other half of route-token supply — the path tokens — always did; this
    /// is the bag key that feeds the rules, exactly as `[site]` feeds a page.
    /// A rule declaring its own list overrides this for the rows it governs.
    #[serde(default)]
    pub filename_formats: Vec<String>,
    /// What the site walk does NOT read, and what re-admits it — read from the
    /// `tree` collection only (§4c, IO.md I7b). `load` compiles these two lists
    /// into the one [`crate::store::NotContent`] the tree, marker and
    /// vocabulary walks share; a posts or objects collection writing them
    /// configures nothing (the loader reads only the tree's).
    ///
    /// `include` has first say over `exclude`, and over the engine's own
    /// positional not-content rule (a site-root `themes/`): it is the one
    /// key that means "publish this anyway".
    ///
    /// Since IO.md I7d these lists govern the WHOLE walk, posts sources
    /// included — there is only one walk left to govern. An `exclude` naming a
    /// scope's `source` is therefore a contradiction rather than a redundancy,
    /// and `load::walk_site` refuses it: before the merge it was a harmless
    /// line (the dot/underscore skip kept `_posts` out of the tree anyway),
    /// and after it, it empties the scope.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// See [`Collection::exclude`] — the same key, in the other direction,
    /// with the same tree-only restriction.
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
    /// relations.NAME]` is a small row-relative query — `from` (candidate
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
    /// True when this entry came from the base config rather than the site's
    /// own file (§4d) — [`View::inherited`] and [`Rule::inherited`] are the
    /// same flag on the other two registries, recorded the same way: the
    /// site's own TOML is read before the merge blurs the two.
    ///
    /// It lets a load error name where a collection came from: an error about
    /// a collection the author never wrote has to say it is inherited, or it
    /// is an error about a line that is not in their file.
    #[serde(skip)]
    pub inherited: bool,
}

impl Collection {
    /// The **sourceless** scope — the objects role. It owns no subtree and
    /// picks its rows out of the whole walk by shape (IO.md I7a). What the
    /// deleted `kind` enum spelled `objects`, read now off the one fact that
    /// always distinguished it: an objects collection has no `source` at all.
    pub fn is_objects(&self) -> bool {
        self.source.is_none()
    }

    /// The **site-root** scope — the tree role. Its `source` is `"."`
    /// (decorative: it names the table, and the walk reads the root whatever
    /// it wrote — see [`Collection::source`]). What `kind = "tree"` spelled.
    pub fn is_tree(&self) -> bool {
        self.source.as_deref() == Some(".")
    }

    /// A **proper-source** scope — the posts role, which OWNS its subtree
    /// (IO.md I7d). Everything that is neither sourceless nor the root: what
    /// `kind = "posts"` spelled.
    pub fn is_posts(&self) -> bool {
        !self.is_objects() && !self.is_tree()
    }
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
    ///
    /// Spelled `from`, the same word a view spells (§5c): both name a
    /// candidate pool, and one word is what MERGE.md G1 bought. The retired
    /// `over` is simply gone — `deny_unknown_fields` refuses it, naming the
    /// knowns, and `from` is first in that list.
    pub from: Option<String>,
    /// A boolean over `self`/`candidate` (qualified fields) and relation
    /// names (`!(candidate in earlier)`). Absent = every candidate.
    #[serde(rename = "where")]
    pub filter: Option<String>,
    /// A path glob scoping which `self` rows carry this relation — and, when
    /// the pool spans a subtree with its own `.schema.toml`, the schema
    /// `self.*`/`candidate.*` type-check against (§6g: `same_course` needs
    /// `self.course`, a recipes-only field).
    ///
    /// Spelled `scope`, because the key does both jobs and `match` named
    /// only the first (MERGE.md G2). The retired spelling is simply gone —
    /// `deny_unknown_fields` refuses it — and `match` now means one thing in
    /// this config: a rule's glob over files.
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
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(rename = "match")]
    pub pattern: String,
    /// Where a matching row lands. One template, or a LIST for the default-axis
    /// case (§6f): `["/{theme}/{axis:locale}/", "/{theme}/", "/"]` lets a member
    /// at its canonical value drop its segment by falling to a shorter template.
    /// The engine picks the shortest template that still spends every
    /// non-canonical axis. One template is the ordinary case and behaves exactly
    /// as before.
    #[serde(default, deserialize_with = "one_or_many_string")]
    pub route: Vec<String>,
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
    /// This rule's rows have **no canonical address**: the embed policy gives
    /// them one (IO.md §4a, I11).
    ///
    /// The other half of `route`, and declared for the same reason `route` is
    /// — "a rule that claims a file must say where it lands" stays the law,
    /// and this is one of the two answers rather than the absence of both.
    /// Which is what keeps *no rule supplies a route* the error it has always
    /// been: a rule that says neither has forgotten, and a rule that says this
    /// has decided.
    ///
    /// What the row gets instead is a `strong_url` — `/static/{hash}.{ext}`,
    /// the content store made public — which an EMBEDDED citation (`<img>`,
    /// `<iframe>`, a generated affordance) resolves to and an authored link
    /// refuses to, because a bookmarkable address exists on purpose and this
    /// is not one. Nothing publishes until something embeds it: the pull model
    /// is the garbage collector.
    ///
    /// Site-wide, `[embeds]` decides whether the policy runs at all and over
    /// what; a rule marked here whose row the policy declines is a load error
    /// naming the asset. Declaring it beside `route` is a config error — an
    /// address is one decision — and so is declaring it beside `on_demand`,
    /// which defers a ROUTE that this rule does not mint.
    #[serde(default)]
    pub embed: Option<bool>,
    /// Key extraction from a file's stem, for the rows this rule governs
    /// (IO.md I6). Tried in order; the first format that describes the stem
    /// supplies its tokens (`{year}`, `{month}`, `{day}`, `{slug}`) to this
    /// rule's `route`, and the row's `date` and `slug` with it.
    ///
    /// It is a rule's key because routing is: path tokens have always come
    /// from the rule's own template, and an extractor is the second supplier
    /// of the same table. Absent, the collection's
    /// [`Collection::filename_formats`] is the default — first writer wins per
    /// key across the matching rules, like `defaults`, then the collection.
    ///
    /// A rule needs none: a route spending only path tokens (`/{dir}/{stem}/`)
    /// works in a posts scope exactly as it does in the tree, which is the
    /// whole of what one supplier means.
    #[serde(default)]
    pub filename_formats: Vec<String>,
    #[serde(default)]
    pub defaults: BTreeMap<String, toml::Value>,
    /// True when this rule came from the base config rather than the site's
    /// own file (§4d) — [`View::inherited`] one table over, and recorded the
    /// same way: the site's own TOML is read before the merge blurs the two.
    ///
    /// It buys one rule: **only a rule the site WROTE can be dead.** §4's
    /// "dead rule (matches zero rows) → warning" is a message to the author
    /// of the glob, and the base's globs are nobody's to fix — a site with no
    /// `_posts/` never asked for `match = "**"` there, and a site with no
    /// `index.md` never asked for `**/index.{html,md}` (both are live in
    /// `examples/minimal`, which has neither).
    #[serde(skip)]
    pub inherited: bool,
}

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
/// ```
///
/// Those two keys are the whole table (`deny_unknown_fields`). Where the
/// members land is not said here: a route template spends `{theme}` (or
/// `{axis:theme}`) — a rule's for rows, a view's for landings — and one that
/// does not spend it opts its rows out, which is why WHICH rows multiply
/// needs no key either.
///
/// The one thing an axis may not be is implicit: every value and the field it
/// sets are declared, and a route has to spend the axis by name, because an
/// axis multiplies the URL space and §4's constraint exists to make that
/// deliberate.
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

/// What a view ranges over (§5c). One name — a collection or another view —
/// or a union of collections.
///
/// A view may also range over *no* name: absent `from` on a fold shell is the
/// whole output pool (IO.md §4, I3), which is why [`View::from`] is an
/// `Option` of this rather than this carrying a variant for it. The star
/// spelling that used to say so is retired, and a `*` here now names nothing,
/// like any other word that names nothing — see [`Config::check_base`].
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

/// A view is a *query* plus, optionally, a *materialization*.
///
/// The split gives three shapes (DESIGN.md §5c):
///
///   * query only (no route, no layout) — a named set, e.g. `published`
///   * query + layout, no route         — embeddable, e.g. `latest`
///   * query + layout + route(s)        — materialized, e.g. `blog_index`
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// A collection name, another set/route's name, or a LIST of collections
    /// to union. Spelled `from` — one namespace, so what it names decides
    /// whether this selects, subdivides (§5c) or unions; the engine derives
    /// that from the referent rather than taking a keyword for each.
    ///
    /// **Absent, on a fold shell, is the whole output pool** (IO.md §4, I3):
    /// a fold sits on a query over outputs, and "all of them" is a query.
    /// Absent on anything else is a load error — a listing has to say what it
    /// lists ([`crate::shell::check_absent_from`]). At this stage the pool is
    /// the route set (the facts half of the outputs database, which is what
    /// already exists); the join makes `from` naming a *set* select those
    /// inputs' outputs at I9.
    #[serde(default)]
    pub from: Option<From>,
    /// The predicate, path scoping included. A view once carried a separate
    /// `match` glob; it compiled to `glob(path, …)` and conjoined with this,
    /// so it was a clause of this expression wearing its own key. Written as
    /// the clause it always was (MERGE.md G2) — `where = 'glob(path,
    /// "recipes/**") && !draft'` — which is also what makes the
    /// collection-relative vs root-relative footgun unwritable: `path` is the
    /// column, and a column has one meaning.
    #[serde(rename = "where")]
    pub filter: Option<String>,
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
    /// Which section declared this view: `true` for a `[sets]` entry, `false`
    /// for a `[routes]` one. `merge_queries` folds the two into one map and
    /// the namespace really is one — but the split is the config's own
    /// statement about what an entry IS (a query that never lands vs. a
    /// landing), and a profile is held to it (§4a, MERGE.md C6c).
    ///
    /// Recorded rather than derived from `route`, because
    /// `resolve_default_content` takes a declined offer's path away: that
    /// leaves a `[routes]` entry with no path, which is not a set.
    #[serde(skip)]
    pub declared_set: bool,
    /// The profile that wrote this view's `filter`, if one did (§4a). Carried
    /// so the re-validation after `apply_profile` (MERGE.md C6b) can name it:
    /// the filter is checked by the same pass every other one is, and an
    /// error that did not say which profile wrote it would send the reader to
    /// a `[sets]` entry whose text is not the text in the message.
    #[serde(skip)]
    pub filter_profile: Option<String>,
    /// Computed fields (§6d): columns this view adds to its rows, each
    /// defined by a deriver. Views composed `from` this one inherit them —
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
    /// is why it is the only thing `from` may name.
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

    /// **The fold over every output**: no `from` at all (IO.md §4, I3).
    ///
    /// One field answers it because `check_absent_from` has already refused
    /// absent-`from` on anything but a fold, so "has no pool named" and "folds
    /// the whole pool" are the same fact by load time. It is the successor to
    /// `From::is_star`, and the pool is the same pool: today's route set.
    pub fn reads_all_outputs(&self) -> bool {
        self.from.is_none()
    }
}

/// A view's query, with the `from` chain flattened.
#[derive(Debug)]
pub struct Query {
    /// The collections this ranges over — **empty** for a fold over every
    /// output, which ranges over no collection at all (IO.md §4). More than
    /// one is a union (§5c), and every member shares a kind — checked where
    /// the chain terminates, so a materializer can read the kind off the
    /// first.
    pub base: Vec<String>,
    /// Every filter along the chain, outermost view last. All must hold — so
    /// a child narrows within its parent and can never widen out of it,
    /// path scoping (`glob(path, …)`) included, that being an ordinary
    /// clause of an ordinary `where` since MERGE.md G2.
    pub filters: Vec<String>,
    /// The nearest `order_by` along the chain — nearest wins, like `fields`.
    /// Re-sorting a parent's rows is ordinary; there is nothing to conjoin.
    pub order_by: Option<String>,
    /// A sentence per view along the chain whose `where` a profile replaced.
    /// A profile's filter is type-checked by the pass that
    /// evaluates it (§4a, MERGE.md C6a) — and that pass sees a conjunction of
    /// the whole chain, so without this its error would name whichever
    /// descendant happened to be built first and no text the author wrote.
    pub patched: Vec<String>,
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
        // The projection happens inside `from_toml`, on the merged TOML, and
        // not to the `Config` afterwards (MERGE.md E2): a profile is an
        // OVERLAY, so what it produces is an ordinary config that has been
        // through the same merge, the same deserializer and — below — the same
        // `validate` as the default projection. There is nothing left here that
        // knows a profile from a site.
        let mut cfg = Config::from_toml_profile(&text, profile)
            .with_context(|| format!("parsing config {}", path.display()))?;
        cfg.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        cfg.config_file = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        cfg.resolve_default_content();
        cfg.validate()?;
        Ok(cfg)
    }

    /// Whether this config inherits the base (§4d). Written once so that the
    /// error naming the two legal values is one sentence with one author, and
    /// `--effective` cannot come to a different verdict than the load does.
    fn extends_of(value: &toml::Value) -> Result<bool> {
        match value
            .get("extends")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
        {
            "default" => Ok(true),
            "none" => Ok(false),
            other => anyhow::bail!(
                "extends = {other:?} — the only values are \"default\" (inherit \
                 the engine's base config, §4d) and \"none\" (declare \
                 everything yourself)."
            ),
        }
    }

    /// The config the engine actually runs, as TOML with per-key provenance —
    /// `grackle config --effective` (MERGE.md B3). DESIGN.md §4d calls this
    /// the thing that makes §4d "inheritance rather than magic".
    ///
    /// Stops before deserialization on purpose, for two reasons. The merged
    /// `toml::Value` is the honest artifact — it is exactly what the
    /// deserializer is handed, where a re-serialization of `Config` would be a
    /// second rendering of the truth with its own bugs. And it means the
    /// command answers on a config the engine has REJECTED, which is when a
    /// person most needs to see what the engine thinks they wrote.
    pub fn effective(path: &Path, profile: Option<&str>) -> Result<String> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        Config::effective_toml(&text, &path.display().to_string(), profile)
            .with_context(|| format!("reading config {}", path.display()))
    }

    /// [`Config::effective`] on text already in hand; `label` names the file
    /// in the preamble.
    pub fn effective_toml(text: &str, label: &str, profile: Option<&str>) -> Result<String> {
        let value: toml::Value = toml::from_str(text)?;
        let mut trace = Trace::recording();
        let inherits_base = Config::extends_of(&value)?;
        let mut merged = if inherits_base {
            merge_base_traced(value, &mut trace)?
        } else {
            // `extends = "none"`: no merge happened, so there is nothing for
            // the merge to have recorded — every key is the site's own. Walked
            // through the recorder's own descent so the atoms land at exactly
            // the paths a merged config's would.
            note_table(
                &mut trace,
                &mut Vec::new(),
                &Config::shape(),
                &value,
                Prov::Site,
            );
            value
        };
        // The keys neither file wrote still have values, and those are the ones
        // a reader has least chance of finding. Not a copy of serde's defaults:
        // the same functions `#[serde(default = "…")]` names.
        if let Some(t) = merged.as_table_mut() {
            for (k, v) in engine_defaults() {
                if !t.contains_key(k) {
                    trace.record(&[k.to_string()], Prov::Default);
                    t.insert(k.to_string(), v);
                }
            }
        }
        let mut preamble = format!("# The effective config for {label}.\n#\n");
        preamble.push_str(if inherits_base {
            "# This site's grackle.toml merged over the base config compiled into\n\
             # the engine (DESIGN.md §4d, MERGE.md §3A). It is the table the\n\
             # deserializer is handed — not a diff of the two files: the merge\n\
             # itself recorded where every line below came from.\n"
        } else {
            "# `extends = \"none\"`, so no base was merged: this site declares its\n\
             # whole config, and every key below is its own (DESIGN.md §4d).\n"
        });
        if let Some(name) = profile {
            // MERGE.md C6e: the note asserts a projection, so it has to check
            // that there is one. `dev` needs no declaration and changes
            // nothing, which is what makes it the safe default for `serve`.
            let declared = merged.get("profiles").and_then(|p| p.as_table());
            let mut known: Vec<&str> = declared
                .map(|t| t.keys().map(String::as_str).collect())
                .unwrap_or_default();
            known.push("dev");
            known.sort_unstable();
            let real = declared.is_some_and(|t| t.contains_key(name));
            preamble.push_str(&match (known.contains(&name), real) {
                // The projection is IN the table below (MERGE.md E2), which is
                // what retired this note's old caveat: the overlay went through
                // the same `merge_table` as the base merge, one layer nearer,
                // so a `# profile` line is the profile writing a key exactly
                // the way a `# site` line is the site writing one.
                (_, true) => format!(
                    "#\n# Projected through profile {name:?} (§4a): the profile's own body,\n\
                     # minus `force`, merged over the table above as the NEAREST writer.\n\
                     # Every `# profile {name}` line below is a key it wrote.\n\
                     #\n\
                     # [profiles.{name}.force] is NOT part of that overlay: it is rung 0\n\
                     # (§2), applied per row and per route at load rather than to the\n\
                     # config, and it is printed below under [profiles] like any other\n\
                     # config value.\n"
                ),
                // `dev`, undeclared: a real projection that writes nothing.
                (true, false) => format!(
                    "#\n# NOTE: profile {name:?} is implicit (§4a) — this config declares no\n\
                     # [profiles.{name}], and an undeclared profile projects nothing. The\n\
                     # table below is what it would build.\n"
                ),
                // Keep printing: the merge below is what the reader asked for
                // and is unaffected — the profile is the part that would not
                // have happened, and the build would have refused outright.
                (false, _) => format!(
                    "#\n# NOTE: {name:?} names no profile (knowns: {}), so nothing\n\
                     # would be projected — `build --profile {name}` is a load error.\n\
                     # The merged config below is unaffected and is printed anyway.\n",
                    known.join(", ")
                ),
            });
            if real {
                // The same `project` the load path runs, with the recorder
                // turned on — so what is printed is what the build did, which
                // is B3's whole design carried to one more writer.
                (merged, _, _) = project(merged, name, &mut trace)?;
            }
        }
        Ok(crate::effective::render(
            &merged,
            &trace,
            &preamble,
            profile.unwrap_or_default(),
        ))
    }

    /// Parse and fold the query sections. The one parse path, so a config
    /// built in a test is the same shape as one read from disk — including
    /// the §4d base merge, which is why a test wanting isolation says
    /// `extends = "none"` rather than reaching for a second entry point.
    pub fn from_toml(text: &str) -> Result<Config> {
        Config::from_toml_profile(text, None)
    }

    /// [`Config::from_toml`], as projected through `profile` (§4a, MERGE.md
    /// E2).
    ///
    /// The projection sits between the base merge and the deserializer, which
    /// is the whole of the design: the profile is one more writer over the
    /// merged table, and everything below this line — deserialization,
    /// `merge_collections`, `merge_queries`, `validate` — runs on the result
    /// without knowing a projection happened.
    ///
    /// **Every declared profile is dry-run here** when none is selected
    /// (MERGE.md R5's principle, E2's shape): the same merge, the same
    /// deserializer and the same `validate`, for each `[profiles.*]` entry, so
    /// a broken overlay is a load error with no `--profile` anywhere.
    pub fn from_toml_profile(text: &str, profile: Option<&str>) -> Result<Config> {
        let value: toml::Value = toml::from_str(text)?;
        let inherits_base = Config::extends_of(&value)?;
        // Whose view is whose, recorded before the merge blurs the two. A view
        // the PROFILE declared is the author's too — it is in the file they are
        // reading — so the overlay's names join the list below.
        let mut declared: Vec<String> = ["sets", "routes"]
            .iter()
            .filter_map(|k| value.get(k)?.as_table())
            .flat_map(|t| t.keys().cloned())
            .collect();
        // And whose RULE is whose. The site's rules prepend (§1's annotation),
        // so how many it wrote per collection is all the provenance a list
        // needs: the first n are the site's, the tail is the base's.
        let site_rules: BTreeMap<String, usize> = value
            .get("collections")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let n = e
                            .get("rules")
                            .and_then(|r| r.as_array())
                            .map_or(0, |r| r.len());
                        Some((collection_key(e)?, n))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let value = match inherits_base {
            false => value,
            true => merge_base(value)?,
        };
        // The projection (MERGE.md E2). `Trace::off()` here for the same reason
        // the base merge passes it: `--effective` is the only recorder, and it
        // runs this same function through `project` with one turned on.
        let (value, forced, patched) = match profile {
            None => (value, toml::Table::new(), Vec::new()),
            Some(name) => project(value, name, &mut Trace::off())?,
        };
        declared.extend(patched.iter().cloned());
        let mut cfg: Config = match value.try_into() {
            Ok(c) => c,
            Err(e) => {
                // Deserializing from a merged Value loses TOML spans, and a
                // typo in the site's own file is the common failure. So
                // re-parse the site's text alone: if THAT is what's wrong, its
                // error carries the line number and is the actionable one.
                //
                // But the site's text alone is not a valid config on a site
                // that leans on the base for required `[site]` keys — it fails
                // with a `missing field` the merged value never had, and
                // returning THAT would report a fiction and swallow the real
                // error (MERGE.md R7). So the re-parse is only allowed to
                // speak when it says the same thing: `message()` is serde's
                // sentence with the span and the key path stripped off, which
                // is what makes the two comparable — the merged error carries
                // a key path and no span, the re-parse a span and (in its
                // Display) no key path, and only the sentence is common to
                // both. Same sentence = the same failure, now with a line
                // number; anything else = the site's text has a *different*
                // problem (or none), and the merged error is the true one.
                return match toml::from_str::<Config>(text) {
                    Err(spanned) if spanned.message() == e.message() => {
                        Err(anyhow::Error::new(spanned))
                    }
                    _ => Err(anyhow::Error::new(e)),
                };
            }
        };
        cfg.merge_collections()?;
        cfg.merge_queries()?;
        for (name, v) in cfg.views.iter_mut() {
            v.inherited = !declared.contains(name);
        }
        for c in cfg.collections.values_mut() {
            // `site_rules` is keyed by the same identity the merge pairs on,
            // so its KEY SET is "the collections the site declared" and its
            // values are how many rules each of them wrote. One read of the
            // pre-merge TOML answers both questions.
            let mine = identity(c.source.as_deref(), c.name.as_deref())
                .and_then(|k| site_rules.get(&k).copied());
            c.inherited = mine.is_none();
            let mine = mine.unwrap_or(0);
            for (i, r) in c.rules.iter_mut().enumerate() {
                r.inherited = i >= mine;
            }
        }
        cfg.check_objects_rule_gate()?;
        cfg.check_rule_address()?;
        if let Some(name) = profile {
            // Rung 0, lifted out of the profile so the loader can reach it
            // without knowing about profiles (§2, MERGE.md E1).
            cfg.forced = forced.into_iter().collect();
            // The profile's record of ITSELF, distinct from what it forces:
            // "this projection asks search engines away", carried on `Site` for
            // a theme or a future surface to read. `data-profile` is the other
            // half and comes off `cfg.profile`.
            cfg.site.noindex =
                matches!(cfg.forced.get("noindex"), Some(toml::Value::Boolean(true)));
            cfg.profile = Some(name.to_string());
            // Who wrote the `where` a view carries. The overlay replaced the
            // whole definition, so the config no longer remembers on its own —
            // and an error about a filter must not send a reader to a `[sets]`
            // entry whose text is not the text in the message (MERGE.md C6a).
            for vname in &patched {
                if let Some(v) = cfg.views.get_mut(vname) {
                    v.filter_profile = Some(name.to_string());
                }
            }
        } else {
            // Every declared profile, projected and validated, at every load —
            // so a typo in a projection nobody is building today is a load
            // error today (MERGE.md R5). It is the same three passes the
            // selected profile gets: fence, merge + deserialize, validate.
            //
            // `resolve_default_content` is deliberately NOT re-run per profile:
            // it reads the filesystem, `from_toml` has no directory, and every
            // difference it makes is one that ADDS an error (a claimed row, a
            // route stood down) — so the dry run is strictly the more lenient
            // of the two and cannot invent a failure the real load would not
            // have. See MERGE.md §6, E2.
            for name in cfg.profiles.keys() {
                Config::from_toml_profile(text, Some(name))
                    .and_then(|p| p.validate())
                    .with_context(|| {
                        format!(
                            "profile {name} (checked at every load — a projection \
                             is part of this config, §4a)"
                        )
                    })?;
            }
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

    /// An objects rule may not declare `front_matter` — either value
    /// (IO.md IR9). The dead-key family one table over, and the reason is one
    /// sentence: **an objects rule selects by shape; the identity gate belongs
    /// to the scopes that parse.**
    ///
    /// Two questions run over an image and they are not the same question
    /// (IO.md I7e). *Is this row a picture* is the extension fact — `load`'s
    /// `is_obj`, the objects globs asked of the path alone, before anything is
    /// peeked — and it is what keys `object_ix`, `by_name` and the header read
    /// that fills `width`/`height`, whichever scope ends up claiming the row.
    /// *Which scope claims this row* is the ordered rule sequence. A
    /// `front_matter` gate is the one spelling that makes the two disagree,
    /// because the gate reads identity and the fact reads the path:
    ///
    /// - `front_matter = true` — an image with a sidecar passes the gate, so
    ///   this scope claims it and spends its route; the blockless image beside
    ///   it fails and falls to whatever scope comes next, keeping its place in
    ///   the objects index all the same. One directory of pictures, split
    ///   across two scopes by whether someone wrote a `.toml`.
    /// - `front_matter = false` — the same split with the sides swapped.
    ///
    /// The corner was recorded three items running rather than guarded (I7a,
    /// I7d's flag 5, I7e's "the one corner where the two questions could
    /// disagree") on the strength of a premise I8 retired: that an object is
    /// never peeked, so `has_front_matter` was always `false` and such a rule
    /// claimed nothing. Since I8 the gate reads IDENTITY, and a sidecar is
    /// identity a `.png` can have — so the corner is live, and this is where
    /// it stops. Refused at config time, in the I7b family, because it is a
    /// question about the config's shape alone.
    fn check_objects_rule_gate(&self) -> Result<()> {
        for (name, c) in &self.collections {
            // Objects scopes only: a parsing scope's rule (posts or the tree)
            // MAY gate on `front_matter` — that is how a `.md` with no front
            // matter becomes a static copy. The gate is a contradiction only
            // where the row is never parsed, which is the sourceless scope.
            if !c.is_objects() {
                continue;
            }
            // Inherited rules are checked too. The base declares no such rule,
            // so this can only fire on something a site wrote — but the reason
            // is the rule's own text, not who wrote it, and a base that grew
            // one would be exactly as wrong.
            for r in &c.rules {
                let Some(want) = r.front_matter else { continue };
                anyhow::bail!(
                    "collection {}: rule `match = {:?}` declares \
                     `front_matter = {want}`. An objects rule selects by SHAPE — \
                     what makes a row a picture is its extension, read off the \
                     path alone, and the objects index answers that way whichever \
                     scope claims the row — while the identity gate belongs to \
                     the scopes that PARSE. Gating here splits one directory of \
                     images between two scopes by whether someone wrote a sidecar \
                     beside them, and calls all of them pictures either way. \
                     Delete the line.",
                    describe_collection(name, c),
                    r.pattern,
                );
            }
        }
        Ok(())
    }

    /// A rule decides an address ONCE (IO.md §4a, I11).
    ///
    /// `route` and `embed` are the two answers to "where does a row this rule
    /// claims land", and they are not layers: a routed output wins, so a rule
    /// declaring both has written a fallback that can never be reached and a
    /// reader cannot tell which half is the mistake. The routed+strong twin —
    /// one output at a canonical URL that ALSO publishes its hash address, for
    /// an affordance to expand into — is a real shape and is I12's; it is not
    /// this line, which would give it no way to say which address a citation
    /// takes.
    ///
    /// `on_demand` beside `embed` is the I7b dead-key family: it defers a
    /// ROUTE, and an embed rule mints none. Every embed-addressed row is
    /// already published on demand — that is what the policy is — so the key
    /// configures nothing here.
    ///
    /// Config time, like `check_objects_rule_gate`: it is a question about the
    /// rule's own text, so no walk and no file can change the answer, and every
    /// declared profile is projected through the same deserializer.
    fn check_rule_address(&self) -> Result<()> {
        for (name, c) in &self.collections {
            for r in &c.rules {
                if r.embed != Some(true) {
                    continue;
                }
                if !r.route.is_empty() {
                    anyhow::bail!(
                        "collection {}: rule `match = {:?}` declares both `route` \
                         and `embed = true`. A rule decides an address once: \
                         `route` mints a canonical URL and a routed output WINS, \
                         so the embed policy beneath it could never be reached. \
                         Keep the route, or delete it and let `/static/` address \
                         these rows (IO.md §4a).",
                        describe_collection(name, c),
                        r.pattern,
                    );
                }
                if r.on_demand == Some(true) {
                    anyhow::bail!(
                        "collection {}: rule `match = {:?}` declares `on_demand` \
                         beside `embed = true`, and `on_demand` defers a ROUTE \
                         this rule does not mint — so it configures nothing. An \
                         embed-addressed row publishes when something embeds it, \
                         which is the whole of the policy (IO.md §4a). Delete the \
                         line.",
                        describe_collection(name, c),
                        r.pattern,
                    );
                }
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
        let owned = sets
            .into_iter()
            .map(|(n, v)| (n, v, true))
            .chain(routes.into_iter().map(|(n, v)| (n, v, false)));
        for (name, mut v, declared_set) in owned {
            // Which section declared it, kept because the fold below is what
            // loses it and a profile is held to the same split (§4a).
            v.declared_set = declared_set;
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

    /// The vocabulary a view's own `where` type-checks against — one function,
    /// so a profile patching that `where` is held to exactly the same words.
    ///
    /// It is the same three-way dispatch the build makes: a fold over every
    /// output ranges over ROUTES (`resolve_pool_folds`), an objects view over
    /// objects, and
    /// everything else over rows with every declared field beside the
    /// built-ins (`Base::resolve` → `Schemas::row_filter_schema`). Which
    /// matters because the three vocabularies genuinely differ — `kind` is a
    /// route column, `title` is a row column, and `dir` is a `Str` on a row
    /// and a `Bool` on a route — so "the union of all three" is not a schema
    /// anything could type-check against.
    ///
    /// **One narrowing, and it is why this is a pre-check rather than the
    /// check.** `.schema.toml` declarations are read during the tree walk,
    /// which has not run wherever a `Config` method can be called, so
    /// `config_declared_schema()` stands in for `Schemas::declared()`. A name
    /// only a positional file declares is invisible here — and is deferred,
    /// not rejected: see [`Config::check_profile_filters`].
    fn view_filter_schema(&self, name: &str) -> grackle_db::filter::Schema {
        let declared = self.config_declared_schema();
        let Some(v) = self.views.get(name) else {
            return grackle_model::row_schema();
        };
        if v.reads_all_outputs() {
            return grackle_model::route_schema(&declared);
        }
        let mut s = grackle_model::row_schema();
        for (k, t) in &declared {
            s.insert(k, *t);
        }
        s
    }

    /// The fence and rung 0, for EVERY declared profile, at load (§4a,
    /// MERGE.md E2 and E1).
    ///
    /// Both are facts about this config alone — which top-level keys a profile
    /// writes, and whether a forced name is declared and typed — so both are
    /// answerable for every `[profiles.*]` entry rather than only for the one
    /// being applied (MERGE.md R5): `--profile` is a flag that picks a
    /// projection, not the moment its declaration becomes checkable.
    ///
    /// It is deliberately the CHEAP half. The expensive half — merge the
    /// overlay, deserialize it, validate the result — is the dry run in
    /// [`Config::from_toml_profile`], which needs the config's own TOML and so
    /// cannot live on a `&self`. The two do not overlap: nothing below reaches
    /// past a profile's top-level keys.
    ///
    /// Placement (`sets` vs `routes`) and view names were checked here until
    /// E2 and are not any more, because the overlay subsumes both: a profile
    /// naming an unknown view now ADDS a definition, which is what a registry
    /// does, and the addition is held to the same rules as any other — a set
    /// with no `from` is `missing field \`from\``, and a name declared under
    /// both sections collides in the one namespace `merge_queries` folds them
    /// into. (A set with no `from` and no fold shell is
    /// `crate::shell::check_absent_from`'s error, by the same argument: the
    /// overlay is held to the rules every other entry is.)
    fn check_profiles(&self) -> Result<()> {
        // The vocabulary rung 0 may name: the site's own `[schema]`, parsed by
        // the parser `Schemas::set_site` uses, so the two cannot come to
        // different verdicts about what a declaration says. A positional
        // `.schema.toml` is deliberately NOT in it — see `schema::site_fields`.
        let declared = crate::schema::site_fields(&self.schema, "grackle.toml [schema]")?;
        let field_knowns = || {
            let mut names: Vec<&str> = declared.keys().map(String::as_str).collect();
            names.sort_unstable();
            match names.is_empty() {
                true => "none".to_string(),
                false => names.join(", "),
            }
        };
        for (pname, p) in &self.profiles {
            // The fence: §4a's iron law, and the two retired spellings, which
            // are checked before anything else this profile says because
            // everything else it says is beside the point until the key moves.
            for key in p.body().keys() {
                fence(pname, key)?;
            }
            // Rung 0: every forced name is declared, and every forced value
            // fits its declaration. Both are checked for a profile nobody is
            // building, which is R5's whole sentence one table over.
            let (_, force) = split_profile(pname, p.body())?;
            for (field, v) in &force {
                let Some(ty) = declared.get(field) else {
                    anyhow::bail!(
                        "profile {pname}: [profiles.{pname}.{FORCE}] {field} — a \
                         forced field is written onto every row and every route, \
                         so it must be declared in the site's own [schema]\n  \
                         declared fields: {}",
                        field_knowns()
                    );
                };
                crate::schema::typed(
                    *ty,
                    field,
                    v,
                    &format!("profile {pname}: [profiles.{pname}.{FORCE}]"),
                )?;
            }
        }
        Ok(())
    }

    /// Type-check every `where` a profile wrote (§4a, MERGE.md C6a/C6b).
    ///
    /// Run from [`Config::validate`], which the projection goes through like
    /// any other config (MERGE.md C6b, E2) — so a profile's `where` is checked
    /// by the same pass as everything else the config says, rather than at the
    /// moment it happened to be written. It is keyed off `View::filter_profile`
    /// and is therefore vacuous on a config no profile wrote to.
    ///
    /// **Unknown names are deferred, not accepted.** The vocabulary reachable
    /// from a `Config` is short of the positional `.schema.toml` declarations
    /// by exactly one tree walk (see [`Config::view_filter_schema`]), so an
    /// unknown field here may be a typo or may be a perfectly good name this
    /// early. Rejecting it would make a profile's `where` STRICTER than the
    /// `where` it replaces, which is the one thing §4a says a profile is not
    /// allowed to be; and it cannot escape, because `build_views` and
    /// `resolve_pool_folds` parse the filter they find with the full schema
    /// and error naming the view. What is caught here is everything that is
    /// wrong however the tree walk turns out: syntax, arity, and types.
    fn check_profile_filters(&self) -> Result<()> {
        for (vname, v) in &self.views {
            let (Some(p), Some(f)) = (v.filter_profile.as_deref(), v.filter.as_deref()) else {
                continue;
            };
            let schema = self.view_filter_schema(vname);
            if let Err(e) = grackle_db::filter::Filter::parse(f, &schema) {
                let msg = format!("{e:#}");
                if msg.contains("unknown field") {
                    continue;
                }
                return Err(e).with_context(|| format!("profile {p}: view {vname}: filter {f:?}"));
            }
        }
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
                 [[collections]]\n    source = \"_posts\"\n\n  \
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
            // §7 q5 / MERGE.md F3: a set's `theme` can never apply, so declaring
            // one is declared-and-ignored. A set does not materialize, so there
            // is no document for a theme to dress; embedded, it is content
            // inside the HOST's document, and a document wears one stylesheet.
            // `layout` and `variant` on a set are LIVE by contrast — `tags.rs`'s
            // `{% view %}` dispatches on the layout and renders through the
            // variant — so this is about `theme` alone.
            if v.declared_set && v.theme.is_some() {
                anyhow::bail!(
                    "[sets.{vname}] declares a theme, and nothing could ever \
                     wear it. A set never lands, so there is no page for a \
                     theme to dress; embedded with {{% view {vname} %}} it \
                     wears the embedding page's theme. Theme belongs on a \
                     route — move it to the [routes.*] entry that lands this \
                     query, or drop it."
                );
            }
            // The same family, one field over (IO.md §4, IR1(c)): a set may
            // not wear a FOLD shell, because a fold lands at a route. A fold
            // serializes its query into one artifact, and an artifact is a
            // file at a path — every fold pass in `build.rs` (atom, sitemap,
            // search, the script shells) ranges over `db.routes` and finds a
            // view by the route that carries it, so a routeless one is
            // unreachable by construction. Today it fails LATE and only half
            // the time: a `from`-less set reaches `build_pool_folds` and dies
            // with "view x needs a route" mid-build, while a set WITH a `from`
            // goes through `build_views` into `insert_routeless` and publishes
            // nothing at all, silently. Config-time, both say why.
            //
            // Only a fold, deliberately: a MAP shell here is an arity mistake,
            // and `check_view` below owns that sentence.
            if v.declared_set {
                if let Some(s) = v
                    .shell
                    .as_deref()
                    .filter(|s| crate::shell::is_fold(s) || self.shells.contains_key(*s))
                {
                    anyhow::bail!(
                        "[sets.{vname}] wears shell = {s:?}, and a set never \
                         lands. A fold shell serializes its query into ONE \
                         artifact, and an artifact needs an address to be \
                         written at — so a fold belongs on a route: move it to \
                         `[routes.{vname}]` with a `path`, or drop the shell \
                         and let the set stay a query."
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
                .find_map(|c| c.tags.as_deref());
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
                    grackle_db::template::render(tmpl, |tok| match grackle_db::template::classify(
                        tok,
                    ) {
                        (None | Some("group"), "key" | "tags") => Some("probe".to_string()),
                        _ => None,
                    })
                    .with_context(|| {
                        format!("view {name}: tag route template needs more than {{key}}")
                    })?;
                }
            }
        }
        // `trail` is the same shape of reference as `tags` — a collection
        // naming a view — and until MERGE.md C3 it was the only one nothing
        // checked: `chain` stops at an unknown name and `post_trail` walks an
        // empty chain, so `trail = "montly_archive"` produced no trail and
        // said nothing. What the machinery needs is not "a view" but a
        // SUBDIVISION CHAIN it can render a crumb from at every level
        // (`trails.rs::post_trail`), so that is what is checked.
        for (cname, c) in &cfg.collections {
            let Some(name) = c.trail.as_deref() else {
                continue;
            };
            let knowns = || cfg.views.keys().cloned().collect::<Vec<_>>().join(", ");
            if !cfg.views.contains_key(name) {
                anyhow::bail!(
                    "collection {cname}: trail {name:?} is not a declared view \
                     — views: {}",
                    knowns()
                );
            }
            // The trail renders each GROUPED view along the `from` chain, so
            // the named view need not itself be grouped — but something in
            // its chain must be, or the trail is a chain of nothing.
            let chain = cfg.grouped_chain(name);
            if chain.is_empty() {
                anyhow::bail!(
                    "collection {cname}: trail {name:?} declares no `group_by`, \
                     and neither does anything it composes `from` — a trail is a \
                     subdivision chain (a year archive, then a month archive), \
                     rendered from a row's own group keys. Grouped views: {}",
                    cfg.views
                        .iter()
                        .filter(|(_, v)| v.group_by.is_some())
                        .map(|(n, _)| n.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            for level in &chain {
                let v = &cfg.views[level];
                // A level with no route, or nothing to label it with, is
                // SKIPPED by `post_trail` — the crumb between its neighbours
                // silently goes missing, which is the failure this whole item
                // is about, one rung in.
                if v.route.is_none() {
                    anyhow::bail!(
                        "collection {cname}: trail {name:?} — its subdivision \
                         chain ({}) passes through view {level:?}, which lands \
                         at no single `path`, so that crumb has no URL and the \
                         level would be dropped from every trail.",
                        chain.join(" > ")
                    );
                }
                if v.crumb.is_none() && v.title.is_none() {
                    anyhow::bail!(
                        "collection {cname}: trail {name:?} — its subdivision \
                         chain ({}) passes through view {level:?}, which declares \
                         neither `crumb` nor `title`, so that crumb has no label \
                         and the level would be dropped from every trail.",
                        chain.join(" > ")
                    );
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
            // `[i18n.names]` obeys the same rule one level out (MERGE.md C4a).
            // It is the one localized string `check` cannot see: its LOCALES
            // are keys, not the keys of a `LocalizedStr` value, so nothing
            // above ever looked at them. `names = { fr_CA = "…" }` on a site
            // declaring `locales = ["fr"]` labels a member of the translations
            // axis that will never exist — `name_of` is only ever asked about
            // a locale a path may declare, and only a declared locale is one.
            for loc in cfg.i18n.names.keys() {
                if *loc != cfg.i18n.default && !cfg.i18n.locales.iter().any(|l| l == loc) {
                    anyhow::bail!(
                        "i18n.names: names locale {loc:?}, which is neither the \
                         default ({:?}) nor in i18n.locales {:?} — nothing would \
                         ever read it",
                        cfg.i18n.default,
                        cfg.i18n.locales
                    );
                }
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
                if (v.intro.is_some() || v.content.is_some()) && v.reads_all_outputs() {
                    anyhow::bail!(
                        "view {vname}: a fold over every output serializes the \
                         route set and has no landing to give prose to"
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
                if v.reads_all_outputs() {
                    anyhow::bail!(
                        "view {vname}: a fold over every output serializes the \
                         whole route set and never materializes per locale — \
                         filter on `locale` instead (§6f)"
                    );
                }
            }
            // One vocabulary, one validator (IO.md §4, I2). A view is a query,
            // so its declared shell is a FOLD — and a map shell here is an
            // arity error rather than an unknown word, because `html` is a
            // perfectly good shell that simply wraps one output.
            let registered: Vec<&str> = cfg.shells.keys().map(|k| k.as_str()).collect();
            if let Some(s) = v.shell.as_deref() {
                crate::shell::check_view(s, vname, &registered)?;
            }
            // And the other half of the same contract (IO.md §4, I3): a fold
            // with no `from` reads every output — the successor to `from =
            // "*"` — while every other view is a listing and has to say what
            // it lists. Runs after the shell check so a map shell here is
            // still diagnosed as the arity mistake it is.
            if v.reads_all_outputs() {
                crate::shell::check_absent_from(v.shell.as_deref(), vname, &registered)?;
            }
        }
        // A per-member route is one output, so an axis spending `shell`
        // declares MAP shells. Checked here because the values never pass
        // through a row's cascade: `build.rs` reads the member's value
        // directly, and an unchecked one renders the wrong tier in silence.
        for (aname, a) in &cfg.axes {
            if a.field == "shell" {
                for value in &a.values {
                    crate::shell::check_axis_value(value, aname)?;
                }
            }
        }
        for name in cfg.shells.keys() {
            crate::shell::check_registered_name(name)?;
        }
        cfg.check_profiles()?;
        cfg.check_profile_filters()?;
        Ok(())
    }

    /// Flatten a view's `from` chain into a base collection plus every filter
    /// along the way.
    ///
    /// `from` may name a **query-only** view (nothing to inherit ambiguously)
    /// or a **grouped, unpaginated** view — subdivision (§5c): the composer
    /// refines the parent's partition, so it must itself be grouped, and the
    /// parent's route/layout are *not* inherited (the child declares its own).
    /// Composing over a paginated view is punted (open question 30): a
    /// pageable year with months on its root raises a URL-namespace question
    /// we haven't answered.
    pub fn query(&self, name: &str) -> Result<Query> {
        let mut filters = Vec::new();
        let mut patched = Vec::new();
        // Nearest wins, and we walk outermost-first, so the first one seen.
        let mut order_by: Option<String> = None;
        let mut seen: Vec<&str> = Vec::new();
        let mut cur = name;
        loop {
            let v = self
                .views
                .get(cur)
                .with_context(|| format!("view {name}: `from` names unknown view {cur:?}"))?;
            if seen.contains(&cur) {
                anyhow::bail!("view {name}: `from` chain is cyclic at {cur:?}");
            }
            seen.push(cur);
            if let Some(f) = &v.filter {
                if let Some(p) = &v.filter_profile {
                    patched.push(format!("profile {p} replaced view {cur}'s `where`"));
                }
                filters.push(f.clone());
            }
            if order_by.is_none() {
                order_by.clone_from(&v.order_by);
            }
            // No `from` at all terminates it hardest: a fold over every output
            // ranges over no collection, so there is nothing to name and
            // nothing to check (IO.md §4). Its filters still travel — a chain
            // cannot compose over it (nothing may name a route), but its own
            // `where` is the query.
            let Some(from) = &v.from else {
                filters.reverse();
                patched.reverse();
                return Ok(Query {
                    base: Vec::new(),
                    filters,
                    order_by,
                    patched,
                });
            };
            // A collection or a union terminates the chain.
            let next = from.single().and_then(|s| self.views.get(s));
            let Some(next) = next else {
                // `cur`, not `name`: the entry that CARRIES the `from` is the
                // one an author has to edit, and on a composed chain it is not
                // the one whose query was asked for.
                self.check_base(cur, name, from)?;
                filters.reverse();
                patched.reverse();
                return Ok(Query {
                    base: from.names().to_vec(),
                    filters,
                    order_by,
                    patched,
                });
            };
            if !next.is_query_only() {
                let subdividable = next.group_by.is_some()
                    && next.paginate.is_none()
                    && next.limit.is_none()
                    && next.template.is_none();
                if !subdividable {
                    anyhow::bail!(
                        "{cur}: `from = {}` names something that is neither a set nor a \
                         grouped route. Only sets and grouped, unpaginated routes may be \
                         composed over (subdivision, §5c); pagination × subdivision is \
                         punted (open question 30).{}",
                        from.display(),
                        self.whose_from(cur, name)
                    );
                }
                if v.group_by.is_none() {
                    anyhow::bail!(
                        "{cur}: `from = {}` names a grouped route, but {cur} has no \
                         `group_by`. Composing over a grouped route means subdividing its \
                         partition (§5c), so the composer must be grouped too.{}",
                        from.display(),
                        self.whose_from(cur, name)
                    );
                }
            }
            cur = from.single().expect("a union terminates the chain above");
        }
    }

    /// What a terminated chain is allowed to name (§5c).
    ///
    /// One name may be a collection. A union may name only collections,
    /// and they must share a kind: the members decide the vocabulary a `where`
    /// type-checks against and whether the rows are parsed, so two kinds in one
    /// union is a query with two answers to both questions.
    ///
    /// `"*"` is a name like any other now (IO.md I3) and names nothing, so it
    /// lands in the generic arm below — except that the generic arm would send
    /// its reader off to look for a collection called `*`, and the fix is to
    /// delete a line rather than to write one. It gets a sentence of its own:
    /// the value is invalid, not deprecated.
    ///
    /// `carrier` is the view whose `from` this is; `asked` is the view whose
    /// query was requested, which on a composed chain is a different entry
    /// (`blog_index` composes over `published`, and it is `published`'s
    /// `from` that terminates). Both, because a message naming only one of
    /// them sends the reader to the wrong table — see [`Config::whose_from`].
    fn check_base(&self, carrier: &str, asked: &str, from: &From) -> Result<()> {
        if matches!(from, From::One(s) if s == "*") {
            anyhow::bail!(
                "{carrier}: `from = \"*\"` names nothing — the star spelling is \
                 gone (IO.md §4). A fold shell reads every output by having no \
                 `from` at all, so delete the line: the `shell` ({}) is what \
                 says this folds the pool.{}",
                crate::shell::FOLD.join(", "),
                self.whose_from(carrier, asked)
            );
        }
        for member in from.names() {
            if self.collections.contains_key(member) {
                continue;
            }
            if matches!(from, From::Union(_)) {
                anyhow::bail!(
                    "{carrier}: `from` unions {member:?}, which is not a collection. A union \
                     ranges over collections; to narrow a set, compose over it with `from = \
                     {member:?}` and a `where`.{}",
                    self.whose_from(carrier, asked)
                );
            }
            anyhow::bail!(
                "{carrier}: `from = {}` is neither a collection, a set nor a route \
                 (collections: {}; sets and routes: {}){}",
                from.display(),
                self.collections
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                self.views.keys().cloned().collect::<Vec<_>>().join(", "),
                self.whose_from(carrier, asked)
            );
        }
        if from.names().is_empty() {
            anyhow::bail!(
                "{carrier}: `from = []` names nothing to range over.{}",
                self.whose_from(carrier, asked)
            );
        }
        Ok(())
    }

    /// Who owns a bad `from`, when it is not the entry the reader is looking
    /// at (MERGE.md C7b).
    ///
    /// Two things blur that. A view composes over another, so the entry
    /// carrying the broken reference need not be the one whose query was
    /// asked for. And **an inherited `from` is the one reference in this
    /// config a site can break without touching the entry that carries it**:
    /// views are a registry keyed by NAME, so an inherited set survives every
    /// rename a site can perform, while collections key on `source` (§1's
    /// annotation), so renaming the collection at `_posts` retires the name
    /// `posts` — and the base's `[sets.published] from = "posts"` then names
    /// nothing, on a site whose grackle.toml contains no `published` at all.
    /// The old message quoted that line back at its reader as if they had
    /// written it.
    ///
    /// Empty for a view the site wrote and asked about directly, which is
    /// every message this does not need to explain.
    fn whose_from(&self, carrier: &str, asked: &str) -> String {
        let mut note = String::new();
        if carrier != asked {
            note.push_str(&format!(
                "\n  (reached from {asked:?}, which composes over it.)"
            ));
        }
        let Some(v) = self.views.get(carrier) else {
            return note;
        };
        if v.inherited {
            let table = if v.declared_set { "sets" } else { "routes" };
            note.push_str(&format!(
                "\n  {carrier:?} is inherited from the base config (§4d) — it is not in your \
                 grackle.toml, and its `from` names a collection the BASE declares. A site \
                 that renames or drops that collection has to say what {carrier:?} means to \
                 it: declare your own [{table}.{carrier}] over the inherited one, or keep a \
                 collection under the name it asks for."
            ));
        }
        note
    }

    /// The `from` chain from `name` down to its base, nearest view first.
    /// The one chain walker — everything derived from composition
    /// (`fields_for`, `group_specs`, `grouped_chain`) reads this. Assumes the
    /// chain is acyclic, which `query()` validated at load.
    pub fn chain<'a: 'b, 'b>(&'a self, name: &'b str) -> Vec<(&'b str, &'a View)> {
        let mut out = Vec::new();
        let mut cur = name;
        while let Some(v) = self.views.get(cur) {
            out.push((cur, v));
            let Some(n) = v.from.as_ref().and_then(From::single) else {
                break;
            };
            cur = n;
        }
        out
    }

    /// The `group_by` specs governing a view, outermost ancestor first. This
    /// is subdivision (§5c): a grouped view composed `from` a grouped view
    /// refines the parent's partition, so the parent's spec applies before
    /// the child's.
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
    /// `from` chain, nearest declaration winning per name — fields compose
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

    /// The view that owns tag routes: the first collection's declared `tags`
    /// view, else the unique view grouped by tags. Ambiguity without a
    /// declaration is a load error, so None means "no tag archive".
    pub fn tags_view(&self) -> Option<(&str, &View)> {
        if let Some(name) = self
            .collections
            .values()
            .find_map(|c| c.tags.as_deref())
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
        let url =
            grackle_db::template::render(tmpl, |tok| match grackle_db::template::classify(tok) {
                (None | Some("group"), "key" | "tags") => Some(self.tag_slug(id).to_string()),
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

    /// The text every helper here parses: one posts collection, no base.
    fn cfg_source(views: &str) -> String {
        format!(
            "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"blog\"\nsource = \"_posts\"\n{views}"
        )
    }

    /// The same site as [`cfg`], PROJECTED through `profile` (MERGE.md E2).
    ///
    /// A projection is a config built through the ordinary entry point — the
    /// overlay merges into the merged table, the deserializer sees the result,
    /// and `validate` runs on it — so the test drives exactly what
    /// `Config::load_profile` drives, minus the filesystem.
    fn projected(views: &str, profile: &str) -> Result<Config> {
        let c = Config::from_toml_profile(&cfg_source(views), Some(profile))?;
        c.validate()?;
        Ok(c)
    }

    /// Parsed, with collections keyed — but queries not yet folded, which is
    /// what the `merge_queries` checks below are about.
    fn cfg_unmerged(views: &str) -> Config {
        let src = cfg_source(views);
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
             [[collections]]\nsource = \"_posts\"\n\
             [[collections]]\nsource = \"recipes\"\n",
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
             [[collections]]\nsource = \".\"\n",
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
             [[collections]]\nname = \"notes\"\nsource = \"_posts\"\n",
        )
        .unwrap();
        assert!(c.collections.contains_key("notes"));
    }

    /// Objects are matched by their rules' globs, so no directory names them.
    #[test]
    fn a_sourceless_collection_must_be_named() {
        let e = Config::from_toml(
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\n",
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("give it a `name`"), "{e}");
    }

    #[test]
    fn two_collections_may_not_resolve_to_one_name() {
        let e = Config::from_toml(
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource = \"_posts\"\n\
             [[collections]]\nsource = \"posts\"\n",
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
                   [[collections]]\nsource = \"_posts\"\n\
                   filename_formats = [\"{slug}\"]\n\
                   [routes.x]\npath = \"/x/\"\nfrom = \"posts\"\nlayout = \"tag_index\"\n";
        let c = Config::from_toml(src).expect("it parses; validation is the gate");
        let e = format!("{:#}", c.validate().unwrap_err());
        assert!(e.contains("layout \"tag_index\" is not a layout"), "{e}");
        assert!(e.contains("listing, link_list, card"), "{e}");
    }

    /// §7 q5 / MERGE.md F3: a set's `theme` can never apply — a set never
    /// lands, and an embedded set is content in the HOST's document, which
    /// wears one stylesheet. Declared-and-ignored, so it is a load error.
    ///
    /// The controls are the point of the item: a ROUTE's theme is the shape
    /// this key exists for (its NAME is checked against the registry once the
    /// themes are loaded — C2), and `layout`/`variant` on a set are LIVE, since
    /// `{% view %}` dispatches on the one and renders through the other.
    #[test]
    fn a_set_may_not_declare_a_theme() {
        let e = cfg_err(
            "[sets.latest]\nfrom = \"blog\"\nlimit = 3\n\
             layout = \"link_list\"\ntheme = \"loud\"\n",
        );
        assert!(e.contains("[sets.latest] declares a theme"), "{e}");
        assert!(e.contains("never lands"), "{e}");

        let c = cfg("[routes.blog_index]\npath = \"/blog/\"\nfrom = \"blog\"\n\
             layout = \"listing\"\ntheme = \"loud\"\n\
             [sets.latest]\nfrom = \"blog\"\nlimit = 3\n\
             layout = \"link_list\"\nvariant = \"compact\"\n");
        assert_eq!(c.views["blog_index"].theme.as_deref(), Some("loud"));
        assert_eq!(c.views["latest"].layout.as_deref(), Some("link_list"));
        assert_eq!(c.views["latest"].variant.as_deref(), Some("compact"));
    }

    /// IO.md §4 / IR1(c), the same family one field over: a set may not wear a
    /// fold shell, because a fold lands at a route.
    ///
    /// Verified before the check was written: there is no routeless-fold
    /// shape. All four fold passes in `build.rs` (atom, sitemap, search, the
    /// script shells) iterate `db.routes` and reach a view through the route
    /// carrying it, and a routeless view only ever reaches `db.views` via
    /// `insert_routeless`, which `{% view %}` embedding reads by layout and
    /// variant — no reader of `shell` at all. So the two live outcomes today
    /// are both bad and neither says why: `from`-less, it reaches
    /// `build_pool_folds` and dies mid-build with "view x needs a route"; with
    /// a `from`, it goes quietly through `insert_routeless` and publishes
    /// nothing.
    ///
    /// Mutation: delete the `declared_set`/fold check in `validate` and the
    /// first case validates clean (then dies late), the second validates and
    /// publishes nothing.
    #[test]
    fn a_set_may_not_wear_a_fold_shell() {
        for src in [
            "[sets.everything]\nshell = \"sitemap\"\n",
            "[sets.everything]\nfrom = \"blog\"\nshell = \"atom\"\n",
            "[shells.llms]\ncommand = \"cat\"\n\
             [sets.everything]\nfrom = \"blog\"\nshell = \"llms\"\n",
        ] {
            let e = cfg_err(src);
            assert!(e.contains("[sets.everything] wears shell ="), "{e}");
            assert!(e.contains("a set never lands"), "{e}");
            assert!(e.contains("[routes.everything]"), "{e}");
        }
        // The controls. A routed fold is the shape the key exists for, and a
        // set with no shell is still an ordinary query.
        let c = cfg(
            "[routes.everything]\npath = \"/sitemap.xml\"\nshell = \"sitemap\"\n\
             [sets.latest]\nfrom = \"blog\"\nlimit = 3\n",
        );
        assert_eq!(c.views["everything"].shell.as_deref(), Some("sitemap"));
        assert!(c.views["latest"].shell.is_none());
        // And a MAP shell on a set is still the ARITY mistake `check_view`
        // owns — this check does not steal that sentence.
        let e = cfg_err("[sets.latest]\nfrom = \"blog\"\nshell = \"html\"\n");
        assert!(e.contains("is a map shell"), "{e}");
    }

    /// noindex was once hardcoded as `view != "blog_index"`, making every
    /// other listing noindex by accident. It is editorial, so it is
    /// declared; an undeclared listing is indexed.
    #[test]
    fn noindex_is_a_view_declaration_defaulting_to_indexed() {
        let head = "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
                    [[collections]]\nsource = \"_posts\"\n\
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
             [[collections]]\nsource=\"_posts\"\n\
             [[collections.rules]]\nmatch=\"**\"\nroute=\"/writing/{slug}/\"\n",
        )
        .unwrap();
        let rules = &c.collections["posts"].rules;
        assert_eq!(rules[0].route, vec!["/writing/{slug}/"]);
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
             [[collections]]\nname=\"notes\"\nsource=\"_posts\"\n",
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

    /// The law dispatch with a base of the test's own. `base.toml` declares
    /// no `[axes]` and no `[links]`, so `Config::from_toml` cannot reach the
    /// arms below — a key the base never wrote is the site's whole under
    /// every law. This is the same `merge_table` the §4d merge runs, so the
    /// law read here is the law that ships.
    fn merged(base: &str, site: &str) -> toml::Table {
        let b = toml::from_str(base).expect("test base should parse");
        let s = toml::from_str(site).expect("test site should parse");
        match merge_table(b, s, &Config::shape(), &mut Vec::new(), &mut Trace::off()) {
            toml::Value::Table(t) => t,
            v => panic!("merging two tables should give a table: {v:?}"),
        }
    }

    /// A registry, not an atom: declaring an axis of your own must not take
    /// the inherited ones down with it. This is the bug Law 2 was derived
    /// from — `[axes]` fell through to wholesale replace.
    #[test]
    fn a_base_declared_axis_survives_a_site_declaring_a_different_one() {
        let m = merged(
            "[axes.theme]\nvalues = [\"ledger\", \"atlas\"]\nfield = \"theme\"\n",
            "[axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n",
        );
        let axes = m["axes"].as_table().expect("axes is a table");
        assert!(
            axes.contains_key("theme"),
            "the inherited axis was swept away: {axes:?}"
        );
        assert!(axes.contains_key("locale"), "{axes:?}");
    }

    /// And the other half of the registry law: a definition is the unit, so
    /// redeclaring one replaces it entire. `values` and `field` are one
    /// thought — an axis assembled half from each side is nobody's axis.
    #[test]
    fn a_redeclared_axis_shadows_the_inherited_one_entire() {
        let m = merged(
            "[axes.theme]\nvalues = [\"ledger\", \"atlas\"]\nfield = \"theme\"\n",
            "[axes.theme]\nvalues = [\"ledger\"]\n",
        );
        let theme = m["axes"]["theme"].as_table().expect("the axis is a table");
        assert_eq!(
            theme["values"].as_array().map(Vec::len),
            Some(1),
            "the site's values: {theme:?}"
        );
        assert!(
            !theme.contains_key("field"),
            "the base's `field` leaked into the site's axis: {theme:?}"
        );
    }

    /// A marker is a definition under its filename, so redeclaring one says
    /// what it means WHOLE — MERGE.md §7 q10, and the reason `MarkerDef`
    /// exists. The base declares `".noindex" = { noindex = true }`; a site
    /// that repurposes the name gets its own payload and nothing else.
    ///
    /// This is the live path, not a stand-in: `base.toml` really does declare
    /// the three markers, so `from_toml` reaches the arm.
    #[test]
    fn a_redeclared_marker_replaces_the_payload_whole() {
        let c = Config::from_toml(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [markers]\n\".noindex\" = { hidden = true }\n",
        )
        .unwrap();
        let payload = &c.markers[".noindex"].0;
        let keys: Vec<&str> = payload.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["hidden"],
            "the base's `noindex = true` composed itself into the site's \
             marker: {payload:?}"
        );
        // The markers the site left alone are untouched — a definition is the
        // unit, and `[markers]` itself is the namespace.
        assert!(c.markers.contains_key(".draft"), "{:?}", c.markers.keys());
    }

    /// `[links]` is a bag like `[site]`: setting one key keeps the others.
    ///
    /// A3 could only state that with a hypothetical — `policy` is `LinksCfg`'s
    /// only key, so nothing can be left behind and no real config can tell the
    /// bag law from wholesale replace. It is a hypothetical no longer:
    /// `table_as_depths_fall_out_of_the_types` reads the law off the type
    /// (`LinksCfg` is a struct under an ENGINE-chosen name, so it descends per
    /// field), which is a statement about every key it will ever have. What is
    /// left here is the demonstration — `merge_to_depth` is what actually runs
    /// — and `reach` is a stand-in for the key that has not been added yet.
    #[test]
    fn links_keys_merge_one_at_a_time() {
        let m = merged(
            "[links]\npolicy = \"loose\"\nreach = \"site\"\n",
            "[links]\npolicy = \"strict\"\n",
        );
        let links = m["links"].as_table().expect("links is a table");
        assert_eq!(
            links.get("policy").and_then(toml::Value::as_str),
            Some("strict"),
            "the nearer writer wins the key: {links:?}"
        );
        assert_eq!(
            links.get("reach").and_then(toml::Value::as_str),
            Some("site"),
            "a key the site never wrote was dropped: {links:?}"
        );
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
             [[collections]]\nsource=\".\"\n\
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

    /// A path scope conjoins along the chain: a child narrows within its
    /// parent's subtree and cannot escape it. This used to be the `match`
    /// key's own rule; since MERGE.md G2 the glob is a clause of `where`, so
    /// it is the filter conjunction that carries it — one law instead of two,
    /// and the assertion is on the source the type-checker actually sees.
    #[test]
    fn a_path_scope_conjoins_along_the_chain() {
        let c = cfg(
            "[sets.recipes]\nfrom = \"blog\"\nwhere = 'glob(path, \"recipes/**\")'\n\
             [sets.desserts]\nfrom = \"recipes\"\n\
             where = 'glob(path, \"**/sweet/**\") && !draft'\n",
        );
        assert_eq!(
            c.query("desserts").unwrap().predicate().unwrap(),
            "(glob(path, \"recipes/**\")) && (glob(path, \"**/sweet/**\") && !draft)"
        );
        // The parent keeps only its own.
        assert_eq!(
            c.query("recipes").unwrap().predicate().unwrap(),
            "glob(path, \"recipes/**\")"
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

    // ---------------------------------------------------------------- trail
    //
    // MERGE.md C3(b). `cfg_unmerged` splices its argument straight after the
    // collection's `source`, so a `trail = …` line lands on the collection
    // and the `[routes]` after it close the table — which is exactly the
    // shape these need.

    /// The control, and the shape grack.com really has: a month archive
    /// composed `over` a year archive, both routed and both labelled.
    #[test]
    fn a_grouped_routed_trail_validates() {
        cfg(TRAIL_CHAIN);
    }

    /// The typo. Also a fixture (`trail-unknown-view`), which is what pins
    /// that the SITE fails rather than that the function does.
    #[test]
    fn a_trail_naming_no_view_is_a_load_error() {
        let e = cfg_err(&TRAIL_CHAIN.replace("monthly_archive\"\n", "montly_archive\"\n"));
        assert!(e.contains("is not a declared view"), "{e}");
        assert!(e.contains("monthly_archive"), "the knowns are listed: {e}");
    }

    /// A trail is a SUBDIVISION chain — `post_trail` renders each grouped
    /// view along the `from` chain from the row's own group keys. A view
    /// that groups by nothing, over nothing that groups, is a chain of
    /// nothing, and produced a silently empty trail.
    #[test]
    fn a_trail_over_nothing_grouped_is_a_load_error() {
        let e = cfg_err(
            "trail = \"flat\"\n\
             [routes.flat]\npath = \"/flat/\"\nfrom = \"blog\"\nlayout = \"listing\"\ntitle = \"F\"\n",
        );
        assert!(e.contains("subdivision chain"), "{e}");
        assert!(e.contains("Grouped views: "), "the knowns are listed: {e}");
    }

    /// A level with no `path` has no URL to hang its crumb on, so
    /// `post_trail` skips it and the trail comes out with a hole in the
    /// middle — Home > December > 16, the year gone.
    #[test]
    fn a_trail_level_that_lands_nowhere_is_a_load_error() {
        let e = cfg_err(
            &TRAIL_CHAIN
                .replace("[routes.yearly_archive]", "[sets.yearly_archive]")
                .replace("path = \"/blog/{year}/\"\n", ""),
        );
        assert!(e.contains("lands at no single `path`"), "{e}");
        assert!(e.contains("yearly_archive > monthly_archive"), "{e}");
    }

    /// Same hole, other cause: nothing to write in the crumb.
    #[test]
    fn a_trail_level_with_no_label_is_a_load_error() {
        let e = cfg_err(&TRAIL_CHAIN.replace("title = \"{year}\"\n", ""));
        assert!(e.contains("neither `crumb` nor `title`"), "{e}");
    }

    const TRAIL_CHAIN: &str = "trail = \"monthly_archive\"\n\
         [routes.yearly_archive]\n\
         path = \"/blog/{year}/\"\n\
         from = \"blog\"\n\
         group_by = \"date.year\"\n\
         layout = \"listing\"\n\
         title = \"{year}\"\n\
         [routes.monthly_archive]\n\
         path = \"/blog/{year}/{month:02}/\"\n\
         from = \"yearly_archive\"\n\
         group_by = \"date.month\"\n\
         layout = \"listing\"\n\
         crumb = \"{month_name}\"\n";

    /// The field names serde accepts for `T`, read out of its own
    /// `deny_unknown_fields` complaint — renames applied, skipped fields
    /// absent. This is the list the merge actually keys on.
    ///
    /// Two shapes to read: "expected one of `a`, `b`" for a struct with
    /// several fields, and plain "expected `head`" for one with a single
    /// field (`HtmlCfg`, `LinksCfg`). Splitting on the shorter prefix takes
    /// both, and the invented key sits before it either way.
    fn serde_keys<T: serde::de::DeserializeOwned>() -> Vec<String> {
        let e = toml::from_str::<T>("no_such_key = 1")
            .err()
            .expect("deny_unknown_fields should reject an invented key")
            .to_string();
        let listed = e
            .split_once("expected ")
            .expect("the error names the fields it knows")
            .1
            .lines()
            .next()
            .expect("the list is on one line");
        let mut keys: Vec<String> = listed
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect();
        keys.sort();
        assert!(!keys.is_empty(), "no fields parsed out of: {e}");
        keys
    }

    /// The other half of the completeness check, struct by struct.
    /// `every_config_key_has_a_law` pins the FIELDS at compile time; this pins
    /// their TOML SPELLINGS, which is what the merge dispatches on — a renamed
    /// or skipped field would otherwise leave a key no shape claims, silently,
    /// and `law_of` would hand it back whole.
    ///
    /// (A2 wrote this against the law table; B2 points it at the description,
    /// which is now the only place a key can be named.)
    ///
    /// The description is in TOML's name space, so `collections` (renamed from
    /// `declared_collections`) must appear and `noindex`, `dir`, `views` —
    /// `#[serde(skip)]` every one — must not. Only the structs the merge
    /// DESCENDS are listed: a definition's fields are nobody's business
    /// (see [`Shape::definition`]).
    #[test]
    fn the_shape_covers_the_config_surface() {
        for (what, shape, serde) in [
            ("Config", Config::shape(), serde_keys::<Config>()),
            (
                "Collection",
                Collection::shape(),
                serde_keys::<Collection>(),
            ),
            ("Site", Site::shape(), serde_keys::<Site>()),
            ("HtmlCfg", HtmlCfg::shape(), serde_keys::<HtmlCfg>()),
            ("HeadCfg", HeadCfg::shape(), serde_keys::<HeadCfg>()),
            ("I18nCfg", I18nCfg::shape(), serde_keys::<I18nCfg>()),
            ("LinksCfg", LinksCfg::shape(), serde_keys::<LinksCfg>()),
        ] {
            let mut named: Vec<String> =
                shape.fields().iter().map(|(k, _)| k.to_string()).collect();
            named.sort();
            assert_eq!(named, serde, "{what}'s shape and its serde keys drifted");
        }
    }

    /// §1's annotation is the one thing here that is not derived, and there
    /// are exactly two of it. B1 shipped a `KNOWN_EXCEPTIONS` list beside the
    /// hand tables — one entry, `[markers]`, which §7 q10 settled and
    /// `MarkerDef` retired — and this is what replaces it now that the tables
    /// are gone: with the law read off the shape, the only way to write a law
    /// by hand is `annotated(…)`, so counting those IS counting the
    /// exceptions.
    ///
    /// A third one means someone decided a key does not merge the way its
    /// type says. That deserves a §6 entry and probably a §7 question, not a
    /// quiet line in a field list — and this fails until it gets one.
    #[test]
    fn only_the_annotated_keys_have_a_hand_written_law() {
        let hand_written = |shape: &Shape| -> Vec<(String, Law)> {
            shape
                .fields()
                .iter()
                .filter(|(_, s)| matches!(s, Shape::Annotated(..)))
                .map(|(k, s)| (k.to_string(), s.law()))
                .collect()
        };
        assert_eq!(
            hand_written(&Config::shape()),
            [("collections".to_string(), Law::Collections)],
            "a config key merges by a hand-written law"
        );
        assert_eq!(
            hand_written(&Collection::shape()),
            [("rules".to_string(), Law::Prepend)],
            "a collection key merges by a hand-written law"
        );
    }

    /// The depths §3 table A calls out, each traced back to the type it falls
    /// out of. These are the rows the table describes as "falls out" — this
    /// is where that stops being a claim.
    #[test]
    fn table_as_depths_fall_out_of_the_types() {
        // `law_of` is the merge's own lookup, not a test-side restatement:
        // this reads the laws that ship.
        let law = |key: &str| law_of(&Config::shape(), key);
        let collection_law = |key: &str| law_of(&Collection::shape(), key);
        // `[site]`: a struct under an engine-chosen name, all scalars.
        assert_eq!(law("site"), Law::Descend(1));
        // `[axes.*]`: a map whose value is a definition — `Axis` is a struct
        // under the axis's own name, so the descent stops above it. A3 fixed
        // this by hand; here it is a consequence of `BTreeMap<String, Axis>`.
        assert_eq!(law("axes"), Law::Descend(1));
        // `[links]`: `LinksCfg` is a struct under an ENGINE-chosen name, so
        // it descends per field however many fields it grows. A3 could only
        // state this with a hypothetical second key; now the type states it.
        assert_eq!(law("links"), Law::Descend(1));
        // `[schema]`: `toml::Table` — a map of values the merge does not type.
        assert_eq!(law("schema"), Law::Descend(1));
        // `[records.<field>.<id>]`: map → map → `RecordCfg`, a definition.
        assert_eq!(law("records"), Law::Descend(2));
        // `[i18n]`: the bag, then `names`/`strings` by key. Two of `I18nCfg`'s
        // five fields are maps and the deepest governs; the scalars beside
        // them are unharmed, since no descent can split a string.
        assert_eq!(law("i18n"), Law::Descend(2));
        // `[html.head.meta.<name>]`: struct → struct → map → the expression.
        assert_eq!(law("html"), Law::Descend(3));
        // `[markers.<filename>]`: a map whose value is a `MarkerDef` — a
        // definition under the marker's own filename, so what a marker MEANS
        // is taken whole (§7 q10). Unwrap that newtype back to a bare table
        // and this is the assertion that fails.
        assert_eq!(law("markers"), Law::Descend(1));
        // Arrays and scalars are atoms whatever they hold.
        assert_eq!(law("extends"), Law::Atom);
        // And the annotation is the annotation: structurally `[[collections]]`
        // is an array, and nothing but §1's exception tells collections apart
        // from a plain atom array.
        assert_eq!(law("collections"), Law::Collections);
        assert_eq!(collection_law("rules"), Law::Prepend);
        assert_eq!(
            collection_law("relations"),
            Law::Descend(1),
            "a named relation is a definition"
        );
    }

    /// One described field: its TOML name, the depth of its own shape, and
    /// whether that shape is an atom a descent would SPLIT (`Shape::TableAtom`
    /// — see `a_nested_struct_ends_at_one_depth`).
    type Field = (&'static str, usize, bool);

    /// Every struct in `shape`, with whether it sits under an ENGINE-chosen
    /// name (a field) or a user-chosen one (a map value).
    fn each_struct(shape: &Shape, engine_named: bool, seen: &mut Vec<(Vec<Field>, bool)>) {
        match shape {
            Shape::Atom | Shape::TableAtom => {}
            // The annotation overrides the law, not the description: walk
            // what it wraps, so an annotated field is held to the same
            // invariants as any other.
            Shape::Annotated(_, inner) => each_struct(inner, engine_named, seen),
            Shape::Struct(fields) => {
                seen.push((
                    fields
                        .iter()
                        .map(|(k, s)| (*k, s.depth(), s.is_table_atom()))
                        .collect(),
                    engine_named,
                ));
                for (_, s) in fields {
                    each_struct(s, true, seen);
                }
            }
            Shape::Map(value) => each_struct(value, false, seen),
        }
    }

    fn config_structs() -> Vec<(Vec<Field>, bool)> {
        let mut seen = Vec::new();
        each_struct(&Config::shape(), true, &mut seen);
        each_struct(&Collection::shape(), true, &mut seen);
        seen
    }

    /// [`Shape::definition`] leaves a definition's fields undescribed because
    /// nothing descends into one. That holds only while every undescribed
    /// struct sits under a user-chosen name: a `View`-shaped field of `Site`
    /// would be a namespace whose fields this file claims not to have, and
    /// would merge as if it had none.
    #[test]
    fn a_definition_never_sits_under_an_engine_name() {
        for (fields, engine_named) in config_structs() {
            assert!(
                !engine_named || !fields.is_empty(),
                "an undescribed struct sits under an engine-chosen name: {fields:?}"
            );
        }
    }

    /// The depth invariant, as a function of the shapes rather than as a body
    /// of assertions: the only way to mutation-check a tripwire whose whole
    /// point is that nothing in the config trips it
    /// (`a_localized_string_beside_a_map_would_be_split` fires it at a shape
    /// that does).
    ///
    /// A field at the table's deepest level is the one `Descend(n)` was
    /// measured from. Anything shallower is descended PAST, which is safe
    /// exactly while `merge_to_depth` would then be handed a non-table and
    /// hand it back whole — so a scalar or an array at depth 0 is fine, and a
    /// table-spelled atom at depth 0 is the case that would be merged key by
    /// key by a descent that was measured for its sibling.
    fn an_atom_a_deeper_sibling_would_split(structs: &[(Vec<Field>, bool)]) -> Option<String> {
        for (fields, _) in structs {
            let deepest = fields.iter().map(|(_, d, _)| *d).max().unwrap_or(0);
            for (name, depth, table_atom) in fields {
                if *depth == deepest {
                    continue;
                }
                if *table_atom {
                    return Some(format!(
                        "`{name}` is an atom spelled as a TABLE at depth {depth}, beside a \
                         field at {deepest}: `Descend({deepest})` would merge into it \
                         (MERGE.md §3 table D — the atom is the whole value)"
                    ));
                }
                if *depth != 0 {
                    return Some(format!(
                        "`{name}` sits at depth {depth} beside a field at {deepest}: \
                         one `Descend(n)` cannot serve both"
                    ));
                }
            }
        }
        None
    }

    /// Why one `Descend(n)` can govern a whole table: see
    /// [`an_atom_a_deeper_sibling_would_split`], which is this invariant.
    /// `[i18n]`'s `LocalizedStr`s are at the bottom of the deepest path, not
    /// beside it, so the config has none — and this says so for the next field
    /// anyone adds.
    #[test]
    fn a_nested_struct_ends_at_one_depth() {
        let mut nested = Vec::new();
        for (_, s) in Config::shape().fields() {
            each_struct(s, true, &mut nested);
        }
        for (_, s) in Collection::shape().fields() {
            each_struct(s, true, &mut nested);
        }
        assert_eq!(an_atom_a_deeper_sibling_would_split(&nested), None);
    }

    /// The tripwire, fired — the mutation check for a guard that nothing in
    /// the config can trip today (batch review 2, finding 1; MERGE.md R3).
    ///
    /// `[i18n]` is the table most likely to grow the field: a `LocalizedStr`
    /// beside `strings` — a site-wide `title`, say — reads as depth 0 under a
    /// `Descend(2)`, passes `a_definition_never_sits_under_an_engine_name`
    /// (it is not a struct) and `the_shape_covers_the_config_surface` (serde
    /// knows the key), and would be composed out of two writers by the merge.
    #[test]
    fn a_localized_string_beside_a_map_would_be_split() {
        let i18n_with_a_title = Shape::Struct(vec![
            // The three that are there today: a scalar at depth 0 is
            // descended past harmlessly, which is why the whitelist existed.
            ("default", Shape::Atom),
            ("locales", Shape::Atom),
            ("names", Shape::Map(Box::new(Shape::Atom))),
            // The hypothetical field. Not added to `I18nCfg` — the point is
            // that it never has to be for the guard to speak.
            ("title", LocalizedStr::shape()),
            ("strings", Shape::Map(Box::new(LocalizedStr::shape()))),
        ]);
        assert_eq!(
            i18n_with_a_title.law(),
            Law::Descend(2),
            "the sibling's law"
        );

        let mut nested = Vec::new();
        each_struct(&i18n_with_a_title, true, &mut nested);
        let msg = an_atom_a_deeper_sibling_would_split(&nested)
            .expect("a table-spelled atom beside a map must trip the invariant");
        assert!(msg.contains("`title`"), "{msg}");

        // And what the invariant is protecting, since a shape alone does not
        // say: at `Descend(2)` the base's `en` and the site's `fr` come back
        // as ONE localized string, written by two files and by no author.
        let base = toml::from_str::<toml::Value>("title = { en = \"Home\" }\n").unwrap();
        let site = toml::from_str::<toml::Value>("title = { fr = \"Accueil\" }\n").unwrap();
        let merged = merge_to_depth(base, site, 2, &mut Vec::new(), &mut Trace::off());
        let title = merged["title"].as_table().expect("a localized string");
        assert_eq!(
            title.keys().collect::<Vec<_>>(),
            ["en", "fr"],
            "the merge composed a LocalizedStr out of two writers"
        );
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
            // MERGE.md G1: a relation's candidate pool is `from` now, the
            // word a view already spelled. Hard cutoff — the key is gone and
            // `deny_unknown_fields` is the whole of the answer, with `from`
            // first in the knowns it lists.
            "[collections.relations.related]\nover = \"published\"\n",
            // MERGE.md G2: `match` survives only in rules. A view's path
            // scope is a `glob(path, …)` clause of its `where`, and a
            // relation's is `scope` — the name that owns both of that key's
            // jobs. Hard cutoff on both, one sentence from serde.
            "[sets.s]\nfrom = \"blog\"\nmatch = \"recipes/**\"\n",
            "[collections.relations.related]\nmatch = \"recipes/**\"\n",
            // IO.md I7a: an objects scope's membership is what its rules
            // claim, so the extension list is a `match` glob
            // (`**/*.{png,jpg,…}`) and the key is gone. Appended to the
            // `[[collections]]` table above, which is where it used to sit.
            "extensions = [\"png\"]\n",
        ] {
            let src = format!(
                "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
                 [[collections]]\nsource = \"_posts\"\n{stale}"
            );
            let e = Config::from_toml(&src)
                .expect_err("stale spelling should not parse")
                .to_string();
            assert!(e.contains("unknown field"), "{stale} -> {e}");
        }
    }

    /// The strictness reaches the leaf tables too. Each of these parsed and
    /// dropped the key before: `[site] them =` left the site on the base
    /// theme, `[i18n] locale =` left i18n off, `[links] strict =` left the
    /// policy at its default.
    #[test]
    fn an_unknown_key_on_a_leaf_table_is_a_parse_error() {
        for stale in [
            "[i18n]\nlocale = \"fr\"\n",
            "[links]\nstrict = true\n",
            "[shells.x]\ncommand = \"c\"\nargs = []\n",
        ] {
            let src = format!(
                "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
                 [[collections]]\nsource = \"_posts\"\n{stale}"
            );
            let e = Config::from_toml(&src)
                .expect_err("stale spelling should not parse")
                .to_string();
            assert!(e.contains("unknown field"), "{stale} -> {e}");
        }
        let e = Config::from_toml(
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             them = \"ledger\"\n",
        )
        .expect_err("a misspelled [site] key should not parse")
        .to_string();
        assert!(e.contains("unknown field"), "{e}");
    }

    /// `Site.noindex` is `#[serde(skip)]`, so `deny_unknown_fields` rejects
    /// it in `[site]` — which is what the doc comment there already claims
    /// ("set by a profile, never by the site"). The profile still sets it,
    /// because it does so in Rust, off the forced field (MERGE.md E1), which
    /// is the profile's record of itself rather than the mechanism.
    #[test]
    fn a_profile_still_sets_the_skipped_noindex() {
        const S: &str =
            "[schema]\nnoindex = { type = \"bool\" }\n[profiles.p.force]\nnoindex = true\n";
        assert!(
            !Config::from_toml(&cfg_source(S))
                .expect("the default projection")
                .site
                .noindex
        );
        assert!(projected(S, "p").expect("the profile applies").site.noindex);

        let e = Config::from_toml(
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             noindex = true\n",
        )
        .expect_err("[site] noindex is the profile's to set")
        .to_string();
        assert!(e.contains("unknown field `noindex`"), "{e}");
    }

    /// The two views every profile test below writes over: a set that never
    /// lands, and a route that does.
    const PROFILE_VIEWS: &str = "[schema]\nhidden = { type = \"bool\" }\n\
         [sets.published]\nfrom = \"blog\"\nwhere = \"!hidden\"\n\
         [routes.blog_index]\npath = \"/blog/\"\nfrom = \"published\"\nlayout = \"listing\"\n";

    /// MERGE.md E2, the fence — §4a's iron law made checkable. A profile may
    /// touch what a projection SAYS and SELECTS and never what LOADS, and the
    /// two lists beside `Config::shape` are exhaustive over the config surface,
    /// so the error can tell "you may not project this" apart from "this is not
    /// a config key at all".
    ///
    /// Mutation check: delete `"axes"` from `PROJECTABLE` and the first arm
    /// below fails (an axis overlay is refused); delete the `fence` call from
    /// `check_profiles` and the `[profiles.p.collections]` half loads in
    /// silence — the fixture `profile-projects-collections` is the same
    /// sentence at site scale, where the overlay would really have applied.
    #[test]
    fn the_fence_refuses_what_a_profile_may_not_write() {
        // The control: every projectable key, written by a profile that also
        // forces a field, on a config that loads.
        let ok = format!(
            "{PROFILE_VIEWS}[profiles.p.site]\nurl = \"https://drafts.example.com\"\n\
             [profiles.p.axes.look]\nfield = \"look\"\nvalues = [\"plain\"]\n\
             [profiles.p.widgets]\nnote = \"<aside>{{body}}</aside>\"\n"
        );
        let c = projected(&ok, "p").expect("site, axes and widgets are projectable");
        assert_eq!(c.site.url, "https://drafts.example.com");
        assert!(c.axes.contains_key("look") && c.widgets.contains_key("note"));

        // What loads is not a profile's to change, and the error says so.
        let e = cfg_err(&format!(
            "{PROFILE_VIEWS}[[profiles.p.collections]]\nname = \"x\"\n"
        ));
        assert!(e.contains("[profiles.p.collections]"), "{e}");
        assert!(e.contains("never changes what loads"), "{e}");
        assert!(e.contains("identical under every profile"), "{e}");
        assert!(e.contains("site, html, sets, routes"), "the knowns: {e}");
        // Every non-projectable key says it, not just the interesting one.
        for key in ["schema", "markers", "extends", "root", "links"] {
            let e = cfg_err(&format!("{PROFILE_VIEWS}[profiles.p]\n{key} = \"x\"\n"));
            assert!(e.contains("never changes what loads"), "{key}: {e}");
        }

        // No recursion: an overlay is one layer, not a ladder.
        let e = cfg_err(&format!(
            "{PROFILE_VIEWS}[profiles.p.profiles.q.site]\nurl = \"u\"\n"
        ));
        assert!(e.contains("never contains profiles"), "{e}");

        // And a key that is no config key at all is told that instead.
        let e = cfg_err(&format!("{PROFILE_VIEWS}[profiles.p]\nnosuch = 1\n"));
        assert!(e.contains("names no config key"), "{e}");
    }

    /// The fence is a decision, so it must be TOTAL over the config surface —
    /// which is what makes "names no config key" a true sentence rather than a
    /// guess. A field added to `Config` has to be put on one side or the other
    /// here, the same discipline `every_config_key_has_a_law` applies to the
    /// merge itself.
    ///
    /// Mutation check: delete any key from either list and this fails naming
    /// it; move one to the other list and the disjointness assert fires.
    #[test]
    fn the_fence_classifies_every_top_level_key() {
        let shape = Config::shape();
        let keys: Vec<&str> = shape.fields().iter().map(|(k, _)| *k).collect();
        for k in &keys {
            let projectable = PROJECTABLE.contains(k);
            assert!(
                projectable != NOT_PROJECTABLE.contains(k),
                "{k} is on both sides of the fence, or on neither"
            );
        }
        for k in PROJECTABLE.iter().chain(NOT_PROJECTABLE) {
            assert!(
                keys.contains(k),
                "the fence names {k}, which is no config key"
            );
        }
        // `force` is reserved rather than projectable: it is rung 0, lifted out
        // before the overlay is merged, and it is deliberately NOT a config key.
        assert!(!keys.contains(&FORCE));
    }

    /// MERGE.md E2's law, and the reason grack.com's drafts profile restates
    /// `[sets.published]` in full: the shape decides. `[site]` is a bag, so a
    /// profile patches one key of it and the rest survive; a `[sets.*]` entry
    /// is a DEFINITION, and a definition is an atom — the profile's entry
    /// replaces the site's entire, `order_by` and all.
    ///
    /// Mutation check: give `sets` `Law::Atom`'s depth-0 shape and the bag half
    /// fails; annotate `site` as an atom and the definition half stops being
    /// the distinction this test is about. The site-scale version of the second
    /// half is the parity gate: drop `order_by` from grack.com's restatement
    /// and `--profile drafts` lists by path.
    #[test]
    fn a_bag_patches_per_key_and_a_definition_replaces_whole() {
        let views = "[schema]\nhidden = { type = \"bool\" }\n\
             [sets.published]\nfrom = \"blog\"\nwhere = \"!hidden\"\norder_by = \"-date\"\n";
        let c = projected(
            &format!("{views}[profiles.p.site]\nurl = \"https://drafts.example.com\"\n"),
            "p",
        )
        .expect("a bag patches");
        assert_eq!(c.site.url, "https://drafts.example.com");
        assert_eq!(c.site.title, "t", "the rest of the bag stands");

        let c = projected(
            &format!("{views}[profiles.p.sets.published]\nfrom = \"blog\"\nwhere = \"true\"\n"),
            "p",
        )
        .expect("a restatement is a whole definition");
        assert_eq!(c.views["published"].filter.as_deref(), Some("true"));
        assert_eq!(
            c.views["published"].order_by, None,
            "the site's `order_by` is not half-inherited — the definition is the atom"
        );
    }

    /// The migrations, both of them: the closed profile vocabulary's two typed
    /// keys are ordinary config paths now, and each old spelling names its new
    /// one rather than leaving the fence to say "not a config key".
    ///
    /// Mutation check: delete either arm of `fence` and the spelling gets the
    /// generic sentence, which is true and says nothing about the fix.
    #[test]
    fn the_old_profile_spellings_name_the_new_ones() {
        // MERGE.md E1's tombstone. `[profiles.NAME] noindex = true` meant
        // something materially different — it overwrote the head declaration
        // with a constant — so `noindex = false` is refused too, since it never
        // meant anything either.
        for old in ["noindex = true", "noindex = false"] {
            let e = cfg_err(&format!(
                "[schema]\nnoindex = {{ type = \"bool\" }}\n[profiles.drafts]\n{old}\n"
            ));
            assert!(e.contains("no longer a profile key"), "{e}");
            assert!(e.contains("[profiles.drafts.force]"), "{e}");
            assert!(e.contains("noindex = true"), "the new spelling: {e}");
        }
        // E2's: `url` was the profile's own key and is the site's key now.
        // Serde says nothing here — the body is a partial config, so an
        // unknown top-level key is the fence's to explain, and `url` is live
        // in DESIGN.md §4a's example.
        let e = cfg_err("[profiles.drafts]\nurl = \"https://drafts.example.com\"\n");
        assert!(e.contains("no longer a profile key of its own"), "{e}");
        assert!(e.contains("[profiles.drafts.site]"), "{e}");
        assert!(e.contains("url = "), "the new spelling: {e}");
    }

    /// The site every R7 test below leans on: it declares `url` and lets the
    /// base supply `title` and `author`, which is the ordinary shape (a site
    /// need not restate what `extends` already said) and the shape that breaks
    /// a re-parse of the site's text alone.
    const BASE_LEANING: &str = "root = \".\"\n[site]\nurl = \"u\"\n";

    /// MERGE.md R7. The spanned re-parse is a *second opinion*, and a second
    /// opinion that changes the subject must not be published: on this site the
    /// text alone is missing base-supplied `[site]` keys, so the re-parse says
    /// `missing field` — a fiction, since the merged config had every one of
    /// them — while the real error is the retired `match` spelling in the
    /// overlay. Post-hard-cutoff (§5 Phase G) `deny_unknown_fields` is the only
    /// thing that teaches the three retired spellings, so masking it is the
    /// whole cost.
    ///
    /// Mutation check: drop the `message()` comparison (re-parse's error
    /// whenever it errors, the pre-R7 `?`) and this reports `missing field
    /// title` at line 2 instead.
    #[test]
    fn a_re_parse_that_changes_the_subject_does_not_speak() {
        let e = Config::from_toml_profile(
            &format!("{BASE_LEANING}[profiles.q.sets.y]\nmatch = \"recipes/**\"\n"),
            Some("q"),
        )
        .expect_err("`match` is retired on sets — G2")
        .to_string();
        assert!(e.contains("unknown field `match`"), "the real error: {e}");
        assert!(
            !e.contains("missing field"),
            "the site's own text is short of base-supplied keys; that is not an error: {e}"
        );
    }

    /// The other half of R7, and B3's original intent: when the re-parse DOES
    /// reproduce the failure, its error is the one worth having, because it
    /// carries the line and column that deserializing a merged `toml::Value`
    /// threw away.
    ///
    /// Mutation check: delete the fallback (return the merged error always) and
    /// the sentence survives while the span does not — no `line 4`, no caret.
    #[test]
    fn a_genuine_error_in_the_sites_own_text_keeps_its_span() {
        let e = Config::from_toml(&format!("{BASE_LEANING}nope = 1\n"))
            .expect_err("`nope` is not a `[site]` key")
            .to_string();
        assert!(e.contains("unknown field `nope`"), "{e}");
        assert!(e.contains("line 4"), "the span is the point: {e}");
    }

    /// The control that keeps the two above honest: leaning on the base is not
    /// itself an error, with a profile or without one. If this ever fails, the
    /// other two are passing for the wrong reason.
    #[test]
    fn a_site_that_leans_on_the_base_for_site_keys_loads() {
        let text = format!("{BASE_LEANING}[profiles.q.site]\ntitle = \"drafts\"\n");
        let plain = Config::from_toml(&text).expect("the base supplies title and author");
        assert_eq!(plain.site.title, "A grackle site");
        assert_eq!(plain.site.author, "");
        let projected =
            Config::from_toml_profile(&text, Some("q")).expect("and the overlay patches one key");
        assert_eq!(projected.site.title, "drafts");
        assert_eq!(projected.site.author, "");
    }

    /// MERGE.md C6a: a profile's `where` is accepted exactly where the `where`
    /// it replaces is — the row built-ins AND every declared field, one
    /// schema, because that is what `Schemas::row_filter_schema` hands
    /// `Base::resolve`.
    ///
    /// The two-shot try this replaces (`row_schema()`, then
    /// `route_schema(declared)`, with `?`) could not MIX them: `title` is in
    /// the first and not the second, `hidden` — a declared field since §4e —
    /// is in the second and not the first, so a filter naming both failed
    /// both parses and the profile was refused. Mutation-checked by restoring
    /// the two-shot, which fails on `unknown field \`title\``.
    #[test]
    fn a_profile_filter_may_mix_builtins_and_declared_fields() {
        let c = projected(
            &format!(
                "{PROFILE_VIEWS}[profiles.p.sets.published]\nfrom = \"blog\"\n\
                 where = 'title != \"\" && !hidden'\n"
            ),
            "p",
        )
        .expect("one vocabulary, not two");
        assert_eq!(
            c.views["published"].filter.as_deref(),
            Some("title != \"\" && !hidden")
        );
        // Who wrote it. The overlay replaced the whole definition, so the
        // config cannot recover this from the text — it is recorded as the
        // projection is built (MERGE.md E2), and it is what keeps the profile
        // in an error about a filter the reader cannot find in `[sets]`.
        assert_eq!(c.views["published"].filter_profile.as_deref(), Some("p"));
    }

    /// The other half of C6a: WHICH vocabulary is the patched view's own, and
    /// the three genuinely differ. `kind` is a route column no row has;
    /// `title` is a row column no route has; and `dir` is a `Str` on a row and
    /// a `Bool` on a route — so "the union of all three", which is what a
    /// two-shot try is reaching for, is not a schema anything could
    /// type-check against. The dispatch is `build_views`'s, restated nowhere:
    /// an all-outputs fold → routes, otherwise rows plus every declared field —
    /// and an object is a row like any other now (IO.md §3), so a gallery reads
    /// the same row vocabulary a post's view does.
    #[test]
    fn a_profile_filter_takes_the_patched_views_own_vocabulary() {
        let c = cfg_raw(&format!(
            "{PROFILE_VIEWS}\
             [[collections]]\nname = \"pics\"\n\
             [sets.gallery]\nfrom = \"pics\"\n\
             [routes.sitemap]\npath = \"/sitemap.xml\"\nshell = \"sitemap\"\n"
        ));
        let rows = c.view_filter_schema("published");
        assert!(rows.contains_key("title") && rows.contains_key("hidden"));
        assert!(!rows.contains_key("kind"), "a row has no route kind");

        let routes = c.view_filter_schema("sitemap");
        assert!(routes.contains_key("kind") && routes.contains_key("hidden"));
        assert!(!routes.contains_key("title"), "a route has no title");

        // A gallery reads the ONE row schema now: the object columns (`width`)
        // and every declared field (`hidden`) alike — the narrow object
        // vocabulary is gone with `kind`.
        let objects = c.view_filter_schema("gallery");
        assert!(objects.contains_key("width"), "an object has dimensions");
        assert!(objects.contains_key("hidden"), "and the declared fields");

        // The collision that rules the union out, stated rather than implied.
        use grackle_db::filter::Type;
        assert_eq!(rows.get("dir"), Some(&Type::Str));
        assert_eq!(routes.get("dir"), Some(&Type::Bool));

        // And the fold's own overlay applies, against route words.
        projected(
            &format!(
                "{PROFILE_VIEWS}[routes.sitemap]\npath = \"/sitemap.xml\"\n\
                 shell = \"sitemap\"\n\
                 [profiles.p.routes.sitemap]\npath = \"/sitemap.xml\"\n\
                 shell = \"sitemap\"\nwhere = 'kind == \"post\" && !hidden'\n"
            ),
            "p",
        )
        .expect("an all-outputs fold reads routes");
    }

    /// The deferral C6a's fix rests on, at the unit level: a name this early
    /// vocabulary does not have is NOT rejected here, because a positional
    /// `.schema.toml` declares fields the tree walk has not read yet and
    /// refusing them would make a profile's `where` stricter than the `where`
    /// it replaces. What is caught is everything that is wrong however the
    /// walk turns out — `a_profile_filter_that_does_not_type_check_is_caught_at_load`
    /// is that half. The tree-driven proof of both directions lives in
    /// `load::profile_filter_tests`.
    #[test]
    fn an_unknown_name_in_a_profile_filter_is_deferred_not_rejected() {
        let c = projected(
            &format!(
                "{PROFILE_VIEWS}[profiles.p.sets.published]\nfrom = \"blog\"\nwhere = \"!cover\"\n"
            ),
            "p",
        )
        .expect("`cover` may yet be a positional declaration");
        assert_eq!(c.views["published"].filter.as_deref(), Some("!cover"));
    }

    /// MERGE.md C6b: the projection is a config that `validate` has never seen,
    /// so it is validated — and `check_profile_filters` is what makes that
    /// load-bearing, since it is keyed off the provenance E2 records as the
    /// overlay is merged.
    ///
    /// Mutation-checked by deleting the `filter_profile` loop in
    /// `from_toml_profile`, after which this config projects happily and the
    /// type error surfaces at the pass that evaluates the filter, naming no
    /// profile.
    #[test]
    fn a_profile_filter_that_does_not_type_check_is_caught_at_load() {
        let e = format!(
            "{:#}",
            projected(
                &format!(
                    "{PROFILE_VIEWS}[profiles.p.sets.published]\nfrom = \"blog\"\n\
                     where = 'title > 3'\n"
                ),
                "p",
            )
            .expect_err("a string is not an int")
        );
        assert!(e.contains("profile p"), "names the profile: {e}");
        assert!(e.contains("view published"), "names the view: {e}");
    }

    /// MERGE.md E2: what the retired placement checks were guarding is now
    /// said by the ordinary rules, because the overlay produces an ordinary
    /// config. C6c refused `[profiles.p.sets.blog_index]` because `blog_index`
    /// is a route; today that entry ADDS a `[sets]` definition of that name,
    /// which collides in the one namespace `merge_queries` folds the two
    /// sections into — the same error a site writing it twice would get, and
    /// the reason no third rule is needed.
    ///
    /// Mutation check: delete the `views.insert(...).is_some()` bail in
    /// `merge_queries` and the misplaced entry loads, its view patched twice
    /// in map order.
    #[test]
    fn a_misplaced_profile_entry_collides_in_the_one_namespace() {
        // The control: both entries where they belong, both restated whole.
        let ok = projected(
            &format!(
                "{PROFILE_VIEWS}[profiles.p.sets.published]\nfrom = \"blog\"\nwhere = \"!hidden\"\n\
                 [profiles.p.routes.blog_index]\npath = \"/blog/\"\nfrom = \"published\"\n\
                 layout = \"listing\"\nwhere = 'title != \"\"'\n"
            ),
            "p",
        )
        .expect("both are where they belong");
        assert_eq!(ok.views["published"].filter.as_deref(), Some("!hidden"));
        assert_eq!(
            ok.views["blog_index"].filter.as_deref(),
            Some("title != \"\"")
        );

        let e = format!(
            "{:#}",
            Config::from_toml(&cfg_source(&format!(
                "{PROFILE_VIEWS}[profiles.p.sets.blog_index]\npath = \"/blog/\"\n\
                 from = \"published\"\nlayout = \"listing\"\nwhere = \"!hidden\"\n"
            )))
            .expect_err("`blog_index` is already a route")
        );
        assert!(e.contains("profile p"), "{e}");
        assert!(
            e.contains("declares a path") || e.contains("both a set and a route"),
            "{e}"
        );
    }

    /// MERGE.md R5's principle at E2's scale: EVERY declared profile is
    /// projected, deserialized and validated at every load, so a broken
    /// overlay in a projection nobody is building is a load error today. The
    /// config below is loaded the way `grackle build` loads it, with no
    /// `--profile` anywhere.
    ///
    /// Its typo is `publised` for `[sets.published]`, the query a drafts-shaped
    /// profile relaxes — and what makes it an error is no longer a name check.
    /// A profile naming an unknown view ADDS a definition, which is what a
    /// registry does; the addition is then held to the same rules as any other
    /// entry, and a set with no `from` is not a set — since IO.md I3 an absent
    /// `from` is legal on a FOLD shell (it reads every output, the retired
    /// `from = "*"`) and on nothing else, and this entry declares no shell.
    ///
    /// Mutation check: delete the dry-run loop in `from_toml_profile` and both
    /// halves load in silence, failing only under `--profile staging`. The
    /// site-scale version is the `profile-unknown-view` fixture.
    #[test]
    fn a_broken_overlay_fails_a_load_that_never_applies_it() {
        let e = format!(
            "{:#}",
            Config::from_toml(&cfg_source(&format!(
                "{PROFILE_VIEWS}[profiles.staging.sets.publised]\nwhere = \"!hidden\"\n"
            )))
            .expect_err("a set with no `from` is not a set")
        );
        assert!(e.contains("profile staging"), "names the profile: {e}");
        assert!(e.contains("checked at every load"), "and why: {e}");
        assert!(e.contains("no `from`"), "{e}");

        // The other direction: a profile ADDING a well-formed view is legal —
        // a registry gains an entry, which is what a registry is for.
        let c = projected(
            &format!(
                "{PROFILE_VIEWS}[profiles.staging.sets.drafts_only]\nfrom = \"blog\"\n\
                 where = \"hidden\"\n"
            ),
            "staging",
        )
        .expect("a profile may add a set");
        assert_eq!(c.views["drafts_only"].filter.as_deref(), Some("hidden"));
        // …and it is the author's, not the base's: an error about it must not
        // send them looking in a config they did not write.
        assert!(!c.views["drafts_only"].inherited);
    }

    /// R5's three controls, in one place because they are one sentence: a
    /// profile that is CORRECT is not disturbed by being checked early.
    ///
    /// The `dev` one is the load-bearing half. DESIGN.md §4a makes `dev`
    /// implicit — `serve` defaults to it and undeclared it changes nothing —
    /// so the dry run must never be the thing that invents a `[profiles.dev]`
    /// requirement. It cannot be: it iterates the profiles a config DECLARES,
    /// and an implicit one declares nothing.
    #[test]
    fn checking_every_profile_leaves_the_correct_ones_alone() {
        // A site with no profiles at all — reaching the next line is the
        // assertion, since the dry run runs inside `from_toml`.
        let plain = Config::from_toml(&cfg_source(PROFILE_VIEWS)).expect("no profiles, no checks");
        assert!(plain.profiles.is_empty());

        // grack.com's shape: one profile, correct, never applied.
        let declared = format!(
            "{PROFILE_VIEWS}[profiles.drafts.force]\nhidden = false\n\
             [profiles.drafts.sets.published]\nfrom = \"blog\"\nwhere = \"true\"\n"
        );
        let both = Config::from_toml(&cfg_source(&declared)).expect("declared, not applied");
        assert_eq!(
            both.views["published"].filter.as_deref(),
            Some("!hidden"),
            "the default projection is the config exactly as written"
        );
        assert!(both.forced.is_empty(), "nothing is forced until applied");
        // And applying it still works, which is the same config one flag on.
        let applied = projected(&declared, "drafts").expect("as declared");
        assert_eq!(applied.views["published"].filter.as_deref(), Some("true"));
        assert_eq!(applied.forced["hidden"], toml::Value::Boolean(false));

        // `serve`'s default: undeclared `dev` needs no `[profiles.dev]`, and a
        // config carrying an unrelated profile still loads under it.
        let dev = projected(&declared, "dev").expect("dev is implicit (§4a)");
        assert!(!dev.profiles.contains_key("dev"));
        assert_eq!(dev.profile.as_deref(), Some("dev"));
        // …and changes nothing: `drafts` was declared, not applied.
        assert_eq!(dev.views["published"].filter.as_deref(), Some("!hidden"));
        assert!(dev.views["published"].filter_profile.is_none());

        // A name that is neither declared nor implicit is a load error naming
        // what exists, rather than a build that ships the wrong projection.
        let e = format!(
            "{:#}",
            Config::from_toml_profile(&cfg_source(&declared), Some("stagin"))
                .expect_err("a typo is not a projection")
        );
        assert!(e.contains("unknown profile \"stagin\""), "{e}");
        assert!(e.contains("declared: dev, drafts"), "{e}");
    }

    /// A `[routes]` entry whose `default_content` offer was DECLINED loses its
    /// path — and what that leaves is not a set. The section an entry was
    /// declared under is recorded rather than re-derived for exactly this
    /// case: `is_materialized()` would call this view a set, and C7b's error
    /// tells the author to "declare your own [sets.home]" over an entry that
    /// lives under `[routes]`.
    ///
    /// (C6c's placement check was the other reader and is retired with E2 —
    /// `whose_from` is what keeps `declared_set` live. Mutation check: derive
    /// it from `is_materialized()` in `merge_queries` and this fails.)
    #[test]
    fn a_declined_default_content_route_is_still_a_route() {
        let mut c = cfg_raw(
            "[routes.home]\npath = \"/\"\nfrom = \"blog\"\nlayout = \"listing\"\n\
             default_content = \"index.md\"\n",
        );
        // What `resolve_default_content` does to a route whose offered row
        // exists and does not place `{% view home %}`: the row wants the URL to
        // itself, so the route stands down.
        let v = c.views.get_mut("home").expect("declared");
        v.route = None;
        v.routes.clear();
        assert!(!v.is_materialized());
        assert!(
            !c.views["home"].declared_set,
            "a route with no path left is still a route"
        );
    }

    /// MERGE.md E1, the whole point of the shape: the profile writes the
    /// FIELD, and the site's own `robots` expression is left exactly as its
    /// author wrote it. C6d's key overwrote `[html.head.meta] robots` with the
    /// constant `"noindex,follow"` on every page of the projection, which is
    /// why it needed a warning to be honest; there is nothing left to warn
    /// about, and the two configs below — one inheriting the base's
    /// expression, one writing its own — now come out saying different things
    /// about the same forced fact, which is what "the site's vocabulary"
    /// means.
    ///
    /// Mutation check: leave `force` in the overlay (`split_profile` reading it
    /// rather than removing it) and the projected table carries a top-level
    /// `force` key the `Config` deserializer refuses — rung 0 is reserved, not
    /// config surface, and the fence lets it through for exactly that reason.
    #[test]
    fn a_forced_field_leaves_the_sites_robots_expression_alone() {
        let site = "[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n";
        let inherited = Config::from_toml_profile(
            &format!("root = \".\"\n{site}[profiles.drafts.force]\nnoindex = true\n"),
            Some("drafts"),
        )
        .expect("a base-inheriting config");
        assert_eq!(
            inherited.html.head.meta["robots"], "noindex ? \"noindex,follow\" : \"\"",
            "the base's expression, untouched — it EVALUATES the forced field"
        );

        let own = Config::from_toml_profile(
            &format!(
                "root = \".\"\n{site}[profiles.drafts.force]\nnoindex = true\n\
                 [html.head.meta]\nrobots = 'noindex ? \"noindex,nofollow\" : \"index,follow\"'\n"
            ),
            Some("drafts"),
        )
        .expect("a site may write its own robots expression");
        assert_eq!(
            own.html.head.meta["robots"], "noindex ? \"noindex,nofollow\" : \"index,follow\"",
            "an editorial policy its author spelled out is not a profile's to \
             replace — it answers the forced fact its own way"
        );
        assert_eq!(own.forced["noindex"], toml::Value::Boolean(true));
    }

    /// MERGE.md E1: rung 0's names come from the site's own `[schema]`, and
    /// they are checked for EVERY declared profile at every load — R5's
    /// sentence, one table over. `cfg_err` applies no profile at all.
    ///
    /// Mutation-checked three ways: deleting the `declared.get` arm accepts
    /// `nosuchfield` (first half); deleting the `schema::typed` call accepts
    /// `noindex = "yes"` (second half); and deleting the whole block from
    /// `check_profiles` loses both.
    #[test]
    fn a_forced_field_is_declared_and_typed_for_every_profile() {
        const S: &str = "[schema]\nnoindex = { type = \"bool\" }\n";

        let e = cfg_err(&format!(
            "{S}[profiles.staging.force]\nnosuchfield = true\n"
        ));
        assert!(e.contains("profile staging"), "names the profile: {e}");
        assert!(e.contains("[profiles.staging.force] nosuchfield"), "{e}");
        assert!(e.contains("declared in the site's own [schema]"), "{e}");
        assert!(e.contains("declared fields: noindex"), "the knowns: {e}");

        let e = cfg_err(&format!("{S}[profiles.staging.force]\nnoindex = \"yes\"\n"));
        assert!(e.contains("[profiles.staging.force]"), "{e}");
        assert!(e.contains("declared bool"), "{e}");

        // Rung 0 is not overlay: `force` is lifted out before the merge, so a
        // table under it is never a config path.
        let e = cfg_err(&format!("{S}[profiles.staging]\nforce = 3\n"));
        assert!(e.contains("[profiles.staging.force] is a table"), "{e}");

        // The control: correct, and inert on a load that applies no profile.
        let ok = cfg(&format!("{S}[profiles.staging.force]\nnoindex = true\n"));
        assert!(ok.forced.is_empty(), "nothing is forced until applied");
    }

    #[test]
    fn unknown_from_is_an_error() {
        let c = cfg("[sets.latest]\nfrom = \"pubished\"\nlimit = 3\n");
        let e = c.query("latest").unwrap_err().to_string();
        assert!(
            e.contains("neither a collection, a set nor a route"),
            "unexpected error: {e}"
        );
        // The author wrote this one, so there is nothing to explain about
        // where it came from — the control for the two tests below.
        assert!(!e.contains("inherited from the base config"), "{e}");
        assert!(!e.contains("reached from"), "{e}");
    }

    /// MERGE.md C7b: renaming the collection at `_posts` retires the name
    /// `posts`, and the base's `[sets.published] from = "posts"` then names
    /// nothing — on a site whose grackle.toml has no `published` in it.
    ///
    /// Views key on NAME and survive every rename; collections key on
    /// `source` and do not. That asymmetry is the whole of this bug, and it
    /// is why an inherited `from` is the one reference a site can break
    /// without touching the entry that carries it.
    #[test]
    fn an_inherited_sets_dangling_from_says_it_came_from_the_base() {
        let c = Config::from_toml(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"notes\"\nsource=\"_posts\"\n",
        )
        .unwrap();
        let e = c.query("published").unwrap_err().to_string();
        assert!(e.starts_with("published: `from = \"posts\"`"), "{e}");
        assert!(
            e.contains("\"published\" is inherited from the base config (§4d)"),
            "{e}"
        );
        assert!(
            e.contains("declare your own [sets.published]"),
            "the fix, in the table the entry would live in: {e}"
        );
        // The knowns are what show the author their own rename.
        assert!(e.contains("collections: entries, notes, objects"), "{e}");
    }

    /// The other half of the same blame: `blog_index` composes over
    /// `published`, so asking for `blog_index`'s query is what surfaces
    /// `published`'s broken `from` — and the old message put `blog_index`'s
    /// name in front of a `from` that is not in `blog_index`.
    #[test]
    fn a_composed_chain_blames_the_view_that_carries_the_from() {
        let c = Config::from_toml(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"notes\"\nsource=\"_posts\"\n",
        )
        .unwrap();
        let e = c.query("blog_index").unwrap_err().to_string();
        assert!(
            e.starts_with("published: `from = \"posts\"`"),
            "the carrier, not the asker: {e}"
        );
        assert!(
            e.contains("(reached from \"blog_index\", which composes over it.)"),
            "{e}"
        );
    }

    /// The control, and the shape `examples/field-notes` really has: rename
    /// the collection AND say what the inherited set means now. One line, and
    /// it is the line the error above asks for.
    #[test]
    fn a_renamed_collection_with_its_own_published_set_resolves() {
        let c = Config::from_toml(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"notes\"\nsource=\"_posts\"\n\
             [sets.published]\nfrom=\"notes\"\nwhere=\"!draft\"\n",
        )
        .unwrap();
        let q = c
            .query("blog_index")
            .expect("the chain terminates at `notes`");
        assert_eq!(q.base, vec!["notes".to_string()]);
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

    /// §6f, C4a: `[i18n.names]` is keyed by locale, so a key naming no
    /// declared locale is dead — it labels a translations-axis member that
    /// can never exist. The error names the default and the declared set,
    /// like every other locale error in this block.
    #[test]
    fn an_i18n_name_must_name_a_declared_locale() {
        let c = cfg("[i18n]\nlocales = [\"fr\"]\n\n\
             [i18n.names]\nen = \"English\"\nfr = \"Français\"\n");
        assert_eq!(c.i18n.name_of("fr"), "Français");
        assert_eq!(c.i18n.name_of("en"), "English");
        // The default locale needs no `locales` entry, and a name for it is
        // the shape every live site uses.
        let e =
            cfg_err("[i18n]\nlocales = [\"fr\"]\n\n[i18n.names]\nfr_CA = \"Français canadien\"\n");
        assert!(e.contains("fr_CA"), "{e}");
        assert!(e.contains("\"en\""), "the default is named: {e}");
        assert!(e.contains("\"fr\""), "the knowns are named: {e}");
        // …and with i18n off, only the default locale may be named.
        let e = cfg_err("[i18n.names]\nfr = \"Français\"\n");
        assert!(e.contains("\"fr\""), "{e}");
        assert!(e.contains("[]"), "the empty locale set is shown: {e}");
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

    // ------------------------------------------- `config --effective` (B3)

    /// The effective config of a site whose text is `site`, with the preamble
    /// stripped so an assertion is about the config and not about the prose.
    fn effective(site: &str) -> String {
        let printed =
            Config::effective_toml(site, "test", None).expect("the effective config should print");
        printed
            .lines()
            .skip_while(|l| l.starts_with('#') || l.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The line that carries `key`, comment and all.
    fn provenance_of(printed: &str, key: &str) -> String {
        printed
            .lines()
            .find(|l| l.trim_start().starts_with(key))
            .unwrap_or_else(|| panic!("no line for {key} in:\n{printed}"))
            .to_string()
    }

    /// Law 2 at the surface a person reads: a redeclared registry entry says
    /// SITE and the base's entry is gone entirely — not merged, not half
    /// present. `limit = 20` is the base's `[routes.feed]`, and its absence
    /// here is the whole claim.
    #[test]
    fn a_shadowed_registry_entry_reads_as_one_atom_from_the_site() {
        let out = effective(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [routes.feed]\npath=\"/feed.xml\"\nfrom=\"published\"\nshell=\"atom\"\n",
        );
        assert!(
            provenance_of(&out, "[routes.feed]").contains("# site over base, whole"),
            "{out}"
        );
        assert!(
            !out.contains("limit = 20"),
            "the base's feed entry survived the shadow:\n{out}"
        );
        // And the entry's own keys carry no provenance: the atom said it once.
        assert_eq!(
            provenance_of(&out, "path = \"/feed.xml\""),
            "path = \"/feed.xml\""
        );
        // A neighbour the site never wrote is the point of the command.
        assert!(
            provenance_of(&out, "[routes.home]").contains("# base, whole"),
            "{out}"
        );
    }

    /// A bag is the other law at the same depth, and reads differently: three
    /// keys, three answers, on one table.
    #[test]
    fn a_merged_bag_shows_its_sources_key_by_key() {
        let out = effective("[site]\ntitle = \"Mine\"\nemail = \"me@example.com\"\n");
        assert!(
            provenance_of(&out, "title =").contains("# site over base"),
            "{out}"
        );
        assert!(provenance_of(&out, "email =").contains("# site"), "{out}");
        assert!(provenance_of(&out, "url =").ends_with("# base"), "{out}");
        assert!(
            provenance_of(&out, "author =").ends_with("# base"),
            "the base's empty author is still the base's:\n{out}"
        );
    }

    /// A whole table the site never mentioned. `[markers]` is the base's
    /// three, and a site that has never heard of `.draft` still has it — the
    /// invisible base, made visible, which is the reason this command exists.
    #[test]
    fn an_untouched_table_is_all_base() {
        let out = effective("[site]\ntitle = \"Mine\"\n");
        for m in ["\".draft\"", "\".hidden\"", "\".noindex\""] {
            assert!(
                provenance_of(&out, m).contains("# base, whole"),
                "{m} in:\n{out}"
            );
        }
        assert!(
            provenance_of(&out, "[sets.published]").contains("# base, whole"),
            "{out}"
        );
        // Never written by either file: serde's default, and it is named as
        // such rather than passed off as the base's.
        assert!(
            provenance_of(&out, "gitignore =").ends_with("# default"),
            "{out}"
        );
        assert!(
            provenance_of(&out, "root =").ends_with("# default"),
            "{out}"
        );
    }

    /// §1's annotation, read out loud. A site's rules go in front and say
    /// `site`; the base's catch-all sits behind them and says `base`, which is
    /// how "first writer wins" looks when you can see the list.
    #[test]
    fn prepended_rules_carry_provenance_per_rule() {
        let out = effective(
            "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource = \"_posts\"\n\
             [[collections.rules]]\nmatch = \"drafts/**\"\nroute = \"/d/{slug}/\"\n",
        );
        // Only the posts collection's rules: the base's other two collections
        // are printed too, and their rules are a different list.
        let posts = out
            .split("\n[[collections]]")
            .find(|c| c.contains("source = \"_posts\""))
            .unwrap_or_else(|| panic!("no posts collection in:\n{out}"));
        let rules: Vec<&str> = posts
            .lines()
            .filter(|l| l.starts_with("[[collections.rules]]"))
            .collect();
        assert_eq!(rules.len(), 2, "site rule + the base's catch-all:\n{out}");
        assert!(rules[0].contains("# site, whole"), "{out}");
        assert!(rules[1].contains("# base, whole"), "{out}");
        assert!(
            out.contains("match = \"drafts/**\""),
            "the site's rule is first:\n{out}"
        );
    }

    /// `extends = "none"` has no merge to record, so the walk that stands in
    /// for one must reach the same atoms: every key the site's own, at the
    /// same granularity (`[sets.x]` whole, `[site]` per key).
    #[test]
    fn an_uninheriting_site_owns_every_key() {
        let out = effective(
            "extends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [sets.mine]\nfrom = \"posts\"\nwhere = \"!draft\"\norder_by = \"-date\"\n\
             [markers]\n\".x\" = { draft = true }\n",
        );
        for (key, want) in [
            ("extends =", "# site"),
            ("url =", "# site"),
            ("[sets.mine]", "# site, whole"),
            ("\".x\"", "# site, whole"),
        ] {
            assert!(provenance_of(&out, key).contains(want), "{key} in:\n{out}");
        }
        assert!(!out.contains("# base"), "nothing was inherited:\n{out}");
    }

    /// The printer neither drops a key nor invents one: parsed back, the text
    /// IS the merged table. Comments are TOML's own, so nothing is stripped —
    /// the parser does that.
    ///
    /// This is the test that makes the rest safe to read. Provenance is a
    /// comment and a comment cannot be wrong about a value it does not carry;
    /// what could go wrong is the VALUE — a definition flattened, an inline
    /// table mis-quoted, a key printed under the wrong header — and a
    /// round-trip catches every one of those.
    #[test]
    fn printing_the_merged_config_loses_nothing() {
        for site in [
            "",
            "[site]\ntitle = \"Mine\"\n",
            "extends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n",
            // Every shape the printer distinguishes, in one file: an
            // array-of-tables keyed by identity, its rules, a nested map of
            // definitions, a localized string, a quoted key, an inline table.
            "root = \"..\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource = \"_posts\"\n\
             [collections.schema]\ncover = { type = \"image\" }\n\
             [[collections.rules]]\nmatch = \"**\"\ndefaults = { layout = \"post\" }\n\
             [collections.relations.related]\nfrom = \"published\"\nlimit = 3\n\
             [axes.locale]\nfield = \"locale\"\ntemplate = \"/{locale}{path}\"\n\
             [html.head.meta]\n\"apple-title\" = 'site.title'\n\
             [i18n]\nlocales = [\"fr\"]\n[i18n.strings]\nhome = { en = \"Home\", fr = \"Accueil\" }\n\
             [records.course.dinner]\nname = { en = \"Dinner\", fr = \"Dîner\" }\n\
             [widgets]\nnote = \"<aside>{body}</aside>\"\n",
        ] {
            let printed = Config::effective_toml(site, "test", None).expect("prints");
            let back: toml::Value = toml::from_str(&printed)
                .unwrap_or_else(|e| panic!("the printed config is not TOML: {e}\n{printed}"));

            let value: toml::Value = toml::from_str(site).unwrap();
            let mut want = match Config::extends_of(&value).unwrap() {
                true => merge_base(value).unwrap(),
                false => value,
            };
            let t = want.as_table_mut().unwrap();
            for (k, v) in engine_defaults() {
                if !t.contains_key(k) {
                    t.insert(k.to_string(), v);
                }
            }
            assert_eq!(back, want, "printed:\n{printed}");
        }
    }

    /// A key TOML would not accept bare has to be quoted in a HEADER too, not
    /// only in a `k = v` line — `[markers.".archive"]`. Found by mutating the
    /// base-recording loop away, which turned every inherited marker into a
    /// block and printed `[markers..draft]`; the payload here is long enough
    /// to take that path without a mutation.
    #[test]
    fn a_quoted_key_stays_quoted_in_a_table_header() {
        let site = "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n[markers]\n\
                    \".archive\" = { noindex = true, hidden = true, draft = true, layout = \"post\" }\n";
        let out = effective(site);
        assert!(out.contains("[markers.\".archive\"]"), "{out}");
        toml::from_str::<toml::Value>(&Config::effective_toml(site, "t", None).unwrap())
            .expect("a quoted header must parse");
    }

    /// The family check, on the view side (IO.md §4, I2). A view is a query,
    /// so its declared shell folds the collection the query selects — and a
    /// MAP shell here is an arity error, not an unknown word: `html` is a
    /// perfectly good shell that happens to wrap one output, which is the
    /// distinction the old "unknown shell" message could not make because the
    /// two vocabularies never met.
    ///
    /// Mutation check: replace `shell::check_view`'s body with the pre-I2
    /// membership test (`is_fold(name) || registered.contains(&name)` alone,
    /// erroring with "unknown shell") and the map half fails on the message
    /// while the control still passes.
    #[test]
    fn a_map_shell_on_a_view_is_an_arity_error() {
        for map in crate::shell::MAP {
            let e = cfg_err(&format!(
                "[routes.feed]\npath = \"/f.xml\"\nfrom = \"blog\"\nshell = \"{map}\"\n"
            ));
            assert!(e.contains("is a map shell"), "{map}: {e}");
            assert!(e.contains("wraps ONE output"), "{map}: {e}");
            assert!(e.contains("atom, sitemap, search"), "{map}: {e}");
        }
        // The controls: every fold, and a registered script shell beside them.
        for fold in crate::shell::FOLD {
            cfg_raw(&format!(
                "[routes.feed]\npath = \"/f.xml\"\nfrom = \"blog\"\nshell = \"{fold}\"\n"
            ))
            .validate()
            .unwrap_or_else(|e| panic!("{fold} is a fold shell: {e:#}"));
        }
        cfg_raw(
            "[shells.llms]\ncommand = \"c\"\n\
             [routes.feed]\npath = \"/f.txt\"\nfrom = \"blog\"\nshell = \"llms\"\n",
        )
        .validate()
        .expect("a registered script shell is a fold");
        // And the retired spellings are hard cutoffs on this side too.
        for stale in ["none", "light"] {
            let e = cfg_err(&format!(
                "[routes.feed]\npath = \"/f.xml\"\nfrom = \"blog\"\nshell = \"{stale}\"\n"
            ));
            assert!(e.contains("unknown shell"), "{stale}: {e}");
        }
    }

    /// The per-member half of the arity check. An `[axes.*]` over `shell`
    /// declares the serializations its members leave through, and a member is
    /// ONE output — so the values are map shells.
    ///
    /// This is the one path a shell reaches `build.rs` on without passing
    /// through a row's cascade, which is why it needs a check of its own:
    /// before I2 the axis fixture's `light` was never validated anywhere, and
    /// a value outside the vocabulary rendered the fallback tier in silence.
    ///
    /// Mutation check: delete the `a.field == "shell"` loop in `check` and both
    /// halves here pass an unchecked value straight through.
    #[test]
    fn an_axis_over_shell_takes_map_shells_only() {
        let e = cfg_err("[axes.serialization]\nvalues = [\"html\", \"atom\"]\nfield = \"shell\"\n");
        assert!(e.contains("spends the `shell` field"), "{e}");
        assert!(e.contains("fold shell"), "{e}");
        let e = cfg_err("[axes.s]\nvalues = [\"html\", \"light\"]\nfield = \"shell\"\n");
        assert!(e.contains("not a map shell"), "{e}");
        // Controls: the map family passes, and an axis over another field is
        // none of this check's business (a theme value carries subtheme
        // tokens and would fail every shell test there is).
        cfg_raw("[axes.s]\nvalues = [\"html\", \"light_html\"]\nfield = \"shell\"\n")
            .validate()
            .expect("map shells are what a member leaves through");
        cfg_raw("[axes.t]\nvalues = [\"default\", \"ledger:dark\"]\nfield = \"theme\"\n")
            .validate()
            .expect("a theme axis is not a shell axis");
    }

    /// A script shell may not take a built-in's name — it would be a command
    /// nobody could reach, because `check_view` answers from the built-in
    /// vocabulary first.
    ///
    /// Mutation check: delete the `check_registered_name` loop and
    /// `[shells.atom]` registers a command the atom shell shadows, silently.
    #[test]
    fn a_script_shell_may_not_take_a_builtins_name() {
        for taken in ["atom", "sitemap", "search", "raw", "html", "light_html"] {
            let e = cfg_err(&format!("[shells.{taken}]\ncommand = \"c\"\n"));
            assert!(e.contains("is a built-in shell"), "{taken}: {e}");
        }
        cfg_raw("[shells.llms]\ncommand = \"c\"\n")
            .validate()
            .expect("a name of its own is fine");
    }

    /// The cost argument, asserted rather than claimed: the load path merges
    /// with a recorder that is off, and an off recorder holds nothing however
    /// much config goes past it.
    #[test]
    fn the_load_path_records_nothing() {
        let mut off = Trace::off();
        let site: toml::Value = toml::from_str("[site]\ntitle = \"Mine\"\n").unwrap();
        merge_base_traced(site, &mut off).unwrap();
        assert_eq!(off.len(), 0);
    }
}
