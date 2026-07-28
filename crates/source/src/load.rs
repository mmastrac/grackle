//! The load: one walk of the site, and the rows it produces.
//!
//! Reads the tree, applies collection rules, routes every row, and hands the
//! result to `SiteDb::insert_rows` — the only way into the database.

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use globset::{Glob, GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use grackle_db::template;
use grackle_model::{AxisMember, Kind, Route, RouteKind, Row, SiteDb};

use crate::config::{Collection, Config};
use crate::filename::{self, FileKey, FilenameFormat};
use crate::markers::{Defaults, Markers};
use crate::schema::{self, Schemas};
use crate::store::{self, RawRow};

/// Front matter's `date:`. `YYYY-MM-DD`; a bare `YYYY-MM` means the first of
/// that month.
fn front_matter_date(raw: &str, path: &Path) -> Result<NaiveDate> {
    let s = raw.trim();
    let parsed = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d"));
    parsed.with_context(|| {
        format!(
            "{}: date: {s:?} is not YYYY-MM-DD (or YYYY-MM)",
            path.display()
        )
    })
}

struct CompiledRule<'a> {
    matcher: GlobMatcher,
    route: &'a [String],
    front_matter: Option<bool>,
    on_demand: bool,
    pattern: &'a str,
    /// This rule's own extractors, compiled. Empty means "the collection's",
    /// which [`apply_rules`] resolves — the rule holds what it DECLARED, so
    /// first-writer-wins can tell silence from a list.
    formats: Vec<FilenameFormat>,
    defaults: &'a BTreeMap<String, toml::Value>,
    /// From the base config rather than the site's own file (§4d).
    inherited: bool,
    /// Whether the walk ever found this rule eligible for a row — the corpus
    /// answering the glob. Written as the rows go past, read once afterwards
    /// by [`dead_rules`]; a `Cell` because the rule list is shared (`&[…]`)
    /// across a walk that only ever visits one row at a time.
    governed: Cell<bool>,
}

fn compile_rules(c: &Collection) -> Result<Vec<CompiledRule<'_>>> {
    c.rules
        .iter()
        .map(|r| {
            Ok(CompiledRule {
                // Case-INSENSITIVE, and for every rule of every scope
                // (IO.md I7a): a `match` glob names a KIND of file and the
                // shift key is not part of the kind. Objects forced it —
                // their membership is a glob now (below) where it used to be
                // a lowercased extension scan, and `after-theme-hack.PNG` is
                // the corpus row that tells the two apart.
                matcher: GlobBuilder::new(&r.pattern)
                    .case_insensitive(true)
                    .build()
                    .with_context(|| format!("bad rule glob {:?}", r.pattern))?
                    .compile_matcher(),
                route: &r.route,
                front_matter: r.front_matter,
                on_demand: r.on_demand.unwrap_or(false),
                pattern: r.pattern.as_str(),
                formats: compile_formats(&r.filename_formats)?,
                defaults: &r.defaults,
                inherited: r.inherited,
                governed: Cell::new(false),
            })
        })
        .collect()
}

/// One declared `filename_formats` list, compiled. Once per rule (and once
/// per collection, for the default), never per row.
fn compile_formats(formats: &[String]) -> Result<Vec<FilenameFormat>> {
    formats.iter().map(|f| FilenameFormat::compile(f)).collect()
}

/// The site's own rules that governed no row — DESIGN.md §4's promised
/// **"Dead rule (matches zero rows) → warning"**, which nothing provided
/// before MERGE.md C3.
///
/// A warning and not an error, on the doc's word: a rule may be written for
/// content that has not landed yet, and an empty `_posts/` is documented as
/// legal (§4d). It reports what the corpus said, so it is computed here —
/// after the walk — rather than guessed from the glob's text.
///
/// **Only rules the site WROTE are reported**, which is what
/// [`crate::config::Rule::inherited`] is for. The base's globs are not the
/// author's to fix and go dead for perfectly ordinary reasons: `examples/
/// minimal` has no `index.md`, so the base's `**/index.{html,md}` matches
/// nothing there, and a site with no `_posts/` never asked for a rule over it.
/// Warning about those would put a permanent, unfixable line on every
/// base-inheriting site, which is how a warning stops being read.
///
/// "Matched zero rows" means *eligible for* zero rows: a rule gated
/// `front_matter = false` in a tree of nothing but pages governs nothing,
/// whatever its glob would say on its own.
///
/// A collection that produced NO rows reports nothing either (`found`), for
/// the same reason and one level up: a rule is dead relative to a corpus, and
/// an absent `_posts/` (or a site with no images) is a statement about the
/// source, not about any one glob. Three warnings for one missing directory
/// would bury the case this is for.
fn dead_rules(collection: &str, rules: &[CompiledRule], found: usize) -> Vec<String> {
    if found == 0 {
        return Vec::new();
    }
    rules
        .iter()
        .filter(|r| !r.inherited && !r.governed.get())
        .map(|r| {
            format!(
                "collection {collection}: the rule `match = {:?}` governs no rows \
                 — nothing in the site matches it. Fix the glob or delete the rule.",
                r.pattern
            )
        })
        .collect()
}

/// The engine-fallback title rung (IO.md §1): a name implied from the slug.
///
/// One derivation, shared by both loaders, and deliberately the dumbest one
/// that could work — the posts loader has spelled it exactly this way since
/// before the ledger, and grack.com's caret draft has been publishing
/// `<title>why is a cursor called a caret</title>` from it. Anything prettier
/// (title-casing, an acronym table) would move published bytes on a live page
/// the moment it landed, so the string is PINNED and a test says so.
///
/// It is the bottom rung and nothing else: front matter beats it, and so does
/// any rule or marker default, because both are read before it is.
fn implied_title(slug: &str) -> String {
    slug.replace('-', " ")
}

/// The degenerate row's warning (IO.md §1, I7c) — a nudge, never an error.
///
/// A file with no front-matter block that rules nonetheless send through a
/// document shell renders anyway, with the title above. That is a softening of
/// the identity contract and not a licence, so the build says so once per row
/// and keeps going: the author wanted a page, they have one, and the fix is
/// three characters.
fn degenerate_warning(rel: &Path, shell: &str, title: &str) -> String {
    format!(
        "{}: no front-matter block, but a rule sends it through the \
         `{shell}` shell — so it renders as a degenerate row (IO.md §1) whose \
         title is implied from its slug: {title:?}. Add a `---` block to give \
         it identity, or route it `shell = \"raw\"` to ship it as bytes.",
        rel.display()
    )
}

// D1's `declared_and_unread` lived here and went with its subject (MERGE.md
// F1): `bucket` was the only key it reported, and a key that no longer parses
// needs no warning — `deny_unknown_fields` names it at the line that wrote it,
// which is strictly the better error. If a second declared-and-ignored key ever
// turns up, the shape is one `filter_map` over `cfg.collections` and D1's §6
// note describes it; nothing here is worth keeping warm for it.

/// What the rule cascade decided for one row.
struct Routing<'a> {
    templates: &'a [String],
    /// The glob of the rule that supplied `templates` — carried so a routing
    /// error can name the rule the reader has to edit, not just the template
    /// text (IO.md I6). Empty when no rule supplied a route.
    pattern: &'a str,
    /// The extractors in force for this row: the first matching rule that
    /// declared any, else the collection's own list.
    formats: &'a [FilenameFormat],
    /// The winning route rule was on-demand: compute the URL, emit no route
    /// until something references it.
    on_demand: bool,
    /// Every on-demand rule that COVERED this path, winner or not. Two is a
    /// config error: an on-demand rule declares where a class of files
    /// lives, and two declarations for one file is ambiguous rather than a
    /// cascade. Checked per file against the real corpus, the way §4's dead
    /// rule is.
    on_demand_cover: Vec<&'a str>,
    defaults: BTreeMap<&'a str, &'a toml::Value>,
}

/// First-writer-wins per key (DESIGN.md §4).
fn apply_rules<'a>(
    rules: &'a [CompiledRule<'a>],
    // The collection's own `filename_formats`: the default its rules inherit,
    // read only where no matching rule declared a list of its own (§4).
    collection_formats: &'a [FilenameFormat],
    rel: &Path,
    has_front_matter: bool,
) -> Routing<'a> {
    let mut templates: &[String] = &[];
    let mut pattern: &str = "";
    let mut formats: Option<&[FilenameFormat]> = None;
    let mut on_demand = false;
    let mut on_demand_cover: Vec<&str> = Vec::new();
    let mut defaults: BTreeMap<&str, &toml::Value> = BTreeMap::new();
    for rule in rules {
        if let Some(want) = rule.front_matter {
            if want != has_front_matter {
                continue;
            }
        }
        if !rule.matcher.is_match(rel) {
            continue;
        }
        // Past both gates: this rule governs this row, whether or not it is
        // the one that wins the route. That is what keeps a rule shadowed by
        // a nearer one (it still fills defaults) out of `dead_rules`.
        rule.governed.set(true);
        if rule.on_demand && !rule.route.is_empty() {
            on_demand_cover.push(rule.pattern);
        }
        if templates.is_empty() && !rule.route.is_empty() {
            templates = rule.route;
            pattern = rule.pattern;
            on_demand = rule.on_demand;
        }
        // First writer wins here too, and deliberately independent of which
        // rule won the ROUTE: `filename_formats` is a key like any other, so a
        // rule that names the extractor for a subtree governs it whether or
        // not it is also the rule that says where those rows land.
        if formats.is_none() && !rule.formats.is_empty() {
            formats = Some(&rule.formats);
        }
        for (k, v) in rule.defaults {
            defaults.entry(k.as_str()).or_insert(v);
        }
    }
    Routing {
        templates,
        pattern,
        formats: formats.unwrap_or(collection_formats),
        on_demand,
        on_demand_cover,
        defaults,
    }
}

/// At most one on-demand rule may cover a path (§4).
fn check_on_demand_cover(rel: &Path, r: &Routing) -> Result<()> {
    if r.on_demand_cover.len() > 1 {
        anyhow::bail!(
            "{}: covered by {} on-demand rules — an on-demand rule declares \
             where a class of files lives, so two of them covering one file \
             is ambiguous:\n  {}",
            rel.display(),
            r.on_demand_cover.len(),
            r.on_demand_cover.join("\n  ")
        );
    }
    Ok(())
}

/// Precedence (§4b): front matter > nearest marker > rule. Markers go in
/// first so `or_insert` cannot let a rule override them.
fn merged_defaults<'a>(
    marker_defaults: &'a Defaults,
    rule_defaults: BTreeMap<&'a str, &'a toml::Value>,
) -> BTreeMap<&'a str, &'a toml::Value> {
    let mut out: BTreeMap<&str, &toml::Value> = BTreeMap::new();
    for (k, v) in marker_defaults {
        out.insert(k.as_str(), v);
    }
    for (k, v) in rule_defaults {
        out.entry(k).or_insert(v);
    }
    out
}

/// What a row wears, after the cascade: the fields the engine reads off a row
/// by name (`schema::CASCADE`).
#[derive(Debug)]
struct Cascaded {
    theme: Option<String>,
    shell: Option<String>,
    layout: Option<String>,
    toc: bool,
}

