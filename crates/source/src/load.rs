//! The load: one walk of the site, and the rows it produces.
//!
//! Reads the tree, applies collection rules, routes every row, and hands the
//! result to `SiteDb::insert_rows` — the only way into the database.

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use grackle_db::template;
use grackle_model::{Kind, ObjectsTable, Route, RouteKind, Row, SiteDb};

use crate::config::{Collection, Config};
use crate::filename::FilenameFormat;
use crate::markers::{Defaults, Markers};
use crate::schema::{self, Schemas};
use crate::store::{self, RawRow};

/// Front matter's `date:`, for either table. `YYYY-MM-DD`; a bare
/// `YYYY-MM` means the first of that month, which is what the tree side
/// was spelling as a string field before it could hold a real date.
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
// ------------------------------------------------------------------ rules

struct CompiledRule<'a> {
    matcher: GlobMatcher,
    route: Option<&'a str>,
    front_matter: Option<bool>,
    defaults: &'a BTreeMap<String, toml::Value>,
}

fn compile_rules(c: &Collection) -> Result<Vec<CompiledRule<'_>>> {
    c.rules
        .iter()
        .map(|r| {
            Ok(CompiledRule {
                matcher: Glob::new(&r.pattern)
                    .with_context(|| format!("bad rule glob {:?}", r.pattern))?
                    .compile_matcher(),
                route: r.route.as_deref(),
                front_matter: r.front_matter,
                defaults: &r.defaults,
            })
        })
        .collect()
}

/// First-writer-wins per key (DESIGN.md §4).
fn apply_rules<'a>(
    rules: &'a [CompiledRule<'a>],
    rel: &Path,
    has_front_matter: bool,
) -> (Option<&'a str>, BTreeMap<&'a str, &'a toml::Value>) {
    let mut route: Option<&str> = None;
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
        if route.is_none() {
            if let Some(r) = rule.route {
                route = Some(r);
            }
        }
        for (k, v) in rule.defaults {
            defaults.entry(k.as_str()).or_insert(v);
        }
    }
    (route, defaults)
}

