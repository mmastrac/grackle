//! `root.html`: a theme root may be document-shaped (IO.md §6, items I4 and
//! I5).
//!
//! Four claims, and the first is the one the migration rests on:
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
//! 3. **A head `<style>` is CSS, so it goes where the CSS goes** (I5). The
//!    fence lets a theme root declare presentation; the assembly is what
//!    makes that declaration mean something. The style is compiled into the
//!    theme layer of the theme's own sheet — after `theme.scss`, so the file
//!    that states the theme's frame outranks the general sheet — and the page
//!    keeps exactly one stylesheet link and no inline `<style>` at all. I4
//!    emitted it inline in the computed head as a declared interim; this is
//!    the byte that moved.
//! 4. **`shell.html` is gone, and says so.** The chrome part kind renamed
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
    site_with(who, root_html, &[])
}

/// The same, plus any extra theme files the caller needs — `theme.scss`, for
/// the tests about where a head style lands relative to it.
fn site_with(who: &str, root_html: &str, extra: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-root-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    let mut files = vec![
        (
            "grackle.toml".to_string(),
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\
             theme = \"mine\"\n"
                .to_string(),
        ),
        (
            "_posts/2020-01-01-hello.md".to_string(),
            "---\ntitle: Hello\n---\n\nProse.\n".to_string(),
        ),
        ("themes/mine/root.html".to_string(), root_html.to_string()),
        // An identity slot with words in it: the derivation reads the ROOT
        // kind's schema and the root fragment's slots, so a fill that lands
        // is the cheapest proof both followed the rename.
        (
            ".slots/copyright.md".to_string(),
            "Copyright the tree.\n".to_string(),
        ),
    ];
    files.extend(
        extra
            .iter()
            .map(|(rel, body)| (rel.to_string(), body.to_string())),
    );
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

fn render(dir: &Path) -> grackle::build::SiteOutput {
    render_stats(dir).0
}

fn render_stats(dir: &Path) -> (grackle::build::SiteOutput, grackle::build::Stats) {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let out = grackle::build::render_site(&cfg, &mut db).expect("the site renders");
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
/// `binder::split_root`. **The outcome changed at I5, and was re-measured on
/// the mutant with the real binary rather than carried over.** At I4 the head
/// half was emitted verbatim, so the tag was PUBLISHED — a theme `<title>`
/// gave every page two of them, valid HTML no build would complain about. Now
/// the head half leaves as CSS, so a probe root carrying `<meta
/// name="theme-color">`, `<title>Themed</title>` and a `<style>` builds clean
/// and publishes a page with **neither the meta nor the title** — only the
/// style, in the sheet. The failure moved from "quietly wrong output" to
/// "quietly no output", which is the disease this ledger refuses in either
/// direction, and the fence is what makes it neither.
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

/// **I5: a `<style>` passes the fence and leaves through the CSS assembly.**
/// The head carries no theme styles at all — the rule the theme declared is
/// in the theme layer of the theme's own sheet, and the page links that sheet
/// and nothing else.
///
/// This is the byte I4 declared would move: it emitted the `<style>` inline
/// in the computed head as an interim, on the grounds that a validated
/// declaration must take effect. It still does; it takes effect where CSS
/// takes effect. What the move buys is the model's own rule — one artifact,
/// one link — instead of a second, unlayered stylesheet riding along on every
/// page.
///
/// Mutation: restore I4's emission (`head_html.push_str(&self.head)` in
/// `Theme::page`) and the head carries the rule again → the `!contains` and
/// the `<style` assertions go red. Mutation, the other half: drop the
/// `head_style` argument at `css_pass`'s call sites (pass `""`) and the sheet
/// loses the rule while the theme still loads clean — the silent-drop shape
/// the fence exists to refuse.
#[test]
fn a_head_style_lands_in_the_css_and_not_in_the_head() {
    let out = render(&site(
        "style",
        &format!(
            "<head><style>:root {{ --accent: rebeccapurple; }}</style></head>\
             <body>{CHROME}</body>"
        ),
    ));
    let html = page(&out, "/blog/2020/01/01/hello/");
    // Nowhere in the document — not the head, not the body, no inline
    // `<style>` element of any kind.
    assert!(!html.contains("rebeccapurple"), "{html}");
    assert!(
        !html.contains("<style"),
        "the head carries no styles: {html}"
    );
    // And the one thing the head does carry about CSS is still the one link.
    assert_eq!(
        html.matches("rel='stylesheet'").count(),
        1,
        "exactly one stylesheet link: {html}"
    );
    assert!(html.contains("/css/mine.css"), "{html}");

    // In the theme layer of the theme's sheet — after the base layer, which
    // is what makes a theme's rule beat the engine's whatever the selectors
    // say (themes/DESIGN.md §7).
    let css = page(&out, "/css/mine.css");
    let at = css.find("rebeccapurple").unwrap_or_else(|| panic!("{css}"));
    let theme_layer = css
        .find("@layer theme {")
        .unwrap_or_else(|| panic!("the sheet opens a theme layer: {css}"));
    let base_layer = css
        .find("@layer base {")
        .unwrap_or_else(|| panic!("the sheet opens a base layer: {css}"));
    assert!(base_layer < theme_layer && theme_layer < at, "{css}");
    // A head style is not a `theme.scss`: the theme still never wrote a sheet,
    // so it never declined the decorative half of the base (§7's skins).
    assert!(css.contains("border-left"), "the skins still ship: {css}");
}

/// **The ordering pin.** A rule in `theme.scss` and the same rule in the
/// root's head, same selector and same property: the root's wins. Both land
/// in the one `theme` layer, so the cascade decides on source order, and the
/// order is `theme.scss` first — `root.html` is the file that states the
/// theme's own frame, so what it says about the frame outranks the general
/// sheet. It is also the placement that preserves I4's inline behaviour: a
/// `<style>` last in a `<head>` beat the stylesheet link above it, and last
/// in the layer keeps the same rule winning.
///
/// Mutation: swap the two pushes in `css_pass` so the head style goes in
/// first, and `from-scss` wins instead.
#[test]
fn a_root_head_style_outranks_the_themes_own_sheet() {
    let out = render(&site_with(
        "order",
        &format!("<head><style>main {{ color: teal; }}</style></head><body>{CHROME}</body>"),
        &[("themes/mine/theme.scss", "main { color: peru; }\n")],
    ));
    let css = page(&out, "/css/mine.css");
    let scss = css
        .find("peru")
        .unwrap_or_else(|| panic!("theme.scss reaches the sheet: {css}"));
    let root = css
        .find("teal")
        .unwrap_or_else(|| panic!("the head style reaches the sheet: {css}"));
    assert!(scss < root, "the root's head style comes last: {css}");
    // One layer block, not two — same layer, so this really is source order
    // deciding and not a second `@layer theme` sorting after the first.
    assert_eq!(css.matches("@layer theme {").count(), 1, "{css}");
    let layer = css.find("@layer theme {").expect("a theme layer");
    assert!(layer < scss, "both are inside it: {css}");
}

/// A head style is SCSS, compiled through grass exactly as `theme.scss` is
/// (the I5 decision): nesting works, and `@import "tokens"` reaches the
/// theme's own partial. The other face of that decision is that a style which
/// does not compile is the same event as a `theme.scss` that does not —
/// reported on stderr, recorded in `Stats::css_errors`, and a publishing
/// build refuses rather than shipping a sheet with a hole in it.
///
/// Mutation: drop the `Err` arm's `css_errors.push` for the head style and
/// the broken style is silently absent from a sheet that publishes fine.
#[test]
fn a_head_style_is_scss_and_a_broken_one_refuses_to_publish() {
    // Nesting, and a partial the theme owns: neither is available to a style
    // passed through verbatim, which is the whole of why this compiles.
    let out = render(&site_with(
        "scss",
        &format!(
            "<head><style>@import \"tokens\";\n\
             main {{ color: var(--edge); a {{ color: teal; }} }}</style></head>\
             <body>{CHROME}</body>"
        ),
        &[("themes/mine/_tokens.scss", ":root { --edge: peru; }\n")],
    ));
    let css = page(&out, "/css/mine.css");
    assert!(css.contains("main a"), "nesting compiled: {css}");
    assert!(css.contains("--edge: peru"), "the partial resolved: {css}");

    let (out, stats) = render_stats(&site(
        "scss-broken",
        &format!("<head><style>@include nothing;</style></head><body>{CHROME}</body>"),
    ));
    assert_eq!(stats.css_errors.len(), 1, "{:?}", stats.css_errors);
    assert!(
        stats.css_errors[0].contains("root.html"),
        "it names the file the style came from: {:?}",
        stats.css_errors
    );
    // The sheet is still emitted (so `serve` shows the page), and simply has
    // no theme layer — `build` is where the refusal happens.
    assert!(
        !page(&out, "/css/mine.css").contains("@layer theme {"),
        "nothing half-compiled reached the layer"
    );
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
    // The style lands where I5 puts every root head style — the sheet, not
    // the page.
    assert!(
        page(&out, "/css/mine.css").contains("color: red"),
        "the style lands in the sheet"
    );
    assert!(!html.contains("color: red"), "and not in the page: {html}");
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