/// Read the engine's four off a row's RESOLVED fields — one spelling, so posts
/// and tree rows cannot drift apart on which fields cascade.
///
/// This is no longer a cascade of its own (MERGE.md C1). It used to reach into
/// raw TOML with `as_str()`/`as_bool()`, which is why `defaults = { toc =
/// "true" }` was a silent `false` and `theme = 1` silently vanished. The
/// cascade is `schema::cascade_front` (nearest) then `schema::apply_defaults`
/// (markers, then rules), the same two calls every other declared key takes;
/// what is left here is the typed read, plus the one vocabulary the engine
/// closes.
///
/// The values stay in `fields` as well as landing on the row's named fields:
/// they are declared, so a `where`, an `order_by` or a fold's route may
/// name them, and a name that type-checks against nothing readable is the
/// worse failure (§4e).
fn cascade(fields: &schema::Fields, whose: &Path) -> Result<Cascaded> {
    let worn = |key: &str| match fields.values.get(key) {
        Some(grackle_db::Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    // A typo'd shell would silently render the wrong tier — the failure mode
    // this codebase keeps finding. Closed vocabulary, checked at load.
    //
    // The vocabulary is now the WHOLE axis (IO.md §4, I2), not a tier ladder of
    // its own: one row's worth of it is the map family, and a fold name here is
    // an arity error naming what that fold eats rather than an unknown word.
    let shell = worn("shell");
    if let Some(sh) = shell.as_deref() {
        crate::shell::check_row(sh, whose)?;
    }
    Ok(Cascaded {
        // Theme is chosen per row (§5a): a marker or a rule can restyle a
        // subtree, and the row's own front matter still beats both — the
        // ladder is `fields`', not this read's.
        theme: worn("theme"),
        shell,
        layout: worn("layout"),
        toc: matches!(
            fields.values.get("toc"),
            Some(grackle_db::Value::Bool(true))
        ),
    })
}

/// Rung 0's other half (§2, MERGE.md E1): the selected profile's forced fields,
/// written into EVERY route.
///
/// **This is the load-bearing half.** A head expression is evaluated against a
/// ROW when the surface has one and against the ROUTE when it does not, so a
/// force that reached only rows would leave every listing, archive and tag page
/// saying the opposite of the documents beneath it — `/blog/` in a search index
/// under the drafts profile, which is precisely the leak §4a exists to close.
/// The row half is not a substitute: a view route has no row to read.
///
/// **Placement: after every route exists, before anything filters routes**
/// (MERGE.md R6). It runs once materialization and `build_views`/
/// `build_pool_folds` have minted the last route, and the engine's one
/// `db.routes.select` — `views::resolve_pool_folds` — runs at the end of
/// `load`, so an all-outputs fold's `where` reads FORCED routes. That is the law and
/// not an accident of ordering: rung 0 sits above every reader, the ones that
/// SELECT as well as the ones that SAY, because §4a's fence puts "which rows
/// the views admit" inside profile territory in the first place. The row half
/// is already there by the same law (`schema::force` runs before any view
/// materializes), so the two pools answer one question the same way.
///
/// E1 placed this call here and read it the other way round — "rung 0 says
/// what a surface SAYS, not what a query SELECTS" — on the strength of
/// `build_star_views` running one line above (`build_pool_folds` since IO.md
/// I3). But that pass only *mints* the fold's route; it filters nothing.
/// Nothing between the two calls
/// reads a route field, so the sentence never described the code.
///
/// The one route this does not reach: an on-demand row published by
/// `build::materialize_referenced`, which mints its route after `load` has
/// returned. Those are `RouteKind::Object` byte publishes with no head, and
/// the route pool resolved before they existed, so no reader of theirs is a
/// reader of rung 0 — stated rather than fixed, as E1 stated it.
///
/// The types come from the site vocabulary (`Schemas::declared`) rather than
/// from a row's resolved schema, because a route is not in a directory — it is
/// the same table `Schemas::declared_schema` builds a route's own filter
/// environment from.
fn force_route_fields(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    if cfg.forced.is_empty() {
        return Ok(());
    }
    let declared = schemas.declared();
    let mut values: Vec<(String, grackle_db::Value)> = Vec::new();
    for (name, v) in &cfg.forced {
        // `Config::check_profiles` already refused a name the site's `[schema]`
        // does not declare, and `declared()` is a superset of that table — so
        // this cannot fire, and it is a lookup rather than an `unwrap` for the
        // reason `schema::force`'s is.
        let Some(ty) = declared.get(name.as_str()) else {
            bail!("the profile forces {name:?}, which no schema declares");
        };
        // "the profile", the same subject `schema::force` names one layer
        // over — with no file to blame, because a route is not in the tree.
        values.push((name.clone(), schema::typed(*ty, name, v, "the profile")?));
    }
    for r in db.routes.iter_mut() {
        for (name, value) in &values {
            r.fields.insert(name.clone(), value.clone());
        }
    }
    Ok(())
}

/// The axes a rule's template(s) opt a row into (q53 step 2): a `{theme}` (or
/// `{axis:theme}`) segment is what spends the theme axis, and locale via
/// `{axis:locale}`. A rule that writes the segment decides the row lands there,
/// so `[axes.*]` declares only values and a field. The names drive the product;
/// locale is excluded here (it is the row's own, not a product dimension).
fn row_axes(cfg: &Config, templates: &[String]) -> Vec<grackle_model::RowAxis> {
    cfg.axes
        .keys()
        .filter(|n| templates.iter().any(|t| spends(t, n)))
        .map(|name| grackle_model::RowAxis { name: name.clone() })
        .collect()
}

/// Whether a template spends an axis: `{name}` or the namespaced `{axis:name}`.
/// Locale also answers to bare `{locale}`.
///
/// **The one spend test.** Every reader of a route template asks this question —
/// the materializer, the view loader's declared-but-never-spent check, the link
/// resolver — and each used to ask it with a `format!("{{{name}}}")` of its own,
/// which is how a route written `{axis:theme}` came to load in one place and
/// fail in another (MERGE.md C5). `pub` for that reason: a second spelling of
/// this predicate is the bug, not the convenience.
pub fn spends(tmpl: &str, axis: &str) -> bool {
    tmpl.contains(&format!("{{{axis}}}"))
        || tmpl.contains(&format!("{{axis:{axis}}}"))
        || (axis == "locale" && tmpl.contains("{locale}"))
}

/// Spend one axis's segment: the dual of `spends`, and it must accept exactly
/// the spellings `spends` recognizes or a template that passes the check comes
/// back with a placeholder still in it.
pub fn fill_axis(tmpl: &str, axis: &str, value: &str) -> String {
    tmpl.replace(&format!("{{{axis}}}"), value)
        .replace(&format!("{{axis:{axis}}}"), value)
}

/// One axis coordinate of a materialized route: the axis, its value, and whether
/// that value is canonical — which is what a shorter template may omit.
pub struct Coord<'a> {
    pub axis: &'a str,
    pub value: &'a str,
    pub canonical: bool,
}

/// Pick the shortest template whose spent axes cover every NON-canonical coord,
/// and fill it (§6f, the default-axis case). A canonical coord's segment drops
/// when a shorter template omits it — `["/{theme}/{axis:locale}/", "/{theme}/",
/// "/"]` lands the all-canonical member at `/`. Locale that no template spends
/// falls back to a prefix, which is the behavior a config without `{axis:locale}`
/// has always had. Errors only if no template covers a required set, which the
/// fullest template always does unless the templates are pathologically split.
pub fn select_path(templates: &[String], coords: &[Coord]) -> Result<String> {
    let loc_spendable = templates.iter().any(|t| spends(t, "locale"));
    let required: Vec<&str> = coords
        .iter()
        .filter(|c| !c.canonical && (c.axis != "locale" || loc_spendable))
        .map(|c| c.axis)
        .collect();
    // The shortest template that still spends everything required; ties keep
    // declaration order, so the first-listed shape wins.
    let tmpl = templates
        .iter()
        .filter(|t| required.iter().all(|r| spends(t, r)))
        .min_by_key(|t| coords.iter().filter(|c| spends(t, c.axis)).count())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no route template spends {required:?} together; add one that does\n  \
                 templates: {templates:?}"
            )
        })?;
    let mut url = tmpl.clone();
    for c in coords.iter().filter(|c| spends(tmpl, c.axis)) {
        url = fill_axis(&url, c.axis, c.value);
    }
    if let Some(loc) = coords.iter().find(|c| c.axis == "locale") {
        if !loc.canonical && !spends(tmpl, "locale") {
            url = format!("/{}{url}", loc.value);
        }
    }
    Ok(tidy(url))
}

fn build_globset(pats: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in pats {
        b.add(Glob::new(p).with_context(|| format!("bad glob {p:?}"))?);
    }
    Ok(b.build()?)
}

/// `{dir}`, `{stem}`, `{name}`, `{path}`, `{ext}` — the tokens a path carries
/// on its own. Every row has them, whichever collection read it.
fn path_tokens(rel: &Path, k: &str) -> Option<String> {
    let path = rel.to_string_lossy().to_string();
    match k {
        "path" => Some(path),
        "dir" => Some(
            rel.parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        ),
        "stem" => rel.file_stem().map(|s| s.to_string_lossy().to_string()),
        "name" => rel.file_name().map(|s| s.to_string_lossy().to_string()),
        "ext" => rel.extension().map(|s| s.to_string_lossy().to_string()),
        _ => None,
    }
}

/// The names the path always supplies, for the error that lists them.
const PATH_TOKENS: &[&str] = &["path", "dir", "stem", "name", "ext"];

/// **One route-token supplier** (IO.md I6, DESIGN.md q51): everything a rule's
/// route template may spend for one row.
///
/// Three sources, and the point of the type is that they are one table:
///
/// - **the path**, always — `{path}`, `{dir}`, `{stem}`, `{name}`, `{ext}`,
///   relative to what the rule's own glob matches (collection-relative, so
///   `match = "rust/**"` and `route = "/{dir}/{stem}/"` read the same words
///   in `_posts` as they do in the tree);
/// - **the extractor**, where a `filename_formats` entry described the stem —
///   `{year}`, `{month}`, `{day}`, `{slug}`, or whichever of them the format
///   named;
/// - **the axes**, which are not filled here at all: a declared axis (and
///   `locale`) is handed back as its own placeholder, for `select_path` and
///   the materializer to spend per member (q53).
///
/// Before this type there were two suppliers with no overlap: the tree offered
/// path tokens and the posts loader offered date/slug inline, so a file in a
/// posts scope could not route by its directory and a tree page could not
/// route by a date in its name. Both halves now reach every rule.
struct RouteTokens<'a> {
    cfg: &'a Config,
    /// The path the rule matched — see the doc above on why it is that one.
    rel: &'a Path,
    /// The row's resolved date: front matter first where the loader has read
    /// it, else the extractor's. `None` is a row with no date at all, and the
    /// three date tokens are then unfillable — which is the error below.
    date: Option<NaiveDate>,
    /// What the extractor yielded, for the tokens a date does not cover.
    key: Option<&'a FileKey>,
    /// The row's slug: the extractor's where a format named one, else the
    /// stem. Always fillable, on every row — which is the pre-I6 posts
    /// behaviour made general rather than a new promise.
    slug: &'a str,
}