fn as_bool(defaults: &BTreeMap<&str, &toml::Value>, key: &str) -> bool {
    defaults.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
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

/// What a row wears, after the cascade: the fields front matter shares with
/// markers and rules.
#[derive(Debug)]
struct Cascaded {
    theme: Option<String>,
    shell: Option<String>,
    layout: Option<String>,
    draft: bool,
    hidden: bool,
    noindex: bool,
    toc: bool,
}

/// Resolve those fields once, for any row.
///
/// Both loaders spelled this out separately and the two spellings had drifted
/// apart: `toc` and `layout` cascaded for a post and not for a tree row, and
/// the shell vocabulary was checked on a tree row and not on a post. Neither
/// asymmetry was intended and neither was reachable from this site's config,
/// which is how they survived.
fn cascade(
    front: &store::FrontMatter,
    defaults: &BTreeMap<&str, &toml::Value>,
    whose: &Path,
) -> Result<Cascaded> {
    let inherit = |key: &str| defaults.get(key).and_then(|v| v.as_str()).map(String::from);
    // A typo'd shell would silently render the wrong tier — the failure mode
    // this codebase keeps finding. Closed vocabulary, checked at load.
    let shell = front.shell.clone().or_else(|| inherit("shell"));
    if let Some(sh) = shell.as_deref() {
        if !matches!(sh, "none" | "light" | "html") {
            bail!(
                "{}: shell = \"{sh}\" is not a shell — expected none, light or html (§5g)",
                whose.display()
            );
        }
    }
    Ok(Cascaded {
        // Theme is chosen per row (§5a): front matter beats the rule default,
        // so one rule can restyle a subtree.
        theme: front.theme.clone().or_else(|| inherit("theme")),
        shell,
        layout: front.layout.clone().or_else(|| inherit("layout")),
        draft: front.draft.unwrap_or_else(|| as_bool(defaults, "draft")),
        hidden: front.hidden.unwrap_or_else(|| as_bool(defaults, "hidden")),
        noindex: front
            .noindex
            .unwrap_or_else(|| as_bool(defaults, "noindex")),
        toc: front.toc.unwrap_or_else(|| as_bool(defaults, "toc")),
    })
}

fn build_globset(pats: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in pats {
        b.add(Glob::new(p).with_context(|| format!("bad glob {p:?}"))?);
    }
    Ok(b.build()?)
}

/// `{dir}`, `{stem}`, `{name}`, `{path}`, `{ext}` for a tree/object row.
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
// ------------------------------------------------------------------ posts

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

    let formats: Vec<FilenameFormat> = c
        .filename_formats
        .iter()
        .map(|f| FilenameFormat::compile(f))
        .collect::<Result<_>>()?;
    if formats.is_empty() {
        bail!("collection {name} has kind=posts but no filename_formats");
    }
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
        // The two loaders used OPPOSITE conventions for this field, and the
        // page `field()` only derived `stem` correctly because of it.
        //
        // ROOT-relative too, like `rel`: it was collection-relative, so a
        // post at `2020/x.md` and a tree page at `2020/x.md` shared a
        // logical identity. That was harmless while the two tables kept
        // separate `by_logical` maps and is a collision the moment they do
        // not.
        let logical = source_rel.join(&logical_rel).to_string_lossy().to_string();
        let key = formats.iter().find_map(|f| f.parse(&stem));
        let from_name = match &key {
            Some(k) => Some(
                NaiveDate::from_ymd_opt(k.year, k.month, k.day).with_context(|| {
                    format!(
                        "{} has an impossible date in its filename",
                        raw.path.display()
                    )
                })?,
            ),
            None => None,
        };
        // Front matter beats the filename, the same precedence every other
        // field has (§4b) — and the same `date:` a tree page now carries.
        // Before this it landed in `extra`, where a governed post rejected
        // it as undeclared and an ungoverned one dropped it.
        let date = match &raw.front.date {
            Some(s) => Some(front_matter_date(s, &raw.path)?),
            None => from_name,
        };
        let slug = key
            .as_ref()
            .map(|k| k.slug.clone())
            .unwrap_or_else(|| stem.clone());

        // `true`, not `raw.has_front_matter`: every post is a rendered row
        // (`rendered` below says the same), so a `front_matter = false` rule
        // describes a static file and cannot describe a post.
        let (route_tmpl, rule_defaults) = apply_rules(&rules, &logical_rel, true);
        let root_rel = raw
            .path
            .strip_prefix(&root)
            .unwrap_or(&raw.rel)
            .to_path_buf();
        let marker_defaults = markers.defaults_for(&root_rel);
        let defaults = merged_defaults(&marker_defaults, rule_defaults);
        let title = Some(
            raw.front
                .title
                .clone()
                .unwrap_or_else(|| slug.replace('-', " ")),
        );
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
        let checked = match schemas.resolve(&parent) {
            Some(schema) => schema::validate(&schema, &raw.front.extra, &raw.path)?,
            None => Default::default(),
        };
        let worn = cascade(&raw.front, &defaults, &raw.path)?;

        let url = if let Some(p) = &raw.front.permalink {
            p.clone()
        } else {
            let tmpl = route_tmpl.ok_or_else(|| {
                anyhow::anyhow!("no rule supplies a route for {}", raw.path.display())
            })?;
            if date.is_none() {
                let needs: Vec<String> = template::tokens(tmpl)?
                    .into_iter()
                    .filter(|t| matches!(t.as_str(), "year" | "month" | "day"))
                    .collect();
                if !needs.is_empty() {
                    bail!(
                        "{} has no date (filename doesn't match any filename_formats), \
                         but its route {:?} requires {{{}}}",
                        raw.path.display(),
                        tmpl,
                        needs.join("}, {")
                    );
                }
            }
            template::render(tmpl, |k| match k {
                "year" => date.map(|d| d.format("%Y").to_string()),
                "month" => date.map(|d| d.format("%-m").to_string()),
                "day" => date.map(|d| d.format("%-d").to_string()),
                "slug" => Some(slug.clone()),
                _ => None,
            })
            .with_context(|| format!("routing {}", raw.path.display()))?
        };
        // §6f: a translation lands at the locale-prefixed twin of its
        // original's URL.
        let url = if locale != cfg.i18n.default {
            format!("/{locale}{url}")
        } else {
            url
        };

        rows.push(Row {
            // Assigned by `insert_rows`, which is where rows become the
            // database's rather than the loader's.
            key: Default::default(),
            collection: collection.clone(),
            path: raw.path,
            // ROOT-relative since the merge, so `path`/`dir` mean one thing
            // on either table. Rule globs still match the collection-
            // relative form (`apply_rules` takes `logical_rel`), which is
            // what `match = "hidden/**"` inside `_posts` has always meant.
            rel: source_rel.join(&raw.rel),
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
            draft: worn.draft,
            hidden: worn.hidden,
            noindex: worn.noindex,
            toc: worn.toc,
            locale,
            logical,
            url,
            body_bytes: raw.body.len(),
            // A post is always parsed; the tree distinction does not apply.
            rendered: true,
            size: raw.size,
            claimed: false,
        });
    }

    Ok((rows, read_ms))
}

