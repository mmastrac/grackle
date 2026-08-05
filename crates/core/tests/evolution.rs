//! Evolution map: trace one site through a sequence of edits, asserting the
//! DELTA each makes to the built output — which URLs appear, change, or vanish,
//! or that the build errors. A behaviour test that pins what a change *does*,
//! not the exact bytes or internal shape, so it survives an internal redesign
//! the byte oracle would flag as churn. Each step's expected delta is also the
//! blast radius of that edit, and the chain reads top-to-bottom as a spec.
//!
//! Deltas are asserted EXACTLY, because the surprising entries are the point:
//! step 2 of `posts_evolution` re-renders an *existing* post (prev/next nav
//! couples neighbours), and step 3 proves a page touches nothing in the blog.
//! When a real behaviour change moves a step's delta, re-record it with
//! [`Evo::observe`] (prints the delta instead of asserting) and update the
//! `step` — the map is meant to be edited, not treated as a frozen golden.

mod support;

use grackle_core::build::SiteOutput;
use std::path::{Path, PathBuf};

// ---- the diff between two builds --------------------------------------------

#[derive(Default, Debug)]
struct Delta {
    added: Vec<String>,
    removed: Vec<String>,
    modified: Vec<String>,
}

fn diff(old: &SiteOutput, new: &SiteOutput) -> Delta {
    let mut d = Delta::default();
    for (k, v) in new {
        match old.get(k) {
            None => d.added.push(k.clone()),
            Some(prev) if prev != v => d.modified.push(k.clone()),
            _ => {}
        }
    }
    for k in old.keys() {
        if !new.contains_key(k) {
            d.removed.push(k.clone());
        }
    }
    d
}

fn build(dir: &Path) -> anyhow::Result<SiteOutput> {
    let cfg = grackle_core::config::Config::load(&dir.join("grackle.toml"))?;
    let mut db = grackle_source::load(&cfg)?;
    let (out, _) = grackle_core::build::render_site(&cfg, &mut db)?;
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    Ok(out)
}

// ---- the expected delta of one step -----------------------------------------

#[derive(Default)]
struct Want {
    added: Vec<&'static str>,
    removed: Vec<&'static str>,
    modified: Vec<&'static str>,
    /// Content-addressed additions: pin the count and `/static/*.css` shape,
    /// not the hash-derived names, which change with the bytes.
    added_static: Option<usize>,
}

/// The empty delta — a build that changed nothing.
fn nothing() -> Want {
    Want::default()
}

