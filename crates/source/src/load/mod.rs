//! The load: one walk of the site, and the rows it produces.
//!
//! Reads the tree, applies collection rules, routes every row, and hands the
//! result to `SiteDb::insert_rows` — the only way into the database.

mod join;
mod walk;

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use globset::{Glob, GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

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
        // i18n prefix (`fr/recipes/dal.md`) still matches `recipes/**`.
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
///
/// **The one spend test.** Every reader of a route template asks this question —
/// the materializer, the view loader's declared-but-never-spent check, the link
/// resolver — and each used to ask it with a `format!("{{{name}}}")` of its own,
/// which is how a route written `{axis:theme}` came to load in one place and
/// fail in another (MERGE.md C5). `pub` for that reason: a second spelling of
/// this predicate is the bug, not the convenience.
pub fn spends(tmpl: &str, axis: &str) -> bool {
    tmpl.contains(&format!("{{{axis}}}")) || tmpl.contains(&format!("{{axis:{axis}}}"))
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
/// non-canonical i18n member must be spent by a template, same as any other axis.
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
                self.cfg.axes.contains_key(bare).then(|| format!("{{{k}}}"))
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

/// The canonical address of one row: every route axis at its canonical value,
/// plus the i18n member when templates spend that axis (§6f). `select_path`
/// drops a canonical segment where a shorter template allows.
fn canonical_url(
    cfg: &Config,
    templates: &[String],
    pairing_value: &str,
    file: &[filename::FilePattern],
) -> Result<String> {
    let row_axis = row_axes(cfg, templates, file);
    let mut coords: Vec<Coord> = row_axis
        .iter()
        .map(|ra| Coord {
            axis: &ra.name,
            value: cfg.axes[&ra.name].canonical().unwrap_or_default(),
            canonical: true,
        })
        .collect();
    if let Some((axis_name, _)) = cfg.pairing_axis() {
        if templates.iter().any(|t| spends(t, axis_name)) {
            let canon = cfg
                .pairing_axis()
                .and_then(|(_, a)| a.canonical())
                .unwrap_or(cfg.i18n.default.as_str());
            coords.push(Coord {
                axis: axis_name,
                value: pairing_value,
                canonical: pairing_value == canon,
            });
        }
    }
    select_path(templates, &coords)
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
    schemas.set_site(cfg.schema.fields.clone(), "grackle.toml [schema]")?;
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
    let (post_rows, page_rows, objects) = walk::walk_site(
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
    let dated_keep = cfg.pairing_axis().map(|(_, a)| {
        (
            a.field.as_str(),
            a.canonical().unwrap_or(cfg.i18n.default.as_str()),
        )
    });
    db.insert_rows(
        sort_posts(post_rows),
        page_rows,
        objects,
        dated_keep,
    )?;
    walk::resolve_image_fields(&db, &schemas)?;
    db.stats.index_ms += t_index.elapsed().as_secs_f64() * 1000.0;

    // Unified route list.
    let t = std::time::Instant::now();
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
        let one = |url: String, axis: Vec<AxisMember>| {
            let mut fields = {
                let mut f = p.fields.clone();
                for m in axis.iter().filter(|m| m.field == "shell") {
                    f.insert("shell".to_string(), grackle_db::Value::Str(m.value.clone()));
                }
                f
            };
            // Rows keep every axis field; routes leave the pairing axis's
            // canonical unstamped so filters see Null (same as other axes).
            if let Some((name, _)) = cfg.pairing_axis() {
                if let Some(value) = cfg.axis_on(p, name) {
                    cfg.stamp_axis_field(&mut fields, name, &value);
                }
            }
            Route {
                row: Some(p.key.clone()),
                source: Some(p.path.clone()),
                // The row's fields, with one correction: a member of an axis over
                // `shell` IS a different serialization of the same row (q53's md
                // twin), so THIS output left through the member's shell, not the
                // row's. Only `shell` is corrected — it is the column IO.md §3
                // puts on the output side, and the axis's other field (`theme`) has
                // no reader on the route pool to lie to.
                fields,
                axis,
                // The row's identity fact, carried to the output side (IO.md §3)
                // for the same reason `fields` is: a fold over the route pool can
                // only filter on what the route answers.
                front_mattered: p.front_mattered,
                ..Route::new(url, kind)
            }
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
        // template (i18n a coordinate beside the theme members) so a canonical
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
        let pairing = cfg.pairing_axis();
        let pairing_value = pairing
            .and_then(|(n, _)| cfg.axis_on(p, n))
            .unwrap_or_else(|| cfg.i18n.default.clone());
        let pairing_canon = pairing
            .and_then(|(_, a)| a.canonical())
            .unwrap_or(cfg.i18n.default.as_str());
        for tuple in tuples {
            let url = {
                let mut coords: Vec<Coord> = tuple
                    .iter()
                    .map(|m| Coord {
                        axis: &m.axis,
                        value: &m.value,
                        canonical: m.canonical,
                    })
                    .collect();
                if let Some((axis_name, _)) = pairing {
                    if p.route_templates.iter().any(|t| spends(t, axis_name)) {
                        coords.push(Coord {
                            axis: axis_name,
                            value: pairing_value.as_str(),
                            canonical: pairing_value == pairing_canon,
                        });
                    }
                }
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
    join::join_outputs(&mut db);
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
            // A promise route repeats per i18n member/page, so the same message can
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
        join::join_outputs(&mut db);
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
    // resolved `content` is this row's logical path, for this i18n member); a LITERAL
    // one points at its owner view's bare route. A twin whose partition
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
                        && match cfg.pairing_axis() {
                            Some((n, _)) => cfg.same_on(*r, p, n),
                            None => true,
                        }
                })
                .map(|r| r.url.clone())
                .or_else(|| {
                    let owner = *claims.get(p.logical.as_str())?;
                    db.routes
                        .iter()
                        .find(|r| {
                            r.view.as_deref() == Some(owner)
                                && match cfg.pairing_axis() {
                                    Some((n, _)) => cfg.same_on(*r, p, n),
                                    None => true,
                                }
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
    // (q53: i18n members, and whatever follows them). Nothing produces N today, so
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
    join::join_arrangement(cfg, &mut db);
    // IO.md §5: the graph exists the moment the join does, so its one refusal
    // is asked here — at load, like relations' dependency order, and never as
    // a render surprise.
    join::check_graph(&db)?;
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
        assert_eq!(
            fields.values.get("toc"),
            Some(&grackle_db::Value::Bool(false))
        );
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
        assert_eq!(
            fields.values.get("toc"),
            Some(&grackle_db::Value::Bool(true))
        );
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
