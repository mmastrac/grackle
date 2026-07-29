//! Authored config surface and merge laws.

use crate::config::effective::{collection_seg, index_seg, Prov, Trace};
use crate::markers::MarkerDef;
use crate::shape::{annotated, field, Law, Shape, Shaped};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
    pub(crate) declared_collections: Vec<Collection>,
    /// Sets and routes, merged. One namespace (§5c): `from` names a
    /// collection, a set or a route, so the three cannot collide — checked
    /// in `validate`. One map internally because the split is a
    /// config-surface distinction, not an engine one: a set is a route
    /// with no path.
    #[serde(skip)]
    pub views: BTreeMap<String, View>,
    /// Queries that never land — no `path`. Composable, embeddable.
    #[serde(default)]
    pub(crate) sets: BTreeMap<String, View>,
    /// Queries that land: every URL the site emits from a query.
    #[serde(default)]
    pub(crate) routes: BTreeMap<String, View>,
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
    /// `[html]` (§4e): what the engine puts on the document — head tags and
    /// root-element attributes — declared rather than compiled in.
    #[serde(default)]
    pub html: HtmlCfg,
    /// `[schema]` (§5b): typed fields every row has, plus how embeddings and
    /// search read those fields. Field declarations are the flatten; the two
    /// named subtables are engine consumers of the vocabulary, not fields.
    #[serde(default)]
    pub schema: SchemaBag,
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
    /// i18n (§6f): display strings for the pairing axis. Absent `[i18n]` still
    /// points at axis `"locale"`; membership lives on `[axes.*]`.
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
pub(crate) const FORCE: &str = "force";

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
pub(crate) fn split_profile(pname: &str, body: &toml::Table) -> Result<(toml::Table, toml::Table)> {
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

pub(crate) fn default_extends() -> String {
    "default".to_string()
}

/// The base config, compiled in (§4d) — the same move as `parts.toml` and the
/// base theme, for the same reason: a site can forget to copy a file, and
/// cannot forget the binary.
pub(crate) const BASE: &str = include_str!("../../assets/base.toml");

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
pub(crate) const PROJECTABLE: &[&str] = &[
    "site", "html", "sets", "routes", "i18n", "records", "widgets", "shells", "axes",
];

/// What a profile may NEVER write: everything that decides what LOADS.
///
/// The database is identical under every profile — that is what makes two
/// projections comparable, and what lets one resident db answer for several
/// (§4a). `profiles` is here for a second reason: a profile does not contain
/// profiles, so the overlay is one layer and not a ladder.
pub(crate) const NOT_PROJECTABLE: &[&str] = &[
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
pub(crate) fn fence(pname: &str, key: &str) -> Result<()> {
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
            field("file", |c: &Collection| &c.file),
            field("exclude", |c: &Collection| &c.exclude),
            field("include", |c: &Collection| &c.include),
            // The other half of §1's annotation: the site's rules go FIRST,
            // which is Law 1 expressed in list order — nearer writer, earlier
            // in the file, first to claim a key.
            annotated("rules", |c: &Collection| &c.rules, Law::Prepend),
            field("trail", |c: &Collection| &c.trail),
            field("archives", |c: &Collection| &c.archives),
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
        ])
    }
}

impl Shaped for HtmlCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("head", |h: &HtmlCfg| &h.head),
            field("html", |h: &HtmlCfg| &h.html),
            field("body", |h: &HtmlCfg| &h.body),
        ])
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

impl Shaped for AttrCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![field("attribute", |a: &AttrCfg| &a.attribute)])
    }
}

impl Shaped for I18nCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("axis", |i: &I18nCfg| &i.axis),
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

