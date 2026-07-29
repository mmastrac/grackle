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

use grackle_db::filter;
use grackle_db::template;
use grackle_model::{AxisMember, Route, RouteKind, Row, SiteDb};

use crate::config::{Collection, Config};
use crate::filename::{self, FileKey};
use crate::markers::{Defaults, Markers};
use crate::schema::{self, Schemas};
use crate::sidecar::Sidecars;
use crate::store;

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
    /// IO.md §4a: this rule declines to route — the embed policy addresses
    /// its rows. The other half of `route`, and read in the same place.
    embed: bool,
    pattern: &'a str,
    /// This rule's own extractors, compiled. Empty means "the collection's",
    /// which [`apply_rules`] resolves — the rule holds what it DECLARED, so
    /// first-writer-wins can tell silence from a list.
    formats: Vec<filename::FilePattern>,
    defaults: &'a BTreeMap<String, toml::Value>,
    /// From the base config rather than the site's own file (§4d).
    inherited: bool,
    /// Whether the walk ever found this rule eligible for a row — the corpus
    /// answering the glob. Written as the rows go past, read once afterwards
    /// by [`dead_rules`]; a `Cell` because the rule list is shared (`&[…]`)
    /// across a walk that only ever visits one row at a time.
    governed: Cell<bool>,
}

fn compile_rules<'a>(
    c: &'a Collection,
    axes: &filename::AxisValues<'_>,
) -> Result<Vec<CompiledRule<'a>>> {
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
                embed: r.embed.unwrap_or(false),
                pattern: r.pattern.as_str(),
                formats: compile_formats(&r.file, axes)?,
                defaults: &r.defaults,
                inherited: r.inherited,
                governed: Cell::new(false),
            })
        })
        .collect()
}

/// One declared `file` list, compiled. Once per rule (and once per
/// collection, for the default), never per row.
fn compile_formats(
    formats: &[String],
    axes: &filename::AxisValues<'_>,
) -> Result<Vec<filename::FilePattern>> {
    formats
        .iter()
        .map(|f| filename::FilePattern::compile(f, axes))
        .collect()
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

/// A scope whose source held files and that claimed none of them (IO.md IR8).
///
/// This is the hole [`dead_rules`]' `found == 0` early return leaves, and I7d's
/// **a scope owns its source** is what makes the hole matter: what a sourced
/// scope does not claim is not content and leaves the walk without a word. So a
/// typo'd glob — `match = "**/*.markdwn"` over a full `_posts/` — used to be a
/// load error (`load_dir` read the directory and demanded rows) and became a
/// clean, silent build with an empty blog. Silently emptying a blog is the
/// disease this ledger exists to refuse.
///
/// **The key is `offered > 0 && found == 0`**, and the denominator is the whole
/// point. `found == 0` alone cannot tell a typo from the two shapes that are
/// documented legal and must stay silent:
///
/// - an **absent** source — §4d's site with no `_posts/`, which pays nothing;
/// - an **empty but present** source — a directory waiting for its first post.
///
/// Both offer zero files, so both stay silent, and neither needs an exception.
///
/// **Only scopes with a PROPER source** ([`Scope::owned`]), which is where the
/// narrowing lives. A sourceless scope (objects) selects by shape and owns
/// nothing, so "asked about a file it did not want" is its ordinary day — the
/// mutation admitting it puts a line on four existing warning fixtures. The
/// root scope is excluded by the same call and is unreachable anyway: it is
/// asked only when no owner stopped the search, so a file it declines is the
/// engine's own *no rule supplies a route* error rather than a silent drop.
///
/// A warning and not an error, for `dead_rules`' reason one level up: a source
/// holding nothing but assets is legal (an `_drafts/caret/` bundle of images
/// under a scope that claims markdown), and the author may be mid-move. Reported
/// for inherited scopes too, unlike a dead rule: the base's glob is not the
/// author's to fix, but the FILES are — they are in a directory the author
/// filled, and moving them or writing a rule are both theirs.
///
/// **Keyed on the scope, not the rule**, and the residual is carried honestly:
/// a typo in ONE rule of several, where a sibling rule still claims something,
/// does not trip this — the scope found rows. `dead_rules` reports that case
/// instead, and only when the site wrote the rule; a per-rule census of what
/// each glob was offered is `query stats`' shape, not stderr's.
fn empty_source(scope: &Scope) -> Option<String> {
    let source = scope.owned()?;
    if scope.offered.get() == 0 || scope.found.get() > 0 {
        return None;
    }
    let globs = scope
        .rules
        .iter()
        .map(|r| format!("`{}`", r.pattern))
        .collect::<Vec<_>>()
        .join(", ");
    let n = scope.offered.get();
    Some(format!(
        "collection {}: `source = {:?}` offered {n} file{} and no rule of this \
         scope claimed one — the collection is empty, and because a scope owns \
         its source those files are not content and ship nowhere (IO.md I7d). \
         The globs asked: {globs}. Fix a glob, or move the files out of {}.",
        scope.name,
        source,
        match n {
            1 => "",
            _ => "s",
        },
        source.display()
    ))
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
    /// The glob of the FIRST rule of this scope that matched — the claim
    /// (IO.md I7d). `None` is "no rule of this scope wanted this file", which
    /// is what sends the ordered sequence on to the next scope, and what a
    /// scope's own source turns into "not content".
    ///
    /// Distinct from `pattern` below, which is the first rule that ROUTED: a
    /// defaults-only rule claims a file it cannot land, and that is an error
    /// naming the file rather than a quiet drop.
    claimed: Option<&'a str>,
    templates: &'a [String],
    /// The glob of the rule that supplied `templates` — carried so a routing
    /// error can name the rule the reader has to edit, not just the template
    /// text (IO.md I6). Empty when no rule supplied a route.
    pattern: &'a str,
    /// The extractors in force for this row: the first matching rule that
    /// declared any, else the collection's own list.
    formats: &'a [filename::FilePattern],
    /// The rule that decided the address said `embed = true` (IO.md §4a):
    /// there is no route to render, and the embed policy supplies a
    /// `strong_url` instead.
    ///
    /// Decided by the SAME first-writer step `templates` is, and that matters
    /// on every base-inheriting site: `route` and `embed` are two answers to
    /// one question, so the first rule that answers it wins and the base's
    /// `embed` line beneath a site's own routing rule never speaks.
    embed: bool,
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

/// First-writer-wins per key (DESIGN.md §4) — and, since IO.md I7d,
/// first-rule-wins for MEMBERSHIP: the first rule of this scope past both
/// gates is the claim, and `walk_site` asks the scopes in order until one
/// claims.
fn apply_rules<'a>(
    rules: &'a [CompiledRule<'a>],
    // The collection's own `file`: the default its rules inherit,
    // read only where no matching rule declared a list of its own (§4).
    collection_formats: &'a [filename::FilePattern],
    rel: &Path,
    has_front_matter: bool,
) -> Routing<'a> {
    let mut claimed: Option<&str> = None;
    let mut templates: &[String] = &[];
    let mut pattern: &str = "";
    let mut formats: Option<&[filename::FilePattern]> = None;
    let mut embed = false;
    // The address question is answered ONCE, by the first rule past both gates
    // that answers it either way (IO.md §4a). `templates.is_empty()` used to
    // stand in for this, and cannot any more: an embed rule supplies no
    // template, so silence and a decision would look the same.
    let mut addressed = false;
    let mut on_demand = false;
    let mut on_demand_cover: Vec<&str> = Vec::new();
    let mut defaults: BTreeMap<&str, &toml::Value> = BTreeMap::new();
    let path_key = path_key(rel);
    for rule in rules {
        if let Some(want) = rule.front_matter {
            if want != has_front_matter {
                continue;
            }
        }
        // Globs see the logical path: strip spent file axes first so a prefix
        // locale (`fr/recipes/dal.md`) still matches `recipes/**`.
        let rule_formats = if rule.formats.is_empty() {
            collection_formats
        } else {
            rule.formats.as_slice()
        };
        let match_rel = match filename::extract(rule_formats, &path_key) {
            Some(m) => with_logical(rel, &m.logical_stem),
            None => rel.to_path_buf(),
        };
        if !rule.matcher.is_match(&match_rel) {
            continue;
        }
        // Past both gates: this rule governs this row, whether or not it is
        // the one that wins the route. That is what keeps a rule shadowed by
        // a nearer one (it still fills defaults) out of `dead_rules`.
        rule.governed.set(true);
        // …and the first one past them is the CLAIM (IO.md I7d): first rule
        // wins, and the rule that wins says which scope this row is in.
        claimed.get_or_insert(rule.pattern);
        if rule.on_demand && !rule.route.is_empty() {
            on_demand_cover.push(rule.pattern);
        }
        if !addressed && !rule.route.is_empty() {
            templates = rule.route;
            pattern = rule.pattern;
            on_demand = rule.on_demand;
            addressed = true;
        } else if !addressed && rule.embed {
            embed = true;
            pattern = rule.pattern;
            addressed = true;
        }
        // First writer wins here too, and deliberately independent of which
        // rule won the ROUTE: `file` is a key like any other, so a
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
        claimed,
        templates,
        pattern,
        formats: formats.unwrap_or(collection_formats),
        embed,
        on_demand,
        on_demand_cover,
        defaults,
    }
}

