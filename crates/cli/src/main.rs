//! The CLI. Thin: parse args, load config, call into `grackle_core`.

use grackle_core::{build, config, debug, embed, filter, model, serve, store, urls, views};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "grackle", about = "A virtual database over the site")]
struct Cli {
    #[arg(long, default_value = "grackle.toml", global = true)]
    config: PathBuf,
    /// Build profile (§4a). Absent means the default projection — the
    /// config exactly as written, which is what publishing uses. `serve`
    /// defaults to `dev` instead.
    #[arg(long, global = true)]
    profile: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Query the content database.
    #[command(subcommand)]
    Query(Query),
    /// Everything known about one URL — `query explain` under the name the
    /// docs teach it by (DESIGN.md §0, TODO-1.0.md).
    Explain { url: String },
    /// Show the config the engine actually runs, with per-key provenance.
    ///
    /// `--effective` is the name DESIGN.md §4d gives it and the only thing
    /// this subcommand does today, so the bare command prints the same thing;
    /// the flag exists so the documented spelling works.
    Config {
        /// Print the merged config: the site's file over the base's, with
        /// where each value came from (§4d).
        #[arg(long)]
        effective: bool,
    },
    /// Dump the whole database as JSON.
    Export {
        /// Write to a file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Pretty-print.
        #[arg(long)]
        pretty: bool,
    },
    /// Render the site to a directory.
    Build {
        #[arg(long, default_value = "_site-grackle")]
        out: PathBuf,
    },
    /// Serve the site from a resident database, rebuilding on change.
    Serve {
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
    /// Check the URL set against a reference build (§4 parity).
    Urls {
        /// Reference site directory: `_site-prod`, or a tree rsynced from prod.
        #[arg(long)]
        against: PathBuf,
        /// How many URLs to list per category.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// URL prefixes exempt from parity (q12: derived assets). The
        /// configured static dir is always exempt; pass a reference build's
        /// legacy scheme too, e.g. --exempt /_thumbs/
        #[arg(long)]
        exempt: Vec<String>,
    },
    /// Show the generated routes as a tree.
    Routes {
        /// Collapse below this depth.
        #[arg(long, default_value_t = 3)]
        depth: usize,
        /// Only show routes under this prefix.
        #[arg(long)]
        under: Option<String>,
    },
}

#[derive(Subcommand)]
enum Query {
    /// Row counts, index sizes, and load timing.
    Stats,
    /// Every routable URL, one per line.
    Urls {
        /// post | page | static | object | view
        #[arg(long)]
        kind: Option<String>,
    },
    /// List posts, newest first.
    Posts {
        /// Keep rows whose list field contains a value (`field=value`).
        #[arg(long = "has")]
        has: Option<String>,
        #[arg(long)]
        year: Option<i32>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Values of a list field with row counts (`by_multi_key`).
    ///
    /// Absent a field name, every indexed list field is printed. `tags` is a
    /// visible alias for the same command.
    #[command(visible_alias = "tags")]
    Values {
        /// List field to dump (e.g. `tags`). Absent = every indexed list field.
        field: Option<String>,
    },
    /// Search the TF-IDF index the site ships (§6b) — same code the browser runs.
    Search {
        query: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Posts most similar to a URL, by embedding cosine (§6b).
    Similar {
        url: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// Monthly archive buckets.
    Archives,
    /// Everything known about one URL.
    Explain { url: String },
    /// The graph edges one output stands on, and the work of pulling it
    ///.
    Pull { url: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let t0 = std::time::Instant::now();
    let profile = cli
        .profile
        .clone()
        .or_else(|| matches!(cli.cmd, Cmd::Serve { .. }).then(|| "dev".to_string()));
    // Before the load, deliberately: `--effective` answers about a config the
    // engine has rejected, which is when it is most wanted.
    if let Cmd::Config { .. } = cli.cmd {
        print!(
            "{}",
            config::Config::effective(&cli.config, profile.as_deref())?
        );
        return Ok(());
    }
    let cfg = config::Config::load_profile(&cli.config, profile.as_deref())?;
    let mut db = grackle_core::load(&cfg).context("loading site database")?;
    let total_ms = t0.elapsed().as_secs_f64() * 1000.0;

    match cli.cmd {
        Cmd::Query(q) => run_query(q, &cfg, &db, total_ms)?,
        Cmd::Explain { url } => run_query(Query::Explain { url }, &cfg, &db, total_ms)?,
        // Handled above, before the database load.
        Cmd::Config { .. } => unreachable!(),
        Cmd::Export { out, pretty } => {
            let json = if pretty {
                serde_json::to_string_pretty(&db)?
            } else {
                serde_json::to_string(&db)?
            };
            match out {
                Some(p) => {
                    std::fs::write(&p, &json)?;
                    eprintln!("wrote {} ({} bytes)", p.display(), json.len());
                }
                None => println!("{json}"),
            }
        }
        Cmd::Build { out } => {
            let t = std::time::Instant::now();
            let s = build::build(&cfg, &mut db, &out)?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!("built {} in {:.0}ms", out.display(), ms);
            println!("  posts     {}", s.posts);
            println!("  pages     {}", s.pages);
            println!("  listings  {}", s.listings);
            println!("  copied    {}", s.copied);
            println!("  thumbs    {} (/static/)", s.thumbs);
            if s.on_demand > 0 {
                println!("  on-demand {} (published because referenced)", s.on_demand);
            }
            println!("  xml       {} (feed + sitemap)", s.serialized);
            println!("  css       {} bytes", s.css);
            if !s.skipped.is_empty() {
                println!(
                    "  skipped   {} (page templates using liquid)",
                    s.skipped.len()
                );
                for u in s.skipped.iter().take(8) {
                    println!("              {u}");
                }
            }
        }
        Cmd::Urls {
            against,
            limit,
            exempt,
        } => {
            let mut prefixes = exempt;
            // q12: our own derived output is exempt by construction. The
            // prefix is thumbs.rs's constant.
            prefixes.push("/static/".to_string());
            let (out_map, _) = build::render_site(&cfg, &mut db)?;
            let ours = urls::parity_set(out_map.keys().cloned(), &prefixes);
            let reference = urls::parity_set(
                urls::urls_in_dir(&against)
                    .with_context(|| format!("reading reference {}", against.display()))?,
                &prefixes,
            );
            println!("url parity vs {}", against.display());
            println!("  exempt    {}", prefixes.join(" "));
            let p = urls::Parity::compare(&ours, &reference);
            p.report(limit);
            if !p.ok() {
                anyhow::bail!(
                    "{} URL(s) present in the reference are not produced by this build",
                    p.missing.len()
                );
            }
            println!("OK — every reference URL is produced.");
        }
        Cmd::Serve { port } => serve::serve(&cli.config, port, profile.as_deref())?,
        Cmd::Routes { depth, under } => routes_tree(&db, depth, under.as_deref()),
    }
    Ok(())
}

/// A node in the route trie.
#[derive(Default)]
struct Node {
    kids: BTreeMap<String, Node>,
    /// Set when a route terminates here.
    leaf: Option<(model::RouteKind, Option<String>, Option<usize>)>,
    /// The route ends in `/` — it is served as a directory. Tracked separately
    /// because a childless node would otherwise render without its slash.
    dir_url: bool,
}

fn insert(root: &mut Node, url: &str, r: &model::Route) {
    let mut cur = root;
    for seg in url.split('/').filter(|s| !s.is_empty()) {
        cur = cur.kids.entry(seg.to_string()).or_default();
    }
    cur.leaf = Some((r.kind, r.view.clone(), r.rows));
    cur.dir_url = url.ends_with('/');
}

fn count(n: &Node) -> usize {
    n.leaf.is_some() as usize + n.kids.values().map(count).sum::<usize>()
}

/// The `[kind]` annotation `grackle routes` hangs off a trie node, and the
/// `kind` line `grackle explain <url>` prints for an output.
///
/// **Kept real, deliberately.** The facts pass deleted the ROW branch's hardcoded
/// `kind post` because a row has no kind; this is the ROUTE branch, where the
/// value is a live column a site's `where` can name (grack.com's search filter
/// does). A debug surface that prints a column the query language still has is
/// not a fossil — it is the surface. The day the column goes, this line and
/// `query urls --kind` go with it, together.
fn tag(kind: model::RouteKind, view: &Option<String>, rows: Option<usize>) -> String {
    let base = match (kind, view) {
        (model::RouteKind::View, Some(v)) => format!("view {v}"),
        (kind, _) => kind.as_str().to_string(),
    };
    match rows {
        Some(n) => format!("{base}, {n} rows"),
        None => base,
    }
}

fn render(n: &Node, prefix: &str, depth: usize, max: usize, out: &mut String) {
    let kids: Vec<_> = n.kids.iter().collect();
    for (i, (name, kid)) in kids.iter().enumerate() {
        let last = i + 1 == kids.len();
        let branch = if last { "└── " } else { "├── " };
        let label = if kid.kids.is_empty() && !kid.dir_url {
            name.to_string()
        } else {
            format!("{name}/")
        };
        let ann = match &kid.leaf {
            Some((k, v, r)) => format!("  [{}]", tag(*k, v, *r)),
            None => String::new(),
        };

        if depth + 1 >= max && !kid.kids.is_empty() {
            let n_below = count(kid);
            out.push_str(&format!(
                "{prefix}{branch}{label}{ann}  ({n_below} routes)\n"
            ));
            continue;
        }
        out.push_str(&format!("{prefix}{branch}{label}{ann}\n"));
        let next = format!("{prefix}{}", if last { "    " } else { "│   " });
        render(kid, &next, depth + 1, max, out);
    }
}

fn routes_tree(db: &model::SiteDb, depth: usize, under: Option<&str>) {
    let mut root = Node::default();
    let mut n = 0;
    for r in &db.routes {
        if let Some(u) = under {
            if !r.url.starts_with(u) {
                continue;
            }
        }
        insert(&mut root, &r.url, r);
        n += 1;
    }
    let mut out = String::new();
    out.push_str("/\n");
    render(&root, "", 0, depth, &mut out);
    print!("{out}");
    println!("\n{n} routes (depth {depth}; --depth N to expand, --under PREFIX to focus)");
}

fn run_query(q: Query, cfg: &config::Config, db: &model::SiteDb, total_ms: f64) -> Result<()> {
    match q {
        Query::Stats => {
            let dated = db.rows.iter().filter(|r| db.row_date(r).is_some()).count();
            let pictures: usize = db.by_name.values().map(|v| v.len()).sum();
            let shared: Vec<_> = db.by_slug.iter().filter(|(_, v)| v.len() > 1).collect();
            println!("rows            {}", db.rows.len());
            println!("  dated         {}", dated);
            // One line per declared bool/list the site actually uses (§4e).
            for (name, ty) in &db.declared {
                let n = match ty {
                    filter::Type::Bool => db.rows.iter().filter(|r| r.flag(name)).count(),
                    filter::Type::List => {
                        db.rows.iter().filter(|r| !r.list(name).is_empty()).count()
                    }
                    _ => continue,
                };
                if n > 0 {
                    println!("  {name:<13} {n}");
                }
            }
            println!(
                "  rendered      {}",
                db.rows.iter().filter(|r| r.rendered).count()
            );
            println!(
                "  static        {}",
                db.rows
                    .iter()
                    .filter(|r| !r.rendered && !db.by_name.values().any(|v| v.contains(&r.key)))
                    .count()
            );
            println!("  pictures      {}", pictures);
            println!("  distinct names{:>4}", db.by_name.len());
            let dupes = db.by_name.values().filter(|v| v.len() > 1).count();
            println!("  ambiguous     {}", dupes);
            println!("indexes");
            println!("  by_key        {}  (date, slug) unique", db.by_key.len());
            println!(
                "  by_slug       {}  ({} reused across dates)",
                db.by_slug.len(),
                shared.len()
            );
            for (field, idx) in &db.by_multi_key {
                println!("  by_multi_key.{field:<12} {}", idx.len());
            }
            for (field, idx) in &db.by_date_keys {
                println!("  by_date_keys.{field:<12} {}", idx.len());
            }
            println!(
                "markers         {}  files found ({:.1}ms scan)",
                db.stats.markers, db.stats.markers_ms
            );
            // Beside the marker census and for its reason: a
            // declaration family whose whole effect lands on OTHER files
            // leaves no trace in a build's file list, so the count is the only
            // way to ask "is this mechanism in use here".
            println!(
                "sidecars        {}  files found (on the declaration walk)",
                db.stats.sidecars
            );
            println!("routes          {}", db.routes.len());
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for r in &db.routes {
                let k = match r.kind {
                    model::RouteKind::View => {
                        format!("view:{}", r.view.clone().unwrap_or_default())
                    }
                    other => other.as_str().to_string(),
                };
                *kinds.entry(k).or_default() += 1;
            }
            for (k, n) in &kinds {
                println!("  {k:<20} {n}");
            }
            println!("timing");
            println!("  read+parse    {:.1}ms", db.stats.read_ms);
            println!("  index         {:.1}ms", db.stats.index_ms);
            println!("  views+routes  {:.1}ms", db.stats.views_ms);
            println!("  total         {:.1}ms", total_ms);
        }
        Query::Urls { kind } => {
            for r in &db.routes {
                if let Some(k) = &kind {
                    if r.kind.as_str() != k {
                        continue;
                    }
                }
                println!("{}", r.url);
            }
        }
        Query::Posts { has, year, limit } => {
            let has = has.as_deref().map(parse_has).transpose()?;
            let mut n = 0;
            // Newest first, default locale — the table carries no ordering
            // index of its own.
            let mut ix: Vec<usize> = (0..db.rows.len())
                .filter(|&i| match cfg.pairing_axis() {
                    Some((n, _)) => cfg.on_canonical(&db.rows[i], n),
                    None => true,
                })
                .collect();
            ix.sort_by(|&a, &b| views::chronological(&db.declared, &db.rows, a, b));
            for &i in &ix {
                let r = &db.rows[i];
                if let Some((field, value)) = &has {
                    if !r.list(field).iter().any(|x| x == value) {
                        continue;
                    }
                }
                if let Some(y) = year {
                    let hit = model::date_fields(&db.declared).iter().any(|f| {
                        matches!(
                            filter::Row::field(r, &format!("{f}.year")),
                            filter::Value::Int(yy) if yy == y as i64
                        )
                    });
                    if !hit {
                        continue;
                    }
                }
                println!(
                    "{}  {}",
                    fmt_date(r, &db.declared),
                    r.title.as_deref().unwrap_or("-")
                );
                println!("    {}", r.url);
                n += 1;
                if n >= limit {
                    break;
                }
            }
        }
        Query::Search { query, limit } => {
            // The CLI runs no render pass, so the raw markdown stands in for
            // the rendered body — a smoke query over the same projection.
            let docs =
                build::search_docs(cfg, db, |p| store::read_body(&p.path).unwrap_or_default());
            let (index, _) = grackle_search_core::build_index(&docs);
            let q = query.join(" ");
            for (url, store) in index.search(&q, limit) {
                let title = store.get("title").map(String::as_str).unwrap_or("-");
                let date = store.get("date").map(String::as_str).unwrap_or("-");
                println!("  {date:>18}  {url}  {title}");
            }
        }
        Query::Similar { url, limit } => {
            let loaded = embed::fresh(db, cfg, &cfg.root().join("_cache/embeddings"))?;
            // The raw embedding order (no recency shaping) — the diagnostic
            // that shows what `embedding_similarity` sees before a relation's
            // `where`/`rank` narrows it.
            let policy = embed::RankPolicy {
                limit,
                ..Default::default()
            };
            let rel = embed::rank(db, &loaded.keys, &loaded.vectors, &policy);
            let Some(key) = db.by_url.get(&url).cloned() else {
                anyhow::bail!("no post at {url}");
            };
            println!(
                "similar to {} — {}",
                url,
                db.row(&key).and_then(|r| r.title.as_deref()).unwrap_or("-")
            );
            for (k, score) in rel.by_post.get(&key).map(Vec::as_slice).unwrap_or(&[]) {
                let Some(p) = db.row(k) else { continue };
                println!(
                    "  {score:.3}  {}  {}",
                    p.url,
                    p.title.as_deref().unwrap_or("-")
                );
            }
        }
        Query::Values { field } => {
            let fields: Vec<&str> = match field.as_deref() {
                Some(f) => {
                    if !db.by_multi_key.contains_key(f) {
                        let known: Vec<&str> = db.by_multi_key.keys().map(String::as_str).collect();
                        anyhow::bail!(
                            "no by_multi_key index for {f:?} — indexed list fields: {}",
                            if known.is_empty() {
                                "(none)".into()
                            } else {
                                known.join(", ")
                            }
                        );
                    }
                    vec![f]
                }
                None => db.by_multi_key.keys().map(String::as_str).collect(),
            };
            let header = fields.len() > 1;
            for f in fields {
                if header {
                    println!("{f}");
                }
                let mut vals: Vec<(&String, usize)> = db
                    .by_multi_key
                    .get(f)
                    .map(|m| m.iter().map(|(t, v)| (t, v.len())).collect())
                    .unwrap_or_default();
                vals.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
                for (t, n) in vals {
                    println!("{n:4}  {t}");
                }
            }
        }
        Query::Archives => {
            let header = db.by_date_keys.len() > 1;
            for (field, idx) in &db.by_date_keys {
                if header {
                    println!("{field}");
                }
                let mut months: BTreeMap<(i32, u32), usize> = BTreeMap::new();
                for (iso, keys) in idx {
                    // Keys are `iso_date` output (`YYYY-MM-DD`).
                    let Some((y, rest)) = iso.split_once('-') else {
                        continue;
                    };
                    let Some((m, _)) = rest.split_once('-') else {
                        continue;
                    };
                    let (Ok(y), Ok(m)) = (y.parse(), m.parse()) else {
                        continue;
                    };
                    *months.entry((y, m)).or_default() += keys.len();
                }
                for ((y, m), n) in months {
                    println!("{y}-{m:02}  {n:3} posts");
                }
            }
        }
        Query::Explain { url } => {
            if let Some(r) = db.by_url.get(&url).and_then(|k| db.rows.get(k)) {
                println!("url         {}", r.url);
                print!("{}", debug::row_facts(r));
                println!("source      {}", r.path.display());
                println!("version     {:016x}", r.version);
                println!("date        {}", fmt_date(r, &db.declared));
                println!("slug        {}", r.slug);
                println!("stem        {}", r.stem);
                println!("source      {}  (embedding cache key)", r.rel.display());
                println!("title       {}", r.title.as_deref().unwrap_or("-"));
                print!("{}", debug::row_fields(r));
                println!("body        {} bytes", r.body_bytes);
                let seq = db
                    .adjacency
                    .get(&r.collection)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let (newer, older) = model::neighbors_in(seq, &r.key);
                let url_of = |k: Option<model::Key>| {
                    k.and_then(|k| db.row(&k).map(|n| n.url.clone()))
                        .unwrap_or_else(|| "-".into())
                };
                println!("newer       {}", url_of(newer));
                println!("older       {}", url_of(older));
                // The other two input-side join fields. `alternates` is
                // this row's other FORMS (q53's axis); `viewed_by` is the
                // outputs that ARRANGE it — which is why a citation of this
                // row appears in neither (that is `linked_from`).
                print!("{}", debug::join_list("alternates", &r.alternates));
                print!("{}", debug::join_list("viewed_by", &r.viewed_by));
                return Ok(());
            }
            let Some(r) = db.routes.iter().find(|r| r.url == url) else {
                anyhow::bail!("no row routes to {url}");
            };
            println!("url         {}", r.url);
            println!("kind        {}", tag(r.kind, &r.view, r.rows));
            if let Some(s) = &r.source {
                println!("source      {}", s.display());
            }
            if let Some(k) = &r.key {
                println!("key         {k}");
            }
            // The join, from the output side. This branch answers
            // for the routes NO row claims — a listing, an archive, a fold —
            // which is exactly where "what fed this" has no other answer.
            // Planning edges only: the citation half is added by the render
            // pass, and `explain` runs none.
            print!("{}", debug::join_list("inputs", &r.inputs));
        }
        // The graph, from the standpoint of one output. `explain`
        // answers "what is this"; this answers "what does it stand on, and in
        // what order would a pull do the work". Planning edges only, for
        // `explain`'s reason — the citation half is added by the render pass,
        // and the CLI runs none.
        Query::Pull { url } => {
            let Some(r) = db.routes.iter().find(|r| r.url == url) else {
                anyhow::bail!("no output at {url} — `grackle query urls` lists them");
            };
            let g = model::graph::Graph::of(db);
            let node = model::graph::Node::Output(r.id.clone());
            println!("output      {}", r.url);
            // The edge list, by what it demands: `content` is a row whose
            // bytes this output reads, `facts` an output this one arranges by
            // its planning facts alone. The split is why a fold that selects
            // itself is not a cycle.
            let edges: Vec<String> = g
                .needs(&node)
                .map(|e| {
                    let demand = match e.demand {
                        model::graph::Demand::Content => "content",
                        model::graph::Demand::Facts => "facts",
                    };
                    format!("{demand:<8} {}", e.from.label())
                })
                .collect();
            print!("{}", debug::capped_list("needs", &edges));
            let order: Vec<String> = g.pull(&node).iter().map(|n| n.label()).collect();
            print!("{}", debug::capped_list("pull", &order));
        }
    }
    Ok(())
}

/// `--has field=value` for `query posts`.
fn parse_has(spec: &str) -> Result<(String, String)> {
    let Some((field, value)) = spec.split_once('=') else {
        anyhow::bail!("--has expects field=value (e.g. tags=rust), got {spec:?}");
    };
    if field.is_empty() || value.is_empty() {
        anyhow::bail!("--has expects field=value (e.g. tags=rust), got {spec:?}");
    }
    Ok((field.to_string(), value.to_string()))
}

fn fmt_date(p: &model::Row, declared: &filter::Schema) -> String {
    model::date_fields(declared)
        .into_iter()
        .find_map(|f| p.as_date(f))
        .map(model::iso_date)
        .unwrap_or_else(|| "----------".into())
}
