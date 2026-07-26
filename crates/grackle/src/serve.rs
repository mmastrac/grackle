//! `grackle serve`: the resident database behind a raw-hyper HTTP server.
//!
//! This is the phase change from compiler to CMS (DESIGN §7): the `SiteDb` and
//! the rendered output live in the process across requests and are served from
//! memory — no output directory, exactly what §2 calls for. A file watcher
//! turns saves into a background re-render and pings the browser to reload.
//!
//! v1 is deliberately coarse. §2's incremental invalidation (rebuild only the
//! pages that depend on the changed row) is future work; here the watcher just
//! **rebuilds the whole world** on any content change. At ~0.4s warm that is
//! already imperceptible for this site, and it keeps the first cut small: the
//! resident half of the thesis is real; the incremental half comes later.

use anyhow::{Context, Result};
use keepcalm::SharedMut;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::header::{CONTENT_TYPE, LOCATION};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use notify::{EventKind, RecursiveMode, Watcher};
use tokio::net::TcpListener;

use crate::build::{self, SiteOutput};
use crate::config::Config;
use std::sync::atomic::{AtomicBool, Ordering};

/// A rendered snapshot plus a version the browser polls to know when to reload.
///
/// `Clone` is required by `new_rcu` (it builds a copy-on-write cloner) but is
/// never invoked at runtime — the watcher replaces the whole snapshot with
/// `set`, not `write`, so the map is never actually deep-copied.
#[derive(Clone)]
struct Snapshot {
    version: u64,
    pages: SiteOutput,
    /// The inspector's payload (§7c), rebuilt with the site so it can never
    /// describe a database the pages didn't come from.
    debug: Vec<u8>,
}

/// An RCU cell (keepcalm): reads are lock-free clones of the current snapshot
/// and never contend with the writer; the watcher swaps a whole new snapshot in
/// with `set`, which — unlike an RCU write lock — skips the copy entirely. This
/// is exactly the read-mostly, wholesale-replaced shape a resident site wants.
type Shared = SharedMut<Snapshot>;

const VERSION_PATH: &str = "/__grackle/version";

pub fn serve(config_path: &Path, port: u16, profile: Option<&str>) -> Result<()> {
    // The base theme is compiled into the binary, which is right for
    // publishing and wrong for iterating ON the base: an edit to
    // `assets/base/` would need a rebuild before the preview moved. When the
    // source tree this binary came from is still on disk, read from it
    // instead, so the floor gets the same edit-reload loop the gallery has.
    if crate::base::use_source_tree_if_present() {
        println!("grackle: base theme is live from the source tree (dev build)");
    }
    let t = Instant::now();
    let (snap, pending) = render(config_path, 1, profile)?;
    println!(
        "grackle: rendered {} routes in {:.0}ms",
        snap.pages.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    let shared: Shared = SharedMut::new_rcu(snap);

    let root = Config::load(config_path)?.root();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        // Keep the watcher alive for the process lifetime by binding it here.
        let _watcher = spawn_watcher(
            config_path.to_path_buf(),
            root.clone(),
            profile.map(str::to_string),
            shared.clone(),
            tx.clone(),
            rx,
        )?;
        // Stale-while-revalidate (§6b): the first render served whatever
        // embeddings the cache had; bring them current off-thread and
        // re-render when done.
        embed_in_background(&root, pending, tx);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding http://{addr}/ (is the port in use?)"))?;
        println!("grackle: serving http://{addr}/  — watching for changes, Ctrl-C to stop");

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let shared = shared.clone();
            tokio::task::spawn(async move {
                let service = service_fn(move |req: Request<Incoming>| {
                    let shared = shared.clone();
                    async move { handle(req, shared).await }
                });
                // Client-side resets (BrokenPipe etc.) are normal; ignore them.
                let _ = http1::Builder::new().serve_connection(io, service).await;
            });
        }
    })
}

