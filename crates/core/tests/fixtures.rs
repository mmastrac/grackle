//! Fixture tests: a directory in, a directory out.
//!
//! Each fixture under `tests/fixtures/` is a whole site plus what it should
//! render to. That shape exists because a great many tests in this codebase
//! were testing *a site* while pretending to test a function: they built
//! `Row` values by hand, wired up `object_ix` themselves, faked the
//! pagination stamp a route would have carried — reconstructing what the
//! loader produces and therefore unable to catch a loader bug. A fixture
//! cannot lie that way, because its input is the same bytes a user would
//! write.
//!
//! It does NOT replace unit tests. A test that isolates one rule — a hole in
//! a fragment, an operator in the filter language — is clearer as a unit test
//! and should stay one. The line: if the subject is *a site*, it belongs
//! here; if the subject is *a function*, it does not.
//!
//! ```text
//! tests/fixtures/<name>/site/            the input: grackle.toml + content
//! tests/fixtures/<name>/out/             the expected rendered tree
//! tests/fixtures/<name>/expected-error   ...or the load error it must fail with
//! ```
//!
//! A fixture has `out/` or `expected-error`, never both. Blessing new output:
//!
//! ```bash
//! UPDATE_EXPECT=1 cargo test --test fixtures
//! ```
//!
//! Read what it wrote before committing it. A blessing tool that is trusted
//! blindly is a test suite that asserts the code does what the code does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Render one fixture's site, or the error it failed with.
fn render(site: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let cfg = grackle_core::config::Config::load(&site.join("grackle.toml"))
        .map_err(|e| format!("{e:#}"))?;
    let mut db = grackle_source::load(&cfg).map_err(|e| format!("{e:#}"))?;
    let rendered = grackle_core::build::render_site(&cfg, &mut db).map_err(|e| format!("{e:#}"));
    // Rendering creates `_cache/` beside the content (thumbnails, embedding
    // vectors). A fixture's `site/` must stay exactly the bytes its author
    // wrote, or `git status` goes dirty every time the suite runs and the
    // next person cannot tell a real change from a test artifact.
    let _ = std::fs::remove_dir_all(site.join("_cache"));
    let (out, _) = rendered?;
    // A route URL is the path it lands at; `/blog/` is `blog/index.html` on
    // disk, which is what a fixture author sees in `out/`.
    Ok(out
        .into_iter()
        .map(|(url, bytes)| (url_to_path(&url), bytes))
        .collect())
}

fn url_to_path(url: &str) -> String {
    let p = url.trim_start_matches('/');
    if p.is_empty() {
        "index.html".to_string()
    } else if p.ends_with('/') {
        format!("{p}index.html")
    } else {
        p.to_string()
    }
}

// A build is a pure function of its inputs: the feed's `<updated>` reads the
// newest entry's date (`shells::modified`), so fixtures compare raw bytes.

fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(rel) = p.strip_prefix(root) {
                let key = rel.to_string_lossy().replace('\\', "/");
                out.insert(key, std::fs::read(&p).unwrap_or_default());
            }
        }
    }
    out
}

fn write_tree(root: &Path, files: &BTreeMap<String, Vec<u8>>) {
    let _ = std::fs::remove_dir_all(root);
    for (rel, bytes) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("creating fixture output dir");
        }
        std::fs::write(&path, bytes).expect("writing fixture output");
    }
}

/// Compare, returning every difference rather than the first — a fixture that
/// moved three files should say so once.
fn compare(
    expected: &BTreeMap<String, Vec<u8>>,
    actual: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for name in expected.keys() {
        if !actual.contains_key(name) {
            problems.push(format!("missing from the build: {name}"));
        }
    }
    for name in actual.keys() {
        if !expected.contains_key(name) {
            problems.push(format!("built but not expected: {name}"));
        }
    }
    for (name, want) in expected {
        let Some(got) = actual.get(name) else {
            continue;
        };
        if want == got {
            continue;
        }
        let (sizes, texts) = (
            (want.len(), got.len()),
            (
                String::from_utf8(want.clone()),
                String::from_utf8(got.clone()),
            ),
        );
        problems.push(match texts {
            (Ok(w), Ok(g)) => format!("{name} differs:{}", first_line_diff(&w, &g)),
            _ => format!("{name} differs (binary, {} vs {} bytes)", sizes.0, sizes.1),
        });
    }
    problems
}

/// The first differing line, with its number — enough to see what happened
/// without printing a whole page.
fn first_line_diff(want: &str, got: &str) -> String {
    let (mut w, mut g) = (want.lines(), got.lines());
    let mut n = 0;
    loop {
        n += 1;
        match (w.next(), g.next()) {
            (None, None) => return " (whitespace only)".to_string(),
            (a, b) if a == b => continue,
            (a, b) => {
                return format!(
                    "\n      line {n}\n      expected: {}\n      actual:   {}",
                    a.unwrap_or("<end of file>"),
                    b.unwrap_or("<end of file>")
                )
            }
        }
    }
}

fn check(name: &str, dir: &Path, bless: bool) -> Vec<String> {
    let site = dir.join("site");
    if !site.join("grackle.toml").exists() {
        return vec![format!("{name}: no site/grackle.toml")];
    }
    let expected_error = dir.join("expected-error");
    let out_dir = dir.join("out");

    match (render(&site), expected_error.exists()) {
        (Err(e), true) => {
            let want = std::fs::read_to_string(&expected_error).unwrap_or_default();
            let want = want.trim();
            if e.contains(want) {
                Vec::new()
            } else {
                vec![format!(
                    "{name}: expected an error containing\n      {want}\n    got\n      {e}"
                )]
            }
        }
        (Err(e), false) => vec![format!("{name}: failed to build:\n      {e}")],
        (Ok(_), true) => vec![format!(
            "{name}: built successfully, but expected-error says it should not have"
        )],
        (Ok(files), false) => {
            if bless {
                write_tree(&out_dir, &files);
                println!("blessed {name} ({} files)", files.len());
                return Vec::new();
            }
            compare(&read_tree(&out_dir), &files)
                .into_iter()
                .map(|p| format!("{name}: {p}"))
                .collect()
        }
    }
}

/// One test over every fixture, collecting all failures before panicking — so
/// a broken fixture cannot hide the ones after it, and one run tells you
/// everything that moved.
#[test]
fn fixtures() {
    let bless = std::env::var_os("UPDATE_EXPECT").is_some();
    let root = fixtures_dir();
    let mut names: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("reading {}: {e}", root.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no fixtures in {}", root.display());

    let mut problems = Vec::new();
    for dir in &names {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        problems.extend(check(&name, dir, bless));
    }
    if !problems.is_empty() {
        panic!(
            "{} fixture problem(s) across {} fixture(s):\n  {}\n\n\
             If the change is intentional, re-bless with:\n  \
             UPDATE_EXPECT=1 cargo test --test fixtures\n\
             then READ the diff before committing it.",
            problems.len(),
            names.len(),
            problems.join("\n  ")
        );
    }
}