impl RouteTokens<'_> {
    /// Resolve one token, or `None` if this row cannot fill it.
    fn get(&self, k: &str) -> Option<String> {
        if let Some(v) = path_tokens(self.rel, k) {
            return Some(v);
        }
        match k {
            // The resolved date first: front matter beats the filename (§4b),
            // so `{year}` must read what the row wears rather than what its
            // name said. The extractor is the fallback for a format that named
            // a part without naming a whole date.
            "year" => self
                .date
                .map(|d| d.format("%Y").to_string())
                .or_else(|| self.key?.year.map(|y| y.to_string())),
            "month" => self
                .date
                .map(|d| d.format("%-m").to_string())
                .or_else(|| self.key?.month.map(|m| m.to_string())),
            "day" => self
                .date
                .map(|d| d.format("%-d").to_string())
                .or_else(|| self.key?.day.map(|d| d.to_string())),
            "slug" => Some(self.slug.to_string()),
            // An axis placeholder is spent per member, not here (q53).
            k => {
                let (_, bare) = template::classify(k);
                (self.cfg.axes.contains_key(bare) || (bare == "locale" && self.cfg.i18n.enabled()))
                    .then(|| format!("{{{k}}}"))
            }
        }
    }

    /// Render one rule's route templates for one row, refusing any template
    /// that spends a token this row cannot fill.
    ///
    /// The refusal is DESIGN.md §4's constraint, generalized off the one shape
    /// it used to have (a dateless post under a dated template): the supplier
    /// knows what it can fill, so "this row cannot go there" is one question
    /// asked in one place, for tree rows and posts alike.
    fn render_all(&self, tmpls: &[String], pattern: &str, path: &Path) -> Result<Vec<String>> {
        tmpls
            .iter()
            .map(|tmpl| {
                self.check(tmpl, pattern, path)?;
                template::render(tmpl, |k| self.get(k))
                    .map(tidy)
                    .with_context(|| format!("routing {}", path.display()))
            })
            .collect()
    }

    fn check(&self, tmpl: &str, pattern: &str, path: &Path) -> Result<()> {
        let named =
            template::tokens(tmpl).with_context(|| format!("routing {}", path.display()))?;
        let unfillable: Vec<String> = named
            .iter()
            .filter(|t| self.get(t).is_none())
            .map(|t| t.to_string())
            .collect();
        if unfillable.is_empty() {
            return Ok(());
        }
        let dated: Vec<&String> = unfillable
            .iter()
            .filter(|t| matches!(t.as_str(), "year" | "month" | "day"))
            .collect();
        // The date case keeps its own sentence, because "unfillable" is the
        // mechanism and "this file carries no date" is the diagnosis.
        if dated.len() == unfillable.len() {
            bail!(
                "{} has no date (its filename matches none of the \
                 filename_formats in force, and it declares no `date:`), but the \
                 rule `match = {pattern:?}` routes it to {tmpl:?}, which requires \
                 {{{}}}",
                path.display(),
                dated
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join("}, {")
            );
        }
        let axes: String = match self.cfg.axes.is_empty() {
            true => String::new(),
            false => format!(
                ", and the declared axes ({})",
                self.cfg
                    .axes
                    .keys()
                    .map(|a| a.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        bail!(
            "{}: the rule `match = {pattern:?}` routes it to {tmpl:?}, which \
             spends {{{}}} — nothing supplies that token. A route may spend the \
             path tokens ({}), the row's {{slug}}, the date tokens (year, month, \
             day) wherever `filename_formats` or a `date:` gives the row a \
             date{axes}.",
            path.display(),
            unfillable.join("}, {"),
            PATH_TOKENS.join(", ")
        )
    }
}

/// Collapse `//` that arise when `{dir}` is empty at the root.
fn tidy(url: String) -> String {
    let mut out = String::with_capacity(url.len());
    let mut prev_slash = false;
    for ch in url.chars() {
        if ch == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        out.push(ch);
    }
    if !out.starts_with('/') {
        out.insert(0, '/');
    }
    out
}

/// Read one posts collection's rows. Indexing is deliberately NOT here:
/// several collections can contribute to the one posts table (`_posts` and
/// `_drafts`), and an index built per collection would see only part of the
/// corpus — `by_url` could not detect a collision between them, and `order`
/// would restart per source.
fn read_posts(
    cfg: &Config,
    name: &str,
    c: &Collection,
    markers: &Markers,
    schemas: &Schemas,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Row>, f64)> {
    // Bound here because the row loop shadows `name` with the post's own
    // path identity — silently, since both are strings.
    let collection = name.to_string();
    let root = cfg.root();
    let source_rel = PathBuf::from(
        c.source
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("collection {name} has kind=posts but no source"))?,
    );
    let source = root.join(&source_rel);

    let t0 = std::time::Instant::now();
    let raws: Vec<RawRow> = store::load_dir(&source, &["md", "markdown"])?;
    let read_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // The collection's list is the DEFAULT its rules inherit (§4, IO.md I6),
    // not a requirement: a posts scope whose rules route by path tokens
    // (`/{dir}/{stem}/`) needs no extractor at all, and the pre-I6 refusal —
    // "kind=posts but no filename_formats" — would have refused exactly the
    // config q51 exists to make possible. What replaces it is per row and per
    // template: a route that spends a date the row does not have is the error,
    // wherever that route came from (`RouteTokens::check`).
    let collection_formats = compile_formats(&c.filename_formats)?;
    let rules = compile_rules(c)?;

    let mut rows: Vec<Row> = Vec::with_capacity(raws.len());
    for raw in raws {
        // §6f: the path selector strips the locale first, so filename
        // parsing, rules and routing all see the logical path — a
        // translation rides the same machinery as its original.
        let (logical_rel, locale) = cfg.i18n.split(&raw.rel);
        let stem: String = logical_rel
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        // `logical` keeps its extension, matching the tree side — where the
        // convention is config-visible (`content = "recipes/index.md"`).
        // ROOT-relative too, like `rel`: collection-relative would collide a
        // post at `2020/x.md` with a tree page at `2020/x.md`.
        let logical = source_rel.join(&logical_rel).to_string_lossy().to_string();

        // `true`, not `raw.front_mattered`, and since I7c that is a claim this
        // loader makes rather than a fact it reads: a `front_matter = ` rule
        // key selects on the row's IDENTITY (§4), and this hands the cascade a
        // constant. It is byte-inert on the corpus — no posts rule of any site
        // writes the key — and it is deliberately left alone here, because the
        // shape that fixes it is the one walk: **I7d**, where the two loaders
        // become one and there is a single answer to hand over. Until then a
        // blockless draft is offered to `front_matter = true` rules it does not
        // satisfy, and the row's own column (`front_mattered`, below) is the
        // one that tells the truth.
        //
        // The rules run before the extractor because they are what says which
        // extractor this row has (IO.md I6).
        let routing = apply_rules(&rules, &collection_formats, &logical_rel, true);
        check_on_demand_cover(&logical_rel, &routing)?;
        let (route_tmpls, rule_defaults) = (routing.templates, routing.defaults);
        let key = filename::extract(routing.formats, &stem);
        let from_name = match key.as_ref().and_then(|k| k.ymd()) {
            Some((y, m, d)) => Some(NaiveDate::from_ymd_opt(y, m, d).with_context(|| {
                format!(
                    "{} has an impossible date in its filename",
                    raw.path.display()
                )
            })?),
            None => None,
        };
        // Front matter beats the filename, the same precedence every other
        // field has (§4b).
        let date = match &raw.front.date {
            Some(s) => Some(front_matter_date(s, &raw.path)?),
            None => from_name,
        };
        let slug = key
            .as_ref()
            .and_then(|k| k.slug.clone())
            .unwrap_or_else(|| stem.clone());
        let root_rel = raw
            .path
            .strip_prefix(&root)
            .unwrap_or(&raw.rel)
            .to_path_buf();
        let marker_defaults = markers.defaults_for(&root_rel);
        let defaults = merged_defaults(&marker_defaults, rule_defaults);
        // Governance follows the LOGICAL path (§6f), exactly as the tree
        // loader does it: a translation is governed by its original's
        // `.schema.toml`.
        // The path is made root-relative first, because schemas are keyed
        // root-relative by the root-wide `.schema.toml` walk — a
        // `_posts/.schema.toml` is registered under `_posts`, and resolving
        // the bare filename would never find it.
        let parent = source_rel
            .join(&logical_rel)
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        // Every row is governed (§4e): declare a field before you use it.
        let schema = schemas.resolve(&collection, &parent);
        let mut checked = schema::validate(&schema, &raw.front.extra, &raw.path)?;
        // The engine's own four arrive on named front-matter fields rather
        // than in `extra`, so they are seeded here — nearest writer first.
        schema::cascade_front(&schema, &raw.front, &mut checked, &raw.path)?;
        // Markers and rules fill whatever front matter left unset (§4b).
        schema::apply_defaults(&schema, &defaults, &mut checked, &raw.path)?;
        // …and rung 0 overrules all three (§2, MERGE.md E1). Above `cascade`,
        // so a forced `theme` or `toc` is what the row wears.
        schema::force(&cfg.forced, &schema, &mut checked, &raw.path)?;
        let worn = cascade(&checked, &raw.path)?;

        // The law (IO.md I7c), and it is the same call the tree loader makes —
        // which is the point of it being a call. `rendered: true` stood here
        // before, and was true only because every posts rule a site writes
        // sends its rows through a document shell; now the config says so and
        // the row believes the config.
        let rendered = crate::shell::renders(raw.front_mattered, worn.shell.as_deref());
        // The engine-fallback rung, below front matter and every default
        // (§4b). A row that is not a document has no title to imply: its
        // content is its bytes.
        let title = match rendered {
            true => Some(
                raw.front
                    .title
                    .clone()
                    .unwrap_or_else(|| implied_title(&slug)),
            ),
            false => raw.front.title.clone(),
        };
        let row_rel = source_rel.join(&raw.rel);
        // A degenerate row carries no front matter, so its title IS the
        // implied one — the warning states the derivation rather than reading
        // back a value it would have to prove is there.
        if let Some(sh) = crate::shell::degenerate(raw.front_mattered, worn.shell.as_deref()) {
            warnings.push(degenerate_warning(&row_rel, sh, &implied_title(&slug)));
        }

        // A `permalink` is a literal URL, spending no axis; otherwise each of
        // the rule's template(s) is rendered by the one supplier — path tokens,
        // the extractor's, axis and locale placeholders preserved for
        // per-member selection.
        let route_templates: Vec<String> = if let Some(p) = &raw.front.permalink {
            vec![p.clone()]
        } else {
            if route_tmpls.is_empty() {
                bail!("no rule supplies a route for {}", raw.path.display());
            }
            RouteTokens {
                cfg,
                rel: &logical_rel,
                date,
                key: key.as_ref(),
                slug: &slug,
            }
            .render_all(route_tmpls, routing.pattern, &raw.path)?
        };
        let row_axis = row_axes(cfg, &route_templates);
        // `Row.url` is the canonical address (every axis at canonical, the row's
        // own locale); §6f's locale prefix is applied by `select_path` when no
        // template spends locale.
        let coords: Vec<Coord> = row_axis
            .iter()
            .map(|ra| Coord {
                axis: &ra.name,
                value: cfg.axes[&ra.name].canonical().unwrap_or_default(),
                canonical: true,
            })
            .chain(std::iter::once(Coord {
                axis: "locale",
                value: &locale,
                canonical: locale == cfg.i18n.default,
            }))
            .collect();
        let url = select_path(&route_templates, &coords)?;
        drop(coords);

        rows.push(Row {
            axis: row_axis,
            route_templates,
            width: None,
            height: None,
            // Assigned by `insert_rows`, which is where rows become the
            // database's rather than the loader's.
            key: Default::default(),
            // A post publishes because it exists; nothing needs to cite it.
            on_demand: false,
            collection: collection.clone(),
            path: raw.path,
            // ROOT-relative, so `path`/`dir` mean one thing on every row.
            // Rule globs match the collection-relative form (`apply_rules`
            // takes `logical_rel`): `match = "hidden/**"` is relative to
            // `_posts`.
            rel: row_rel,
            version: raw.version,
            date,
            slug,
            stem,
            title,
            description: raw.front.description,
            layout: worn.layout,
            tags: raw.front.tags,
            theme: worn.theme,
            shell: worn.shell,
            fields: checked.values,
            images: checked.images,
            order: raw.front.order,
            toc: worn.toc,
            locale,
            logical,
            url,
            body_bytes: raw.body.len(),
            rendered,
            // Identity, which is a different question (IO.md §3): a `.md` in a
            // posts scope with no `---` block is parsed all the same — the
            // scope hands it a date, a slug and a route — but it carries no
            // front matter, and this column says so. It is now also one half
            // of the law above, which is where the two questions meet.
            front_mattered: raw.front_mattered,
            size: raw.size,
            claimed: false,
        });
    }

    warnings.extend(dead_rules(&collection, &rules, rows.len()));
    Ok((rows, read_ms))
}

/// Posts arrive from several collections (`_posts` and `_drafts` are two
/// sources of one corpus), so they are gathered first and ordered once.
/// Indexing belongs to `SiteDb::insert_rows` (q51).
fn sort_posts(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}

/// Is `rel` (root-relative) *inside* the site's `themes/` directory?
///
/// The directory itself, were a site ever to hold a root-level FILE by that
/// name, is not: what the engine reads is `root.join("themes")` as a
/// directory, so the positional claim is over its contents. `Path::starts_with`
/// compares whole components, so `themes-old/x.md` is ordinary content.
fn under_themes(rel: &Path) -> bool {
    let mut parts = rel.components();
    parts.next().is_some_and(|c| c.as_os_str() == "themes") && parts.next().is_some()
}

/// One walk of the site root, partitioned by membership precedence
/// (DESIGN.md §3): a file the objects scope's rules claim is an object,
/// tree takes the rest.
fn build_tree_and_objects(
    cfg: &Config,
    tree_name: &str,
    tree_c: Option<&Collection>,
    obj_name: &str,
    obj_c: Option<&Collection>,
    markers: &Markers,
    schemas: &Schemas,
    // Compiled by `load` from this very collection, and shared with the marker
    // and vocabulary walks so all three agree on what is not content (§4c).
    not_content: &store::NotContent,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Row>, Vec<Row>)> {
    let Some(tree_c) = tree_c else {
        return Ok((Vec::new(), Vec::new()));
    };
    let root = cfg.root();
    let files = store::walk_tree(&root, not_content, cfg.gitignore)?;

    // A file claimed as a view's template is not independently routable: the
    // view owns its routes. (`blog/index.html` is rendered once per paginated
    // page; `atom.xml` is the feed.)
    let templates: Vec<PathBuf> = cfg
        .views
        .values()
        .filter_map(|v| v.template.as_ref())
        .map(PathBuf::from)
        .collect();
    let files: Vec<_> = files
        .into_iter()
        .filter(|f| !templates.iter().any(|t| *t == f.rel))
        // A marker declares defaults; it is not itself content.
        .filter(|f| !markers.is_marker(&f.path))
        // Nor is the config that declared all of this. Matched by identity,
        // not by glob, so a site needs no `exclude` entry to avoid
        // publishing its own grackle.toml.
        .filter(|f| {
            std::fs::canonicalize(&f.path)
                .map(|p| p != cfg.config_file)
                .unwrap_or(true)
        })
        // Nor are theme SOURCES (IO.md I7b). A site-root `themes/` is engine
        // vocabulary by POSITION, the class `.slots/`, `.section` and
        // `.schema.toml` already occupy: the build reads themes from exactly
        // one place (`root.join("themes")`), so what sits there is input to
        // the build in the same sense the config file is, and publishing a
        // theme's `root.html` at `/themes/mine/root.html` is the same
        // accident as publishing `grackle.toml`.
        //
        // `include` stays the escape hatch — asked the way `NotContent::keeps`
        // asks it, so a site that deliberately publishes something underneath
        // says so in the one key that already means that.
        .filter(|f| not_content.included(&f.rel) || !under_themes(&f.rel))
        .collect();

    // q45: rows named by a view's `content` — claimed landings. Matched
    // by logical identity so every locale variant is claimed with its
    // original.
    let claims = cfg.content_claims();

    let tree_rules = compile_rules(tree_c)?;
    let obj_rules = obj_c.map(compile_rules).transpose()?.unwrap_or_default();
    // The collection-level default, on this side too (IO.md I6): the tree and
    // the objects collection may name an extractor for their rules exactly as
    // a posts collection does — one key, one meaning, three kinds.
    let tree_formats = compile_formats(&tree_c.filename_formats)?;
    let obj_formats = obj_c
        .map(|c| compile_formats(&c.filename_formats))
        .transpose()?
        .unwrap_or_default();

    // Membership in the objects scope is what that scope's rules claim
    // (IO.md I7a) — `**/*.{png,jpg,…}` says "these files are objects" in the
    // one mechanism that also says where they land.
    //
    // The GLOB only, and not `apply_rules`, because a rule's front-matter gate
    // cannot be consulted here: whether a file was peeked for front matter is
    // decided BY this answer (the peek skips binaries, below). Nothing is lost
    // — an object's `has_front_matter` is always false, so a `front_matter =
    // true` objects rule routed nothing before this either.
    //
    // Bare matchers rather than `obj_rules` itself: this closure runs inside
    // the parallel peek, and a `CompiledRule` carries the `Cell<bool>` that
    // `dead_rules` writes.
    let obj_globs: Vec<&GlobMatcher> = obj_rules.iter().map(|r| &r.matcher).collect();
    let is_obj = |rel: &Path| obj_globs.iter().any(|m| m.is_match(rel));

    // Only text rows can carry front matter, and only non-objects need the
    // page/static decision — so skip the peek for the ~800 binaries and run the
    // rest in parallel. (Sequential-over-everything cost ~140ms.)
    let mut files = files;
    files.par_iter_mut().for_each(|f| {
        if !is_obj(&f.rel) {
            f.has_front_matter = store::peek_front_matter(&f.path);
        }
    });

    let mut pages: Vec<Row> = Vec::new();
    let mut objects: Vec<Row> = Vec::new();

    for f in files {
        let is_object = is_obj(&f.rel);

        // §6f: rendered pages carry the locale axis; objects (images) are
        // shared across locales and skip the selector.
        let (logical_rel, locale) = if is_object {
            (f.rel.clone(), cfg.i18n.default.clone())
        } else {
            cfg.i18n.split(&f.rel)
        };

        let rules = if is_object { &obj_rules } else { &tree_rules };
        let collection_formats = if is_object {
            &obj_formats
        } else {
            &tree_formats
        };
        let routing = apply_rules(rules, collection_formats, &logical_rel, f.has_front_matter);
        check_on_demand_cover(&logical_rel, &routing)?;
        let on_demand = routing.on_demand;
        let (tmpls, rule_defaults) = (routing.templates, routing.defaults);
        let marker_defaults = markers.defaults_for(&f.rel);
        let defaults = merged_defaults(&marker_defaults, rule_defaults);
        if tmpls.is_empty() {
            bail!("no rule supplies a route for {}", f.path.display());
        }
        // `stem` is computed here, above the object/page split, because both
        // halves want it and the extractor wants it before either: it is what
        // a `filename_formats` entry describes.
        //
        // STORED rather than re-derived later: recomputing it from `logical`
        // via `file_stem()` returns `v1` for `v1.2-release.md`.
        let stem = logical_rel
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        // The extractor, wherever a rule (or this collection) named one. No
        // corpus tree or objects collection does today, so `key` is `None` and
        // every token below comes from the path — which is exactly what this
        // side has always supplied.
        let key = filename::extract(routing.formats, &stem);
        let from_name = match key.as_ref().and_then(|k| k.ymd()) {
            Some((y, m, d)) => Some(NaiveDate::from_ymd_opt(y, m, d).with_context(|| {
                format!(
                    "{} has an impossible date in its filename",
                    f.path.display()
                )
            })?),
            None => None,
        };
        let slug = key
            .as_ref()
            .and_then(|k| k.slug.clone())
            .unwrap_or_else(|| stem.clone());
        // Render each of the rule's route template(s) through the one supplier:
        // path tokens and extractor results filled, axis and locale
        // placeholders preserved for the materializer to spend per member. A
        // single template is the ordinary case; a list is the default-axis case
        // (§6f), where a canonical member drops its segment.
        //
        // The date offered here is the FILENAME's, not front matter's: this
        // loader reads a page's front matter below, after routing, so a tree
        // route cannot spend a `date:` the way a post's can. That seam is the
        // remaining half of "one supplier" and belongs to I7, which dissolves
        // the two loaders into one walk; nothing here depends on it.
        let route_templates: Vec<String> = RouteTokens {
            cfg,
            rel: &logical_rel,
            date: from_name,
            key: key.as_ref(),
            slug: &slug,
        }
        .render_all(tmpls, routing.pattern, &f.path)?;
        let row_axis = row_axes(cfg, &route_templates);
        // `Row.url` is the CANONICAL address: every axis at its canonical value,
        // the row's own locale. `select_path` drops a canonical segment where a
        // shorter template allows, and applies the locale prefix when no template
        // spends locale — the shape a config without `{axis:locale}` has always
        // had.
        let coords: Vec<Coord> = row_axis
            .iter()
            .map(|ra| Coord {
                axis: &ra.name,
                value: cfg.axes[&ra.name].canonical().unwrap_or_default(),
                canonical: true,
            })
            .chain(std::iter::once(Coord {
                axis: "locale",
                value: &locale,
                canonical: locale == cfg.i18n.default,
            }))
            .collect();
        let url = select_path(&route_templates, &coords)?;

        if is_object {
            // An object is a row that was never rendered. Everything else it
            // could carry — front matter, a locale axis — a binary file does
            // not have, so the defaults are the honest values. Its `slug` is
            // its stem unless a rule's extractor said otherwise, which is the
            // same sentence every other row now gets.
            objects.push(Row {
                key: grackle_db::Key::new(f.rel.to_string_lossy()),
                collection: obj_name.to_string(),
                path: f.path,
                rel: f.rel,
                version: f.version,
                url,
                size: f.size,
                slug,
                stem,
                date: from_name,
                locale,
                rendered: false,
                on_demand,
                ..Default::default()
            });
        } else {
            // Only rendered rows have schema; 41 files, so parsing is cheap.
            let (fm, body_bytes) = if f.has_front_matter {
                read_page_schema(&f.path)?
            } else {
                Default::default()
            };
            // §5b: a governed row's extra front matter is validated — an
            // undeclared key or wrong type fails the load naming the file.
            // Ungoverned rows stay as tolerant as they always were. Schema
            // governance follows the LOGICAL path (§6f): a translation is
            // governed by the same .schema.toml as its original.
            let parent = logical_rel.parent().unwrap_or(Path::new("")).to_path_buf();
            // Every row is governed (§4e). A file with no front matter has
            // nothing to validate, but still takes marker and rule defaults.
            let schema = schemas.resolve(tree_name, &parent);
            let mut checked = match f.has_front_matter {
                true => schema::validate(&schema, &fm.extra, &f.path)?,
                false => Default::default(),
            };
            schema::cascade_front(&schema, &fm, &mut checked, &f.path)?;
            schema::apply_defaults(&schema, &defaults, &mut checked, &f.path)?;
            // Rung 0, above all three (§2, MERGE.md E1) — the posts loader
            // says the same thing at the same seam.
            schema::force(&cfg.forced, &schema, &mut checked, &f.path)?;
            let worn = cascade(&checked, &f.rel)?;
            // Front matter beats the filename, exactly as it does for a post
            // (§4b) — and the filename half is `None` on every corpus row,
            // since no tree rule names an extractor.
            let date = match &fm.date {
                Some(s) => Some(front_matter_date(s, &f.path)?),
                None => from_name,
            };
            // The law (IO.md I7c) — the same call the posts loader makes.
            // `rendered: f.has_front_matter` stood here, which was the first
            // clause alone; the second is what makes a degenerate row possible
            // at all, and on this side it is what lets a rule turn a blockless
            // `.md` into a page by saying `shell = "html"` and nothing else.
            let rendered = crate::shell::renders(f.has_front_matter, worn.shell.as_deref());
            // The engine-fallback rung, below front matter and every default
            // (§4b), and the same rung the posts loader offers — a byte row
            // gets none, because its content is its bytes.
            let title = match (fm.title, rendered) {
                (Some(t), _) => Some(t),
                (None, true) => Some(implied_title(&slug)),
                (None, false) => None,
            };
            // As on the posts side: no block, so the title is the implied one.
            if let Some(sh) = crate::shell::degenerate(f.has_front_matter, worn.shell.as_deref()) {
                warnings.push(degenerate_warning(&f.rel, sh, &implied_title(&slug)));
            }
            let logical = logical_rel.to_string_lossy().to_string();
            // q45: a row named by some view's `content` is claimed — every
            // locale variant of it (the claim is on the logical identity).
            let claimed = claims.contains_key(logical.as_str());
            if claimed && !f.has_front_matter {
                bail!(
                    "view {}: content {logical:?} has no front matter, so it \
                     is a static file, not a claimable row",
                    claims[logical.as_str()]
                );
            }
            pages.push(Row {
                axis: row_axis.clone(),
                route_templates,
                width: None,
                height: None,
                key: Default::default(),
                on_demand,
                collection: tree_name.to_string(),
                slug,
                stem,
                body_bytes,
                path: f.path,
                rel: f.rel,
                version: f.version,
                url,
                rendered,
                // The tree's old page/static gate IS this fact, and since I7c
                // it is no longer the whole of the gate: the fact is one clause
                // of the law above, which the shell can also satisfy.
                front_mattered: f.has_front_matter,
                size: f.size,
                title,
                layout: worn.layout,
                description: fm.description,
                order: fm.order,
                date,
                tags: fm.tags,
                toc: worn.toc,
                theme: worn.theme,
                shell: worn.shell,
                fields: checked.values,
                images: checked.images,
                locale,
                logical,
                claimed,
            });
        }
    }
    // Every claim must have found its row — a typo'd content path is a
    // load error naming the view, not a silently bare landing.
    for (path, view) in &claims {
        if !pages.iter().any(|p| p.claimed && p.logical == *path) {
            bail!("view {view}: content {path:?} names no row in the tree");
        }
    }
    // Dimensions are a property of the FILE, so they belong on the row where
    // a query can reach them rather than in a build-time side map. One header
    // read each, in parallel — sequentially this is ~200ms on a corpus with
    // 850 images, which is a third of the whole build.
    objects.par_iter_mut().for_each(|o| {
        if let Ok((w, h)) = image::image_dimensions(&o.path) {
            o.width = Some(w);
            o.height = Some(h);
        }
    });

    warnings.extend(dead_rules(tree_name, &tree_rules, pages.len()));
    warnings.extend(dead_rules(obj_name, &obj_rules, objects.len()));
    Ok((pages, objects))
}

/// Front matter of a tree page: presentation reads its fields directly.
/// A parse failure is a LOAD ERROR naming the file, never an empty schema —
/// an unquoted `title: A: B` must not ship a silently titleless page (§4).
fn read_page_schema(path: &Path) -> Result<(store::FrontMatter, usize)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (yaml, body) = store::split_front_matter(&text);
    let fm = serde_yaml_ng::from_str(yaml)
        .with_context(|| format!("front matter of {}", path.display()))?;
    // `body_bytes` from the same read, so the field means the same thing on
    // every row.
    Ok((fm, body.len()))
}
/// Read the site named by `cfg` and return the database it describes.
/// An image field names a ROW, so check that it does (§5b × §6a).
///
/// `cover: books/covers/x.png` is a foreign key: an objects collection already
/// claimed that file, and its row key IS that path. Nothing said so, so a typo
/// shipped a broken `<img>` — the same silent 404 that strict link policy
/// exists to prevent for prose links. Runs after `insert_rows`, because the
/// row it names may load after the row that names it.
///
/// An ABSOLUTE url is left alone: it names something outside the site, which
/// no row can vouch for.
fn resolve_image_fields(db: &SiteDb, schemas: &Schemas) -> Result<()> {
    for row in db.rows.iter() {
        let dir = row.rel.parent().unwrap_or(Path::new("")).to_path_buf();
        let declared = schemas.resolve(&row.collection, &dir);
        for (name, ty) in &declared {
            if *ty != crate::schema::FieldType::Image {
                continue;
            }
            let Some(grackle_db::Value::Str(target)) = row.fields.get(*name) else {
                continue;
            };
            if target.contains("://") || target.starts_with("//") {
                continue; // outside the site; no row to check it against
            }
            if db
                .rows
                .get(&grackle_db::Key::new(target.as_str()))
                .is_none()
            {
                anyhow::bail!(
                    "{}: field `{name}` names {target:?}, which is not a file this site \
                     loads. An image field is a reference to a row — check the path, and \
                     that an objects collection claims that extension.",
                    row.rel.display()
                );
            }
        }
    }
    Ok(())
}