impl Want {
    fn add(mut self, xs: &[&'static str]) -> Self {
        self.added.extend(xs);
        self
    }
    fn del(mut self, xs: &[&'static str]) -> Self {
        self.removed.extend(xs);
        self
    }
    fn edit(mut self, xs: &[&'static str]) -> Self {
        self.modified.extend(xs);
        self
    }
    fn adds_static(mut self, n: usize) -> Self {
        self.added_static = Some(n);
        self
    }
}

fn assert_set(step: usize, label: &str, kind: &str, got: &[String], want: &[&str]) {
    let mut got: Vec<&str> = got.iter().map(String::as_str).collect();
    let mut want = want.to_vec();
    got.sort_unstable();
    want.sort_unstable();
    assert_eq!(
        got, want,
        "step {step} ({label}): {kind} URLs differ.\n  expected: {want:?}\n  got:      {got:?}"
    );
}

// ---- the evolving site ------------------------------------------------------

struct Evo {
    dir: PathBuf,
    prev: SiteOutput,
    n: usize,
}

impl Evo {
    fn seed(name: &str, files: &[(&str, &str)]) -> Evo {
        let dir = support::site("evolution", name, files);
        let prev = build(&dir).expect("the seed builds");
        Evo { dir, prev, n: 0 }
    }

    /// Apply an edit, rebuild, and assert the delta it made to the output.
    fn step(&mut self, label: &str, edit: impl FnOnce(&Path), want: Want) {
        self.n += 1;
        edit(&self.dir);
        let out = build(&self.dir).unwrap_or_else(|e| {
            panic!(
                "step {} ({label}): expected a delta, the build errored: {e:#}",
                self.n
            )
        });
        let d = diff(&self.prev, &out);
        match want.added_static {
            Some(n) => {
                assert_eq!(
                    d.added.len(),
                    n,
                    "step {} ({label}): expected {n} content-addressed addition(s), got {:?}",
                    self.n,
                    d.added
                );
                assert!(
                    d.added
                        .iter()
                        .all(|p| p.starts_with("/static/") && p.ends_with(".css")),
                    "step {} ({label}): additions are not /static/*.css: {:?}",
                    self.n,
                    d.added
                );
            }
            None => assert_set(self.n, label, "added", &d.added, &want.added),
        }
        assert_set(self.n, label, "removed", &d.removed, &want.removed);
        assert_set(self.n, label, "modified", &d.modified, &want.modified);
        self.prev = out;
    }

    /// Apply an edit to a COPY of the current site, assert the build fails with
    /// `needle`, and discard the copy — the main chain is left untouched. For
    /// the "poke here and you get an error" arm of the map.
    fn probe_error(&self, label: &str, edit: impl FnOnce(&Path), needle: &str) {
        let probe = self.dir.with_extension("probe");
        copy_tree(&self.dir, &probe);
        edit(&probe);
        let e = build(&probe)
            .err()
            .unwrap_or_else(|| panic!("probe ({label}): expected an error, the build succeeded"));
        let e = format!("{e:#}");
        assert!(
            e.contains(needle),
            "probe ({label}): expected an error containing {needle:?}, got: {e}"
        );
        let _ = std::fs::remove_dir_all(&probe);
    }

    /// DEV recorder: apply an edit and print its delta without asserting. Use
    /// it to re-record a step whose behaviour legitimately changed.
    #[allow(dead_code)]
    fn observe(&mut self, label: &str, edit: impl FnOnce(&Path)) {
        self.n += 1;
        edit(&self.dir);
        match build(&self.dir) {
            Ok(out) => {
                let d = diff(&self.prev, &out);
                eprintln!(
                    "step {} ({label}):\n  + {:?}\n  - {:?}\n  ~ {:?}",
                    self.n, d.added, d.removed, d.modified
                );
                self.prev = out;
            }
            Err(e) => eprintln!("step {} ({label}): ERROR {e:#}", self.n),
        }
    }
}

// ---- edits ------------------------------------------------------------------

fn write(rel: &'static str, body: impl AsRef<[u8]>) -> impl FnOnce(&Path) {
    move |d| {
        let p = d.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
}

fn post(title: &str) -> String {
    format!("---\ntitle: {title}\n---\n\nA body.\n")
}

fn copy_tree(src: &Path, dst: &Path) {
    let _ = std::fs::remove_dir_all(dst);
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

// ---- chains -----------------------------------------------------------------

/// From an empty config, watch the blog machinery light up — and watch what
/// each addition does NOT touch.
#[test]
fn posts_evolution() {
    // Empty config over an empty tree: the base's blog routes stand down with
    // nothing to show, so only the sitemap exists.
    let mut ev = Evo::seed("posts", &[("grackle.toml", "")]);

    // The first post lights up the homepage listing, the feed, and the blog
    // index all at once; the sitemap gains its URL.
    ev.step(
        "add the first post",
        write("_posts/2026-01-01-hello.md", post("Hello")),
        nothing()
            .add(&["/", "/atom.xml", "/blog/", "/blog/2026/01/01/hello/"])
            .edit(&["/sitemap.xml"]),
    );

    // A second post adds its own page — and re-renders the FIRST post, because
    // prev/next navigation couples adjacent posts. The listings and feed
    // refresh too. This coupling is the kind of thing the map exists to show.
    ev.step(
        "add a second post",
        write("_posts/2026-02-01-world.md", post("World")),
        nothing().add(&["/blog/2026/02/01/world/"]).edit(&[
            "/",
            "/atom.xml",
            "/blog/",
            "/blog/2026/01/01/hello/",
            "/sitemap.xml",
        ]),
    );

    // A page is isolated: it touches nothing in the blog machinery — not the
    // homepage, not the feed, not the other posts — only itself and the sitemap.
    ev.step(
        "add a page",
        write("about.md", post("About")),
        nothing().add(&["/about/"]).edit(&["/sitemap.xml"]),
    );

    // Poke: a post that links a route nothing serves fails the build (strict
    // links), rather than shipping a 404. Checked on a copy; the chain stands.
    ev.probe_error(
        "a broken internal link fails the build",
        write(
            "_posts/2026-03-01-broken.md",
            "---\ntitle: Broken\n---\n\n[gone](/nope/)\n",
        ),
        "nope",
    );
}

const A_POST: &str = "---\ntitle: P\n---\n\nA body.\n";

/// The q54 blast radius, made testable: switching `[assets] addressing` to
/// `hashed` moves the stylesheet under `/static` and rewrites the `<link>` in
/// every page that carries it. The delta IS the blast radius.
#[test]
fn assets_evolution() {
    let mut ev = Evo::seed(
        "assets",
        &[
            ("grackle.toml", ""),
            ("_posts/2026-01-01-hello.md", A_POST),
            ("_posts/2026-02-01-world.md", A_POST),
        ],
    );
    // The stylesheet moves under /static, and every page that links it is
    // rewritten — but NOT the feed or sitemap, which carry no `<link>`. The
    // modified set is exactly "pages with the stylesheet": the firebreak, seen.
    // The added /static/{hash}.css name is content-derived, so pin its shape.
    ev.step(
        "addressing = hashed",
        write("grackle.toml", "[assets]\naddressing = \"hashed\"\n"),
        nothing().adds_static(1).del(&["/css/main.css"]).edit(&[
            "/",
            "/blog/",
            "/blog/2026/01/01/hello/",
            "/blog/2026/02/01/world/",
        ]),
    );
}
