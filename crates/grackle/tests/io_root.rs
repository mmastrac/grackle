//! `root.html`: a theme root may be document-shaped (IO.md §6, item I4).
//!
//! Three claims, and the first is the one the migration rests on:
//!
//! 1. **A body-only root is the old chrome fragment, byte for byte.** The
//!    `<body>` wrapper is accepted but not required — a file with neither
//!    wrapper IS the body — which is what made `shell.html` → `root.html` a
//!    rename across nine themes and nothing else. The corpus parity run is
//!    this claim at scale; the test here is the claim stated small, on both
//!    spellings of the same chrome.
//! 2. **The head fence.** A theme root's `<head>` may hold `<style>` and
//!    nothing else, because the engine COMPUTES the head — title, charset,
//!    canonical, the `[html.head.*]` tables, hreflang, the one stylesheet
//!    link. A theme's `<title>` would be shadowed on every page, silently,
//!    and a theme's second stylesheet `<link>` would break the one-artifact
//!    rule the CSS assembly is built on. So it is a load error naming the
//!    file and the element, rather than a tag that quietly does nothing.
//! 3. **`shell.html` is gone, and says so.** The chrome part kind renamed
//!    `shell` → `root`, so a stale `shell.html` was never going to be
//!    *silent* — `Fragments::load` rejects a fragment naming no kind. What
//!    it would have been is MISLEADING: "fragment names no layout kind
//!    `shell`" sends its reader hunting for a kind when the fix is a rename.
//!    That is the one case §10's precedent allows a targeted sentence for.
//!
//! A site rather than a unit test, for `io_shell.rs`'s reason: what these
//! assert is what a PAGE comes out as, and the head half in particular is
//! only observable after the computed head, the theme merge and the root
//! shell have all run.

use std::path::{Path, PathBuf};

/// One post and one theme, whose `root.html` is whatever the caller says.
fn site(who: &str, root_html: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-root-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\
             theme = \"mine\"\n"
                .to_string(),
        ),
        (
            "_posts/2020-01-01-hello.md",
            "---\ntitle: Hello\n---\n\nProse.\n".to_string(),
        ),
        ("themes/mine/root.html", root_html.to_string()),
        // An identity slot with words in it: the derivation reads the ROOT
        // kind's schema and the root fragment's slots, so a fill that lands
        // is the cheapest proof both followed the rename.
        (".slots/copyright.md", "Copyright the tree.\n".to_string()),
    ];
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

fn render(dir: &Path) -> grackle::build::SiteOutput {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    out
}

fn fails(dir: &Path) -> String {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let e = grackle::build::render_site(&cfg, &mut db)
        .map(|_| ())
        .expect_err("this root must not load");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    format!("{e:#}")
}

fn page(out: &grackle::build::SiteOutput, route: &str) -> String {
    String::from_utf8(
        out.get(route)
            .unwrap_or_else(|| panic!("no route at {route} — routes: {:?}", out.keys()))
            .clone(),
    )
    .expect("the page is utf-8")
}

/// The chrome every case here arranges, as a bare fragment — which is
/// exactly what a `shell.html` was.
const CHROME: &str = "<header><a data-slot=\"site_title\" href=\"/\"></a>\
                      <nav data-slot=\"nav\"></nav></header>\
                      <main data-slot=\"main\"></main>\
                      <footer><p data-slot=\"copyright\"></p></footer>";

/// **The migration, as a claim about bytes.** A `<body>`-wrapped root and a
/// bare fragment carrying the same chrome render the same site, character
/// for character — so accepting the unwrapped shape is not a second code
/// path, it is the wrapper being optional.
///
/// This is why `shell.html` → `root.html` was `git mv` and nothing else, and
/// why the corpus parity run could be exact modulo one attribute.
///
/// Mutation: make `split_root` require the wrapper (drop the `wrapped` early
/// return) and the bare site fails to load — its chrome parses as a `<header>`
/// beside no `<head>`/`<body>`, which is the error the wrapped shape's
/// top-level rule is for.
#[test]
fn the_body_wrapper_is_optional_and_inert() {
    let bare = render(&site("bare", CHROME));
    let wrapped = render(&site("wrapped", &format!("<body>{CHROME}</body>")));
    let url = "/blog/2020/01/01/hello/";
    assert_eq!(
        page(&bare, url),
        page(&wrapped, url),
        "a bare fragment IS the body"
    );
    let html = page(&bare, url);
    // And it really is the theme's chrome, not the base's fallback — with
    // the tree's own words in the identity slot the root left for them.
    assert!(
        html.contains("<footer><p data-slot=\"copyright\">Copyright the tree.</p></footer>"),
        "{html}"
    );
    // The engine still owns <html> — and the stamp is `root` now.
    assert!(
        html.contains("<html lang=\"en\" data-kind=\"root\">"),
        "{html}"
    );
}