enums_are_atoms![LinkPolicy];

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
pub(crate) fn law_of(shape: &Shape, key: &str) -> Law {
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
/// (`crates/core/tests/base_config.rs`) is for: it reads the
/// `#[serde(default = "…")]` fields off [`Config`]'s own text and requires
/// each one in the empty site's effective config.
pub(crate) fn engine_defaults() -> Vec<(&'static str, toml::Value)> {
    vec![
        ("extends", default_extends().into()),
        ("root", default_root().display().to_string().into()),
        ("gitignore", default_true().into()),
    ]
}

/// Merge the base config underneath a site's own (§4d). Every rule this
/// applies already existed somewhere in the system, which is the evidence that
/// config inheritance needed no new law; [`Config::shape`] is the whole of it.
pub(crate) fn merge_base(site: toml::Value) -> Result<toml::Value> {
    merge_base_traced(site, &mut Trace::off())
}

/// The same merge with a recorder attached — the only entry point
/// `--effective` has, so what it prints is what the load path did. See
/// [`crate::config::effective`].
pub(crate) fn merge_base_traced(site: toml::Value, t: &mut Trace) -> Result<toml::Value> {
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
pub(crate) fn project(
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
pub(crate) fn note_table(
    t: &mut Trace,
    path: &mut Vec<String>,
    shape: &Shape,
    v: &toml::Value,
    prov: Prov,
) {
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
pub(crate) fn merge_table(
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
pub(crate) fn merge_to_depth(
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
pub(crate) fn identity(source: Option<&str>, name: Option<&str>) -> Option<String> {
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
pub(crate) fn describe_collection(name: &str, c: &Collection) -> String {
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
pub(crate) fn brace_alternatives(pat: &str) -> Vec<String> {
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

/// Display strings for the i18n axis (§6f). Member identity lives on the
/// axis named by [`I18nCfg::axis`]; this table is only names and shared strings.
/// Canonical membership is [`Axis::canonical`] on that axis — not cached here.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct I18nCfg {
    /// Which declared axis drives pairing, hreflang, switcher, and
    /// single-locale indexes. Default `"locale"` matches base.toml; a site
    /// that renames the axis sets this to match. Inert until that axis exists
    /// under `[axes.*]`.
    #[serde(default = "default_i18n_axis")]
    pub axis: Option<String>,
    /// Display names for axis members (`fr = "Français"`); a missing entry
    /// falls back to the member code. Keyed by member, so every key must be a
    /// declared axis member — a name for an undeclared member labels nothing,
    /// and is a load error (C4a).
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

impl Default for I18nCfg {
    fn default() -> Self {
        Self {
            // Same as `#[serde(default = "default_i18n_axis")]` — Config's
            // `#[serde(default)]` on `i18n` calls this when `[i18n]` is absent.
            axis: default_i18n_axis(),
            names: BTreeMap::new(),
            strings: BTreeMap::new(),
        }
    }
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

fn default_i18n_axis() -> Option<String> {
    Some("locale".to_string())
}

impl I18nCfg {
    /// The label a locale wears in the translations axis.
    pub fn name_of<'a>(&'a self, locale: &'a str) -> &'a str {
        self.names.get(locale).map(String::as_str).unwrap_or(locale)
    }

    /// A named string (§6f) for a member: the global `[i18n.strings]` entry
    /// if declared, else the engine built-in. `canonical` is the pairing
    /// axis's first member — the fallback key for a per-member map.
    pub fn string<'a>(&'a self, key: &str, member: &str, canonical: &str) -> &'a str {
        if let Some(s) = self.strings.get(key) {
            return s.get(member, canonical);
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
    pub fn text<'a>(&'a self, s: &'a LocalizedStr, member: &str, canonical: &str) -> &'a str {
        match s.reference() {
            Some(key) => self.string(key, member, canonical),
            None => s.get(member, canonical),
        }
    }
}

/// `[schema]` (§5b): field declarations plus the two engine consumers that
/// name those fields — embeddings text and search indexing.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SchemaBag {
    #[serde(default)]
    pub embeddings: EmbeddingsSchema,
    #[serde(default)]
    pub search: SearchSchema,
    /// `draft = { type = "bool" }`, `tags = { type = "list" }`, …
    #[serde(flatten)]
    pub fields: toml::Table,
}

/// `[schema.embeddings]`: the text a row embeds as. `{body}` is the markdown
/// body; every other `{name}` is a row field (lists join with `", "`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmbeddingsSchema {
    #[serde(default)]
    pub string: String,
}

/// `[schema.search]`: which row fields (plus `title` / `body`) feed the
/// search index. List fields contribute each value; scalars contribute once.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchSchema {
    #[serde(default)]
    pub fields: Vec<String>,
}

impl crate::shape::Shaped for SchemaBag {
    fn shape() -> crate::shape::Shape {
        // Same merge law the bare table had: per-key atom replace under [schema].
        crate::shape::Shape::Map(Box::new(crate::shape::Shape::Atom))
    }
}

/// `[html]` (§4e): the parts of the document skeleton that are a site's
/// decision rather than the engine's — head tags and attributes on `<html>` /
/// `<body>`.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HtmlCfg {
    #[serde(default)]
    pub head: HeadCfg,
    /// `<html …>` attributes (`[html.html.attribute]`). Document language
    /// lives here (`lang = 'locale'`), not as engine vocabulary.
    #[serde(default)]
    pub html: AttrCfg,
    /// `<body …>` attributes (`[html.body.attribute]`).
    #[serde(default)]
    pub body: AttrCfg,
}

/// One element's attribute map: name → §5f text expression. Empty result
/// omits the attribute (§5e rule 2 one layer up), same as a head meta.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AttrCfg {
    #[serde(default)]
    pub attribute: BTreeMap<String, String>,
}

/// One `[html.head.*]` value: a single CEL text expression, or an expand that
/// emits one tag per member of a candidate pool (§4e's variable-length residue).
///
/// ```toml
/// canonical = 'site.url + url'                          # single
/// alternate = { from = "axis.locale", hreflang = 'locale', href = 'site.url + url' }
/// ```
///
/// `from` is the same word a relation spells: a pool name. Attributes beside
/// it are CEL expressions evaluated once per member. A table-spelled atom so
/// Descend(3) replaces the whole entry rather than composing its fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum HeadEntry {
    Expr(String),
    Expand(HeadExpand),
}

impl HeadEntry {
    /// The single-expression form, when this is not an expand.
    pub fn as_expr(&self) -> Option<&str> {
        match self {
            HeadEntry::Expr(s) => Some(s.as_str()),
            HeadEntry::Expand(_) => None,
        }
    }
}

impl PartialEq<&str> for HeadEntry {
    fn eq(&self, other: &&str) -> bool {
        self.as_expr() == Some(*other)
    }
}

impl PartialEq<str> for HeadEntry {
    fn eq(&self, other: &str) -> bool {
        self.as_expr() == Some(other)
    }
}

impl PartialEq<String> for HeadEntry {
    fn eq(&self, other: &String) -> bool {
        self.as_expr() == Some(other.as_str())
    }
}

/// An expand: one tag per member of `from`, attributes evaluated as CEL.
#[derive(Debug, Clone, Deserialize)]
pub struct HeadExpand {
    /// Candidate pool — relation/`axis.*` name, same vocabulary as a
    /// relation's `from`.
    pub from: String,
    /// Attribute → CEL text expression (`href`, `hreflang`, `type`, …).
    #[serde(flatten)]
    pub attrs: BTreeMap<String, String>,
}

impl Shaped for HeadEntry {
    fn shape() -> Shape {
        Shape::TableAtom
    }
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
    pub meta: BTreeMap<String, HeadEntry>,
    /// `<meta property="KEY" content="…">`. A separate table because the
    /// ATTRIBUTE is different, not the mechanism: Open Graph and the
    /// `article:*` family are `property=`, and folding them into `meta` would
    /// mean the engine deciding which name takes which attribute — the exact
    /// kind of knowledge §4e is removing.
    #[serde(default)]
    pub property: BTreeMap<String, HeadEntry>,
    /// `<link rel="KEY" href="…">`. Same shape one element over. An expand
    /// under a key (typically `alternate`) emits one link per pool member,
    /// which is how hreflang left the engine (§4e residue).
    #[serde(default)]
    pub link: BTreeMap<String, HeadEntry>,
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
}

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
    /// The extractor's **default for this collection's rules** (§4). Same list
    /// law as `route`: patterns tried in order; `{axis:NAME}` spends a
    /// declared axis into the path (suffix `{stem}.{axis:locale}` or prefix
    /// `{axis:locale}/{stem}`); a shorter pattern without that token is the
    /// canonical member. Date tokens ride the same matcher.
    ///
    /// A rule declaring its own list overrides this for the rows it governs.
    #[serde(default)]
    pub file: Vec<String>,
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
    /// Views that own archive routes for list fields (q32): pills for a
    /// field render URLs from that view's route template. Key = field name
    /// (`tags`, `course`, ...); value = view name. Optional: a unique view
    /// grouped by the field is found on its own; none at all = unlinked pills.
    #[serde(default)]
    pub archives: BTreeMap<String, String>,
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
    /// Key extraction from a file's stem, for the rows this rule governs.
    /// Tried in order; the first pattern that describes the stem supplies its
    /// tokens (`{year}`, `{slug}`, `{stem}`, `{axis:NAME}`, …) and the
    /// logical stem everything downstream treats as identity.
    ///
    /// Absent, the collection's [`Collection::file`] is the default. A rule
    /// needs none: a route spending only path tokens (`/{dir}/{stem}/`) works
    /// without an extractor.
    #[serde(default)]
    pub file: Vec<String>,
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
///
/// Unknown top-level keys are captured in [`View::route_fields`] and checked
/// against `[schema]` at validate — so `noindex = true` is a schema field,
/// not engine vocabulary. (`deny_unknown_fields` cannot coexist with that
/// flatten.)
#[derive(Debug, Deserialize)]
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
    /// §6f pairing-axis partition, DEFAULT-ON: a materializing row-query view
    /// partitions per member of `[i18n] axis` (each member's rows, member-
    /// prefixed routes when templates spend the axis; a member with no rows
    /// materializes nothing). `"default"` opts out (canonical only); `"*"`
    /// states the default explicitly. Star views never multiply; embedded
    /// views follow their embedding page (pending).
    pub partition: Option<String>,
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
    /// Schema-declared values this route answers with (§4e). Spelled as
    /// ordinary top-level keys (`noindex = true`) — the same names rows
    /// use, so `[html.head.meta]` expressions and `where` clauses see one
    /// vocabulary. Validated against `[schema]` (base.toml ships the flag
    /// family); an undeclared key is a load error.
    #[serde(default, flatten)]
    pub route_fields: BTreeMap<String, toml::Value>,
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
