//! One walk of the site; rows, routes, and `SiteDb::insert_rows`.

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
use crate::filename;
use crate::markers::{Defaults, Markers};
use crate::schema::{self, Schemas};
use crate::sidecar::Sidecars;
use crate::store;

struct CompiledRule<'a> {
    matcher: GlobMatcher,
    route: &'a [String],
    front_matter: Option<bool>,
    on_demand: bool,
    embed: bool,
    pattern: &'a str,
    formats: Vec<filename::FilePattern>,
    defaults: &'a BTreeMap<String, toml::Value>,
    inherited: bool,
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
                // Case-insensitive: a glob names a kind of file; shift key is not part of kind.
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

fn compile_formats(
    formats: &[String],
    axes: &filename::AxisValues<'_>,
) -> Result<Vec<filename::FilePattern>> {
    formats
        .iter()
        .map(|f| filename::FilePattern::compile(f, axes))
        .collect()
}

/// Dead rule warning (DESIGN.md §4): site-written rules only; skip inherited and `found == 0`.
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

/// Sourced scope offered files but claimed none: `offered > 0 && found == 0`.
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
         its source those files are not content and ship nowhere. \
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

fn implied_title(slug: &str) -> String {
    slug.replace('-', " ")
}

fn degenerate_warning(rel: &Path, shell: &str, title: &str) -> String {
    format!(
        "{}: no front-matter block, but a rule sends it through the \
         `{shell}` shell — so it renders as a degenerate row whose \
         title is implied from its slug: {title:?}. Add a `---` block to give \
         it identity, or route it `shell = \"raw\"` to ship it as bytes.",
        rel.display()
    )
}

struct Routing<'a> {
    claimed: Option<&'a str>,
    templates: &'a [String],
    pattern: &'a str,
    formats: &'a [filename::FilePattern],
    embed: bool,
    on_demand: bool,
    on_demand_cover: Vec<&'a str>,
    defaults: BTreeMap<&'a str, &'a toml::Value>,
}

/// First rule wins membership; first-writer-wins per default key (DESIGN.md §4).
fn apply_rules<'a>(
    rules: &'a [CompiledRule<'a>],
    collection_formats: &'a [filename::FilePattern],
    rel: &Path,
    has_front_matter: bool,
) -> Routing<'a> {
    let mut claimed: Option<&str> = None;
    let mut templates: &[String] = &[];
    let mut pattern: &str = "";
    let mut formats: Option<&[filename::FilePattern]> = None;
    let mut embed = false;
    // Embed supplies no template; track `addressed` separately from route winner.
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
        // Globs see logical path: strip spent file axes first.
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
        // Shadowed rules still govern; keeps them out of `dead_rules`.
        rule.governed.set(true);
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
        // `file` first-writer is independent of which rule won the route.
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

/// Embed policy address at load, or error if unreachable.
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

/// Markers before rules so `or_insert` cannot let a rule override them (§4b).
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

#[derive(Debug)]
struct Cascaded {
    theme: Option<String>,
    shell: Option<String>,
}