/// **The head fence.** `<meta>` in a theme root's head is a load error that
/// names the file and the element — the house style — and says what the
/// engine owns instead.
///
/// Mutation: delete the `check_head_fence(el, file)?` call in
/// `binder::split_root` and the site LOADS and PUBLISHES the tag — measured
/// on the mutant with the real binary, not reasoned: `<meta
/// name="theme-color" content="#123456">` comes out in the head of all three
/// pages, because the head half is emitted verbatim. Which is worse than
/// dropping it, and is the whole argument for the fence: a theme `<title>`
/// would give every page two of them, a theme `<link rel="canonical">` would
/// compete with the engine's, and a theme stylesheet `<link>` would quietly
/// break the one-artifact rule — all of it valid HTML that no build would
/// ever complain about.
#[test]
fn a_theme_head_may_hold_style_and_nothing_else() {
    let e = fails(&site(
        "fence",
        &format!(
            "<head>\n\t<meta name=\"theme-color\" content=\"#123456\">\n</head>\n\
             <body>{CHROME}</body>\n"
        ),
    ));
    assert!(e.contains("root.html:2"), "it names the file and line: {e}");
    assert!(e.contains("<meta>"), "it names the element: {e}");
    assert!(e.contains("<style> and nothing else"), "{e}");
    // And it says who owns the head, which is the reason rather than the rule.
    assert!(e.contains("The engine computes the head"), "{e}");

    // The other three fenced elements, each by its own name — the fence is
    // "everything but <style>", not a blocklist that could forget one.
    for tag in [
        "<title>x</title>",
        "<link rel=\"stylesheet\" href=\"/x.css\">",
        "<script></script>",
    ] {
        let name = tag.split_once([' ', '>']).map(|(n, _)| n).unwrap_or(tag);
        let e = fails(&site(
            "fence-each",
            &format!("<head>{tag}</head><body>{CHROME}</body>"),
        ));
        assert!(e.contains(&format!("{name}>")), "{tag}: {e}");
    }
}

/// A `<style>` passes the fence, and — the INTERIM decision this item records
/// (IO.md §11) — it is emitted verbatim inside the computed head, after the
/// engine's own facts. I5 moves it into the CSS assembly; until then the
/// least-surprising reading is that the declared style takes effect rather
/// than being validated and then discarded.
///
/// Last in the head is where a `<style>` belongs anyway: the engine's facts
/// are never displaced, and the theme's rules win the cascade against the one
/// stylesheet link above them.
///
/// Mutation: delete the `if !self.head.is_empty()` block in `Theme::page` and
/// the rule vanishes from every page while the theme still loads clean —
/// which is precisely the shape the fence refuses for every other element.
#[test]
fn a_head_style_lands_after_the_computed_head() {
    let out = render(&site(
        "style",
        &format!(
            "<head><style>:root {{ --accent: rebeccapurple; }}</style></head>\
             <body>{CHROME}</body>"
        ),
    ));
    let html = page(&out, "/blog/2020/01/01/hello/");
    let head = html
        .split_once("<head>")
        .and_then(|(_, r)| r.split_once("</head>"))
        .map(|(h, _)| h.to_string())
        .expect("a page has a head");
    assert!(head.contains("--accent: rebeccapurple"), "{head}");
    // AFTER the engine's own head, all of which is still there.
    let style = head.find("<style>").expect("the style is in the head");
    for computed in ["<title>", "<meta charset=", "rel='stylesheet'"] {
        let at = head
            .find(computed)
            .unwrap_or_else(|| panic!("{computed}: {head}"));
        assert!(
            at < style,
            "{computed} comes before the theme's style: {head}"
        );
    }
    // Exactly once, and not loose in the body.
    assert_eq!(html.matches("rebeccapurple").count(), 1, "{html}");
}

/// A root with a `<head>` and no `<body>` is legal, and inherits the base's
/// chrome — which needs no rule of its own: the fragment merge is by name, so
/// a theme contributing no `root` body keeps the base's. This is the
/// cheapest real theme the design allows, and it is why the head half is not
/// simply bolted onto the body half.
///
/// Mutation: make the head-only case keep an empty body fragment instead of
/// dropping out of `own`, and the page loses its chrome entirely — `<body>`
/// comes out empty, the page's own content included.
#[test]
fn a_head_only_root_inherits_the_base_chrome() {
    let out = render(&site(
        "head-only",
        "<head><style>a { color: red; }</style></head>",
    ));
    let html = page(&out, "/blog/2020/01/01/hello/");
    assert!(html.contains("color: red"), "the style lands: {html}");
    // The base's own chrome, which keys its geometry on [data-frame], and
    // the base's own identity slots reading the tree's words.
    assert!(
        html.contains("<main data-frame data-slot=\"main\">"),
        "{html}"
    );
    assert!(
        html.contains("<p data-slot=\"copyright\">Copyright the tree.</p>"),
        "{html}"
    );
}

/// A document-shaped root holds `<head>` and `<body>` and nothing beside
/// them: the engine writes `<html>`, so a stray sibling has nowhere to go
/// and would be dropped without this.
#[test]
fn nothing_may_sit_beside_a_roots_head_and_body() {
    let e = fails(&site(
        "sibling",
        &format!("<body>{CHROME}</body>\n<footer>orphan</footer>\n"),
    ));
    assert!(
        e.contains("<footer> beside a theme root's <head>/<body>"),
        "{e}"
    );
    assert!(e.contains("Move it inside <body>"), "{e}");
}

/// **The stale file.** A theme still carrying `shell.html` is a load error
/// naming `root.html`, because silence here would be silent chrome loss.
///
/// What it would be WITHOUT the check was measured rather than assumed, and
/// it is not silence: the part kind renamed, so `Fragments::load` rejects a
/// fragment whose stem names no layout kind. Mutation — delete the
/// `shell.html` test in `Theme::load` — and the site still fails, with
/// "fragment names no layout kind `shell` — kinds are: root, document, …".
/// True, and useless: it sends its reader to look for a kind when the fix is
/// `git mv`. So this is I3's precedent applied (§10: one targeted sentence
/// only where the generic diagnosis misleads), not a guard against silence.
#[test]
fn a_stale_shell_html_names_root_html() {
    let dir = site("stale", CHROME);
    std::fs::rename(
        dir.join("themes/mine/root.html"),
        dir.join("themes/mine/shell.html"),
    )
    .unwrap();
    let e = fails(&dir);
    assert!(e.contains("`shell.html` is `root.html` now"), "{e}");
    assert!(e.contains("themes/mine"), "it names the theme: {e}");
    assert!(
        e.contains("body chrome, unchanged"),
        "the fix is a rename: {e}"
    );
}