async fn handle(
    req: Request<Incoming>,
    shared: Shared,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    // RCU read: a cheap snapshot of the current site, no lock held. The whole
    // response is built synchronously below (no `.await`), so the guard never
    // crosses a suspend point.
    let snap = shared.read();
    let path = req.uri().path().to_string();

    // Live-reload: the injected script polls this for the current version.
    if path == VERSION_PATH {
        return Ok(reply(
            StatusCode::OK,
            "text/plain",
            snap.version.to_string().into_bytes(),
        ));
    }

    // The inspector owns `/__debug/` outright (§7c): served from the binary,
    // never from the site, never emitted by a build — and a miss inside the
    // prefix 404s here rather than falling through, so a site page cannot
    // shadow it.
    if crate::debug::is_debug_path(&path) {
        if path == "/__debug/site.json" {
            return Ok(reply(
                StatusCode::OK,
                "application/json",
                snap.debug.clone(),
            ));
        }
        return Ok(match crate::debug::asset(&path) {
            Some((ct, bytes)) => reply(StatusCode::OK, ct, bytes.to_vec()),
            None => reply(
                StatusCode::NOT_FOUND,
                "text/plain",
                b"no such inspector asset".to_vec(),
            ),
        });
    }

    if let Some(bytes) = snap.pages.get(&path) {
        return Ok(page(&path, bytes));
    }
    // A directory URL requested without its trailing slash (e.g. the
    // `/blog/page/2` pagination links) redirects to the real route.
    if !path.ends_with('/') {
        let slashed = format!("{path}/");
        if snap.pages.contains_key(&slashed) {
            return Ok(Response::builder()
                .status(StatusCode::FOUND)
                .header(LOCATION, slashed)
                .body(Full::new(Bytes::new()))
                .unwrap());
        }
    }
    Ok(reply(
        StatusCode::NOT_FOUND,
        "text/html; charset=utf-8",
        format!(
            "<!doctype html><meta charset=utf-8><title>404</title>\
                 <h1>404 — no route</h1><p><code>{}</code> is not in the database.</p>",
            crate::render::esc(&path)
        )
        .into_bytes(),
    ))
}

/// Serve a rendered page, injecting the live-reload script into HTML.
fn page(path: &str, bytes: &[u8]) -> Response<Full<Bytes>> {
    let ct = content_type(path);
    let body = if ct.starts_with("text/html") {
        inject_reload(bytes)
    } else {
        bytes.to_vec()
    };
    reply(StatusCode::OK, ct, body)
}

fn reply(status: StatusCode, ct: &'static str, body: Vec<u8>) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, ct)
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}

/// (Re)load config + database and render the whole site into memory.
///
/// This is "rebuild the world": config and db are re-read every time, so an
/// edit to `grackle.toml`, a post, a page, or the SCSS all take effect.
/// Also returns the embeddings that are missing or stale — the render used
/// the old vectors (stale-while-revalidate); the caller re-embeds off-thread.
fn render(
    config_path: &Path,
    version: u64,
    profile: Option<&str>,
) -> Result<(Snapshot, Vec<crate::embed::Pending>)> {
    let cfg = Config::load_profile(config_path, profile)?;
    let db = grackle_source::load(&cfg).context("loading site database")?;
    let mut db = db;
    let (pages, mut stats) = build::render_site(&cfg, &mut db)?;
    let pending = std::mem::take(&mut stats.embed_pending);
    let debug = crate::debug::payload(&cfg, &db)?;
    Ok((
        Snapshot {
            version,
            pages,
            debug,
        },
        pending,
    ))
}

/// Embed pending posts on a plain thread, then poke the rebuild channel so
/// the next render picks up the fresh vectors. One flight at a time; on
/// failure (e.g. offline model download) we log and do NOT re-poke — the
/// next natural rebuild retries, instead of a hot retry loop.
fn embed_in_background(
    root: &Path,
    pending: Vec<crate::embed::Pending>,
    tx: tokio::sync::mpsc::UnboundedSender<()>,
) {
    static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
    if pending.is_empty() || IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return;
    }
    let cache = root.join("_cache/embeddings");
    std::thread::spawn(move || {
        let n = pending.len();
        let t = Instant::now();
        let ok = crate::embed::embed_pending(&cache, &pending);
        IN_FLIGHT.store(false, Ordering::SeqCst);
        match ok {
            Ok(()) => {
                println!(
                    "grackle: embedded {n} posts in {:.1}s (background), re-rendering",
                    t.elapsed().as_secs_f64()
                );
                let _ = tx.send(());
            }
            Err(e) => eprintln!("grackle: background embedding failed: {e:#}"),
        }
    });
}

