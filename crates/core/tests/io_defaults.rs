//! Schema-field defaults (§5b `default =`): the floor of the value ladder.
//!
//! A declared default fills a row's field only where every nearer writer —
//! front matter, markers, rules — left it unset. Proven end to end, through
//! the loader and into the rendered `<head>`: a unit test on
//! `apply_schema_defaults` cannot catch a `walk.rs` that never calls it, and
//! the whole point of a default is a value that arrives without the author.

use std::path::{Path, PathBuf};

// A `description` with a default, surfaced in the head so the rendered bytes
// witness which value won. `shell` is declared because a rule defaults it.
const HEAD: &str = "extends = \"none\"\n\
     [site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
     [schema]\n\
     shell = { type = \"string\" }\n\
     description = { type = \"string\", default = \"fallback blurb\" }\n\n\
     [html.head.meta]\ndescription = 'description'\n\n\
     [[collections]]\nsource = \".\"\n\n\
     [[collections.rules]]\n\
     match = \"**/*\"\nfront_matter = true\nroute = \"/{path}\"\n\
     defaults = { shell = \"html\" }\n";

fn site(who: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-defaults-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("grackle.toml"), HEAD).unwrap();
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

fn build(dir: &Path) -> std::collections::BTreeMap<String, String> {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle_core::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    out.into_iter()
        .map(|(u, b)| (u, String::from_utf8_lossy(&b).into_owned()))
        .collect()
}

/// A row that never mentions `description` takes the declared default, and it
/// reaches the head like any other value.
#[test]
fn a_default_fills_a_silent_row() {
    let dir = site(
        "silent",
        &[("quiet.md", "---\ntitle: Quiet\n---\n\nBody.\n")],
    );
    let out = build(&dir);
    let html = &out["/quiet.md"];
    assert!(
        html.contains(r#"<meta name="description" content="fallback blurb">"#),
        "the default reached the head:\n{html}"
    );
}

/// Front matter is nearer than the declaration, so it wins the key and the
/// default never fires.
#[test]
fn front_matter_beats_the_default() {
    let dir = site(
        "spoken",
        &[(
            "loud.md",
            "---\ntitle: Loud\ndescription: its own words\n---\n\nBody.\n",
        )],
    );
    let out = build(&dir);
    let html = &out["/loud.md"];
    assert!(
        html.contains(r#"<meta name="description" content="its own words">"#),
        "front matter won:\n{html}"
    );
    assert!(
        !html.contains("fallback blurb"),
        "the default did not also leak in:\n{html}"
    );
}
