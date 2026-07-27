//! Theme sources are not content (IO.md I7b).
//!
//! A site-root `themes/` is engine vocabulary by POSITION — the class
//! `.slots/`, `.section`, `.schema.toml` and the config file itself already
//! occupy. The build reads themes from exactly one place (`root.join("themes")`
//! in `build.rs`, twice: `Themes::load_all` and the per-theme CSS pass), so
//! what sits there is input to the build, and a walk that also publishes it
//! ships a theme's `root.html` and `theme.scss` as pages of the site.
//!
//! Built sites rather than loaded ones, because publishing is the disease: a
//! test that asked the loader whether a row exists would pass against an engine
//! that emitted the file anyway.

use std::path::{Path, PathBuf};

/// The gallery shape, minimised: a real theme under `themes/`, and its exact
/// twin one directory over. The twin is the control — the same bytes at
/// `pages/mine/` are ordinary content, so anything the theme copy loses it
/// loses for its POSITION and not for its name, its extension or its shape.
fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "themes/mine/root.html",
            "<main data-slot=\"main\"></main>\n",
        ),
        ("themes/mine/theme.scss", "body { color: red; }\n"),
        ("pages/mine/root.html", "<main data-slot=\"main\"></main>\n"),
        ("pages/mine/theme.scss", "body { color: red; }\n"),
        ("index.html", "hello\n"),
    ]
}

const CONFIG: &str = "extends = \"none\"\n\
     [site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
     [[collections]]\nkind = \"tree\"\nsource = \".\"\n";

const CATCH_ALL: &str = "\n  [[collections.rules]]\n  match = \"**/*\"\n  route = \"/{path}\"\n";

/// Write a site and hand back its directory. Per-test directories: two tests
/// sharing one temp tree that each clear it at both ends is a race, and they
/// run in parallel (IR3).
fn site(who: &str, config: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-themes-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("grackle.toml"), config).unwrap();
    for (rel, body) in files() {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

/// Every URL the build publishes — the set `grackle urls` prints.
fn published(dir: &Path) -> Vec<String> {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    let mut urls: Vec<String> = out.keys().cloned().collect();
    urls.sort();
    urls
}

/// The load error, for the sites that must not load at all.
fn load_error(dir: &Path) -> String {
    match grackle::config::Config::load(&dir.join("grackle.toml")) {
        Ok(_) => panic!("the config loaded, and it must not"),
        Err(e) => format!("{e:#}"),
    }
}

/// **The ruling.** A minimal site with no `exclude` at all does not publish its
/// theme sources — and the twin one directory over proves the walk would
/// otherwise, byte for byte.
///
/// This is the review I-B probe shape returned as a guard. Mutation: delete the
/// `under_themes` filter in `build_tree_and_objects` and `/themes/mine/root.html`
/// and `/themes/mine/theme.scss` join the published set — the theme's chrome
/// fragment served as a page, and its stylesheet source served beside the
/// compiled sheet the engine already publishes at `/css/mine.css`.
#[test]
fn a_site_root_themes_directory_is_not_content() {
    let dir = site("not-content", &format!("{CONFIG}{CATCH_ALL}"));
    let urls = published(&dir);

    assert!(
        !urls.iter().any(|u| u.starts_with("/themes/")),
        "theme sources published: {urls:?}"
    );
    // The control, in the same assertion set: the identical files one
    // directory over are content, so nothing about these bytes is what the
    // rule keys on.
    assert!(
        urls.contains(&"/pages/mine/root.html".to_string()),
        "{urls:?}"
    );
    assert!(
        urls.contains(&"/pages/mine/theme.scss".to_string()),
        "{urls:?}"
    );
    // And the theme itself still LOADED — this is a rule about the walk, not
    // about themes: `/css/mine.css` is the theme's compiled sheet.
    assert!(urls.contains(&"/css/mine.css".to_string()), "{urls:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **The escape hatch, and it is the one that already exists.** `include` has
/// first say over `exclude` (`NotContent::keeps`), so it has first say over the
/// positional rule too — a site that deliberately publishes what sits under
/// `themes/` says so in the key that already means that, and no new machinery
/// exists for it.
///
/// Mutation: drop the `not_content.included(&f.rel) ||` clause and this fails —
/// the hatch is bolted shut and a site has no spelling left.
#[test]
fn include_is_the_escape_hatch_over_the_position_rule() {
    let dir = site(
        "include",
        &format!("{CONFIG}include = [\"themes/**\"]\n{CATCH_ALL}"),
    );
    let urls = published(&dir);

    assert!(
        urls.contains(&"/themes/mine/root.html".to_string()),
        "{urls:?}"
    );
    assert!(
        urls.contains(&"/themes/mine/theme.scss".to_string()),
        "{urls:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **The dead keys.** `exclude`/`include` have exactly one reader — the TREE
/// collection's, compiled into the one `NotContent` all three walks share — so
/// a posts or objects collection writing them configures nothing. theme-preview
/// carried `exclude = ["themes/**"]` on its objects scope for as long as it has
/// existed and deleting it rebuilt the site byte-identical (IO.md I7a).
///
/// Mutation: delete the `check_scope_content_keys` call and both halves load
/// clean, which is the disease — a declared key doing nothing, forever.
#[test]
fn a_non_tree_scope_may_not_write_exclude_or_include() {
    let objects = site(
        "dead-key-objects",
        &format!(
            "{CONFIG}{CATCH_ALL}\n\
             [[collections]]\nname = \"objects\"\nkind = \"objects\"\n\
             exclude = [\"themes/**\"]\n\n  \
             [[collections.rules]]\n  match = \"**/*.png\"\n  route = \"/{{path}}\"\n"
        ),
    );
    let e = load_error(&objects);
    assert!(e.contains("\"objects\""), "{e}");
    assert!(e.contains("`exclude`"), "{e}");
    assert!(e.contains("kind = \"tree\""), "{e}");

    // The other key, the other kind — the check is about the pair, not about
    // one word on one collection.
    let posts = site(
        "dead-key-posts",
        &format!(
            "{CONFIG}{CATCH_ALL}\n\
             [[collections]]\nname = \"notes\"\nkind = \"posts\"\nsource = \"_posts\"\n\
             include = [\"themes/**\"]\n\n  \
             [[collections.rules]]\n  match = \"**\"\n  route = \"/notes/{{stem}}/\"\n"
        ),
    );
    let e = load_error(&posts);
    assert!(e.contains("\"notes\""), "{e}");
    assert!(e.contains("`include`"), "{e}");

    let _ = std::fs::remove_dir_all(&objects);
    let _ = std::fs::remove_dir_all(&posts);
}

/// The control the narrowing owes: the TREE collection's `exclude` is
/// untouched, and still decides what is content.
///
/// Mutation: widen `check_scope_content_keys` to every kind and this site stops
/// loading — which is what "tree only" has to mean to be worth saying.
#[test]
fn the_tree_collections_exclude_still_works() {
    let dir = site(
        "tree-exclude",
        &format!("{CONFIG}exclude = [\"pages/**\"]\n{CATCH_ALL}"),
    );
    let urls = published(&dir);

    assert!(
        !urls.iter().any(|u| u.starts_with("/pages/")),
        "the tree's own exclude stopped working: {urls:?}"
    );
    assert!(urls.contains(&"/index.html".to_string()), "{urls:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