fn spawn_watcher(
    config_path: PathBuf,
    root: PathBuf,
    profile: Option<String>,
    shared: Shared,
    tx: tokio::sync::mpsc::UnboundedSender<()>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<()>,
) -> Result<notify::RecommendedWatcher> {
    let watch_tx = tx.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        // Access events are noise; react to create/modify/remove/rename only.
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        if ev.paths.iter().any(|p| is_content(p)) {
            let _ = watch_tx.send(());
        }
    })
    .context("creating file watcher")?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", root.display()))?;

    // A recursive watch does not descend a SYMLINKED directory — the kernel
    // watches inodes under the root, and a symlink's target is not one of
    // them. `themes/` is the one place a site plausibly points elsewhere (a
    // gallery kept outside the site, which is exactly what
    // `grackle/theme-preview` does), and a theme edit that needs a server
    // restart is a bad enough day to be worth these six lines. Best effort:
    // if the link is dangling or unreadable, the rest of the site still
    // watches.
    let themes = root.join("themes");
    if let Ok(real) = std::fs::canonicalize(&themes) {
        if real != themes {
            let _ = watcher.watch(&real, RecursiveMode::Recursive);
        }
    }

    // And the engine's own base, when a dev build is reading it from source
    // (`base::use_source_tree_if_present`). It is outside the site entirely,
    // so nothing else would ever notice it change.
    if let Some(base) = crate::base::dev_source() {
        let _ = watcher.watch(base, RecursiveMode::Recursive);
    }

    tokio::spawn(async move {
        let mut version = 1u64;
        while rx.recv().await.is_some() {
            // Debounce the save-storm (write + rename + chmod), then drain.
            tokio::time::sleep(Duration::from_millis(150)).await;
            while rx.try_recv().is_ok() {}
            version += 1;
            let t = Instant::now();
            let cp = config_path.clone();
            let pr = profile.clone();
            let result =
                tokio::task::spawn_blocking(move || render(&cp, version, pr.as_deref())).await;
            match result {
                Ok(Ok((snap, pending))) => {
                    let n = snap.pages.len();
                    shared.set(snap); // RCU full replace — no copy of the old map
                    println!(
                        "grackle: rebuilt {n} routes in {:.0}ms (v{version})",
                        t.elapsed().as_secs_f64() * 1000.0
                    );
                    // An edited post rendered with its stale embedding;
                    // refresh it off-thread and re-render on completion.
                    embed_in_background(&root, pending, tx.clone());
                }
                Ok(Err(e)) => eprintln!("grackle: rebuild failed: {e:#}"),
                Err(e) => eprintln!("grackle: rebuild task panicked: {e}"),
            }
        }
    });

    Ok(watcher)
}

/// Whether a changed path is site content worth rebuilding for. Excludes build
/// artifacts and VCS — critically `_cache/`, which a rebuild writes thumbnails
/// into (watching it would loop) — and typical editor temp files.
fn is_content(p: &Path) -> bool {
    const IGNORE: &[&str] = &[
        "/_cache",
        "/.git",
        "/grackle/target",
        "/_site",
        "/node_modules",
        "/vendor",
        "/.jekyll-cache",
        "/.sass-cache",
        "/_log",
    ];
    let s = p.to_string_lossy();
    if IGNORE.iter().any(|d| s.contains(d)) {
        return false;
    }
    // grackle's own tree is not site content — except grackle.toml (the config
    // a running server reloads on), a site's own `.style.scss` (§5b rung 1),
    // anything under a `themes/` directory, and the engine's own `assets/base/`
    // when a dev build serves it from source.
    // All four are presentation, never engine source: without the first
    // exclusion, editing grackle's Rust or DESIGN.md would pointlessly rebuild
    // the whole site; without the exceptions, neither a gallery living inside
    // the grackle tree nor the floor itself could hot-reload.
    if s.contains("/grackle/")
        && !s.ends_with("grackle.toml")
        && !s.ends_with(".style.scss")
        && !s.contains("/themes/")
        && !s.contains("/assets/base/")
    {
        return false;
    }
    !(s.ends_with('~') || s.ends_with(".swp") || s.ends_with(".tmp") || s.contains("/.#"))
}

fn content_type(path: &str) -> &'static str {
    if path.ends_with('/') {
        return "text/html; charset=utf-8";
    }
    match path.rsplit('.').next().unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "zip" => "application/zip",
        "wasm" => "application/wasm",
        "bin" => "application/octet-stream",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// Poll `/__grackle/version` and reload when it changes. Injected before
/// `</body>` (appended if there is none). Polling — not SSE — keeps v1 tiny;
/// no streaming body, no extra endpoint state.
const RELOAD_SCRIPT: &str = r#"<script>
(function () {
  var known = null;
  setInterval(function () {
    fetch('/__grackle/version', { cache: 'no-store' })
      .then(function (r) { return r.text(); })
      .then(function (v) {
        if (known === null) { known = v; return; }
        if (v !== known) { location.reload(); }
      })
      .catch(function () {});
  }, 500);
})();
</script>
"#;

fn inject_reload(bytes: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(bytes);
    match s.rfind("</body>") {
        Some(i) => {
            let mut out = String::with_capacity(s.len() + RELOAD_SCRIPT.len());
            out.push_str(&s[..i]);
            out.push_str(RELOAD_SCRIPT);
            out.push_str(&s[i..]);
            out.into_bytes()
        }
        None => {
            let mut v = bytes.to_vec();
            v.extend_from_slice(RELOAD_SCRIPT.as_bytes());
            v
        }
    }
}