/// The embed policy's answer for one row (IO.md §4a, I11): its strong address,
/// or the load error that says the config left this asset unreachable.
///
/// **Both refusals are here, at load, and that is stricter than §4a's letter
/// on purpose.** The design says an embedded-but-unrouted asset is an error
/// when the policy is off; asking at load asks one question earlier — before
/// anyone knows whether the asset is embedded — and answers it for the uncited
/// asset too. That is the honest place for it: with the policy off, or with
/// this row outside its subset, `embed = true` is a rule that names no address
/// at all, which is a statement about the CONFIG and needs no citation to be
/// wrong. It also means the refusal names the asset (the design's ask) with a
/// path rather than with a URL that does not exist.
///
/// The mint itself is `strong::address`, and the hashing law is stated there:
/// inputs plus parameters, never output bytes.
fn embed_address(
    cfg: &Config,
    subset: &Option<GlobSet>,
    f: &store::TreeFile,
    pattern: &str,
) -> Result<String> {
    let fix = "Route it with a rule (e.g. `route = \"/{path}\"`), or let the \
               embed policy address it";
    if !cfg.embeds.enabled {
        bail!(
            "{}: the rule `match = {pattern:?}` declares `embed = true`, so no \
             rule routes this asset — and `[embeds] enabled = false` turns off \
             the policy that would have given it a `/static/` address. It can \
             be reached by nothing. {fix} by re-enabling `[embeds]`.",
            f.rel.display()
        );
    }
    if let Some(g) = subset {
        if !g.is_match(&f.rel) {
            bail!(
                "{}: the rule `match = {pattern:?}` declares `embed = true`, so \
                 no rule routes this asset — and `[embeds] match` does not \
                 admit it, so the policy publishes no address for it either. \
                 It can be reached by nothing. {fix} by widening `[embeds] \
                 match` ({}).",
                f.rel.display(),
                cfg.embeds.patterns.join(", ")
            );
        }
    }
    let bytes = std::fs::read(&f.path)
        .with_context(|| format!("embed address: reading {}", f.path.display()))?;
    Ok(crate::strong::address(
        &bytes,
        crate::strong::IDENTITY,
        &crate::strong::ext_of(&f.rel),
    ))
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
}

/// Read the engine's cascade keys off a row's RESOLVED fields — one spelling,
/// so posts and tree rows cannot drift apart on which fields cascade.
///
/// This is no longer a cascade of its own (MERGE.md C1). It used to reach into
/// raw TOML with `as_str()`/`as_bool()`, which is why `defaults = { theme =
/// 1 }` silently vanished. The cascade is `schema::cascade_front` (nearest)
/// then `schema::apply_defaults` (markers, then rules), the same two calls
/// every other declared key takes; what is left here is the typed read, plus
/// the one vocabulary the engine closes.
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
/// **The one route this does not reach — closed at IO.md I10.** An on-demand
/// row published by `build::materialize_referenced` mints its route after
/// `load` has returned, so this pass cannot write it. E1 stated the hole and
/// review I-C handed it here as a graph-ordering question; the answer is that
/// minting an output is the graph event, so rung 0 belongs at every minting
/// seam rather than at this one pass. The typed values are kept on
/// `SiteDb::forced_fields` and the second seam applies them from there — one
/// list, two writers, no re-derivation. Byte-inert today (those routes are
/// `RouteKind::Object` byte publishes with no head, minted below every reader
/// of a route field), which is exactly why it is worth closing now rather than
/// when a reader arrives.
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
    db.forced_fields = values.into_iter().collect();
    Ok(())
}

/// The axes a rule's template(s) opt a row into (q53 step 2): a `{theme}` (or
/// `{axis:theme}`) segment is what spends the theme axis. Axes spent by the
/// rule's `file` patterns are FILE axes — each member owns a content file —
/// so they are the row's own coordinate, not a product dimension.
fn row_axes(
    cfg: &Config,
    templates: &[String],
    file: &[filename::FilePattern],
) -> Vec<grackle_model::RowAxis> {
    let file_spent: std::collections::HashSet<&str> =
        file.iter().flat_map(|p| p.spent_axes()).collect();
    cfg.axes
        .keys()
        .filter(|n| templates.iter().any(|t| spends(t, n)))
        .filter(|n| !file_spent.contains(n.as_str()))
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
/// "/"]` lands the all-canonical member at `/`. Locale is not special: a
/// non-canonical locale must be spent by a template, same as any other axis.
/// Errors only if no template covers a required set, which the fullest
/// template always does unless the templates are pathologically split.
pub fn select_path(templates: &[String], coords: &[Coord]) -> Result<String> {
    let required: Vec<&str> = coords
        .iter()
        .filter(|c| !c.canonical)
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
/// - **the extractor**, where a `file` entry described the stem —
///   `{year}`, `{month}`, `{day}`, `{slug}`, or whichever of them the format
///   named;
/// - **the axes**, which are not filled here at all: a declared axis (and
///   `locale`) is handed back as its own placeholder, for `select_path` and
///   the materializer to spend per member (q53);
/// - **`{hash}`**, the row's content hash (IO.md §4a, I11) — for a site that
///   wants hashed CANONICAL addresses by rule rather than by policy. It reads
///   the file, so it is the one token that costs I/O, and it is read lazily
///   and once: a template that does not spend it never opens anything.
///
/// Before this type there were two suppliers with no overlap: the tree offered
/// path tokens and the posts loader offered date/slug inline, so a file in a
/// posts scope could not route by its directory and a tree page could not
/// route by a date in its name. Both halves now reach every rule.
struct RouteTokens<'a> {
    cfg: &'a Config,
    /// The path the rule matched — see the doc above on why it is that one.
    rel: &'a Path,
    /// The row's file, for `{hash}`. Absolute, unlike `rel`, because the token
    /// is about the BYTES rather than about the name.
    path: &'a Path,
    /// `{hash}`, memoized: `check` asks for every token and then `render`
    /// asks again, and hashing a file twice per template is not a thing to do
    /// quietly. `Some(None)` is "asked, and the file would not read".
    hash: std::cell::RefCell<Option<Option<String>>>,
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
            // IO.md §4a's hashing law, spent as a route token: the digest is
            // over the INPUT bytes and the identity transform's parameters,
            // which is what a canonical URL spending `{hash}` has to mean —
            // the address exists at planning, before any shell runs, exactly
            // like every other route token.
            //
            // A site spelling `route = "/static/{hash}.{ext}"` therefore mints
            // the SAME string the embed policy would have, which is the
            // untransformed-twin rule arriving for free: one hash function,
            // one address per byte string, whichever mechanism asked.
            "hash" => self.content_hash(),
            // An axis placeholder is spent per member, not here (q53).
            k => {
                let (_, bare) = template::classify(k);
                (self.cfg.axes.contains_key(bare)
                    || (bare == "locale" && self.cfg.locale_enabled()))
                    .then(|| format!("{{{k}}}"))
            }
        }
    }

    /// The `{hash}` token's value, computed at most once per row.
    fn content_hash(&self) -> Option<String> {
        if let Some(cached) = self.hash.borrow().as_ref() {
            return cached.clone();
        }
        let v = std::fs::read(self.path).ok().map(|b| {
            crate::strong::address(&b, crate::strong::IDENTITY, "")
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string()
        });
        *self.hash.borrow_mut() = Some(v.clone());
        v
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
                 `file` patterns in force, and it declares no `date:`), but the \
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
             path tokens ({}), the row's {{slug}}, `{{hash}}` (the content \
             hash), the date tokens (year, month, day) wherever \
             `file` or a `date:` gives the row a date{axes}.",
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