fn cascade(fields: &schema::Fields, whose: &Path) -> Result<Cascaded> {
    let worn = |key: &str| match fields.values.get(key) {
        Some(grackle_db::Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    // Closed shell vocabulary: typo would silently render the wrong tier.
    let shell = worn("shell");
    if let Some(sh) = shell.as_deref() {
        crate::shell::check_row(sh, whose)?;
    }
    Ok(Cascaded {
        theme: worn("theme"),
        shell,
    })
}

/// Profile forced fields on every route; view routes have no row (§2).
/// After all routes exist, before `resolve_pool_folds` filters them.
fn force_route_fields(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    if cfg.forced.is_empty() {
        return Ok(());
    }
    let declared = schemas.declared();
    let mut values: Vec<(String, grackle_db::Value)> = Vec::new();
    for (name, v) in &cfg.forced {
        let Some(ty) = declared.get(name.as_str()) else {
            bail!("the profile forces {name:?}, which no schema declares");
        };
        values.push((name.clone(), schema::typed(ty, name, v, "the profile")?));
    }
    for r in db.routes.iter_mut() {
        for (name, value) in &values {
            r.fields.insert(name.clone(), value.clone());
        }
    }
    db.forced_fields = values.into_iter().collect();
    Ok(())
}

/// Axes a rule's templates spend; file-pattern axes are row coordinates, not product dims.
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

/// `{name}` or `{axis:name}`: the one spend test every reader must share.
pub fn spends(tmpl: &str, axis: &str) -> bool {
    tmpl.contains(&format!("{{{axis}}}")) || tmpl.contains(&format!("{{axis:{axis}}}"))
}

pub fn fill_axis(tmpl: &str, axis: &str, value: &str) -> String {
    tmpl.replace(&format!("{{{axis}}}"), value)
        .replace(&format!("{{axis:{axis}}}"), value)
}

pub struct Coord<'a> {
    pub axis: &'a str,
    pub value: &'a str,
    pub canonical: bool,
}

/// Shortest template covering every non-canonical coord (§6f).
pub fn select_path(templates: &[String], coords: &[Coord]) -> Result<String> {
    let required: Vec<&str> = coords
        .iter()
        .filter(|c| !c.canonical)
        .map(|c| c.axis)
        .collect();
    // Ties keep declaration order.
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

const PATH_TOKENS: &[&str] = &["path", "dir", "stem", "name", "ext"];

/// Route-token supplier for one row.
struct RouteTokens<'a> {
    cfg: &'a Config,
    rel: &'a Path,
    path: &'a Path,
    hash: std::cell::RefCell<Option<Option<String>>>,
    date: Option<NaiveDate>,
    extracted: Option<&'a filename::FileMatch>,
    slug: &'a str,
}

impl RouteTokens<'_> {
    fn get(&self, k: &str) -> Option<String> {
        if let Some(v) = path_tokens(self.rel, k) {
            return Some(v);
        }
        match k {
            "year" => self
                .date
                .map(|d| d.format("%Y").to_string())
                .or_else(|| self.extracted?.date_part("year").map(str::to_owned)),
            "month" => self
                .date
                .map(|d| d.format("%-m").to_string())
                .or_else(|| self.extracted?.date_part("month").map(str::to_owned)),
            "day" => self
                .date
                .map(|d| d.format("%-d").to_string())
                .or_else(|| self.extracted?.date_part("day").map(str::to_owned)),
            "slug" => Some(self.slug.to_string()),
            "hash" => self.content_hash(),
            k if self.extracted.is_some_and(|m| m.captures.contains_key(k)) => {
                self.extracted.and_then(|m| m.captures.get(k).cloned())
            }
            // Axis placeholders spent per member, not here (q53).
            k => {
                let (_, bare) = template::classify(k);
                self.cfg.axes.contains_key(bare).then(|| format!("{{{k}}}"))
            }
        }
    }

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
        // Dateless file under dated template: keep the specific error sentence.
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

/// Collapse `//` when `{dir}` is empty at the root.
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

struct Scope<'a> {
    name: &'a str,
    source: Option<PathBuf>,
    rules: Vec<CompiledRule<'a>>,
    formats: Vec<filename::FilePattern>,
    found: Cell<usize>,
    offered: Cell<usize>,
}

impl Scope<'_> {
    fn owned(&self) -> Option<&Path> {
        match &self.source {
            Some(p) if !p.as_os_str().is_empty() => Some(p.as_path()),
            _ => None,
        }
    }

    fn relative(&self, rel: &Path) -> Option<PathBuf> {
        match &self.source {
            Some(src) => rel.strip_prefix(src).ok().map(Path::to_path_buf),
            None => Some(rel.to_path_buf()),
        }
    }
}

/// Scopes in walk order: deepest sourced first, then sourceless, then root.
fn scopes(cfg: &Config) -> Result<Vec<Scope<'_>>> {
    let axes = cfg.axis_values_for_file();
    let mut out: Vec<Scope> = Vec::new();
    for (name, c) in &cfg.collections {
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
        let inherited = !s.rules.is_empty() && s.rules.iter().all(|r| r.inherited);
        (class, std::cmp::Reverse(depth), inherited, s.name)
    });
    Ok(out)
}

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

