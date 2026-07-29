//! One walk, first rule wins (IO.md I7d).
//!
//! `read_posts` and `store::load_dir` are gone: there is one walk of the site
//! root, and which table a file lands in is what the first rule to claim it
//! says. Two laws hold the answers in place, and the four tests below are one
//! per law plus the controls a narrowing owes:
//!
//! - **the most-specific-source ordering** — scopes are asked deepest source
//!   first, sourceless scopes above the root, so `_posts` beats the tree
//!   whatever order the config declared them in;
//! - **a scope owns its source** — a file under a scope's source that no rule
//!   of that scope claims is not content, so an image beside a draft never
//!   reaches the objects catch-all or the tree's passthrough;
//! - **the punch-through** — the dot/underscore skip survives, and a declared
//!   `source` is admitted through it while every undeclared `_dir` stays out.
//!
//! Built sites rather than loaded ones, throughout. What these laws decide is
//! what the site PUBLISHES, and a test that asked the loader whether a row
//! exists would pass against an engine that emitted the file anyway — which is
//! precisely the failure mode of dropping scope-owns-source (the rows become
//! on-demand objects, and only a build materializes them).

use std::path::{Path, PathBuf};

/// A site that declines the base, so every rule under test is written here
/// rather than inherited. `$SCOPES` is the part each test varies.
fn config(scopes: &str) -> String {
    format!(
        "extends = \"none\"\n\
         [site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
         [schema]\nshell = {{ type = \"string\" }}\n\n{scopes}"
    )
}

/// The posts scope, spelled the way I7d makes every posts scope spell itself:
/// the rule names the extensions, because a scope owns its source and what its
/// rules do not claim is not content at all.
const POSTS: &str = "[[collections]]\n\
     name = \"posts\"\nsource = \"_posts\"\n\
     file = [\"{year}-{month}-{day}-{slug}\"]\n\n  \
     [[collections.rules]]\n  match = \"**/*.{md,markdown}\"\n  \
     route = \"/blog/{slug}/\"\n  defaults = { shell = \"html\" }\n\n";

/// A second posts source with no dates — grack.com's `_drafts`, minimised. It
/// is the scope whose bundle of non-markdown neighbours the ownership law
/// exists for.
const DRAFTS: &str = "[[collections]]\n\
     name = \"drafts\"\nsource = \"_drafts\"\n\
     file = [\"{slug}\"]\n\n  \
     [[collections.rules]]\n  match = \"**/*.{md,markdown}\"\n  \
     route = \"/drafts/{slug}/\"\n  defaults = { shell = \"html\" }\n\n";

/// The objects scope: sourceless, extension-gated, with the on-demand
/// catch-all grack.com carries — the rule that would publish a draft's images
/// the moment something cited them.
const OBJECTS: &str = "[[collections]]\n\
     name = \"objects\"\n\n  \
     [[collections.rules]]\n  match = \"**/*.{png,jpg,gif}\"\n  \
     route = \"/{path}\"\n  on_demand = true\n\n";

/// The tree: the root scope, catch-all last.
const TREE: &str = "[[collections]]\n\
     source = \".\"\n\n  \
     [[collections.rules]]\n  match = \"**/*.{html,md}\"\n  front_matter = true\n  \
     route = \"/{dir}/{stem}/\"\n  defaults = { shell = \"html\" }\n\n  \
     [[collections.rules]]\n  match = \"**/*\"\n  route = \"/{path}\"\n  \
     defaults = { shell = \"raw\" }\n\n";

/// A 1×1 GIF, so an image under test is a real image the object pass can read
/// a header from.
const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\
\x21\xf9\x04\x01\x00\x00\x00\x00\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\
\x02\x44\x01\x00\x3b";

fn site(who: &str, config: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-walk-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("grackle.toml"), config).unwrap();
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

/// Every URL the build publishes — the set `grackle urls` prints, which is
/// also the set an on-demand row joins only once something cites it.
fn published(dir: &Path) -> Vec<String> {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    let mut urls: Vec<String> = out.keys().cloned().collect();
    urls.sort();
    urls
}

/// `(collection, rule)` for the row at a URL — `grackle explain`'s two claim
/// lines, which is where the ordering law becomes observable.
fn claim(dir: &Path, url: &str) -> (String, String) {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let db = grackle_source::load(&cfg).expect("the site loads");
    let r = db
        .by_url
        .get(url)
        .and_then(|k| db.rows.get(k))
        .unwrap_or_else(|| panic!("no row at {url}"));
    (
        r.collection.clone(),
        r.rule.clone().unwrap_or_else(|| "-".into()),
    )
}

/// **A scope owns its source.** `_drafts/caret/` on grack.com is a draft and
/// eighteen files beside it — fifteen images the draft cites with relative
/// paths, an `.rtf` and an `.xcf` — and none of the eighteen is content. This
/// is that shape minimised, with the citation included, because the citation
/// is what makes the failure visible: an unclaimed image falls to the objects
/// catch-all as an ON-DEMAND row, which publishes nothing until something
/// references it, and the draft references it.
///
/// What the old code did here it did by accident: `store::load_dir` was handed
/// `["md", "markdown"]` and never saw the rest. The law says it out loud.
///
/// Mutation: widen `walk_site`'s `eligible` so an owned path also reaches the
/// sourceless and root scopes (`Some(o) => s.owned() == Some(o) ||
/// s.owned().is_none()`) — `/_drafts/caret/pic.gif` and
/// `/_drafts/caret/notes.rtf` join the published set, the first by the objects
/// catch-all and the citation, the second by the tree's passthrough. Measured
/// on the real corpus too (IO.md §11).
#[test]
fn a_scope_owns_its_source_so_a_drafts_bundle_is_not_content() {
    let dir = site(
        "owns",
        &config(&format!("{POSTS}{DRAFTS}{OBJECTS}{TREE}")),
        &[
            (
                "_drafts/caret/why-a-caret.md",
                b"---\ntitle: Caret\n---\n\n![pic](pic.gif)\n",
            ),
            ("_drafts/caret/pic.gif", GIF),
            ("_drafts/caret/notes.rtf", b"{\\rtf1}\n"),
            ("index.html", b"---\ntitle: Home\n---\nhi\n"),
        ],
    );

    let urls = published(&dir);
    assert!(
        urls.contains(&"/drafts/why-a-caret/".to_string()),
        "the draft itself is a post and lands at its scope's route: {urls:?}"
    );
    assert!(
        !urls.iter().any(|u| u.contains("caret/pic.gif")),
        "an image under a posts source that no posts rule claims is not \
         content — not an object, not a byte copy, nothing: {urls:?}"
    );
    assert!(
        !urls.iter().any(|u| u.contains("caret/notes.rtf")),
        "and neither is anything else the scope's rules did not ask for: {urls:?}"
    );
    // The control the ownership law owes: the same extensions OUTSIDE a
    // proper source are claimed exactly as before, so what the bundle loses it
    // loses for its position and not for its extension.
    let _ = std::fs::remove_dir_all(&dir);
}

/// The other half of the test above, and it is a separate site because a
/// control that shares a tree with its subject proves nothing about position:
/// the same `.gif` and `.rtf` at the ROOT are an object and a byte copy.
#[test]
fn the_same_files_outside_a_source_are_claimed_as_they_always_were() {
    let dir = site(
        "control",
        &config(&format!("{POSTS}{DRAFTS}{OBJECTS}{TREE}")),
        &[
            ("page.md", b"---\ntitle: P\n---\n\n![pic](/pic.gif)\n"),
            ("pic.gif", GIF),
            ("notes.rtf", b"{\\rtf1}\n"),
        ],
    );

    let urls = published(&dir);
    assert!(
        urls.contains(&"/pic.gif".to_string()),
        "cited, so the on-demand objects rule materializes it: {urls:?}"
    );
    assert!(
        urls.contains(&"/notes.rtf".to_string()),
        "and the tree's passthrough still copies bytes: {urls:?}"
    );
    assert_eq!(
        claim(&dir, "/pic.gif"),
        ("objects".into(), "**/*.{png,jpg,gif}".into()),
        "the sourceless scope claims by shape, above the root scope"
    );
    assert_eq!(
        claim(&dir, "/notes.rtf"),
        ("entries".into(), "**/*".into()),
        "and the root scope's catch-all takes what nothing above it wanted"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Most-specific-source, on the site that disqualifies declaration order.**
/// `theme-preview` declares its tree collection FIRST and its posts scope
/// second; under declaration order alone the tree's front-matter rule would
/// claim `_posts/2020-01-01-hello.md` and land it at `/_posts/hello/`, and the
/// blog would quietly cease to exist.
///
/// The assertion is the claim itself rather than only the URL, because the URL
/// alone could be right for the wrong reason.
///
/// Mutation: sort `scopes()` by declaration order alone (drop the class/depth
/// key) and the post is claimed by `entries` at `/_posts/hello/`.
#[test]
fn a_source_beats_the_root_whatever_order_the_config_declared() {
    let dir = site(
        "order",
        // Tree FIRST, posts second: theme-preview's order.
        &config(&format!("{TREE}{POSTS}{OBJECTS}")),
        &[
            (
                "_posts/2020-01-01-hello.md",
                b"---\ntitle: Hello\n---\n\nProse.\n",
            ),
            ("about.md", b"---\ntitle: About\n---\n\nProse.\n"),
        ],
    );

    assert_eq!(
        claim(&dir, "/blog/hello/"),
        ("posts".into(), "**/*.{md,markdown}".into()),
        "the deeper source wins, so the post is the posts scope's"
    );
    assert_eq!(
        claim(&dir, "/about/"),
        ("entries".into(), "**/*.{html,md}".into()),
        "and a page outside every source is still the root scope's"
    );
    let urls = published(&dir);
    assert!(
        !urls.iter().any(|u| u.starts_with("/_posts/")),
        "nothing lands under the source directory's own path: {urls:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The punch-through, and the five directories it does not open.** Jekyll's
/// dot/underscore skip survives — DESIGN.md §9b's "six underscore directories
/// need explicit excludes" obstacle is amended rather than paid — and a
/// declared `source` is what opens one of them.
///
/// Both halves in one site, because the interesting claim is the difference:
/// `_posts` is walked and `_hidden` is not, and the only thing that tells them
/// apart is that a scope named one.
///
/// Mutation: make `store::punches_through` return `false` and every post
/// leaves the corpus — on this site the assertion below, and on grack.com all
/// 331 of them.
#[test]
fn a_declared_source_punches_through_the_underscore_skip_and_nothing_else_does() {
    let dir = site(
        "punch",
        &config(&format!("{POSTS}{OBJECTS}{TREE}")),
        &[
            (
                "_posts/2020-01-01-hello.md",
                b"---\ntitle: Hello\n---\n\nProse.\n",
            ),
            ("_hidden/secret.md", b"---\ntitle: Secret\n---\n\nShh.\n"),
            ("_includes/social.html", b"<a>x</a>\n"),
            ("index.html", b"---\ntitle: Home\n---\nhi\n"),
        ],
    );

    let urls = published(&dir);
    assert!(
        urls.contains(&"/blog/hello/".to_string()),
        "the declared source is admitted: {urls:?}"
    );
    assert!(
        !urls.iter().any(|u| u.contains("secret")),
        "`_hidden/` is not declared by anything, so the skip still holds it: \
         {urls:?}"
    );
    assert!(
        !urls.iter().any(|u| u.contains("social")),
        "nor is `_includes/`: {urls:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The control a narrowing owes**: a site with no posts scope at all walks
/// exactly as it did — no source is declared, so nothing punches through, and
/// no scope owns anything, so the root scope claims or the load fails.
///
/// `examples/minimal` is this site, and it is the reason the control matters:
/// the punch-through must be driven by a declaration and not by the shape of
/// a name, or every `_dir` on every site would become content the day I7d
/// landed.
#[test]
fn a_site_with_no_source_walks_exactly_as_it_did() {
    let dir = site(
        "nosource",
        &config(&format!("{OBJECTS}{TREE}")),
        &[
            ("index.html", b"---\ntitle: Home\n---\nhi\n"),
            ("_posts/2020-01-01-hello.md", b"---\ntitle: H\n---\n\nx\n"),
            ("notes.txt", b"bytes\n"),
        ],
    );

    let urls = published(&dir);
    assert_eq!(
        urls,
        vec![
            "/css/main.css".to_string(),
            "/index/".to_string(),
            "/notes.txt".to_string()
        ],
        "an undeclared `_posts/` is just another underscore directory"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The contradiction the one walk creates, refused rather than suffered: a
/// site whose tree `exclude` names a scope's `source`.
///
/// Before I7d the two governed different walks, so the line was harmless — the
/// underscore skip kept `_posts` out of the tree anyway, and three fixtures
/// carried it as belt-and-braces. With one walk it empties the scope, and
/// nothing would have said so.
///
/// Mutation: delete the `keeps_dir` loop in `walk_site` and this site loads
/// clean, publishing a blog with no posts in it.
#[test]
fn a_source_the_tree_excludes_is_a_load_error() {
    let dir = site(
        "excluded",
        &config(&format!(
            "{POSTS}{OBJECTS}[[collections]]\nsource = \".\"\n\
             exclude = [\"_posts/**\"]\n\n  \
             [[collections.rules]]\n  match = \"**/*\"\n  route = \"/{{path}}\"\n  \
             defaults = {{ shell = \"raw\" }}\n\n"
        )),
        &[
            (
                "_posts/2020-01-01-hello.md",
                b"---\ntitle: Hello\n---\n\nProse.\n",
            ),
            ("index.html", b"hi\n"),
        ],
    );

    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let e = format!(
        "{:#}",
        grackle_source::load(&cfg).expect_err("the load must refuse the contradiction")
    );
    assert!(e.contains("posts"), "names the scope: {e}");
    assert!(e.contains("_posts"), "names the source: {e}");
    assert!(e.contains("exclude"), "names the key to delete: {e}");
    let _ = std::fs::remove_dir_all(&dir);
}