/// One collection, compiled: the rules that decide what it claims, and the
/// subtree they read.
///
/// The word the model uses is **scope** (IO.md §1): a source subtree plus its
/// rules, extractors, schema and relations. A scope's ROLE — which table its
/// rows land in, and so which indexes, relation defaults and route kinds they
/// get — is read off its `source` now that `kind` is gone: a proper source is
/// a posts scope, no source at all the objects scope, and `"."` the tree (see
/// [`crate::config::Collection::is_posts`]).
struct Scope<'a> {
    name: &'a str,
    /// The subtree this scope's rules read, root-relative.
    ///
    /// - `None` — **sourceless** (the objects scope): its rules range over the
    ///   whole walk, and it owns nothing.
    /// - `Some("")` — the **site root** (the tree scope). A tree collection's
    ///   declared `source` is decorative — it names the table and nothing
    ///   else, which [`crate::config::Collection::source`] says at the key —
    ///   so the root is what it reads whatever it wrote.
    /// - `Some("_posts")` — a **proper subtree**, which this scope OWNS (see
    ///   [`walk_site`]).
    source: Option<PathBuf>,
    rules: Vec<CompiledRule<'a>>,
    /// The collection-level `file` list: the default its rules inherit, read
    /// where no matching rule declared a list.
    formats: Vec<filename::FilePattern>,
    /// How many rows this scope claimed — `dead_rules`' `found`. A `Cell`
    /// because the walk holds the scope list by shared reference, which is
    /// also why a rule's `governed` flag is one.
    found: Cell<usize>,
    /// How many files the walk OFFERED this scope: every file under its source
    /// that the ordered sequence actually asked it about. The denominator
    /// `found` never had (IO.md IR8) — "claimed nothing" means one thing when
    /// the source is empty or absent and quite another when it was full, and
    /// only this counter tells the two apart.
    offered: Cell<usize>,
}

impl Scope<'_> {
    /// The subtree this scope owns, if it owns one. The root scope and the
    /// sourceless scopes own nothing — see [`walk_site`].
    fn owned(&self) -> Option<&Path> {
        match &self.source {
            Some(p) if !p.as_os_str().is_empty() => Some(p.as_path()),
            _ => None,
        }
    }

    /// The path this scope's rules read for a file, or `None` when the file is
    /// not under this scope's source at all — which is how a scope declines to
    /// look without having to be filtered out of the sequence.
    fn relative(&self, rel: &Path) -> Option<PathBuf> {
        match &self.source {
            Some(src) => rel.strip_prefix(src).ok().map(Path::to_path_buf),
            None => Some(rel.to_path_buf()),
        }
    }
}

/// **The ordered rule sequence** (IO.md I7d): every scope of the site, in the
/// order the walk asks them.
///
/// The order comes from the **most-specific-source law**, and deriving it is
/// the point — `posts → objects → tree` was a constant in the loader
/// (DESIGN.md §3's membership precedence), and a constant cannot say why:
///
/// 1. **Scopes with a proper source, deepest first.** `_posts` sits inside the
///    tree's `.`, and the more specific statement about a subtree wins — the
///    reading a nearer marker and a nearer `.schema.toml` already get (§4b,
///    §5b). This is q51's rider, decided.
/// 2. **Sourceless scopes** (objects) next. A scope with no source selects by
///    shape rather than by place, and it has to outrank the root scope for the
///    reason the root's own rules are ordered as they are: `**` sorts last, or
///    nothing after it ever matches.
/// 3. **The root scope** (`source = "."`) last, by the same principle.
/// 4. **Ties**: the site's own scopes before the base's, mirroring the rule
///    prepend (§4d) — then the table name, which is deterministic and is as
///    near declaration order as a config keyed BY table name can get. The tie
///    is unobservable while two scopes' sources differ, because a scope only
///    ever sees files under its own source and two scopes sharing one source
///    would be one entry.
///
/// Verified against all four corpus sites to reproduce the retired precedence
/// whatever the declaration order — **theme-preview declares its tree FIRST**,
/// and under declaration order alone that tree would eat its own posts.
fn scopes(cfg: &Config) -> Result<Vec<Scope<'_>>> {
    let axes = cfg.axis_values_for_file();
    let mut out: Vec<Scope> = Vec::new();
    for (name, c) in &cfg.collections {
        // The source IS the role now (`kind` is gone): a sourceless scope is
        // the objects one and owns nothing; a source that reads as `.` is the
        // site root (the tree); anything else is a posts scope that owns its
        // subtree. `.` and the empty path are one statement, and the empty one
        // is the spelling the rest of this file needs: a rule glob is relative
        // to the source, so a root written `.` would grow a `./` on every path.
        let source = c.source.as_deref().map(|s| match s {
            "." => PathBuf::new(),
            other => PathBuf::from(other),
        });
        out.push(Scope {
            name,
            source,
            rules: compile_rules(c, &axes)?,
            formats: compile_formats(&c.file, &axes)?,
            found: Cell::new(0),
            offered: Cell::new(0),
        });
    }
    out.sort_by_key(|s| {
        let (class, depth) = match &s.source {
            Some(p) if !p.as_os_str().is_empty() => (0u8, p.components().count()),
            None => (1, 0),
            Some(_) => (2, 0),
        };
        // "The site's before the base's", read off the rules the way
        // `dead_rules` reads it: a scope the site did not write is one whose
        // every rule arrived inherited.
        let inherited = !s.rules.is_empty() && s.rules.iter().all(|r| r.inherited);
        (class, std::cmp::Reverse(depth), inherited, s.name)
    });
    Ok(out)
}

/// Posts arrive from several scopes (`_posts` and `_drafts` are two sources of
/// one corpus) and, since IO.md I7d, from one walk that visits them in path
/// order — so this is now a statement rather than a fix. It stays a statement:
/// the posts table's load order is the loader's to decide (q51), and leaving it
/// to be a side effect of how a directory walk happens to sort is how an
/// ordering-derived byte (an embedding neighbour, a tag list) moves without
/// anyone choosing to move it.
fn sort_posts(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}

/// Collection-relative path with the extension removed, `/`-separated.
/// The subject `file` patterns match against.
fn path_key(rel: &Path) -> String {
    let s = rel.to_string_lossy();
    let s = match rel.extension().and_then(|e| e.to_str()) {
        Some(ext) => s
            .strip_suffix(ext)
            .and_then(|h| h.strip_suffix('.'))
            .unwrap_or(&s),
        None => &s,
    };
    s.replace('\\', "/")
}

/// Rebuild a path from a logical path key, keeping the physical extension.
///
/// A logical key with `/` replaces the whole relative path (prefix strip).
/// A bare filename key only replaces the final component (suffix / dated).
fn with_logical(physical: &Path, logical: &str) -> PathBuf {
    if logical.contains('/') {
        let mut out = PathBuf::from(logical);
        if let Some(e) = physical.extension() {
            out.set_extension(e);
        }
        return out;
    }
    let mut out = physical.to_path_buf();
    let ext = out.extension().map(|e| e.to_os_string());
    out.set_file_name(logical);
    if let Some(e) = ext {
        out.set_extension(e);
    }
    out
}

/// Is `rel` (root-relative) *inside* the site's `themes/` directory?
///
/// The directory itself, were a site ever to hold a root-level FILE by that
/// name, is not: what the engine reads is `root.join("themes")` as a
/// directory, so the positional claim is over its contents. `Path::starts_with`
/// compares whole components, so `themes-old/x.md` is ordinary content.
///
/// The name of the directory is `store::THEMES`, shared with the declaration
/// walks' prune (IO.md IR6) so the positional layer is one word in one place.
fn under_themes(rel: &Path) -> bool {
    let mut parts = rel.components();
    parts.next().is_some_and(|c| c.as_os_str() == store::THEMES) && parts.next().is_some()
}

/// The canonical address of one row: every axis at its canonical value, the
/// row's own locale (§6f). `select_path` drops a canonical segment where a
/// shorter template allows.
fn canonical_url(
    cfg: &Config,
    templates: &[String],
    locale: &str,
    file: &[filename::FilePattern],
) -> Result<String> {
    let row_axis = row_axes(cfg, templates, file);
    let coords: Vec<Coord> = row_axis
        .iter()
        .map(|ra| Coord {
            axis: &ra.name,
            value: cfg.axes[&ra.name].canonical().unwrap_or_default(),
            canonical: true,
        })
        .chain(std::iter::once(Coord {
            axis: "locale",
            value: locale,
            canonical: locale == cfg.i18n.default,
        }))
        .collect();
    select_path(templates, &coords)
}