/// Root-relative path inside site `themes/` (not the directory node itself).
fn under_themes(rel: &Path) -> bool {
    let mut parts = rel.components();
    parts.next().is_some_and(|c| c.as_os_str() == store::THEMES) && parts.next().is_some()
}

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
            let canon = cfg.pairing_canonical().unwrap_or("");
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

    // Tree collection supplies shared `exclude`/`include` (§4c) for every walk.
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

    let mut schemas = Schemas::new(grackle_model::row_schema());
    schemas.set_site(cfg.schema.decls.clone(), "grackle.toml [schema]")?;
    for (cname, c) in &cfg.collections {
        schemas.add_collection(
            cname,
            c.schema.clone(),
            &format!("grackle.toml [collections.{cname}.schema]"),
        )?;
    }
    // Sidecars on declaration walk: `exclude` is dirs-only, so `*.toml` cannot silence them.
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
    db.declared = schemas.declared_schema();

    let t = std::time::Instant::now();
    let scopes = scopes(cfg)?;
    let (mut rows, picture_rels) = walk::walk_site(
        cfg,
        &scopes,
        &markers,
        &sidecars,
        &schemas,
        &not_content,
        &mut db.warnings,
    )?;
    db.stats.read_ms += t.elapsed().as_secs_f64() * 1000.0;
    for w in &db.warnings {
        eprintln!("grackle: {w}");
    }

    let t_index = std::time::Instant::now();
    let pairing_keep = cfg
        .pairing_axis()
        .map(|(_, a)| (a.field.as_str(), a.canonical().unwrap_or("")));
    let dated: std::collections::HashSet<String> = cfg
        .collections
        .iter()
        .filter(|(_, c)| c.is_posts())
        .map(|(n, _)| n.clone())
        .collect();
    // Dated indexes stay ordered by path within those collections.
    rows.sort_by(|a, b| {
        let a_d = dated.contains(&a.collection);
        let b_d = dated.contains(&b.collection);
        match (a_d, b_d) {
            (true, true) => a.path.cmp(&b.path),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => a.path.cmp(&b.path),
        }
    });
    let pictures: std::collections::HashSet<grackle_db::Key> = picture_rels
        .iter()
        .map(|r| grackle_db::Key::new(r.to_string_lossy()))
        .collect();
    db.insert_rows(rows, pairing_keep, &dated, &pictures)?;
    walk::resolve_image_fields(&db, &schemas)?;
    if cfg.metadata.iter().any(|m| m == "git") {
        let commits = crate::git::last_commits_cached(&root, &root.join("_cache/metadata"));
        for row in db.rows.iter_mut() {
            let Some(m) = commits.get(&row.rel) else {
                continue;
            };
            use grackle_db::Value::Str;
            let md = &mut row.metadata;
            md.insert("git.commit".into(), Str(m.commit.clone()));
            md.insert("git.lastmod".into(), Str(m.date.clone()));
            md.insert("git.author".into(), Str(m.author.clone()));
            md.insert("git.comment".into(), Str(m.subject.clone()));
        }
    }
    db.stats.index_ms += t_index.elapsed().as_secs_f64() * 1000.0;

    let t = std::time::Instant::now();
    let mut new_routes: Vec<Route> = Vec::new();
    for p in db
        .rows
        .iter()
        .filter(|p| !p.claimed && !p.on_demand && p.strong_url.is_none())
    {
        let kind = if pictures.contains(&p.key) {
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
            if let Some((name, _)) = cfg.pairing_axis() {
                if let Some(value) = cfg.axis_on(p, name) {
                    cfg.stamp_axis_field(&mut fields, name, &value);
                }
            }
            Route {
                row: Some(p.key.clone()),
                source: Some(p.path.clone()),
                fields,
                axis,
                front_mattered: p.front_mattered,
                collection: Some(p.collection.clone()),
                ..Route::new(url, kind)
            }
        };
        let axes: &[grackle_model::RowAxis] = if p.rendered { &p.axis } else { &[] };
        if axes.is_empty() {
            new_routes.push(one(p.url.clone(), Vec::new()));
            continue;
        }
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
            .unwrap_or_default();
        let pairing_canon = pairing.and_then(|(_, a)| a.canonical()).unwrap_or("");
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
    // Join output half at planning, before `build_views` reads `output`.
    join::join_outputs(&mut db);
    crate::views::build_adjacency(cfg, &mut db, &schemas)?;
    crate::views::build_views(cfg, &mut db, &schemas)?;
    crate::views::build_pool_folds(cfg, &mut db)?;
    force_route_fields(cfg, &mut db, &schemas)?;
    crate::relations::build_relations(cfg, &mut db, &schemas)?;
    db.stats.views_ms = t.elapsed().as_secs_f64() * 1000.0;

    // q45 templated landings: claims settle once routes and group params exist.
    {
        let mut set_content: Vec<(grackle_db::Key, String)> = Vec::new();
        let mut owner_of: HashMap<String, String> = HashMap::new();
        let mut errors: Vec<String> = Vec::new();
        for r in &db.routes {
            let Some(view) = r.view.as_deref() else {
                continue;
            };
            let Some(v) = cfg.views.get(view) else {
                continue;
            };
            let (tmpl, promise) = match (v.content.as_deref(), v.default_content.as_deref()) {
                (Some(c), _) if crate::config::is_templated(c) => (c, true),
                (_, Some(d)) if crate::config::is_templated(d) => (d, false),
                _ => continue,
            };
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
                continue;
            }
            if !promise {
                let tag = format!("{{% view {view} %}}");
                let places = sibs
                    .into_iter()
                    .flatten()
                    .filter_map(|k| db.rows.get(k))
                    .any(|row| {
                        std::fs::read_to_string(&row.path)
                            .map(|t| store::split_front_matter(&t).2.contains(&tag))
                            .unwrap_or(false)
                    });
                if !places {
                    continue;
                }
            }
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
        db.routes
            .retain(|r| r.row.as_ref().is_none_or(|k| !claimed_keys.contains(k)));
        join::join_outputs(&mut db);
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

    {
        let claims = cfg.content_claims();
        let mut fixed: Vec<(grackle_db::Key, String)> = Vec::new();
        for (k, p) in db
            .rows
            .iter()
            .filter(|p| p.claimed)
            .map(|p| (p.key.clone(), p))
        {
            let url = db
                .routes
                .iter()
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
            fixed.push((k, url.unwrap_or_default()));
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

    // One row, one URL per axis member-tuple (q53).
    let mut by_row: HashMap<(&grackle_db::Key, String), &Route> = HashMap::new();
    for r in &db.routes {
        let Some(k) = &r.row else { continue };
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
    crate::views::resolve_pool_folds(cfg, &mut db, &schemas)?;
    join::join_arrangement(cfg, &mut db);
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
        let mut m: BTreeMap<_, _> = schema::CASCADE
            .iter()
            .map(|(n, t)| (*n, t.clone()))
            .collect();
        m.insert("toc", schema::FieldType::Bool);
        m
    }

    /// The whole row-side cascade, in the order `load` runs it: validated
    /// extra (nearest declared fields), cascade_front for named engine keys,
    /// then markers and rules. Driving all three is the point — the type
    /// checking lives in the middle call, and a test that only
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
    /// The controls are the whole MAP family, and the two
    /// retired spellings sit in the reject list beside the typo: they are hard
    /// cutoffs, so `none` and `light` get the same error a
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

    /// The family check, on the row side: a fold shell eats a
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

    /// A rule or marker default for a declared field is TYPE-CHECKED.
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
/// on a real (tiny) site — DESIGN.md §4's promised dead-rule warning.
///
/// A dead rule's subject is a corpus answering a glob and nothing smaller than
/// a tree can be that, which is why these tests write sites rather than build
/// a `Config`. (D1's `bucket` warning was tested here too, and went with the
/// key — it was the config-only warning this module doc used to contrast
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
    /// nothing. This used to be a load error, so the build that used to fail
    /// now succeeds with an empty blog — a real regression once shipped, and the
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
/// (§4a) — which can only be shown on a real tree, because the
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
    /// The restatement is the overlay law: a view is a
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