pub fn load(cfg: &Config) -> Result<SiteDb> {
    let mut db = SiteDb::default();
    let root = cfg.root();

    // Which collection owns the tree is decided FIRST, because its `exclude` /
    // `include` are the site's declaration of what is not content (§4c) and
    // every walk of the root reads them from here: the tree walk, the marker
    // scan, and the vocabulary walk below. One list, one reader — a walk with
    // a private copy of "what to skip" is q34's disease, and it is how an
    // embedded site's `.schema.toml` (`cover`, under `grackle/examples/`)
    // joined grack.com's own field vocabulary at the same rung.
    //
    // At most ONE collection of each kind reaches these bindings, and the
    // config is what guarantees it (`Config::check_collection_kinds`, MERGE.md
    // C7a): the tree is the root, walked once, and objects come out of that
    // same walk by their own rules, so a second collection of either kind has
    // nothing of its own to read. Before that guard this loop was the silent
    // discard — an unconditional assignment over a `BTreeMap`, so the
    // alphabetically last collection of each kind won and the other's rules,
    // `exclude`, `include` and `schema` went nowhere.
    let mut tree_c = None;
    let mut tree_name = String::new();
    let mut obj_c = None;
    let mut obj_name = String::new();
    for (name, c) in &cfg.collections {
        match c.kind {
            Kind::Tree => {
                tree_c = Some(c);
                tree_name = name.clone();
            }
            Kind::Objects => {
                obj_c = Some(c);
                obj_name = name.clone();
            }
            Kind::Posts => {}
        }
    }
    let empty: &[String] = &[];
    let not_content = store::NotContent::new(
        build_globset(tree_c.map_or(empty, |c| &c.exclude))?,
        build_globset(tree_c.map_or(empty, |c| &c.include))?,
    );

    let t_m = std::time::Instant::now();
    let markers = Markers::scan(&root, &cfg.markers, cfg.gitignore, &not_content)?;
    db.stats.markers_ms = t_m.elapsed().as_secs_f64() * 1000.0;
    db.stats.markers = markers.found;

    // The engine-vocabulary walk: `.section` scope markers (§6e) and
    // `.schema.toml` field declarations (§5b) — positional names like
    // `.slots/`, no config entries. One name-only pass with the same
    // .gitignore and `exclude` defences as the marker scan.
    let mut schemas = Schemas::new(grackle_model::row_schema());
    // The config axes first, so a positional `.schema.toml` is the NEAREST
    // declaration and wins per name (§5b).
    schemas.set_site(cfg.schema.clone(), "grackle.toml [schema]")?;
    for (cname, c) in &cfg.collections {
        schemas.add_collection(
            cname,
            c.schema.clone(),
            &format!("grackle.toml [collections.{cname}.schema]"),
        )?;
    }
    let b = store::walker_declarations(&root, &not_content, cfg.gitignore);
    for entry in b.build().filter_map(|e| e.ok()) {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(&root) else {
            continue;
        };
        let Some(dir) = rel.parent() else { continue };
        if entry.file_name() == ".section" {
            db.sections.push(dir.to_path_buf());
        } else if entry.file_name() == ".schema.toml" {
            let text = std::fs::read_to_string(entry.path())
                .with_context(|| format!("reading {}", entry.path().display()))?;
            schemas.add(dir, &text, rel)?;
        }
    }
    db.sections.sort();
    // The site vocabulary travels with the database (§4e).
    db.declared = schemas.declared_schema();

    // Several collections may feed the posts table — `_posts` and
    // `_drafts` are two sources of one corpus — so rows are gathered
    // first and indexed once, over all of them.
    let mut post_rows: Vec<Row> = Vec::new();
    for (name, c) in &cfg.collections {
        if c.kind == Kind::Posts {
            let (rows, read_ms) = read_posts(cfg, name, c, &markers, &schemas, &mut db.warnings)?;
            post_rows.extend(rows);
            db.stats.read_ms += read_ms;
        }
    }
    let t = std::time::Instant::now();
    let (page_rows, objects) = build_tree_and_objects(
        cfg,
        &tree_name,
        tree_c,
        &obj_name,
        obj_c,
        &markers,
        &schemas,
        &not_content,
        &mut db.warnings,
    )?;
    db.stats.read_ms += t.elapsed().as_secs_f64() * 1000.0;
    // Said once, here, where every caller reaches it — `serve` rebuilds the
    // world through this function, so a warning fixed stops being printed on
    // the next save. The convention is `build.rs`'s and `base.rs`'s: a
    // `grackle: ` line on stderr, because a warning that is not an error must
    // not be mistaken for one.
    for w in &db.warnings {
        eprintln!("grackle: {w}");
    }

    let t_index = std::time::Instant::now();
    db.insert_rows(sort_posts(post_rows), page_rows, objects, &cfg.i18n.default)?;
    resolve_image_fields(&db, &schemas)?;
    db.stats.index_ms += t_index.elapsed().as_secs_f64() * 1000.0;

    // Unified route list.
    let t = std::time::Instant::now();
    let route_locale = |l: &str| (l != cfg.i18n.default).then(|| l.to_string());
    // `RouteKind::Post` survives because a ROUTE kind is real: it is the
    // vocabulary route-pool filters use (`kind == "post"`). Membership, not
    // arithmetic — position in the store carries no meaning.
    let posts: std::collections::HashSet<&grackle_db::Key> = db.post_ix.iter().collect();
    let objects: std::collections::HashSet<&grackle_db::Key> = db.object_ix.iter().collect();
    // §4 on-demand: the row knows its URL, but nothing publishes it until
    // something references it. `materialize_referenced` (build.rs) emits
    // these after the render pass, once the references exist.
    let mut new_routes: Vec<Route> = Vec::new();
    // q45: a claimed row has no route of its own — the owning view materializes
    // the landing. §4: an on-demand row has none YET — a reference materializes
    // it after the render pass.
    for p in db.rows.iter().filter(|p| !p.claimed && !p.on_demand) {
        // Route kind is a question about the row's PROPERTIES, not about which
        // vector it arrived in.
        let kind = if posts.contains(&p.key) {
            RouteKind::Post
        } else if objects.contains(&p.key) {
            RouteKind::Object
        } else if p.rendered {
            RouteKind::Page
        } else {
            RouteKind::Static
        };
        let one = |url: String, axis: Vec<AxisMember>| Route {
            row: Some(p.key.clone()),
            source: Some(p.path.clone()),
            locale: route_locale(&p.locale),
            // The row's fields, with one correction: a member of an axis over
            // `shell` IS a different serialization of the same row (q53's md
            // twin), so THIS output left through the member's shell, not the
            // row's. Only `shell` is corrected — it is the column IO.md §3
            // puts on the output side, and the axis's other field (`theme`) has
            // no reader on the route pool to lie to.
            fields: {
                let mut f = p.fields.clone();
                for m in axis.iter().filter(|m| m.field == "shell") {
                    f.insert("shell".to_string(), grackle_db::Value::Str(m.value.clone()));
                }
                f
            },
            axis,
            // The row's identity fact, carried to the output side (IO.md §3)
            // for the same reason `fields` is: a fold over the route pool can
            // only filter on what the route answers.
            front_mattered: p.front_mattered,
            ..Route::new(url, kind)
        };
        // The row's own rule decided this (q53 step 2): a route template that
        // spends `{theme}` opted its rows in. Only a RENDERED row multiplies —
        // an axis publishes alternative forms of a document, and a static file
        // or an image has one form, the bytes.
        let axes: &[grackle_model::RowAxis] = if p.rendered { &p.axis } else { &[] };
        if axes.is_empty() {
            new_routes.push(one(p.url.clone(), Vec::new()));
            continue;
        }
        // The cartesian product of the axes' values: one route per member-tuple.
        // A single axis is the degenerate product of one. Each tuple picks its
        // template (locale a coordinate beside the theme members) so a canonical
        // member drops its segment where a shorter template allows.
        let mut tuples: Vec<Vec<AxisMember>> = vec![Vec::new()];
        for ra in axes {
            let axis = &cfg.axes[&ra.name];
            tuples = tuples
                .into_iter()
                .flat_map(|t| {
                    axis.values.iter().map(move |value| {
                        let mut t2 = t.clone();
                        t2.push(AxisMember {
                            axis: ra.name.clone(),
                            value: value.clone(),
                            field: axis.field.clone(),
                            canonical: axis.canonical() == Some(value.as_str()),
                        });
                        t2
                    })
                })
                .collect();
        }
        for tuple in tuples {
            let url = {
                let coords: Vec<Coord> = tuple
                    .iter()
                    .map(|m| Coord {
                        axis: &m.axis,
                        value: &m.value,
                        canonical: m.canonical,
                    })
                    .chain(std::iter::once(Coord {
                        axis: "locale",
                        value: &p.locale,
                        canonical: p.locale == cfg.i18n.default,
                    }))
                    .collect();
                select_path(&p.route_templates, &coords)?
            };
            new_routes.push(one(url, tuple));
        }
    }
    db.routes.extend(new_routes);
    crate::views::build_adjacency(cfg, &mut db, &schemas)?;
    crate::views::build_views(cfg, &mut db, &schemas)?;
    crate::views::build_pool_folds(cfg, &mut db)?;
    // Rung 0 into the route pool, at the first point where the pool is whole —
    // and necessarily before `resolve_pool_folds` below, which is the only pass
    // that filters routes (MERGE.md R6). A new route-minting pass belongs above
    // this line; a new route-FILTERING pass belongs below it.
    force_route_fields(cfg, &mut db, &schemas)?;
    // §6g: relations compile after views, so a relation's `over` set is
    // already resolved. Type errors and cycles surface here, at load.
    crate::relations::build_relations(cfg, &mut db, &schemas)?;
    db.stats.views_ms = t.elapsed().as_secs_f64() * 1000.0;

    // q45 TEMPLATED landings (§5c): a view whose `content`/`default_content` is a
    // template (`{group:key}/index.md`) resolves to a different row per route, so
    // its claims can only be settled now — once the routes and their group params
    // and axis members exist. A LITERAL claim was settled at load (rows marked,
    // own routes withheld, excluded from queries) and is untouched here, which is
    // what keeps every existing site byte-identical.
    {
        // Resolve each templated landing route to the logical path it embeds.
        let mut set_content: Vec<(grackle_db::Key, String)> = Vec::new(); // (route id, logical)
        let mut owner_of: HashMap<String, String> = HashMap::new(); // logical -> view
        let mut errors: Vec<String> = Vec::new();
        for r in &db.routes {
            if r.kind != RouteKind::View {
                continue;
            }
            let Some(view) = r.view.as_deref() else {
                continue;
            };
            let Some(v) = cfg.views.get(view) else {
                continue;
            };
            // A templated `content` is a PROMISE (missing row = error); a
            // templated `default_content` is an OFFER (missing, or a row that
            // does not place the embed, = plain landing).
            let (tmpl, promise) = match (v.content.as_deref(), v.default_content.as_deref()) {
                (Some(c), _) if crate::config::is_templated(c) => (c, true),
                (_, Some(d)) if crate::config::is_templated(d) => (d, false),
                _ => continue,
            };
            // Resolve against this route's group params (bare or `group:`) and
            // axis members (`axis:`); a bare name resolves in whichever single
            // namespace has it.
            let cp = template::render(tmpl, |tok| {
                let (ns, k) = template::classify(tok);
                let g = || template::param(&r.params, k);
                let a = || r.axis.iter().find(|m| m.axis == k).map(|m| m.value.clone());
                match ns {
                    Some("group") => g(),
                    Some("axis") => a(),
                    Some(_) => None,
                    None => match (g(), a()) {
                        (Some(x), None) | (None, Some(x)) => Some(x),
                        _ => None,
                    },
                }
            });
            let cp = match cp {
                Ok(c) => c,
                Err(e) => {
                    errors.push(format!("view {view}: content template {tmpl:?}: {e:#}"));
                    continue;
                }
            };
            let sibs = db.by_logical.get(&cp);
            let exists = sibs.is_some_and(|s| !s.is_empty());
            if !exists {
                if promise {
                    errors.push(format!(
                        "view {view}: content {cp:?} names no row in the tree \
                         (resolved from template {tmpl:?} for {})",
                        r.url
                    ));
                }
                continue; // offer: plain landing
            }
            // The OFFER accepts only if the claimed row places the embed;
            // otherwise the row wants the URL to itself, so the route stays a
            // plain listing. The PROMISE requires the embed too, but checks it at
            // render (build.rs), where the literal claim checks it as well.
            if !promise {
                let tag = format!("{{% view {view} %}}");
                let places = sibs
                    .into_iter()
                    .flatten()
                    .filter_map(|k| db.rows.get(k))
                    .any(|row| {
                        std::fs::read_to_string(&row.path)
                            .map(|t| store::split_front_matter(&t).1.contains(&tag))
                            .unwrap_or(false)
                    });
                if !places {
                    continue;
                }
            }
            // A row serves one landing (§5h) — the load-time check cannot see
            // this across two DIFFERENT templates, so it is caught here.
            if let Some(prev) = owner_of.insert(cp.clone(), view.to_string()) {
                if prev != view {
                    errors.push(format!(
                        "row {cp:?} is claimed as content by two views ({prev} and \
                         {view}) — a row serves one landing"
                    ));
                }
            }
            set_content.push((r.id.clone(), cp));
        }
        if !errors.is_empty() {
            // A promise route repeats per locale/page, so the same message can
            // arrive several times.
            errors.sort();
            errors.dedup();
            bail!("{}", errors.join("\n"));
        }

        let claimed_paths: std::collections::HashSet<&str> =
            set_content.iter().map(|(_, cp)| cp.as_str()).collect();
        let claimed_keys: std::collections::HashSet<grackle_db::Key> = db
            .rows
            .iter()
            .filter(|p| claimed_paths.contains(p.logical.as_str()))
            .map(|p| p.key.clone())
            .collect();
        for (rid, cp) in set_content {
            if let Some(r) = db.routes.get_mut(&rid) {
                r.content = Some(cp);
            }
        }
        for k in &claimed_keys {
            if let Some(row) = db.rows.get_mut(k) {
                row.claimed = true;
            }
        }
        // The landing owns the URL now, so the claimed rows' own standalone
        // routes go — exactly as a literal claim withholds them at load.
        db.routes
            .retain(|r| r.row.as_ref().is_none_or(|k| !claimed_keys.contains(k)));
        // And the rows leave every query they were materialized into: a literal
        // claim is excluded at build_views, but a templated one was not known
        // until now, so its rows are dropped from view membership here.
        for r in db.routes.iter_mut() {
            if r.members.iter().any(|k| claimed_keys.contains(k)) {
                r.members.retain(|k| !claimed_keys.contains(k));
                r.rows = Some(r.members.len());
            }
        }
        for vr in db.views.values_mut() {
            let before = vr.members.len();
            vr.members.retain(|k| !claimed_keys.contains(k));
            if vr.members.len() != before {
                vr.rows = vr.members.len();
            }
        }
    }

    // q45: a claimed row's URL becomes its landing's — so source-path links and
    // the ancestors walk see the landing, not the retired standalone URL. A
    // TEMPLATED claim points at the specific route that embeds it (the one whose
    // resolved `content` is this row's logical path, in this locale); a LITERAL
    // one points at its owner view's bare route. A locale variant whose partition
    // didn't materialize keeps no URL (nothing may link it).
    {
        let claims = cfg.content_claims();
        let mut fixed: Vec<(grackle_db::Key, String)> = Vec::new();
        for (k, p) in db
            .page_ix
            .iter()
            .filter_map(|k| db.rows.get(k).map(|r| (k, r)))
        {
            if !p.claimed {
                continue;
            }
            let url = db
                .routes
                .iter()
                .find(|r| {
                    r.kind == RouteKind::View
                        && r.content.as_deref() == Some(p.logical.as_str())
                        && r.locale == route_locale(&p.locale)
                })
                .map(|r| r.url.clone())
                .or_else(|| {
                    let owner = *claims.get(p.logical.as_str())?;
                    db.routes
                        .iter()
                        .find(|r| {
                            r.kind == RouteKind::View
                                && r.view.as_deref() == Some(owner)
                                && r.locale == route_locale(&p.locale)
                                && r.key.is_none()
                                && r.page.is_none_or(|n| n == 1)
                        })
                        .map(|r| r.url.clone())
                });
            fixed.push((k.clone(), url.unwrap_or_default()));
        }
        for (k, url) in fixed {
            if let Some(r) = db.rows.get_mut(&k) {
                r.url = url;
            }
        }
    }

    // Constraint: route collisions across every table.
    let mut seen: HashMap<&str, &Route> = HashMap::new();
    let mut collisions = Vec::new();
    for r in &db.routes {
        if let Some(prev) = seen.insert(&r.url, r) {
            collisions.push(format!(
                "  {}\n    {:?} {}\n    {:?} {}",
                r.url,
                prev.kind,
                prev.source
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| format!("view {:?}", prev.view)),
                r.kind,
                r.source
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| format!("view {:?}", r.view)),
            ));
        }
    }
    if !collisions.is_empty() {
        bail!("route collisions:\n{}", collisions.join("\n"));
    }

    // Constraint: the DUAL of the one above. That check says two rows may not
    // share a URL; this one says one row may not have two URLs.
    //
    // A row is rendered at exactly one route, and the three legal counts are
    // 0 (claimed by a landing view, q45 — the view owns the URL — or on-demand
    // and unreferenced), 1 (everything else), and N **only along an axis**
    // (q53: locales, and whatever follows them). Nothing produces N today, so
    // this cannot currently fire from any config; it is stated now because the
    // axis is the feature that will make it reachable, and a contract written
    // before its first violation is a design decision rather than a patch.
    //
    // It could not even be expressed until `Route.row` did: recovering a
    // route's row meant looking its URL up in `by_url`, which answers "one" by
    // construction and so could never see the second.
    let mut by_row: HashMap<(&grackle_db::Key, String), &Route> = HashMap::new();
    for r in &db.routes {
        let Some(k) = &r.row else { continue };
        // Keyed by (row, member TUPLE): several routes onto one row are legal
        // exactly when they differ in the tuple of members they carry — the
        // cartesian product of the axes. Two routes with the same tuple — or
        // both with none — are the collision this forbids, so composing axes
        // buys the exception it needs and no more. Sorted so the key does not
        // depend on which order the axes were spent.
        let mut members: Vec<(&str, &str)> = r
            .axis
            .iter()
            .map(|a| (a.axis.as_str(), a.value.as_str()))
            .collect();
        members.sort_unstable();
        let member_desc = members
            .iter()
            .map(|(a, v)| format!("{a}={v:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        if let Some(prev) = by_row.insert((k, member_desc.clone()), r) {
            bail!(
                "one row, two routes:\n  {}\n    {}\n    {}\n\
                 A row renders at one URL. Publishing it at several is an AXIS \
                 (q53), which is the only thing allowed to break this{}.",
                r.source
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                prev.url,
                r.url,
                if member_desc.is_empty() {
                    String::new()
                } else {
                    format!(" — and these are both its ({member_desc}) member")
                }
            );
        }
    }

    db.routes.sort_by(|a, b| a.url.cmp(&b.url));
    // All-outputs folds index routes, so they resolve against the final,
    // sorted list.
    crate::views::resolve_pool_folds(cfg, &mut db, &schemas)?;
    Ok(db)
}