/// **One walk of the site, one ordered rule sequence over it, first rule
/// wins** (IO.md I7d). Every row of every table comes from here.
///
/// Two laws decide membership, and between them they retire the hardcoded
/// posts → objects → tree precedence (DESIGN.md §3) that the two loaders were:
///
/// - **First rule wins.** A file is offered to every scope in [`scopes`]'
///   order — each reading the path its own source makes of the file, and
///   skipping the file entirely when it is not under that source — with each
///   scope's rules in its own order. The first rule past both gates claims it,
///   and the scope that rule belongs to is the row's collection. That scope's
///   rules then cascade defaults exactly as they always did: membership is
///   what the sequence answers, and everything after it is §4's per-key law
///   inside the one scope.
/// - **A scope owns its source.** When a scope whose source CONTAINS the file
///   is asked and claims nothing, the search stops there: the file is **not
///   content** and leaves the walk. It does not fall through to the scopes
///   below, which is what makes the law a law rather than an ordering. This
///   is what `store::load_dir`'s `.md`-only argument list did by accident,
///   said out loud — and it is what keeps `_drafts/caret/`'s bundle of images,
///   an `.rtf` and an `.xcf` invisible rather than letting the objects
///   catch-all and the tree's passthrough take eighteen files nobody asked
///   for. (Two scopes on one source own it jointly, so the stop waits for
///   them both; they are adjacent in the order by construction.)
///
///   **The root scope is where the second law reads differently, and
///   deliberately.** Its source is the whole site, so "the owning scope
///   claimed nothing" is not a narrowing that missed — it is a site with no
///   rule for a file, which the engine already refuses by name (*no rule
///   supplies a route*). A proper subtree IS a narrowing: `source = "_posts"`
///   with `match = "**/*.{md,markdown}"` is one statement written in two keys,
///   and a `.png` beside a draft was never being refused, it was never being
///   asked about.
///
/// The walk keeps Jekyll's dot/underscore skip, with the declared sources
/// punching through it (`store::walk_tree`).
///
/// **One constructor** (IO.md I7e). Every row this function returns is built by
/// the one block below: rule defaults, marker defaults, schema validation and
/// rung 0, then the rendering law, then routing. The objects branch that used
/// to build a binary row from `Default::default()` — no cascade, no markers, no
/// declared fields, a Null shell — is gone, and with it the last place where
/// a row's origin decided what a row could carry. What survives of
/// "object" is a fact about the FILE (the extension fact, below), which keys
/// the three things that were ever really about pictures: the objects index,
/// the name index and the header read.
fn walk_site(
    cfg: &Config,
    scopes: &[Scope],
    markers: &Markers,
    // Found by the declaration walk (IO.md I8). Identity from a sidecar is the
    // same fact identity from a block is — it just says nothing about the
    // row's bytes.
    sidecars: &Sidecars,
    schemas: &Schemas,
    // Compiled by `load` from the tree collection, and shared with the marker
    // and vocabulary walks so all three agree on what is not content (§4c).
    not_content: &store::NotContent,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Row>, Vec<Row>, Vec<Row>)> {
    let root = cfg.root();
    // IO.md §4a's policy, compiled once. `None` is "no subset declared" —
    // every row an `embed` rule marked — which is not the same as an empty
    // set, and is why this is an Option rather than a GlobSet that matches
    // everything.
    let embed_subset = cfg.embeds.compiled()?;
    // The sources that punch through the dot/underscore skip: a scope naming a
    // directory has declared it to be content, in the one key that means that.
    let sources: Vec<PathBuf> = scopes
        .iter()
        .filter_map(|s| s.owned())
        .map(|p| p.to_path_buf())
        .collect();
    // The one contradiction the one walk creates, refused rather than
    // suffered. `exclude` is the site saying "this is not content" (§4c) and
    // `source` is a scope saying "this is"; before I7d they governed different
    // walks and could disagree in silence, and the disagreement was even
    // harmless — the dot/underscore skip kept `_posts` out of the tree anyway,
    // so an `exclude = ["_posts/**"]` was a redundant line that did nothing.
    // With one walk it does something: it empties the blog, and nothing says
    // so. A load error naming both keys is the house answer, and the fix is to
    // delete the line.
    for s in scopes {
        let Some(src) = s.owned() else { continue };
        if !not_content.keeps_dir(src) {
            bail!(
                "collection {}: `source = {:?}` declares that directory to be \
                 content, and the tree's `exclude` takes it back out of the \
                 walk — so this scope would load nothing. There is one walk \
                 now: a declared source is content, and the dot/underscore \
                 skip already keeps it out of the tree. Delete the `exclude` \
                 entry that names it.",
                s.name,
                src.display()
            );
        }
    }
    let files = store::walk_tree(&root, not_content, cfg.gitignore, &sources)?;

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
        // Nor is a sidecar (IO.md I8), for the same reason and by the same
        // kind of test: it declares the identity of the file beside it. The
        // set comes from the declaration walk, so what makes a `.toml` a
        // declaration is the file it sits beside — not its name, which is why
        // an ordinary `netlify.toml` is still published like any other file.
        .filter(|f| !sidecars.is_sidecar(&f.rel))
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

    // **The extension fact** (IO.md §7: "name index and dimensions keyed off
    // extension") — the objects scopes' globs, asked on their own. It is I7a's
    // `is_obj`, unchanged in what it computes, and since I7e it is a fact about
    // the FILE that stands beside the sequence rather than a preview of it:
    // the sequence answers which scope claims a row, and this answers whether
    // the row is a picture. Three readers, none of which the sequence can
    // serve:
    //
    //   - the PEEK. Whether a file was peeked is what the front-matter gate
    //     reads, so it cannot itself be gated. Skipping the ~800 binaries is
    //     what keeps the peek off the build's critical path.
    //   - the LOCALE axis. An image is shared across locales (§6f), so an
    //     objects rule does not spend `{axis:locale}` in `file`. Pinned in
    //     both directions by `io_dissolve.rs` — a `photo.fr.png` keeps its
    //     literal name while a `notes.fr.md` beside it is the French variant.
    //   - the OBJECTS index (I7e). `object_ix`, `by_name` and the header read
    //     key off this rather than off which scope claimed the row, which is
    //     what lets the row constructor be one constructor: a former-object
    //     row is built like every other row and is an object because of what
    //     it IS.
    //
    // The three agree with the sequence on every corpus site because no objects
    // rule of any site gates on front matter — one that did would take the
    // glob's answer here and the gate's answer there, and its rows would be
    // indexed as images while belonging to whichever scope claimed them, which
    // is the honest reading of an index keyed off extension. Stated rather than
    // guarded (I7a recorded the same shape: such a rule claimed nothing before
    // either).
    //
    // Bare matchers rather than the rules themselves: this closure runs inside
    // the parallel peek, and a `CompiledRule` carries the `Cell`s the walk
    // writes.
    let obj_globs: Vec<&GlobMatcher> = scopes
        .iter()
        .filter(|s| s.source.is_none())
        .flat_map(|s| s.rules.iter().map(|r| &r.matcher))
        .collect();
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

    let mut posts: Vec<Row> = Vec::new();
    let mut pages: Vec<Row> = Vec::new();
    let mut objects: Vec<Row> = Vec::new();

    for f in files {
        // The extension fact (above), asked once per file and read twice: by
        // the peek that already ran, and by the partition at the bottom of
        // this loop. Locale is not special here — a rule's `file` patterns
        // either spend `{axis:locale}` or they don't. An objects rule that
        // does not leaves `photo.fr.png` as a literal name (one picture
        // serves every locale; `io_dissolve.rs` pins both halves).
        let object_shaped = is_obj(&f.rel);

        // **Identity: a block, or a sidecar** (IO.md §1, I8). The two are peers
        // — neither is nearer than the other, and there is no ladder between
        // them — so a file carrying both is a load error rather than a
        // precedence rule nobody could predict. It is the marker-collision
        // shape (MERGE.md A5) and it gets the same answer: an unrankable
        // disagreement is refused, and the fix is to write one of them.
        let sidecar = sidecars.get(&f.rel);
        if sidecar.is_some() && f.has_front_matter {
            bail!(
                "{}: identity twice — the file carries a front-matter block and \
                 {}.toml is a sidecar for it. A sidecar exists for files that \
                 CANNOT carry a block; nothing ranks the two, so pick one: \
                 delete the sidecar, or delete the block.",
                f.rel.display(),
                f.rel.display()
            );
        }
        // The fact §3's table names, widened by I8 without changing its
        // meaning. What it does NOT widen is `renders` below: a block is IN
        // the file and so says the file is a document, while a sidecar says
        // only that someone wrote a row's fields down somewhere.
        let has_identity = f.has_front_matter || sidecar.is_some();

        // **The ordered rule sequence, first rule wins.** Every scope in turn,
        // each reading the path its own source makes of the file, until one of
        // their rules claims it.
        let mut claim: Option<(&Scope, Routing, PathBuf)> = None;
        // Set when a scope that OWNS this path has been asked and passed. What
        // it means is "not content" — see this function's doc — and the loop
        // keeps going only for the scopes that share that same source, because
        // two scopes on one source own it jointly. Nothing below them looks.
        let mut owner_passed: Option<&Path> = None;
        for s in scopes {
            if let Some(o) = owner_passed {
                if s.owned() != Some(o) {
                    break;
                }
            }
            // Rule globs and route tokens read the path this scope's source
            // makes of it — collection-relative in `_posts`, root-relative in
            // the tree — so a rule's `match` and its `route` spell the same
            // words (IO.md I6). `None` is a file outside this scope's source,
            // which the scope never sees.
            let Some(scope_rel) = s.relative(&f.rel) else {
                continue;
            };
            // Offered: this scope's source contains the file and the sequence
            // reached it, so its rules are about to be asked. Counted here
            // rather than from the file list because "under the source" and
            // "asked" are the same event only at this line — a nearer scope
            // that claimed first, or an owner that already stopped the search,
            // means the scopes below never saw the file at all.
            s.offered.set(s.offered.get() + 1);
            // Match on the logical path: `file` patterns strip spent axes
            // (suffix or prefix) before the glob sees the path.
            let r = apply_rules(&s.rules, &s.formats, &scope_rel, has_identity);
            if r.claimed.is_some() {
                claim = Some((s, r, scope_rel));
                break;
            }
            if let Some(o) = s.owned() {
                owner_passed = Some(o);
            }
        }
        let Some((scope, routing, scope_rel)) = claim else {
            // A scope owns its source: what its rules did not claim is not
            // content, and it leaves without a word — that silence is
            // `load_dir`'s extension filter, which never said anything either.
            // Under the root scope there is no narrowing to have missed, so
            // the site is simply missing a rule, which is the error it was.
            if owner_passed.is_some() {
                continue;
            }
            bail!("no rule supplies a route for {}", f.path.display());
        };
        let extracted = filename::extract(routing.formats, &path_key(&scope_rel));
        let (logical_rel, locale) = match &extracted {
            Some(m) => {
                let loc = m
                    .axes
                    .get("locale")
                    .cloned()
                    .unwrap_or_else(|| cfg.i18n.default.clone());
                (with_logical(&scope_rel, &m.logical_stem), loc)
            }
            None => (scope_rel.clone(), cfg.i18n.default.clone()),
        };
        // Root-relative again for everything that is about the FILE rather
        // than about a rule: schema governance, `logical` identity, the claim.
        let logical_root: PathBuf = match scope.owned() {
            Some(src) => src.join(&logical_rel),
            None => logical_rel.clone(),
        };
        scope.found.set(scope.found.get() + 1);
        check_on_demand_cover(&logical_rel, &routing)?;
        let on_demand = routing.on_demand;
        let marker_defaults = markers.defaults_for(&f.rel);
        let defaults = merged_defaults(&marker_defaults, routing.defaults);

        // STORED rather than re-derived later: recomputing the stem from
        // `logical` via `file_stem()` returns `v1` for `v1.2-release.md`.
        let stem = logical_rel
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let key = extracted.as_ref().map(|m| m.key.clone());
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

        // Front matter, read once. The posts loader read every file whole and
        // the tree loader read only the front-mattered ones; the one walk reads
        // a file that has a block to parse, and reads a blockless one below
        // only if it turns out to render.
        // A sidecar is the same struct read from the other file (IO.md I8), so
        // everything below this line — the title rung, `permalink:`, the date
        // precedence, the declared fields — is one code path for both
        // spellings of identity. A sidecar'd row's `body_bytes` is 0 until the
        // rendering law says otherwise, because the sidecar is not the row's
        // body and the row's own bytes have not been read.
        let (fm, mut body_bytes) = match (f.has_front_matter, sidecar) {
            (true, _) => read_front_matter(&f.path)?,
            (false, Some(sc)) => (sc.front.clone(), 0),
            (false, None) => Default::default(),
        };
        // §5b: a governed row's extra front matter is validated — an undeclared
        // key or wrong type fails the load naming the file. Ungoverned rows
        // stay as tolerant as they always were. Governance follows the LOGICAL
        // path (§6f): a translation is governed by the same `.schema.toml` as
        // its original — and the ROOT-relative one, because that walk keys them
        // root-relative (a `_posts/.schema.toml` is registered under `_posts`,
        // and resolving a bare filename would never find it).
        let parent = logical_root.parent().unwrap_or(Path::new("")).to_path_buf();
        // Every row is governed (§4e): declare a field before you use it.
        let schema = schemas.resolve(scope.name, &parent);
        // Identity's own errors name the file identity was WRITTEN in, which
        // is the file the author has to edit: the row for a block, the sidecar
        // for a sidecar. Every rung below (markers, rules, the profile) keeps
        // naming the row, because that is what those rungs are about.
        let identity_path = sidecar.map_or(f.path.as_path(), |sc| sc.path.as_path());
        let mut checked = match has_identity {
            true => schema::validate(&schema, &fm.extra, identity_path)?,
            false => Default::default(),
        };
        // The engine's own four arrive on named front-matter fields rather
        // than in `extra`, so they are seeded here — nearest writer first.
        schema::cascade_front(&schema, &fm, &mut checked, identity_path)?;
        // Markers and rules fill whatever front matter left unset (§4b).
        schema::apply_defaults(&schema, &defaults, &mut checked, &f.path)?;
        // …and rung 0 overrules all three (§2, MERGE.md E1). Above `cascade`,
        // so a forced `theme` or `toc` is what the row wears.
        schema::force(&cfg.forced, &schema, &mut checked, &f.path)?;
        let worn = cascade(&checked, &f.rel)?;

        // The law (IO.md I7c), asked once now that there is one walk to ask it
        // in. `rendered: true` stood in the posts loader and
        // `rendered: has_front_matter` in the tree's — each the config's answer
        // read off the wrong thing.
        //
        // **It reads the BLOCK, not identity** (IO.md I8), and that is the
        // whole of what a sidecar splits: a block is in the file, so a file
        // with one is a document whose remainder is a body; a sidecar is a
        // second file, and says nothing about the first one's bytes. So a
        // sidecar'd `.png` answers `front_mattered true` and `rendered false`,
        // which is §3's sentence — "a `.png` with a sidecar is a governed row
        // whose bytes are never parsed" — as two columns.
        let rendered = crate::shell::renders(f.has_front_matter, worn.shell.as_deref());
        // **A picture is not a document, and the description page is not
        // built.** IO.md §4a says an image with a sidecar *can* wear an html
        // output — the object's description page. It needs an output whose
        // content is not the row's bytes, which the model has (facts at
        // planning, content at materialization) and the engine does not.
        //
        // **No item owns it.** I8 wrote "one line to delete when I11/I12
        // lands"; both landed and neither built it — I11 gave an input a
        // second ADDRESS and I12 gave it derived BYTES, and a description page
        // is neither: it is an output whose content is a rendered template
        // over the row's fields. Nothing in the ledger is going to build that,
        // so this refusal is not an interim, and the honest thing for the next
        // reader is a shape rather than an item number: the day something
        // materializes an html output from a row's FIELDS instead of its
        // bytes, this check is what has to move. Until then the shape is
        // refused where the author wrote it. Deleting the check does not make
        // the page work — measured: the render path reads the row's file as
        // text and the load dies on `stream did not contain valid UTF-8`,
        // naming a file and no reason.
        if rendered && object_shaped {
            bail!(
                "{}: shell = {:?} would render this file as a document, and its \
                 bytes are a picture. An image's own outputs are its bytes (IO.md \
                 §4a); a description PAGE for one is a second output and is not \
                 built yet. Route it `raw`{}.",
                f.rel.display(),
                worn.shell.as_deref().unwrap_or("-"),
                match sidecar.is_some() {
                    true =>
                        " — the sidecar still gives it a title, fields and a \
                             place in the link graph",
                    false => "",
                }
            );
        }
        // A blockless row that renders is the one shape whose body was not
        // already in hand: it is ALL body. The posts loader read every post
        // whole and so had it; the tree loader read nothing and reported zero;
        // one walk has to pick one answer, and `body_bytes` is a fact about
        // the row rather than about which loader found it.
        if rendered && !f.has_front_matter {
            body_bytes = read_front_matter(&f.path)?.1;
        }
        // Front matter beats the filename, the precedence every other field
        // has (§4b) — and it is read ABOVE routing now, so a `date:` reaches a
        // dated route template on any row rather than only on a post. That is
        // I6's recorded other half of "one supplier", and the seam it named.
        let date = match &fm.date {
            Some(s) => Some(front_matter_date(s, identity_path)?),
            None => from_name,
        };
        // The engine-fallback rung, below front matter and every default
        // (§4b). A row that is not a document has no title to imply: its
        // content is its bytes.
        let title = match (fm.title, rendered) {
            (Some(t), _) => Some(t),
            (None, true) => Some(implied_title(&slug)),
            (None, false) => None,
        };
        // A degenerate row carries no front matter, so its title IS the
        // implied one — the warning states the derivation rather than reading
        // back a value it would have to prove is there.
        //
        // This one asks IDENTITY, where `renders` above asks the block: the
        // warning exists to nudge an unnamed row towards a name, and a sidecar
        // is a name. Two questions, two inputs, one shell (IO.md I8).
        if let Some(sh) = crate::shell::degenerate(has_identity, worn.shell.as_deref()) {
            warnings.push(degenerate_warning(&f.rel, sh, &implied_title(&slug)));
        }

        // The embed policy's half of the address question (IO.md §4a, I11).
        // A rule said `embed = true`, so this row has NO canonical URL; what it
        // has instead is the hash address the policy publishes, computed here —
        // at planning, from the input bytes and the identity transform's
        // parameters — because §1's law says an output's facts exist before
        // anything renders and this address is one of them.
        //
        // Below the `permalink` branch on purpose: front matter beats a rule
        // (§4b), and a `permalink:` is a canonical address written by hand, so
        // a row that carries one is routed however its rule was written.
        let mut strong_url: Option<String> = None;
        if routing.embed && fm.permalink.is_none() {
            strong_url = Some(embed_address(cfg, &embed_subset, &f, routing.pattern)?);
        }
        // A `permalink` is a literal URL, spending no axis; otherwise each of
        // the rule's template(s) is rendered by the one supplier — path tokens,
        // the extractor's, axis and locale placeholders preserved for
        // per-member selection.
        let route_templates: Vec<String> = if let Some(p) = &fm.permalink {
            vec![p.clone()]
        } else if strong_url.is_some() {
            Vec::new()
        } else {
            if routing.templates.is_empty() {
                bail!("no rule supplies a route for {}", f.path.display());
            }
            RouteTokens {
                cfg,
                rel: &logical_rel,
                path: &f.path,
                hash: Default::default(),
                date,
                key: key.as_ref(),
                slug: &slug,
            }
            .render_all(routing.templates, routing.pattern, &f.path)?
        };
        // Empty for an embed-addressed row, and that is the shape the two
        // slots take today: `url` is the canonical address and this row has
        // none, so every reader that asks "where does this land canonically"
        // gets the honest answer rather than a hash dressed as a route.
        let url = match route_templates.is_empty() {
            true => String::new(),
            false => canonical_url(cfg, &route_templates, &locale, routing.formats)?,
        };

        let logical = logical_root.to_string_lossy().to_string();
        // q45: a row named by some view's `content` is claimed — every locale
        // variant of it (the claim is on the logical identity).
        let claimed = claims.contains_key(logical.as_str());
        if claimed && !f.has_front_matter {
            bail!(
                "view {}: content {logical:?} has no front-matter block, so it \
                 is a static file, not a claimable row",
                claims[logical.as_str()]
            );
        }
        let row = Row {
            axis: row_axes(cfg, &route_templates, routing.formats),
            route_templates,
            width: None,
            height: None,
            // Assigned by `insert_rows`, which is where rows become the
            // database's rather than the loader's.
            key: Default::default(),
            on_demand,
            collection: scope.name.to_string(),
            // Which rule of which scope claimed this row: the ordering law's
            // one observable (IO.md I7d), printed by `grackle explain`.
            rule: routing.claimed.map(str::to_string),
            slug,
            stem,
            body_bytes,
            path: f.path,
            // ROOT-relative, so `path`/`dir` mean one thing on every row. Rule
            // globs match the SCOPE-relative form: `match = "hidden/**"` is
            // relative to `_posts`.
            rel: f.rel,
            // A row's identity may live in a second file, so its change stamp
            // has to (IO.md I8). Without the fold, editing a sidecar changes a
            // row's title and nothing notices: `version` is what the
            // incremental machinery compares.
            version: match sidecar {
                Some(sc) => f.version ^ sc.version,
                None => f.version,
            },
            url,
            strong_url,
            rendered,
            // The tree's old page/static gate IS this fact, and since I7c it is
            // no longer the whole of the gate: the fact is one clause of the
            // law above, which the shell can also satisfy. Since I8 it is also
            // no longer the same question as "does this file open with `---`" —
            // a sidecar answers it too, and `sidecar` says which.
            front_mattered: has_identity,
            sidecar: sidecar.is_some(),
            size: f.size,
            title,
            description: fm.description,
            order: fm.order,
            date,
            // Copied into the column for indexes; left in `fields` too so
            // filters and fill see the declared list.
            tags: match checked.values.get("tags") {
                Some(filter::Value::List(t)) => t.clone(),
                _ => Vec::new(),
            },
            theme: worn.theme,
            shell: worn.shell,
            fields: checked.values,
            images: checked.images,
            locale,
            logical,
            claimed,
            // IO.md §2's join: filled by `join_outputs`/`join_arrangement`
            // once the routes exist. Not derivable here — a row cannot say
            // whether it lands until the route table says so.
            output: None,
            alternates: Vec::new(),
            viewed_by: Vec::new(),
        };
        // **The partition, keyed off the fact** (IO.md I7e). The three vectors
        // are the three key lists (`post_ix`, `page_ix`, `object_ix`) and
        // nothing else — there is one constructor above them, so which one a
        // row joins is a question about the row rather than about how it was
        // built. A posts scope's rows are its own by role; an image is an image
        // because the extension fact says so, whichever scope claimed it. A
        // posts scope is the one that OWNS a proper source — the role read off
        // `source` now that `kind` is gone.
        if scope.owned().is_some() {
            posts.push(row);
        } else if object_shaped {
            objects.push(row);
        } else {
            pages.push(row);
        }
    }

    // Every claim must have found its row — a typo'd content path is a
    // load error naming the view, not a silently bare landing.
    for (path, view) in &claims {
        if !pages
            .iter()
            .chain(posts.iter())
            .any(|p| p.claimed && p.logical == *path)
        {
            bail!("view {view}: content {path:?} names no row in the tree");
        }
    }
    // Dimensions are a property of the FILE, so they belong on the row where
    // a query can reach them rather than in a build-time side map. One header
    // read each, in parallel — sequentially this is ~200ms on a corpus with
    // 850 images, which is a third of the whole build. Keyed off the extension
    // fact like the two indexes (IO.md I7e): the reason to open a file looking
    // for a width is that it is a picture.
    objects.par_iter_mut().for_each(|o| {
        if let Ok((w, h)) = image::image_dimensions(&o.path) {
            o.width = Some(w);
            o.height = Some(h);
        }
    });

    for s in scopes {
        warnings.extend(dead_rules(s.name, &s.rules, s.found.get()));
        // The other side of `dead_rules`' `found == 0` (IO.md IR8): where it
        // falls silent because a whole scope came up empty, this asks whether
        // the source was empty or the globs were wrong.
        warnings.extend(empty_source(s));
    }
    Ok((posts, pages, objects))
}

