mod build;
mod config;
mod db;
mod diff;
mod filter;
mod markdown;
mod markers;
mod render;
mod tags;
mod thumbs;
mod route;
mod store;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "grackle", about = "A virtual database over the site")]
struct Cli {
    #[arg(long, default_value = "grackle.toml", global = true)]
    config: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Query the content database.
    #[command(subcommand)]
    Query(Query),
    /// Dump the whole database as JSON.
    Export {
        /// Write to a file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Pretty-print.
        #[arg(long)]
        pretty: bool,
    },
    /// Compare rendered output against a reference Jekyll build.
    Diff {
        /// Reference site directory (e.g. ../_site-prod).
        #[arg(long)]
        against: PathBuf,
        /// Only compare posts whose body contains no liquid (phase 2a).
        #[arg(long, default_value_t = true)]
        liquid_free: bool,
        /// Restrict to these source paths, one per line (e.g. the clean set).
        #[arg(long)]
        only: Option<PathBuf>,
        /// Print the first delta for this URL and exit.
        #[arg(long)]
        show: Option<String>,
    },
    /// Render the site to a directory.
    Build {
        #[arg(long, default_value = "_site-grackle")]
        out: PathBuf,
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
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        year: Option<i32>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Tags with post counts.
    Tags,
    /// Monthly archive buckets.
    Archives,
    /// Everything known about one URL.
    Explain { url: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let t0 = std::time::Instant::now();
    let cfg = config::Config::load(&cli.config)?;
    let db = db::SiteDb::load(&cfg).context("loading site database")?;
    let total_ms = t0.elapsed().as_secs_f64() * 1000.0;

    match cli.cmd {
        Cmd::Query(q) => run_query(q, &db, total_ms)?,
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
            let s = build::build(&cfg, &db, &out)?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!("built {} in {:.0}ms", out.display(), ms);
            println!("  posts     {}", s.posts);
            println!("  pages     {}", s.pages);
            println!("  listings  {}", s.listings);
            println!("  copied    {}", s.copied);
            println!("  thumbs    {} (/static/)", s.thumbs);
            println!("  xml       {} (feed + sitemap)", s.serialized);
            println!("  css       {} bytes", s.css);
            if !s.skipped.is_empty() {
                println!("  skipped   {} (page templates using liquid)", s.skipped.len());
                for u in s.skipped.iter().take(8) {
                    println!("              {u}");
                }
            }
        }
        Cmd::Routes { depth, under } => routes_tree(&db, depth, under.as_deref()),
        Cmd::Diff { against, liquid_free, only, show } => {
            run_diff(&db, &against, liquid_free, only.as_deref(), show.as_deref())?
        }
    }
    Ok(())
}

/// A node in the route trie.
#[derive(Default)]
struct Node {
    kids: BTreeMap<String, Node>,
    /// Set when a route terminates here.
    leaf: Option<(db::RouteKind, Option<String>, Option<usize>)>,
    /// The route ends in `/` — it is served as a directory. Tracked separately
    /// because a childless node would otherwise render without its slash.
    dir_url: bool,
}

fn insert(root: &mut Node, url: &str, r: &db::Route) {
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

fn tag(kind: db::RouteKind, view: &Option<String>, rows: Option<usize>) -> String {
    let base = match kind {
        db::RouteKind::Post => "post".to_string(),
        db::RouteKind::Page => "page".to_string(),
        db::RouteKind::Static => "static".to_string(),
        db::RouteKind::Object => "object".to_string(),
        db::RouteKind::View => match view {
            Some(v) => format!("view {v}"),
            None => "view".into(),
        },
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

fn routes_tree(db: &db::SiteDb, depth: usize, under: Option<&str>) {
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

fn run_query(q: Query, db: &db::SiteDb, total_ms: f64) -> Result<()> {
    let p = &db.posts;
    match q {
        Query::Stats => {
            let dated = p.rows.iter().filter(|r| r.date.is_some()).count();
            let shared: Vec<_> = p.by_slug.iter().filter(|(_, v)| v.len() > 1).collect();
            println!("posts           {}", p.rows.len());
            println!("  dated         {}", dated);
            println!("  tagged        {}", p.rows.iter().filter(|r| !r.tags.is_empty()).count());
            println!("  drafts        {}", p.rows.iter().filter(|r| r.draft).count());
            println!("  hidden        {}", p.rows.iter().filter(|r| r.hidden).count());
            println!("pages           {}", db.pages.rows.len());
            println!("  rendered      {}", db.pages.rows.iter().filter(|r| r.rendered).count());
            println!("  static        {}", db.pages.rows.iter().filter(|r| !r.rendered).count());
            println!("objects         {}", db.objects.rows.len());
            println!("  distinct names{:>4}", db.objects.by_name.len());
            let dupes = db.objects.by_name.values().filter(|v| v.len() > 1).count();
            println!("  ambiguous     {}", dupes);
            println!("indexes");
            println!("  by_key        {}  (date, slug) unique", p.by_key.len());
            println!("  by_name       {}  post_url", p.by_name.len());
            println!("  by_slug       {}  ({} reused across dates)", p.by_slug.len(), shared.len());
            println!("  by_tag        {}", p.by_tag.len());
            println!("  by_year_month {}", p.by_year_month.len());
            println!("markers         {}  files found ({:.1}ms scan)", db.stats.markers, db.stats.markers_ms);
            println!("routes          {}", db.routes.len());
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for r in &db.routes {
                let k = match r.kind {
                    db::RouteKind::View => format!("view:{}", r.view.clone().unwrap_or_default()),
                    other => format!("{other:?}").to_lowercase(),
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
                    let this = format!("{:?}", r.kind).to_lowercase();
                    if &this != k {
                        continue;
                    }
                }
                println!("{}", r.url);
            }
        }
        Query::Posts { tag, year, limit } => {
            let mut n = 0;
            for &i in &p.order {
                let r = &p.rows[i];
                if let Some(t) = &tag {
                    if !r.tags.iter().any(|x| x == t) {
                        continue;
                    }
                }
                if let Some(y) = year {
                    if r.year_month().map(|(yy, _)| yy) != Some(y) {
                        continue;
                    }
                }
                println!("{}  {}", fmt_date(r), r.title);
                println!("    {}", r.url);
                n += 1;
                if n >= limit {
                    break;
                }
            }
        }
        Query::Tags => {
            let mut tags: Vec<(&String, usize)> =
                p.by_tag.iter().map(|(t, v)| (t, v.len())).collect();
            tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            for (t, n) in tags {
                println!("{n:4}  {t}");
            }
        }
        Query::Archives => {
            for ((y, m), v) in &p.by_year_month {
                println!("{y}-{m:02}  {:3} posts  /blog/{y}/{m:02}/", v.len());
            }
        }
        Query::Explain { url } => {
            if let Some(&i) = p.by_url.get(&url) {
                let r = &p.rows[i];
                println!("url         {}", r.url);
                println!("kind        post");
                println!("source      {}", r.path.display());
                println!("version     {:016x}", r.version);
                println!("date        {}", fmt_date(r));
                println!("slug        {}", r.slug);
                println!("stem        {}", r.stem);
                println!("name        {}  (post_url key)", r.name);
                println!("title       {}", r.title);
                println!("layout      {}", r.layout.as_deref().unwrap_or("-"));
                println!("draft       {}", r.draft);
                println!("hidden      {}", r.hidden);
                println!(
                    "tags        {}",
                    if r.tags.is_empty() { "-".into() } else { r.tags.join(", ") }
                );
                println!("body        {} bytes", r.body_bytes);
                let (newer, older) = p.neighbors(i);
                println!(
                    "newer       {}",
                    newer.map(|j| p.rows[j].url.as_str()).unwrap_or("-")
                );
                println!(
                    "older       {}",
                    older.map(|j| p.rows[j].url.as_str()).unwrap_or("-")
                );
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
        }
    }
    Ok(())
}

fn fmt_date(p: &db::Post) -> String {
    p.date
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "----------".into())
}

fn has_liquid(s: &str) -> bool {
    s.contains("{%") || s.contains("{{")
}

fn run_diff(
    db: &db::SiteDb,
    against: &std::path::Path,
    liquid_free: bool,
    only: Option<&std::path::Path>,
    show: Option<&str>,
) -> Result<()> {
    let allow: Option<std::collections::HashSet<String>> = match only {
        Some(p) => Some(
            std::fs::read_to_string(p)?
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
        ),
        None => None,
    };

    let mut rows = Vec::new();
    let mut skipped_liquid = 0usize;
    for p in &db.posts.rows {
        if liquid_free && has_liquid(&p.body) {
            skipped_liquid += 1;
            continue;
        }
        if let Some(a) = &allow {
            // `only` lists repo-relative paths; rows carry absolute ones.
            let hit = a.iter().any(|x| p.path.to_string_lossy().ends_with(x.trim_start_matches("./")));
            if !hit {
                continue;
            }
        }
        rows.push(diff::compare_post(&p.url, &p.body, against)?);
    }

    if let Some(url) = show {
        let Some(r) = rows.iter().find(|r| r.url == url) else {
            anyhow::bail!("{url} not in the compared set");
        };
        println!("{}  [{:?}]", r.url, r.verdict);
        if let Some(c) = r.cause {
            println!("cause: {c}\n");
        }
        let (a, b) = diff::first_delta(&r.reference, &r.mine);
        println!("--- jekyll (kramdown)\n{a}\n");
        println!("+++ grackle (comrak)\n{b}");
        return Ok(());
    }

    let mut tally: diff::Tally = Default::default();
    let mut causes: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &rows {
        *tally.entry(r.verdict).or_default() += 1;
        if let Some(c) = r.cause {
            *causes.entry(c).or_default() += 1;
        }
    }
    let n = rows.len().max(1);
    println!("compared {} posts against {}", rows.len(), against.display());
    if skipped_liquid > 0 {
        println!("  ({skipped_liquid} skipped: body contains liquid)");
    }
    println!();
    for v in [
        diff::Verdict::Identical,
        diff::Verdict::Equivalent,
        diff::Verdict::Differs,
        diff::Verdict::Missing,
    ] {
        let c = tally.get(&v).copied().unwrap_or(0);
        println!("  {:<12} {:>4}   {:>5.1}%", format!("{v:?}"), c, 100.0 * c as f64 / n as f64);
    }
    let ok = tally.get(&diff::Verdict::Identical).copied().unwrap_or(0)
        + tally.get(&diff::Verdict::Equivalent).copied().unwrap_or(0);
    println!("\n  usable (identical+equivalent): {ok}/{}  ({:.1}%)", rows.len(), 100.0 * ok as f64 / n as f64);

    let q = rows.iter().filter(|r| r.quotes_only).count();
    if q > 0 {
        let d = tally.get(&diff::Verdict::Differs).copied().unwrap_or(0);
        println!(
            "\n  of the {d} that differ, {q} differ ONLY in curly-quote choice",
        );
        println!("  (smartypants heuristic, not markup: {}/{} would be usable if matched -> {:.1}%)",
            ok + q, rows.len(), 100.0 * (ok + q) as f64 / n as f64);
    }

    if !causes.is_empty() {
        println!("\ndiffers, by cause:");
        let mut cs: Vec<_> = causes.into_iter().collect();
        cs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        for (c, n) in cs {
            println!("  {n:>4}  {c}");
        }
        println!("\nexamples:");
        for r in rows.iter().filter(|r| r.verdict == diff::Verdict::Differs).take(5) {
            println!("  {}  ({})", r.url, r.cause.unwrap_or(""));
        }
        println!("\n  grackle diff --against {} --show <url>   to see a delta", against.display());
    }
    Ok(())
}