#[cfg(test)]
mod cascade_tests {
    use super::*;

    fn text(s: &str) -> toml::Value {
        toml::Value::String(s.to_string())
    }

    fn yes() -> toml::Value {
        toml::Value::Boolean(true)
    }

    fn defaults<'a>(
        pairs: &'a [(&'static str, toml::Value)],
    ) -> BTreeMap<&'static str, &'a toml::Value> {
        pairs.iter().map(|(k, v)| (*k, v)).collect()
    }

    fn front(yaml: &str) -> store::FrontMatter {
        serde_yaml_ng::from_str(yaml).unwrap()
    }

    /// The engine's four as `base.toml` declares them. A test that typed them
    /// by hand would be asserting its own copy; this reads the one list the
    /// loader reads (`schema::CASCADE`).
    fn governed() -> BTreeMap<&'static str, schema::FieldType> {
        schema::CASCADE.iter().copied().collect()
    }

    /// The whole row-side cascade, in the order `load` runs it: front matter
    /// (nearest), then markers and rules, then the typed read. Driving all
    /// three is the point — the type checking C1 added lives in the middle
    /// call, and a test that only called `cascade` could not see it.
    fn worn(
        schema: &BTreeMap<&str, schema::FieldType>,
        yaml: &str,
        pairs: &[(&'static str, toml::Value)],
    ) -> Result<Cascaded> {
        let fm = front(yaml);
        let mut fields = schema::Fields::default();
        schema::cascade_front(schema, &fm, &mut fields, Path::new("p.md"))?;
        schema::apply_defaults(schema, &defaults(pairs), &mut fields, Path::new("p.md"))?;
        cascade(&fields, Path::new("p.md"))
    }

    #[test]
    fn front_matter_beats_a_default() {
        let d = [("theme", text("inherited")), ("toc", yes())];
        let c = worn(&governed(), "theme: own\ntoc: false\n", &d).unwrap();
        assert_eq!(c.theme.as_deref(), Some("own"));
        assert!(!c.toc);
    }

    /// Every field a silent row inherits.
    #[test]
    fn a_silent_row_inherits_every_cascading_field() {
        let d = [
            ("theme", text("t")),
            ("shell", text("light_html")),
            ("layout", text("l")),
            ("toc", yes()),
        ];
        let c = worn(&governed(), "{}", &d).unwrap();
        assert_eq!(c.theme.as_deref(), Some("t"));
        assert_eq!(c.shell.as_deref(), Some("light_html"));
        assert_eq!(c.layout.as_deref(), Some("l"));
        assert!(c.toc);
    }

    #[test]
    fn an_unset_field_stays_unset() {
        let c = worn(&governed(), "{}", &[]).unwrap();
        assert_eq!(c.theme, None);
        assert_eq!(c.layout, None);
        assert!(!c.toc);
    }

    /// A shell outside the vocabulary must fail loudly — unchecked, a typo
    /// renders the wrong tier in silence.
    ///
    /// The controls are the whole MAP family (IO.md §4, I2), and the two
    /// retired spellings sit in the reject list beside the typo: they are hard
    /// cutoffs (MERGE.md §4), so `none` and `light` get the same error a
    /// misspelling gets and no teaching sentence of their own.
    ///
    /// Mutation check: delete the `check_row` call in `cascade` and this fails
    /// on every rejected value at once.
    #[test]
    fn a_shell_outside_the_vocabulary_is_a_load_error() {
        for bad in ["htlm", "none", "light"] {
            let e = worn(&governed(), &format!("shell: {bad}\n"), &[])
                .unwrap_err()
                .to_string();
            assert!(e.contains("is not a shell"), "{bad}: {e}");
            assert!(e.contains("p.md"), "{bad}: {e}");
        }
        for ok in crate::shell::MAP {
            assert!(worn(&governed(), &format!("shell: {ok}\n"), &[]).is_ok());
        }
    }

    /// The family check, on the row side (IO.md §4): a fold shell eats a
    /// COLLECTION, so a row wearing one is an arity error that says what atom
    /// eats rather than "unknown word" — the row is one output and there is
    /// nothing for the fold to fold.
    ///
    /// Mutation check: drop the `is_fold` arm from `shell::check_row` and this
    /// fails on the message (the value is still rejected, but by the wrong
    /// sentence — which is exactly the diagnosis this item is for).
    #[test]
    fn a_fold_shell_on_a_row_names_what_it_eats() {
        for fold in crate::shell::FOLD {
            let e = worn(&governed(), &format!("shell: {fold}\n"), &[])
                .unwrap_err()
                .to_string();
            assert!(e.contains("is a fold shell"), "{fold}: {e}");
            assert!(e.contains("belongs on a view"), "{fold}: {e}");
            assert!(e.contains("raw, html, light_html"), "{fold}: {e}");
        }
        // The one the design document names, spelled out: `shell = atom` on a
        // row says what atom eats.
        let e = worn(&governed(), "shell: atom\n", &[])
            .unwrap_err()
            .to_string();
        assert!(e.contains("a feed's worth of entries"), "{e}");
    }

    /// An inherited shell is checked too — a rule can typo it as easily as
    /// front matter can.
    #[test]
    fn an_inherited_shell_is_checked() {
        let d = [("shell", text("lite"))];
        let e = worn(&governed(), "{}", &d).unwrap_err();
        // Naming the reason: a bare is_err() passes when the cascade rejects
        // the row for something unrelated.
        assert!(e.to_string().contains("is not a shell"), "{e}");
    }

    /// C1, the whole item: a rule or marker default for one of the engine's
    /// four is TYPE-CHECKED like every other key. `toc = "true"` used to skip
    /// `apply_defaults` entirely and read back through `as_bool()` — `None`,
    /// so `false`, so no outline and nothing said.
    ///
    /// Mutation check: exempt `toc` in `apply_defaults` again (`if
    /// schema::cascade_type(name).is_some() { continue }`) and this returns
    /// `Ok` with `toc == false` — the silence, restored.
    #[test]
    fn a_mistyped_default_is_a_load_error_naming_the_type() {
        let d = [("toc", text("true"))];
        let e = worn(&governed(), "{}", &d).unwrap_err().to_string();
        assert!(e.contains("p.md"), "it names the file: {e}");
        assert!(e.contains("declared bool"), "and the type: {e}");

        // The same failure the other way round: a string field set to a number
        // used to vanish, because `as_str()` on an integer is `None`.
        let d = [("theme", toml::Value::Integer(1))];
        let e = worn(&governed(), "{}", &d).unwrap_err().to_string();
        assert!(e.contains("p.md"), "it names the file: {e}");
        assert!(e.contains("declared string"), "and the type: {e}");
    }

    /// The four are governed like any other name (§4e, "every row is
    /// governed"): a site that declared none of them and a row that wears one
    /// is a load error, not a value only the engine can see.
    #[test]
    fn an_undeclared_cascade_key_is_a_load_error() {
        let empty = BTreeMap::new();
        let e = worn(&empty, "layout: page\n", &[]).unwrap_err().to_string();
        assert!(e.contains("not declared"), "{e}");
        assert!(e.contains("p.md"), "{e}");

        let d = [("theme", text("ledger"))];
        let e = worn(&empty, "{}", &d).unwrap_err().to_string();
        assert!(e.contains("no schema declares"), "{e}");
    }

    #[test]
    fn a_marker_beats_a_rule() {
        let markers: Defaults = [("theme".to_string(), text("marker"))]
            .into_iter()
            .collect();
        let rule = text("rule");
        let rules: BTreeMap<&str, &toml::Value> =
            [("theme", &rule), ("toc", &rule)].into_iter().collect();
        let merged = merged_defaults(&markers, rules);
        assert_eq!(merged["theme"].as_str(), Some("marker"));
        assert_eq!(
            merged["toc"].as_str(),
            Some("rule"),
            "rules still fill gaps"
        );
    }

    /// And the marker's value reaches the ROW — the half `a_marker_beats_a_rule`
    /// cannot see, because the merge is only the first of the three steps.
    /// A marker sets one of the engine's four exactly as it sets a declared
    /// field, front matter still nearer than both.
    #[test]
    fn a_marker_sets_what_the_engine_reads() {
        let markers: Defaults = [
            ("theme".to_string(), text("marker")),
            ("toc".to_string(), yes()),
        ]
        .into_iter()
        .collect();
        let rule = text("rule");
        let rules: BTreeMap<&str, &toml::Value> = [("theme", &rule)].into_iter().collect();
        let merged = merged_defaults(&markers, rules);

        let schema = governed();
        let mut fields = schema::Fields::default();
        schema::cascade_front(&schema, &front("{}"), &mut fields, Path::new("p.md")).unwrap();
        schema::apply_defaults(&schema, &merged, &mut fields, Path::new("p.md")).unwrap();
        let c = cascade(&fields, Path::new("p.md")).unwrap();
        assert_eq!(c.theme.as_deref(), Some("marker"));
        assert!(c.toc, "a marker's bool arrives as a bool");

        // Front matter is nearer than the marker, at the row as at the merge.
        let mut fields = schema::Fields::default();
        schema::cascade_front(
            &schema,
            &front("theme: own\n"),
            &mut fields,
            Path::new("p.md"),
        )
        .unwrap();
        schema::apply_defaults(&schema, &merged, &mut fields, Path::new("p.md")).unwrap();
        assert_eq!(
            cascade(&fields, Path::new("p.md"))
                .unwrap()
                .theme
                .as_deref(),
            Some("own")
        );
    }

    /// The engine's own types are not a site's to choose: declaring `toc` a
    /// string would type a rule's value one way and have `cascade` read it
    /// the other — silence again, one rung further out.
    ///
    /// Mutation check: drop the `ty != engine` test inside `parse_fields`'
    /// `cascade_type` arm and the retype is accepted. (Dropping the whole arm
    /// is not the mutation: `layout` and `toc` are `reserved` names, so the
    /// base's own `[schema]` would stop parsing and every site would fail.)
    #[test]
    fn a_cascade_key_may_not_be_redeclared_at_another_type() {
        let mut s = schema::Schemas::new(grackle_model::row_schema());
        let e = s
            .set_site("theme = { type = \"int\" }".parse().unwrap(), "[schema]")
            .unwrap_err()
            .to_string();
        assert!(e.contains("cascade key"), "{e}");
        assert!(e.contains("declared string"), "{e}");
        // Restating the engine's own line is legal — that is what `raw` does.
        s.set_site("theme = { type = \"string\" }".parse().unwrap(), "[schema]")
            .unwrap();
    }
}

/// What the load says and does not fail over, driven through the real `load`
/// on a real (tiny) site — DESIGN.md §4's promised dead-rule warning (MERGE.md
/// C3).
///
/// A dead rule's subject is a corpus answering a glob and nothing smaller than
/// a tree can be that, which is why these tests write sites rather than build
/// a `Config`. (D1's `bucket` warning was tested here too, and went with the
/// key in F1 — it was the config-only warning this module doc used to contrast
/// against.)
#[cfg(test)]
mod load_warning_tests {
    use super::*;

    /// Write a site under the system temp dir. `who` names the caller —
    /// unit tests run in parallel threads, and a shared scratch directory
    /// means one test loads another's content (`slots.rs`'s precedent).
    fn site(who: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grackle-load-warning-{who}"));
        let _ = std::fs::remove_dir_all(&dir);
        for (rel, body) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
            std::fs::write(&p, body).unwrap();
        }
        dir
    }

    fn warnings(dir: &Path) -> Vec<String> {
        let cfg = Config::load(&dir.join("grackle.toml")).expect("the test site should load");
        load(&cfg).expect("the test site should build").warnings
    }

    const PAGE: &str = "---\ntitle: About\n---\n\nProse.\n";

    /// A site inheriting the base, with ONE rule of its own that names a
    /// directory the site does not have.
    ///
    /// The controls are in the same site, which is the point of driving a
    /// whole load: the site's other rule matches `about.md` and is silent,
    /// and the base's `**/index.{html,md}` matches nothing here (there is no
    /// `index.md`) and is silent too, because it is not this author's rule.
    ///
    /// Mutation check: delete `rule.governed.set(true)` in `apply_rules` and
    /// the live rule is reported dead as well; delete the `dead_rules` call
    /// in `build_tree_and_objects` and nothing is reported at all.
    #[test]
    fn a_site_declared_rule_that_matches_nothing_warns() {
        let dir = site(
            "unmatched",
            &[
                (
                    "grackle.toml",
                    r#"
[site]
url = "https://example.com"
title = "T"
author = "A"

[[collections]]
kind = "tree"
name = "entries"
source = "."

  [[collections.rules]]
  match = "photos/**"
  route = "/pics/{path}"

  [[collections.rules]]
  match = "**/*.md"
  front_matter = true
  route = "/{stem}/"
"#,
                ),
                ("about.md", PAGE),
            ],
        );
        let w = warnings(&dir);
        assert_eq!(w.len(), 1, "one dead rule, and only one: {w:?}");
        assert!(w[0].contains("entries"), "names the collection: {w:?}");
        assert!(w[0].contains(r#""photos/**""#), "names the glob: {w:?}");
    }

    /// The false positive this scope exists to prevent. `examples/minimal` is
    /// this site: no `_posts/`, no `index.md`, so the base's `match = "**"`
    /// over `_posts` and its `**/index.{html,md}` both govern nothing — and
    /// neither is the author's to fix. Every base-inheriting site would carry
    /// these forever.
    ///
    /// Mutation check: drop the `!r.inherited` test in `dead_rules` and this
    /// site starts reporting the base's rules.
    #[test]
    fn an_inherited_rule_governing_nothing_says_nothing() {
        let dir = site(
            "inherited",
            &[
                (
                    "grackle.toml",
                    "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n",
                ),
                ("about.md", PAGE),
            ],
        );
        assert_eq!(warnings(&dir), Vec::<String>::new());
    }

    /// A collection with no rows at all reports nothing, whoever wrote the
    /// rules: a rule is dead relative to a CORPUS, and an absent source is a
    /// statement about the source. `extends = "none"` so every rule below is
    /// the site's own — under the `!inherited` test alone, all three would be
    /// reported for the one missing directory.
    ///
    /// Mutation check: delete the `found == 0` early return and this site
    /// reports its three object rules.
    #[test]
    fn a_collection_with_no_rows_reports_nothing() {
        let dir = site(
            "no-rows",
            &[
                (
                    "grackle.toml",
                    r#"
extends = "none"
[site]
url = "https://example.com"
title = "T"
author = "A"

[[collections]]
kind = "objects"
name = "objects"

  [[collections.rules]]
  match = "covers/**/*.{png,jpg}"
  route = "/covers/{path}"

  [[collections.rules]]
  match = "art/**/*.{png,jpg}"
  route = "/art/{path}"

  [[collections.rules]]
  match = "**/*.{png,jpg}"
  route = "/{path}"

[[collections]]
kind = "tree"
name = "entries"
source = "."

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"
"#,
                ),
                ("about.md", PAGE),
            ],
        );
        assert_eq!(warnings(&dir), Vec::<String>::new());
    }

    /// A rule the cascade never lets WIN is still live: `apply_rules` walks
    /// every eligible rule for its defaults, and only the first with a route
    /// decides the URL. Reporting a shadowed rule would be reporting the
    /// engine's own precedence as an error.
    #[test]
    fn a_rule_shadowed_for_the_route_is_not_dead() {
        let dir = site(
            "shadowed",
            &[
                (
                    "grackle.toml",
                    r#"
[site]
url = "https://example.com"
title = "T"
author = "A"

[[collections]]
kind = "tree"
name = "entries"
source = "."

  [[collections.rules]]
  match = "**/*.md"
  front_matter = true
  route = "/{stem}/"

  # Never wins the route — the rule above claims it first — but it fills a
  # default, which is governing.
  [[collections.rules]]
  match = "**/*.md"
  defaults = { hidden = true }
"#,
                ),
                ("about.md", PAGE),
            ],
        );
        assert_eq!(warnings(&dir), Vec::<String>::new());
    }
}

/// A profile's `where` is accepted exactly where the `where` it replaces is
/// (§4a, MERGE.md C6a) — which can only be shown on a real tree, because the
/// half `Config` alone cannot see is the positional `.schema.toml` vocabulary.
#[cfg(test)]
mod profile_filter_tests {
    use super::*;

    fn site(who: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grackle-profile-{who}"));
        let _ = std::fs::remove_dir_all(&dir);
        for (rel, body) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
            std::fs::write(&p, body).unwrap();
        }
        dir
    }

    /// A site whose vocabulary is declared POSITIONALLY, with one set and a
    /// profile that restates it. `{filter}` is the profile's `where`.
    ///
    /// The restatement is the overlay law (MERGE.md E2): a view is a
    /// definition, so the profile's entry replaces the site's whole, `from`
    /// and all.
    fn files(filter: &str) -> Vec<(String, String)> {
        vec![
            (
                "grackle.toml".to_string(),
                format!(
                    r#"
[site]
url = "https://example.com"
title = "T"
author = "A"

[[collections]]
kind = "tree"
name = "entries"
source = "."

  [[collections.rules]]
  match = "**/*.md"
  front_matter = true
  route = "/{{stem}}/"

[sets.published]
from = "entries"

[profiles.p.sets.published]
from = "entries"
where = "{filter}"
"#
                ),
            ),
            (
                "notes/.schema.toml".to_string(),
                "cover = { type = \"bool\" }\n".to_string(),
            ),
            (
                "notes/one.md".to_string(),
                "---\ntitle: One\ncover: true\n---\n\nProse.\n".to_string(),
            ),
        ]
    }

    fn load_with_profile(who: &str, filter: &str) -> Result<()> {
        let owned = files(filter);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let dir = site(who, &refs);
        let cfg = Config::load_profile(&dir.join("grackle.toml"), Some("p"))?;
        load(&cfg)?;
        Ok(())
    }

    /// The bug, at its own scale: `cover` is declared by a `.schema.toml`, so
    /// a VIEW's `where` may name it — and a profile patching that view could
    /// not, because `apply_profile` runs before the tree walk and refused
    /// every name it had not read yet. The site's own `where` and the
    /// profile's replacement are the same words; only one of them was legal.
    ///
    /// Mutation check: restore the `?` on the two-shot parse in
    /// `apply_profile` and this site fails to load with `unknown field
    /// \`cover\``, naming a field it declares.
    #[test]
    fn a_profile_where_may_name_a_positional_declaration() {
        load_with_profile("positional", "!cover")
            .expect("`cover` is declared by notes/.schema.toml");
    }

    /// The other direction: deferring is not accepting. A name nothing
    /// declares still fails the load — at the pass that evaluates the filter,
    /// which is where a view's own typo has always failed — and the message
    /// names the profile, because the text in it is not in any `[sets]` entry
    /// the reader can go and look at.
    ///
    /// Mutation check: delete the `q.patched` note in `declared_filter` and the
    /// error becomes `view published: filter "!cvoer"` — true, and no help at
    /// all to someone reading a `[sets.published]` that says nothing of the
    /// kind.
    #[test]
    fn a_profile_where_naming_nothing_still_fails_the_load() {
        let e = load_with_profile("typo", "!cvoer").expect_err("`cvoer` is nobody's field");
        let e = format!("{e:#}");
        assert!(e.contains("unknown field `cvoer`"), "{e}");
        assert!(e.contains("did you mean `cover`?"), "{e}");
        assert!(
            e.contains("profile p replaced view published's `where`"),
            "names the profile: {e}"
        );
    }
}