/// Front matter of one row, and the size of its body.
///
/// A parse failure is a LOAD ERROR naming the file, never an empty schema —
/// an unquoted `title: A: B` must not ship a silently titleless page (§4). An
/// EMPTY block is not a failure, though: `---\n---` is a file that carries
/// identity and says nothing with it, which is what the posts loader always
/// read it as, and one walk keeps the more permissive of the two readings
/// because the other one was an accident of never being asked.
///
/// `body_bytes` comes from the same read, so the field means the same thing on
/// every row.
fn read_front_matter(path: &Path) -> Result<(store::FrontMatter, usize)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (yaml, body) = store::split_front_matter(&text);
    let fm = match yaml.trim().is_empty() {
        true => store::FrontMatter::default(),
        false => serde_yaml_ng::from_str(yaml)
            .with_context(|| format!("front matter of {}", path.display()))?,
    };
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

/// The join's OUTPUT half, from the input side (IO.md §2): every row's
/// `output` and `alternates`, read off the routes that name it.
///
/// **Facts at planning.** This is called the moment the route list exists,
/// before `build_views` — which is what makes `output` a column a view's
/// `where` may read and get a true answer from. `viewed_by` and `inputs`
/// cannot be built here (a view's membership is what produces them), and that
/// asymmetry is the whole reason they are not filter columns.
///
/// A recomputation rather than an increment, because it is called twice: once
/// at minting, and again where q45's TEMPLATED landing retracts a claimed
/// row's routes. Cheap (one pass over routes, one over rows) and, more to the
/// point, it cannot drift from the route table by construction — the field is
/// the table's shadow, and a shadow is not a second opinion.
///
/// **The canonical one is the all-canonical member-tuple**, which is the
/// settled axis design: a row published once has an empty tuple and so is
/// trivially canonical; a row on N axes has exactly one tuple where every
/// coordinate is the first-declared value, and that is what `rel="canonical"`
/// names and the only one a fold over the output pool sees.
fn join_outputs(db: &mut SiteDb) {
    let mut by_row: HashMap<grackle_db::Key, (Option<grackle_db::Key>, Vec<grackle_db::Key>)> =
        HashMap::new();
    for r in db.routes.iter() {
        let Some(k) = &r.row else { continue };
        let e = by_row.entry(k.clone()).or_default();
        if r.axis.iter().all(|m| m.canonical) {
            e.0 = Some(r.id.clone());
        } else {
            e.1.push(r.id.clone());
        }
    }
    for row in db.rows.iter_mut() {
        let (canonical, mut alternates) = by_row.remove(&row.key).unwrap_or_default();
        // Routes are minted in axis-product order and only sorted later, so
        // the field sorts itself: an order that depends on when the pass ran
        // is an order a test cannot pin.
        alternates.sort();
        row.output = canonical;
        row.alternates = alternates;
    }
}