/// Index the whole posts table at once, over every collection's rows.
/// Posts arrive from several collections (`_posts` and `_drafts` are two
/// sources of one corpus), so they are gathered first and ordered once.
/// Indexing itself belongs to `SiteDb::insert_rows` now — there is one row
/// store to index (q51), and it is the database's to build.
fn sort_posts(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}
// ------------------------------------------------------- tree + objects

/// One walk of the site root, partitioned by membership precedence
/// (DESIGN.md §3): objects win by extension, tree takes the rest.
fn build_tree_and_objects(
    cfg: &Config,
    tree_name: &str,
    tree_c: Option<&Collection>,
    obj_name: &str,
    obj_c: Option<&Collection>,
    markers: &Markers,
    schemas: &Schemas,
) -> Result<(Vec<Row>, ObjectsTable)> {
    let Some(tree_c) = tree_c else {
        return Ok((Vec::new(), ObjectsTable::default()));
    };
    let root = cfg.root();
    let exclude = build_globset(&tree_c.exclude)?;
    let include = build_globset(&tree_c.include)?;
    let files = store::walk_tree(&root, &exclude, &include, cfg.gitignore)?;

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
        .collect();

    // q45: rows named by a view's `content` — claimed landings. Matched
    // by logical identity so every locale variant is claimed with its
    // original.
    let claims = cfg.content_claims();

    let obj_exts: Vec<String> = obj_c
        .map(|c| c.extensions.iter().map(|e| e.to_lowercase()).collect())
        .unwrap_or_default();
    let tree_rules = compile_rules(tree_c)?;
    let obj_rules = obj_c.map(compile_rules).transpose()?.unwrap_or_default();

    let is_obj = |rel: &Path| {
        let ext = rel
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        obj_exts.iter().any(|e| *e == ext)
    };

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
    let mut objects = ObjectsTable::default();

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
        let (tmpl, rule_defaults) = apply_rules(rules, &logical_rel, f.has_front_matter);
        let marker_defaults = markers.defaults_for(&f.rel);
        let defaults = merged_defaults(&marker_defaults, rule_defaults);
        let Some(tmpl) = tmpl else {
            bail!("no rule supplies a route for {}", f.path.display());
        };
        let url = tidy(
            template::render(tmpl, |k| path_tokens(&logical_rel, k))
                .with_context(|| format!("routing {}", f.path.display()))?,
        );
        let url = if locale != cfg.i18n.default {
            format!("/{locale}{url}")
        } else {
            url
        };

        if is_object {
            let name = f
                .rel
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            objects
                .by_name
                .entry(name.clone())
                .or_default()
                .push(objects.rows.len());
            // An object is a row that was never rendered. Everything else it
            // could carry — front matter, a date, a locale axis — a binary
            // file does not have, so the defaults are the honest values.
            let stem = f
                .rel
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            objects.rows.push(Row {
                key: grackle_db::Key::new(f.rel.to_string_lossy()),
                collection: obj_name.to_string(),
                path: f.path,
                rel: f.rel,
                version: f.version,
                url,
                size: f.size,
                slug: stem.clone(),
                stem,
                locale,
                rendered: false,
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
            let checked = match schemas.resolve(&parent) {
                Some(schema) if f.has_front_matter => {
                    schema::validate(&schema, &fm.extra, &f.path)?
                }
                _ => Default::default(),
            };
            let worn = cascade(&fm, &defaults, &f.rel)?;
            let date = match &fm.date {
                Some(s) => Some(front_matter_date(s, &f.path)?),
                None => None,
            };
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
            // `stem` is STORED, not derived. `Page::field` used to recompute
            // it from `logical` via `file_stem()`, which was correct only
            // because the tree kept the extension that the posts loader
            // stripped — a page named `v1.2-release.md` would have come back
            // `v1` the moment those conventions were unified. Computed once
            // here from the real path, the question stops existing.
            let stem = logical_rel
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            pages.push(Row {
                key: Default::default(),
                collection: tree_name.to_string(),
                slug: stem.clone(),
                stem,
                body_bytes,
                path: f.path,
                rel: f.rel,
                version: f.version,
                url,
                rendered: f.has_front_matter,
                size: f.size,
                title: fm.title,
                layout: worn.layout,
                description: fm.description,
                order: fm.order,
                date,
                tags: fm.tags,
                toc: worn.toc,
                theme: worn.theme,
                shell: worn.shell,
                draft: worn.draft,
                hidden: worn.hidden,
                noindex: worn.noindex,
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
    Ok((pages, objects))
}

/// Front matter of a tree page: presentation reads its fields directly.
/// A parse failure is a LOAD ERROR naming the file — this used to swallow
/// bad YAML into an empty schema, and an unquoted `title: A: B` shipped a
/// silently titleless page. Loud beats lenient (§4's constraint ethos).
fn read_page_schema(path: &Path) -> Result<(store::FrontMatter, usize)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (yaml, body) = store::split_front_matter(&text);
    let fm = serde_yaml_ng::from_str(yaml)
        .with_context(|| format!("front matter of {}", path.display()))?;
    // `body_bytes` from the same read, so the field means the same thing on
    // every row. It was posts-only (0 on a page) before the merge.
    Ok((fm, body.len()))
}
/// Read the site named by `cfg` and return the database it describes.
pub fn load(cfg: &Config) -> Result<SiteDb> {
    let mut db = SiteDb::default();
    let t_m = std::time::Instant::now();
    let root = cfg.root();
    let markers = Markers::scan(&root, &cfg.markers, cfg.gitignore)?;
    db.stats.markers_ms = t_m.elapsed().as_secs_f64() * 1000.0;
    db.stats.markers = markers.found;

    // The engine-vocabulary walk: `.section` scope markers (§6e) and
    // `.schema.toml` field declarations (§5b) — positional names like
    // `.slots/`, no config entries. One name-only pass with the same
    // .gitignore defence as the marker scan.
    let mut schemas = Schemas::new(grackle_model::row_schema());
    let mut b = store::walker(&root, cfg.gitignore);
    b.filter_entry(|e| !(e.file_type().is_some_and(|t| t.is_dir()) && e.file_name() == ".git"));
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
    let mut tree_c = None;
    let mut obj_c = None;
    let mut obj_name = String::new();

    // Several collections may feed the posts table — `_posts` and
    // `_drafts` are two sources of one corpus — so rows are gathered
    // first and indexed once, over all of them.
    let mut post_rows: Vec<Row> = Vec::new();
    let mut tree_name = String::new();
    for (name, c) in &cfg.collections {
        match c.kind {
            Kind::Posts => {
                let (rows, read_ms) = read_posts(cfg, name, c, &markers, &schemas)?;
                post_rows.extend(rows);
                db.stats.read_ms += read_ms;
            }
            Kind::Tree => {
                tree_c = Some(c);
                tree_name = name.clone();
            }
            Kind::Objects => {
                obj_c = Some(c);
                obj_name = name.clone();
            }
        }
    }
    let t = std::time::Instant::now();
    let (page_rows, objects) = build_tree_and_objects(
        cfg, &tree_name, tree_c, &obj_name, obj_c, &markers, &schemas,
    )?;
    db.objects = objects;
    db.stats.read_ms += t.elapsed().as_secs_f64() * 1000.0;

    let t_index = std::time::Instant::now();
    db.insert_rows(sort_posts(post_rows), page_rows, &cfg.i18n.default)?;
    db.stats.index_ms += t_index.elapsed().as_secs_f64() * 1000.0;

    // Unified route list.
    let t = std::time::Instant::now();
    let route_locale = |l: &str| (l != cfg.i18n.default).then(|| l.to_string());
    // One loop over one row store. It was two, and the second one
    // decided `Page` vs `Static` from `p.rendered` — a property the
    // first never had to consult because every post is parsed.
    // `RouteKind::Post` survives because a ROUTE kind is real: it is
    // the vocabulary star-view filters use (`kind == "post"`).
    let n_posts = db.post_ix.len();
    let new_routes: Vec<Route> = db
        .rows
        .iter()
        .enumerate()
        // q45: a claimed row has no route of its own — the owning view
        // materializes the landing.
        .filter(|(_, p)| !p.claimed)
        .map(|(i, p)| {
            let kind = if i < n_posts {
                RouteKind::Post
            } else if p.rendered {
                RouteKind::Page
            } else {
                RouteKind::Static
            };
            Route {
                source: Some(p.path.clone()),
                locale: route_locale(&p.locale),
                draft: p.draft,
                hidden: p.hidden,
                ..Route::new(p.url.clone(), kind)
            }
        })
        .collect();
    db.routes.extend(new_routes);
    for o in &db.objects.rows {
        db.routes.push(Route {
            source: Some(o.path.clone()),
            ..Route::new(o.url.clone(), RouteKind::Object)
        });
    }
    crate::views::build_adjacency(cfg, &mut db, &schemas)?;
    crate::views::build_views(cfg, &mut db, &schemas)?;
    crate::views::build_star_views(cfg, &mut db)?;
    db.stats.views_ms = t.elapsed().as_secs_f64() * 1000.0;

    // q45: a claimed row's URL becomes its landing's — the owning
    // view's route in the row's locale — so source-path links and the
    // ancestors walk see the landing, not the retired standalone URL.
    // A locale variant whose partition didn't materialize keeps no
    // URL (nothing may link it).
    {
        let claims = cfg.content_claims();
        let mut fixed: Vec<(usize, String)> = Vec::new();
        // The GLOBAL index: `enumerate` over `pages()` counts within
        // the tree rows, and every index is a row-store index now.
        for (i, p) in db.page_ix.iter().map(|&i| (i, &db.rows[i])) {
            if !p.claimed {
                continue;
            }
            let owner = claims[p.logical.as_str()];
            let url = db
                .routes
                .iter()
                .find(|r| {
                    r.kind == RouteKind::View
                        && r.view.as_deref() == Some(owner)
                        && r.locale == route_locale(&p.locale)
                        && r.key.is_none()
                        && r.page.is_none_or(|n| n == 1)
                })
                .map(|r| r.url.clone());
            fixed.push((i, url.unwrap_or_default()));
        }
        for (i, url) in fixed {
            if let Some(r) = db.rows.get_mut(i) {
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

    db.routes.sort_by(|a, b| a.url.cmp(&b.url));
    // Star views index routes, so they resolve against the final, sorted list.
    crate::views::resolve_star_views(cfg, &mut db)?;
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

    #[test]
    fn front_matter_beats_a_default() {
        let d = [("theme", text("inherited")), ("toc", yes())];
        let c = cascade(
            &front("theme: own\ntoc: false\n"),
            &defaults(&d),
            Path::new("x"),
        )
        .unwrap();
        assert_eq!(c.theme.as_deref(), Some("own"));
        assert!(!c.toc);
    }

    /// The four that a silent row inherits. `toc` and `layout` are here
    /// because they reached a post and not a tree row.
    #[test]
    fn a_silent_row_inherits_every_cascading_field() {
        let d = [
            ("theme", text("t")),
            ("shell", text("light")),
            ("layout", text("l")),
            ("draft", yes()),
            ("hidden", yes()),
            ("noindex", yes()),
            ("toc", yes()),
        ];
        let c = cascade(&front("{}"), &defaults(&d), Path::new("x")).unwrap();
        assert_eq!(c.theme.as_deref(), Some("t"));
        assert_eq!(c.shell.as_deref(), Some("light"));
        assert_eq!(c.layout.as_deref(), Some("l"));
        assert!(c.draft && c.hidden && c.noindex && c.toc);
    }

    #[test]
    fn an_unset_field_stays_unset() {
        let c = cascade(&front("{}"), &defaults(&[]), Path::new("x")).unwrap();
        assert_eq!(c.theme, None);
        assert_eq!(c.layout, None);
        assert!(!c.draft && !c.toc);
    }

    /// The shell vocabulary was checked on tree rows only, so a post could
    /// name a tier that does not exist and render the wrong one in silence.
    #[test]
    fn a_shell_outside_the_vocabulary_is_a_load_error() {
        let e = cascade(&front("shell: htlm\n"), &defaults(&[]), Path::new("p.md"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("is not a shell"), "{e}");
        assert!(e.contains("p.md"), "{e}");
        for ok in ["none", "light", "html"] {
            assert!(cascade(
                &front(&format!("shell: {ok}\n")),
                &defaults(&[]),
                Path::new("x")
            )
            .is_ok());
        }
    }

    /// An inherited shell is checked too — a rule can typo it as easily as
    /// front matter can.
    #[test]
    fn an_inherited_shell_is_checked() {
        let d = [("shell", text("lite"))];
        assert!(cascade(&front("{}"), &defaults(&d), Path::new("x")).is_err());
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
}
