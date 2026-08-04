//! Shared scaffolding for the `io_*.rs` and `profile_force.rs` integration
//! tests: writing a throwaway site under a temp directory, loading or
//! building it, and reading a rendered sitemap back out.
//!
//! A `support/` subdirectory is not its own test binary — only top-level
//! `tests/*.rs` files are — so this is the correct home for code every one of
//! those binaries wants: `mod support;` and `use support::*;` opts a file in.
//!
//! Not every file's scaffolding fits these shapes byte-for-byte (a `built`
//! that must not clear `_cache`, a `load_err` that must also catch a bad
//! `grackle.toml`, an expect-message that is one file's own prose); those
//! stay local rather than being forced through here.
//!
//! Each `io_*.rs` file is its own test binary and only `use`s the handful of
//! functions it needs, so every binary sees the rest as dead code — allowed
//! here rather than silenced per call site.
#![allow(dead_code)]

use grackle_core::model::SiteDb;
use std::path::{Path, PathBuf};

/// Write `files` under a fresh temp directory, wiping whatever an earlier run
/// left there. `prefix` keeps one test file's temp trees from colliding with
/// another file's; `who` keeps one test's tree from colliding with its
/// neighbours (they run in parallel and each deletes its own at both ends).
pub fn site<B: AsRef<[u8]>>(prefix: &str, who: &str, files: &[(&str, B)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-{prefix}-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

/// Load a site's database without rendering it.
pub fn load(dir: &Path) -> SiteDb {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    grackle_source::load(&cfg).expect("the site loads")
}

/// Load and render, discarding the database the render pass wrote into.
pub fn render(dir: &Path) -> grackle_core::build::SiteOutput {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle_core::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    out
}

/// Load and render, handing back the database the render pass wrote into.
pub fn built(dir: &Path) -> (grackle_core::build::SiteOutput, SiteDb) {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle_core::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    (out, db)
}

/// `built`, under a named profile (or none).
pub fn render_profile(
    dir: &Path,
    profile: Option<&str>,
) -> (grackle_core::build::SiteOutput, SiteDb) {
    let cfg = grackle_core::config::Config::load_profile(&dir.join("grackle.toml"), profile)
        .expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle_core::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    (out, db)
}

/// The site-load error, for a config that loads fine but a site that must
/// not: `Config::load` still gets its own `.expect`, since the shapes this
/// guards are load errors, not config errors.
pub fn load_err(dir: &Path) -> String {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    format!(
        "{:#}",
        grackle_source::load(&cfg).expect_err("the site must not load")
    )
}

/// The render error, for a site that loads but must not build.
pub fn build_err(dir: &Path) -> String {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let e = match grackle_core::build::render_site(&cfg, &mut db) {
        Ok(_) => panic!("the render must fail"),
        Err(e) => format!("{e:#}"),
    };
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    e
}

/// A rendered sitemap's `<loc>` entries, address-only (the `example.com`
/// prefix stripped) — the parse every fold-over-outputs test shares.
pub fn sitemap_locs(xml: &str) -> Vec<String> {
    xml.lines()
        .filter_map(|l| l.strip_prefix("<loc>")?.strip_suffix("</loc>"))
        .map(|u| u.trim_start_matches("https://example.com").to_string())
        .collect()
}

/// Build `dir` and hand back one sitemap route's selected URLs.
pub fn sitemap_urls(dir: &Path, route: &str) -> Vec<String> {
    let out = render(dir);
    let xml = String::from_utf8(
        out.get(route)
            .unwrap_or_else(|| panic!("no route at {route} — routes: {:?}", out.keys()))
            .clone(),
    )
    .expect("a sitemap is utf-8");
    sitemap_locs(&xml)
}