/// The join's ARRANGEMENT half (IO.md §2): each row's `viewed_by`, and each
/// output's `inputs`.
///
/// **Membership at materialization planning.** Both halves read the same
/// finished fact — which rows a view materialized — so they are built
/// together, at the one point where that fact is complete: after
/// `resolve_pool_folds`, with the route list final and q45's claims settled.
/// That is later than any filter the engine runs, which is why neither is a
/// filter column (see `route_schema`'s note).
///
/// `inputs` gets the ROW-LEVEL closure the invalidation edge set needs, minus
/// the half that cannot exist yet: the citation edges are facts about
/// *content*, so `build::join_citations` adds them after the write pass. What
/// lands here is everything planning knows —
///
/// - a row-backed output: the row it renders;
/// - a landing: the row it claims as content (literal or templated), which is
///   the one input a landing has that its member list does not name;
/// - a view route: its members, in materialization order;
/// - a fold over the output pool: the rows behind the routes it selected —
///   `route_members` is the output→output edge, and this is the same edge
///   followed one step further into the inputs database.
fn join_arrangement(cfg: &Config, db: &mut SiteDb) {
    // Arrangement, from the routes that did the arranging.
    let mut viewed_by: HashMap<grackle_db::Key, Vec<grackle_db::Key>> = HashMap::new();
    for r in db.routes.iter() {
        for m in &r.members {
            viewed_by.entry(m.clone()).or_default().push(r.id.clone());
        }
    }
    for row in db.rows.iter_mut() {
        let mut seen = viewed_by.remove(&row.key).unwrap_or_default();
        seen.sort();
        seen.dedup();
        row.viewed_by = seen;
    }

    // The inputs half. Resolved against the row store, so it is computed
    // before the borrow that writes it.
    let mut inputs: Vec<(grackle_db::Key, Vec<grackle_db::Key>)> = Vec::new();
    for r in db.routes.iter() {
        let mut ins: Vec<grackle_db::Key> = Vec::new();
        if let Some(k) = &r.row {
            ins.push(k.clone());
        }
        // q45: a landing's body is a row nothing else names — a literal claim
        // lives on the view, a templated one on the route.
        let claimed = r.content.clone().or_else(|| {
            let v = cfg.views.get(r.view.as_deref()?)?;
            let c = v.content.as_deref()?;
            (!crate::config::is_templated(c)).then(|| c.to_string())
        });
        if let Some(logical) = claimed {
            // §6f: a landing route is per locale, and so is the row it claims.
            // The route's `locale` is None for the default one, which is the
            // same spelling `route_locale` writes.
            for k in db.by_logical.get(&logical).into_iter().flatten() {
                let matches = db.rows.get(k).is_some_and(|row| {
                    let want = (row.locale != cfg.i18n.default).then_some(row.locale.as_str());
                    r.locale.as_deref() == want
                });
                if matches {
                    ins.push(k.clone());
                }
            }
        }
        ins.extend(r.members.iter().cloned());
        // A fold over the output pool arranges OUTPUTS; the rows behind them
        // are what a change to any of them would move.
        for rk in &r.route_members {
            if let Some(row) = db.routes.get(rk).and_then(|o| o.row.clone()) {
                ins.push(row);
            }
        }
        ins.sort();
        ins.dedup();
        inputs.push((r.id.clone(), ins));
    }
    for (id, ins) in inputs {
        if let Some(r) = db.routes.get_mut(&id) {
            r.inputs = ins;
        }
    }
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
    // The FIRST tree collection (`source = "."`) supplies the content lists:
    // the tree is the root, walked once, and every other scope reads out of
    // that same walk by its own rules, so a second tree would have nothing of
    // its own to read.
    let tree_c = cfg.collections.values().find(|c| c.is_tree());
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
    // .gitignore, `exclude` and `themes/` defences as the marker scan
    // (`walker_declarations` owns all three).
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
    // Sidecars ride the same walk (IO.md I8): a sidecar is a declaration — it
    // says what a file IS, the way a marker says what a directory's rows are —
    // and a declaration must not be silenceable by a *content* statement. This
    // walk applies `exclude` to directories only, which is exactly what keeps
    // grack.com's `exclude = ["*.toml"]` from unspeaking every sidecar on the
    // site (MERGE.md R1's narrowing, one family newer).
    let mut sidecars = Sidecars::default();
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
        } else {
            sidecars.offer(entry.path(), rel)?;
        }
    }
    db.stats.sidecars = sidecars.found;
    db.sections.sort();
    // The site vocabulary travels with the database (§4e).
    db.declared = schemas.declared_schema();

    // ONE walk (IO.md I7d), one ordered rule sequence over it, and since I7e
    // one row constructor under that. The three vectors are a PARTITION of its
    // result, not three loaders and not three shapes of row: `posts` is the
    // claiming scope's role, `objects` is the extension fact, and `pages` is
    // everything else.
    let t = std::time::Instant::now();
    let scopes = scopes(cfg)?;
    let (post_rows, page_rows, objects) = walk_site(
        cfg,
        &scopes,
        &markers,
        &sidecars,
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
    // it after the render pass. IO.md §4a: an embed-addressed row has none
    // EVER — no rule minted one — and its strong address publishes on the same
    // pull, for the same reason: what nothing cites never materializes.
    for p in db
        .rows
        .iter()
        .filter(|p| !p.claimed && !p.on_demand && p.strong_url.is_none())
    {
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
    // IO.md §2, the join's output half — here because this is the earliest
    // point its answer is complete, and because `build_views` below is the
    // first reader: a set spelled `where = "!output"` must select the rows
    // that land nowhere, not every row in the store.
    join_outputs(&mut db);
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
            // (A `kind != View` guard stood here and was DELETED at I13, not
            // respelled: the `view` column below already asks it — "is this a
            // view route" is that column being non-empty, IO.md §3.)
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
        // …and with the routes, the join fact that named them. This is the one
        // place a planning fact is corrected rather than decided, for the
        // reason the two lines below are: a TEMPLATED claim is not knowable
        // until the group keys exist. A literal claim never mints the route at
        // all, so it needs no correction.
        join_outputs(&mut db);
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
                // "Is this a view route" is the `view` column being non-empty
                // (IO.md §3, I13). In the second find it was already being
                // said — the route NAMES the owning view — so only the term
                // is gone there.
                .find(|r| {
                    r.view.is_some()
                        && r.content.as_deref() == Some(p.logical.as_str())
                        && r.locale == route_locale(&p.locale)
                })
                .map(|r| r.url.clone())
                .or_else(|| {
                    let owner = *claims.get(p.logical.as_str())?;
                    db.routes
                        .iter()
                        .find(|r| {
                            r.view.as_deref() == Some(owner)
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
    // IO.md §2, the join's arrangement half. Last, because it is the half that
    // reads what every pass above decided — and the render pass adds the
    // citation edges to `inputs` on top (`build::join_citations`).
    join_arrangement(cfg, &mut db);
    // IO.md §5: the graph exists the moment the join does, so its one refusal
    // is asked here — at load, like relations' dependency order, and never as
    // a render surprise.
    check_graph(&db)?;
    Ok(db)
}

/// IO.md §5's tripwire: no output's CONTENT may depend on its own.
///
/// **This cannot fire today, and it is not decoration.** The graph's content
/// edges run input → output (a row's bytes feed an output; nothing derives an
/// output from another output's bytes yet), so the content subgraph is
/// bipartite with every source on the inputs side and has no cycle to find.
/// What CAN loop is the facts half — a pool fold with no `where` selects its
/// own route, so `/all.xml` is its own `route_members` member on any site that
/// writes one — and that is legal by §4's column rule: a facts edge demands
/// only what planning already finished. The two are told apart by the edge
/// label and by nothing else, which is what this call is really guarding.
///
/// The mutation that proves it: label `route_members` as `Demand::Content` in
/// `graph::Graph::of` and every site with a from-less fold stops loading, this
/// error naming the fold's own URL.
///
/// **The check is armed for nothing named** (corrected at I13; `graph.rs` and
/// `io_graph.rs` were corrected at I12 and this third copy was missed). I10
/// expected renditions to bring the first output→output content edge; I12
/// measured that they do not, because the transform reads the INPUT's bytes
/// and the citing page reads the rendition's ADDRESS, which the hashing law
/// makes a planning fact. Nothing in the engine derives an output from another
/// output's content, so no live fixture is owed by any item — the day
/// something does, this is the tripwire it trips.
fn check_graph(db: &SiteDb) -> Result<()> {
    if let Err(cycle) = grackle_model::graph::Graph::of(db).check_acyclic() {
        bail!(
            "dependency cycle: {} — an output's content may not depend on its own (IO.md §5)",
            grackle_model::graph::describe_cycle(&cycle)
        );
    }
    Ok(())
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

    /// The engine's cascade keys plus `toc` (flag family). Tests that typed
    /// the cascade list by hand would be asserting their own copy; this reads
    /// the one list the loader reads (`schema::CASCADE`).
    fn governed() -> BTreeMap<&'static str, schema::FieldType> {
        let mut m: BTreeMap<_, _> = schema::CASCADE.iter().copied().collect();
        m.insert("toc", schema::FieldType::Bool);
        m
    }

    /// The whole row-side cascade, in the order `load` runs it: validated
    /// extra (nearest declared fields), cascade_front for named engine keys,
    /// then markers and rules. Driving all three is the point — the type
    /// checking C1 added lives in the middle call, and a test that only
    /// called `cascade` could not see it.
    fn worn(
        schema: &BTreeMap<&str, schema::FieldType>,
        yaml: &str,
        pairs: &[(&'static str, toml::Value)],
    ) -> Result<(Cascaded, schema::Fields)> {
        let fm = front(yaml);
        let mut fields = schema::validate(schema, &fm.extra, Path::new("p.md"))?;
        schema::cascade_front(schema, &fm, &mut fields, Path::new("p.md"))?;
        schema::apply_defaults(schema, &defaults(pairs), &mut fields, Path::new("p.md"))?;
        let worn = cascade(&fields, Path::new("p.md"))?;
        Ok((worn, fields))
    }

    #[test]
    fn front_matter_beats_a_default() {
        let d = [("theme", text("inherited")), ("toc", yes())];
        let (c, fields) = worn(&governed(), "theme: own\ntoc: false\n", &d).unwrap();
        assert_eq!(c.theme.as_deref(), Some("own"));
        assert_eq!(fields.values.get("toc"), Some(&grackle_db::Value::Bool(false)));
    }

    /// Every field a silent row inherits.
    #[test]
    fn a_silent_row_inherits_every_cascading_field() {
        let d = [
            ("theme", text("t")),
            ("shell", text("light_html")),
            ("toc", yes()),
        ];
        let (c, fields) = worn(&governed(), "{}", &d).unwrap();
        assert_eq!(c.theme.as_deref(), Some("t"));
        assert_eq!(c.shell.as_deref(), Some("light_html"));
        assert_eq!(fields.values.get("toc"), Some(&grackle_db::Value::Bool(true)));
    }

    #[test]
    fn an_unset_field_stays_unset() {
        let (c, fields) = worn(&governed(), "{}", &[]).unwrap();
        assert_eq!(c.theme, None);
        assert!(!fields.values.contains_key("toc"));
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

    /// C1: a rule or marker default for a declared field is TYPE-CHECKED.
    /// `toc = "true"` used to skip `apply_defaults` entirely and read back
    /// through `as_bool()` — `None`, so `false`, so no outline and nothing said.
    ///
    /// Mutation check: exempt declared keys in `apply_defaults` again and this
    /// returns `Ok` with no `toc` — the silence, restored.
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

    /// The cascade keys are governed like any other name (§4e, "every row is
    /// governed"): a site that declared none of them and a row that wears one
    /// is a load error, not a value only the engine can see.
    #[test]
    fn an_undeclared_cascade_key_is_a_load_error() {
        let empty = BTreeMap::new();
        let e = worn(&empty, "slot: root\n", &[]).unwrap_err().to_string();
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
    /// A marker sets a cascade key or a declared flag exactly as it sets any
    /// other field, front matter still nearer than both.
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
        assert_eq!(
            fields.values.get("toc"),
            Some(&grackle_db::Value::Bool(true)),
            "a marker's bool arrives as a bool"
        );

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

    /// Cascade keys keep a fixed type: declaring `theme` an int would type a
    /// rule's value one way and have `cascade` read it the other.
    #[test]
    fn a_cascade_key_may_not_be_redeclared_at_another_type() {
        let mut s = schema::Schemas::new(grackle_model::row_schema());
        let e = s
            .set_site("theme = { type = \"int\" }".parse().unwrap(), "[schema]")
            .unwrap_err()
            .to_string();
        assert!(e.contains("cascade"), "{e}");
        assert!(e.contains("declared string"), "{e}");
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
    /// in `walk_site` and nothing is reported at all.
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

    /// A posts scope over a populated source, with one glob and a typo in it
    /// (`markdwn`). Nothing claims the posts; a scope owns its source, so they
    /// leave the walk silently; `dead_rules` sees `found == 0` and says
    /// nothing. Pre-I7d this was a load error, so the build that used to fail
    /// now succeeds with an empty blog — IO.md IR8's regression, and the
    /// warning is the fix.
    ///
    /// The control is in the same site: the tree scope claims `about.md`, so
    /// its `**/*` is neither dead nor empty and only one line is printed.
    ///
    /// Mutation check: delete the `empty_source` call in `walk_site` and the
    /// site is silent again.
    #[test]
    fn a_sourced_scope_offered_files_and_claiming_none_warns() {
        let dir = site(
            "empty-source",
            &[
                ("grackle.toml", TYPO_CONFIG),
                (
                    "_posts/2020-01-01-hello.md",
                    "---\ntitle: Hello\n---\n\nHi.\n",
                ),
                (
                    "_posts/2020-02-02-again.md",
                    "---\ntitle: Again\n---\n\nHi.\n",
                ),
                ("about.md", PAGE),
            ],
        );
        let w = warnings(&dir);
        assert_eq!(w.len(), 1, "one empty scope, and only one: {w:?}");
        assert!(w[0].contains("collection posts"), "names the scope: {w:?}");
        assert!(w[0].contains(r#""_posts""#), "names the source: {w:?}");
        assert!(w[0].contains("`**/*.markdwn`"), "names the globs: {w:?}");
        assert!(
            w[0].contains("2 files"),
            "says how many were offered: {w:?}"
        );
    }

    /// The first suppression, and it is documented behavior (§4d): a site with
    /// no `_posts/` pays nothing for inheriting a rule about one. Zero offered,
    /// so the scope's silence is the source's, not a glob's.
    ///
    /// Same config as the probe above, typo and all — which is the point: the
    /// glob is exactly as wrong here, and there is nothing to say about it.
    #[test]
    fn an_absent_source_stays_silent() {
        let dir = site(
            "absent-source",
            &[("grackle.toml", TYPO_CONFIG), ("about.md", PAGE)],
        );
        assert_eq!(warnings(&dir), Vec::<String>::new());
    }

    /// The second suppression: a source that EXISTS and holds nothing — a
    /// directory waiting for its first post. Offered zero, so it reads exactly
    /// like the absent one, which is why neither needs an exception.
    #[test]
    fn an_empty_present_source_stays_silent() {
        let dir = site(
            "empty-dir-source",
            &[("grackle.toml", TYPO_CONFIG), ("about.md", PAGE)],
        );
        // `site` writes files; an empty directory has to be made by hand, and
        // it is the whole of what this test varies from the one above.
        std::fs::create_dir_all(dir.join("_posts")).unwrap();
        assert_eq!(warnings(&dir), Vec::<String>::new());
    }

    /// grack.com's shape, at fixture scale: `_drafts/caret/` is a post beside
    /// a bundle of images, an `.rtf` and an `.xcf` that no rule claims. The
    /// scope claimed something, so the unclaimed remainder is the ownership law
    /// working as designed and not a word is said about it — which is what
    /// keeps stderr parity on all six corpus builds.
    ///
    /// Mutation check: key the warning on `found < offered` instead of
    /// `found == 0` and this site starts reporting a deliberate arrangement.
    #[test]
    fn a_claiming_scope_with_unclaimed_extras_stays_silent() {
        let dir = site(
            "unclaimed-extras",
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
name = "posts"
source = "_drafts"

  [[collections.rules]]
  match = "**/*.{md,markdown}"
  route = "/blog/{slug}/"

[[collections]]
name = "entries"
source = "."

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"
"#,
                ),
                ("_drafts/caret/index.md", "---\ntitle: Caret\n---\n\nHi.\n"),
                ("_drafts/caret/sketch.rtf", "{\\rtf1}\n"),
                ("_drafts/caret/cursor.xcf", "gimp\n"),
                ("about.md", PAGE),
            ],
        );
        assert_eq!(warnings(&dir), Vec::<String>::new());
    }

    /// One posts scope with a typo'd glob, one tree scope, no base. Shared by
    /// the probe and both suppressions so that the only thing that varies
    /// between them is what is on disk.
    const TYPO_CONFIG: &str = r#"
extends = "none"
[site]
url = "https://example.com"
title = "T"
author = "A"

[[collections]]
name = "posts"
source = "_posts"

  [[collections.rules]]
  match = "**/*.markdwn"
  route = "/blog/{slug}/"

[[collections]]
name = "entries"
source = "."

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"
"#;
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
