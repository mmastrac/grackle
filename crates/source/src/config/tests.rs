// (Bare item ids, `I5`, `IR4`, `C6a`, `E2`, …, name entries in the
// retired IO.md / MERGE.md build ledgers; git history holds their text.)
use super::*;
use crate::shape::{Law, Shape};

fn cfg(views: &str) -> Config {
    let c = cfg_raw(views);
    c.check_pairing_axis().expect("i18n axis ok");
    c.validate().expect("test config should validate");
    c
}

fn cfg_raw(views: &str) -> Config {
    let mut c = cfg_unmerged(views);
    c.merge_queries()
        .expect("test config sections should merge");
    c
}

/// The text every helper here parses: one posts collection, no base.
fn cfg_source(views: &str) -> String {
    format!(
        "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"blog\"\nsource = \"_posts\"\n{views}"
    )
}

/// The same site as [`cfg`], PROJECTED through `profile`.
///
/// A projection is a config built through the ordinary entry point, the
/// overlay merges into the merged table, the deserializer sees the result,
/// and `validate` runs on it, so the test drives exactly what
/// `Config::load_profile` drives, minus the filesystem.
fn projected(views: &str, profile: &str) -> Result<Config> {
    let c = Config::from_toml_profile(&cfg_source(views), Some(profile))?;
    c.validate()?;
    Ok(c)
}

/// Parsed, with collections keyed, but queries not yet folded, which is
/// what the `merge_queries` checks below are about.
fn cfg_unmerged(views: &str) -> Config {
    let src = cfg_source(views);
    let mut c: Config = toml::from_str(&src).expect("test config should parse");
    c.merge_collections()
        .expect("test collections should resolve");
    c
}

/// The error `merge_queries` produces, as a full anyhow chain.
fn merge_err(views: &str) -> String {
    let mut c = cfg_unmerged(views);
    format!(
        "{:#}",
        c.merge_queries()
            .expect_err("sections should fail to merge")
    )
}

/// The load-time error a config produces, as a full anyhow chain.
fn cfg_err(views: &str) -> String {
    let c = cfg_raw(views);
    format!(
        "{:#}",
        c.validate().expect_err("config should fail validation")
    )
}

#[test]
fn chain_flattens_and_conjoins_filters() {
    let c = cfg(r#"
            [sets.published]
            from = "blog"
            where = "!draft && !hidden"

            [sets.latest]
            from = "published"
            where = "!noindex"
            limit = 3
        "#);
    let q = c.query("latest").unwrap();
    assert_eq!(q.base, ["blog"]);
    // Outermost last, and every link in the chain must hold.
    assert_eq!(q.predicate().unwrap(), "(!draft && !hidden) && (!noindex)");
}

/// `[schema.fields]` is read as a bag and unions into every view, so a
/// listing that declares no fields of its own still carries the row's hero.
#[test]
fn schema_fields_union_into_a_view() {
    let c = cfg("[sets.recent]\nfrom = \"blog\"\n\
         [schema.fields]\nhero = 'images(content)[0]'\n");
    assert!(c.schema_fields().contains_key("hero"));
    assert!(
        c.fields_for("recent").contains_key("hero"),
        "the bag reaches a view with no fields table"
    );
}

/// The vocabulary is open: a name the engine has never heard of is a
/// computed column like any other, typed by its expression.
#[test]
fn an_arbitrary_schema_field_is_accepted() {
    let c = cfg("[schema.fields]\nblurb = 'truncate_chars(content, 50)'\n");
    assert!(c.schema_fields().contains_key("blurb"));
}

/// The name is free, but the expression is still type-checked: a field whose
/// body does not type is a load error naming the table and the field.
#[test]
fn a_malformed_schema_field_is_a_load_error() {
    let e = cfg_err("[schema.fields]\nbogus = 'as_html(4)'\n");
    assert!(e.contains("[schema.fields]"), "{e}");
    assert!(e.contains("bogus"), "{e}");
}

/// A computed field wants an expression; a literal is a load error.
#[test]
fn a_schema_field_literal_is_a_load_error() {
    let e = cfg_err("[schema.fields]\nhero = 42\n");
    assert!(e.contains("must be an expression"), "{e}");
}

#[test]
fn single_filter_is_not_parenthesised() {
    let c = cfg("[sets.published]\nfrom = \"blog\"\nwhere = \"!draft\"\n");
    assert_eq!(c.query("published").unwrap().predicate().unwrap(), "!draft");
}

#[test]
fn unfiltered_chain_has_no_predicate() {
    let c = cfg("[sets.all]\nfrom = \"blog\"\n");
    assert!(c.query("all").unwrap().predicate().is_none());
}

/// The rule that keeps composition from needing inheritance semantics.
#[test]
fn composing_over_a_materialized_view_is_an_error() {
    let c = cfg(r#"
            [routes.blog_index]
            from = "blog"
            where = "!draft"
            paginate = 5
            paths = ["/blog/"]

            [sets.latest]
            from = "blog_index"
            limit = 3
        "#);
    let e = c.query("latest").unwrap_err().to_string();
    assert!(
        e.contains("neither a set nor a grouped route"),
        "unexpected error: {e}"
    );
}

/// Subdivision: a grouped view may compose over a grouped view,
/// the filters flatten straight through it.
#[test]
fn grouped_over_grouped_is_subdivision() {
    let c = cfg(r#"
            [routes.yearly]
            from = "blog"
            where = "!draft"
            group_by = "date.year"
            path = "/blog/{year}/"

            [routes.monthly]
            from = "yearly"
            group_by = "date.month"
            path = "/blog/{year}/{month:02}/"
        "#);
    let q = c.query("monthly").unwrap();
    assert_eq!(q.base, ["blog"]);
    assert_eq!(q.predicate().unwrap(), "!draft");
}

/// Only subdivision is defined: a non-grouped view over a grouped one
/// has no meaning (yet), and pagination × subdivision is punted.
#[test]
fn non_grouped_over_grouped_is_an_error() {
    let c = cfg(r#"
            [routes.yearly]
            from = "blog"
            group_by = "date.year"
            path = "/blog/{year}/"

            [sets.latest]
            from = "yearly"
            limit = 3
        "#);
    let e = c.query("latest").unwrap_err().to_string();
    assert!(e.contains("subdividing"), "unexpected error: {e}");
}

#[test]
fn subdividing_a_paginated_view_is_punted() {
    let c = cfg(r#"
            [routes.yearly]
            from = "blog"
            group_by = "date.year"
            paginate = 10
            path = "/blog/{year}/"

            [routes.monthly]
            from = "yearly"
            group_by = "date.month"
            path = "/blog/{year}/{month:02}/"
        "#);
    let e = c.query("monthly").unwrap_err().to_string();
    assert!(e.contains("punted"), "unexpected error: {e}");
}

/// Computed fields flow with rows through composition: declared
/// once on a shared query view, visible to everything over it; nearest
/// redeclaration wins.
#[test]
fn fields_inherit_along_over_nearest_wins() {
    let c = cfg(r#"
            [sets.published]
            from = "blog"
            [sets.published.fields]
            summary = 'truncate_blocks(content, 4)'

            [routes.blog_index]
            from = "published"
            paginate = 5
            paths = ["/blog/"]

            [routes.tag_index]
            from = "published"
            group_by = "tags"
            path = "/blog/tags/{key}/"
            [routes.tag_index.fields]
            summary = 'truncate_blocks(content, 1)'
        "#);
    let inherited = c.fields_for("blog_index");
    assert_eq!(
        inherited["summary"].as_expr(),
        Some("truncate_blocks(content, 4)")
    );
    let overridden = c.fields_for("tag_index");
    assert_eq!(
        overridden["summary"].as_expr(),
        Some("truncate_blocks(content, 1)")
    );
}

/// The directory names the table, so it is written once.
#[test]
fn a_collection_takes_its_name_from_its_source_directory() {
    let c = Config::from_toml(
        "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource = \"_posts\"\n\
             [[collections]]\nsource = \"recipes\"\n",
    )
    .unwrap();
    let names: Vec<&str> = c.collections.keys().map(String::as_str).collect();
    assert_eq!(names, vec!["posts", "recipes"]);
}

/// A rootward source has no directory to name it.
#[test]
fn a_root_collection_is_named_entries() {
    let c = Config::from_toml(
        "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource = \".\"\n",
    )
    .unwrap();
    assert!(
        c.collections.contains_key("entries"),
        "{:?}",
        c.collections.keys()
    );
}

#[test]
fn an_explicit_name_overrides_the_directory() {
    let c = Config::from_toml(
        "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname = \"notes\"\nsource = \"_posts\"\n",
    )
    .unwrap();
    assert!(c.collections.contains_key("notes"));
}

/// Objects are matched by their rules' globs, so no directory names them.
#[test]
fn a_sourceless_collection_must_be_named() {
    let e = Config::from_toml(
        "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\n",
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("give it a `name`"), "{e}");
}

#[test]
fn two_collections_may_not_resolve_to_one_name() {
    let e = Config::from_toml(
        "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource = \"_posts\"\n\
             [[collections]]\nsource = \"posts\"\n",
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("resolve to the name"), "{e}");
}

/// Layout names are open: themes invent faces by shipping `row--*`.
/// Config only records the name; missing faces bail at render time.
#[test]
fn an_unknown_layout_name_is_accepted_at_config_time() {
    let c = cfg("[routes.x]\npath = \"/x/\"\nfrom = \"blog\"\nlayout = \"tag_index\"\n");
    assert_eq!(c.views["x"].layout.as_deref(), Some("tag_index"));
}

/// A set's `theme` can never apply, a set never
/// lands, and an embedded set is content in the HOST's document, which
/// wears one stylesheet. Declared-and-ignored, so it is a load error.
///
/// The controls are the point of the item: a route's theme is the shape
/// this key exists for (its name is checked against the registry once the
/// themes are loaded, C2), and `layout`/`variant` on a set are LIVE, since
/// `{% view %}` dispatches on the one and renders through the other.
#[test]
fn a_set_may_not_declare_a_theme() {
    let e = cfg_err(
        "[sets.latest]\nfrom = \"blog\"\nlimit = 3\n\
             layout = \"link\"\ntheme = \"loud\"\n",
    );
    assert!(e.contains("[sets.latest] declares a theme"), "{e}");
    assert!(e.contains("never lands"), "{e}");

    let c = cfg("[routes.blog_index]\npath = \"/blog/\"\nfrom = \"blog\"\n\
             layout = \"card\"\ntheme = \"loud\"\n\
             [sets.latest]\nfrom = \"blog\"\nlimit = 3\n\
             layout = \"link\"\nvariant = \"compact\"\n");
    assert_eq!(c.views["blog_index"].theme.as_deref(), Some("loud"));
    assert_eq!(c.views["latest"].layout.as_deref(), Some("link"));
    assert_eq!(c.views["latest"].variant.as_deref(), Some("compact"));
}

/// The same family one field over: a set may not wear a
/// fold shell, because a fold lands at a route.
///
/// Verified before the check was written: there is no routeless-fold
/// shape. All four fold passes in `build.rs` (atom, sitemap, search, the
/// script shells) iterate `db.routes` and reach a view through the route
/// carrying it, and a routeless view only ever reaches `db.views` via
/// `insert_routeless`, which `{% view %}` embedding reads by layout and
/// variant, no reader of `shell` at all. So the two live outcomes today
/// are both bad and neither says why: `from`-less, it reaches
/// `build_pool_folds` and dies mid-build with "view x needs a route"; with
/// a `from`, it goes quietly through `insert_routeless` and publishes
/// nothing.
///
/// Mutation: delete the `declared_set`/fold check in `validate` and the
/// first case validates clean (then dies late), the second validates and
/// publishes nothing.
#[test]
fn a_set_may_not_wear_a_fold_shell() {
    for src in [
        "[sets.everything]\nshell = \"sitemap\"\n",
        "[sets.everything]\nfrom = \"blog\"\nshell = \"atom\"\n",
        "[shells.llms]\ncommand = \"cat\"\n\
             [sets.everything]\nfrom = \"blog\"\nshell = \"llms\"\n",
    ] {
        let e = cfg_err(src);
        assert!(e.contains("[sets.everything] wears shell ="), "{e}");
        assert!(e.contains("a set never lands"), "{e}");
        assert!(e.contains("[routes.everything]"), "{e}");
    }
    // The controls. A routed fold is the shape the key exists for, and a
    // set with no shell is still an ordinary query.
    let c = cfg(
        "[routes.everything]\npath = \"/sitemap.xml\"\nshell = \"sitemap\"\n\
             [sets.latest]\nfrom = \"blog\"\nlimit = 3\n",
    );
    assert_eq!(c.views["everything"].shell.as_deref(), Some("sitemap"));
    assert!(c.views["latest"].shell.is_none());
    // And a MAP shell on a set is still the ARITY mistake `check_view`
    // owns, this check does not steal that sentence.
    let e = cfg_err("[sets.latest]\nfrom = \"blog\"\nshell = \"html\"\n");
    assert!(e.contains("is a map shell"), "{e}");
}

/// noindex is editorial and schema-declared (base.toml `[schema]`). An
/// undeclared listing is indexed; `noindex = true` on a route stamps the
/// field the head expression reads.
#[test]
fn noindex_is_a_view_declaration_defaulting_to_indexed() {
    let head =
        "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
                    [schema]\nnoindex = { type = \"bool\" }\n\
                    [[collections]]\nsource = \"_posts\"\n\
                    file = [\"{slug}\"]\n";
    let c = Config::from_toml(&format!(
        "{head}[routes.blog_index]\npath = \"/blog/\"\nfrom = \"posts\"\nlayout = \"card\"\n\
             [routes.tag_index]\npath = \"/t/\"\nfrom = \"posts\"\nlayout = \"card\"\n\
             noindex = true\n"
    ))
    .unwrap();
    c.validate().unwrap();
    assert!(!c.views["blog_index"].route_fields.contains_key("noindex"));
    assert_eq!(
        c.views["tag_index"].route_fields.get("noindex"),
        Some(&toml::Value::Boolean(true))
    );
}

#[test]
fn a_route_noindex_without_schema_is_a_load_error() {
    let e = cfg_err(
        "[routes.tag_index]\npath = \"/t/\"\nfrom = \"blog\"\nlayout = \"card\"\n\
             noindex = true\n",
    );
    assert!(e.contains("unknown field `noindex`"), "{e}");
    assert!(e.contains("schema fields"), "{e}");
}

/// The flag family reaches the page schema, not just posts,
/// `draft: true` on a page was once read, dropped, and published.
///
/// moved the flags out of `row_schema()` and into declared schema, so
/// the vocabulary the filter type-checks against is the SITE's now. That
/// the assertion still reads the same way is the point: nothing about
/// what a page can be asked changed, only who says so.
#[test]
fn the_flag_family_is_queryable_on_pages() {
    let c = cfg("[sets.pages]\nfrom = \"blog\"\nwhere = \"!draft && !hidden && !noindex\"\n");
    let q = c.query("pages").unwrap();
    let mut schema = grackle_model::row_schema();
    for f in ["draft", "hidden", "noindex"] {
        schema.insert(f, grackle_db::filter::Type::Bool);
    }
    // Type-checking the filter IS the assertion.
    grackle_db::filter::Filter::parse(&q.predicate().unwrap(), &schema)
        .expect("!draft && !hidden should type-check against a page");
}


/// The site's rules go first, which is the whole mechanism:
/// first-writer-wins then hands the route to the site and lets the base's
/// catch-all fill whatever is left. Mutation-checked by reversing the
/// concatenation, which puts the base's `/blog/...` route first.
#[test]
fn a_sites_rules_prepend_to_the_inherited_ones() {
    let c = Config::from_toml(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource=\"_posts\"\n\
             [[collections.rules]]\nmatch=\"**\"\nroute=\"/writing/{slug}/\"\n",
    )
    .unwrap();
    let rules = &c.collections["posts"].rules;
    assert_eq!(rules[0].route, vec!["/writing/{slug}/"]);
    assert_eq!(
        rules.len(),
        2,
        "the base's catch-all should still be there, below"
    );
    // Not restated, so it comes from the base.
    assert_eq!(
        c.collections["posts"].file,
        vec!["{date.year}-{date.month}-{date.day}-{slug}".to_string()]
    );
}

/// Collections are matched by SOURCE, not by name, a site renaming its
/// posts collection is still talking about `_posts/`, and two collections
/// over one directory would read every post twice.
#[test]
fn a_renamed_collection_replaces_the_inherited_one_over_the_same_source() {
    let c = Config::from_toml(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"notes\"\nsource=\"_posts\"\n",
    )
    .unwrap();
    assert!(c.collections.contains_key("notes"));
    assert!(
        !c.collections.contains_key("posts"),
        "`_posts` would be read twice: {:?}",
        c.collections.keys()
    );
}

/// A registry entry is the unit: your `[routes.feed]` replaces the base's
/// whole, so you never have to know what the base put in one.
#[test]
fn a_named_route_shadows_the_inherited_one_entire() {
    let c = Config::from_toml(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [routes.feed]\npath=\"/feed.xml\"\nfrom=\"published\"\nshell=\"atom\"\n",
    )
    .unwrap();
    let feed = &c.views["feed"];
    assert_eq!(feed.route.as_deref(), Some("/feed.xml"));
    assert_eq!(feed.limit, None, "the base's limit = 20 must not leak in");
    // Untouched neighbours survive.
    assert!(c.views.contains_key("blog_index"));
}

/// `[site]` is a settings bag, not a registry: you set the two keys you
/// care about and keep the rest.
#[test]
fn site_keys_merge_one_at_a_time() {
    let c = Config::from_toml("[site]\ntitle = \"Mine\"\n").unwrap();
    assert_eq!(c.site.title, "Mine");
    assert_eq!(c.site.url, "http://localhost:8080", "inherited");
}

/// The law dispatch with a base of the test's own. `base.toml` declares
/// no `[axes]` and no `[links]`, so `Config::from_toml` cannot reach the
/// arms below, a key the base never wrote is the site's whole under
/// every law. This is the same `merge_table` the merge runs, so the
/// law read here is the law that ships.
fn merged(base: &str, site: &str) -> toml::Table {
    let b = toml::from_str(base).expect("test base should parse");
    let s = toml::from_str(site).expect("test site should parse");
    match merge_table(b, s, &Config::shape(), &mut Vec::new(), &mut Trace::off()) {
        toml::Value::Table(t) => t,
        v => panic!("merging two tables should give a table: {v:?}"),
    }
}

/// A registry, not an atom: declaring an axis of your own must not take
/// the inherited ones down with it. This is the bug Law 2 was derived
/// from, `[axes]` fell through to wholesale replace.
#[test]
fn a_base_declared_axis_survives_a_site_declaring_a_different_one() {
    let m = merged(
        "[axes.theme]\nvalues = [\"ledger\", \"atlas\"]\nfield = \"theme\"\n",
        "[axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n",
    );
    let axes = m["axes"].as_table().expect("axes is a table");
    assert!(
        axes.contains_key("theme"),
        "the inherited axis was swept away: {axes:?}"
    );
    assert!(axes.contains_key("locale"), "{axes:?}");
}

/// And the other half of the registry law: a definition is the unit, so
/// redeclaring one replaces it entire. `values` and `field` are one
/// thought, an axis assembled half from each side is nobody's axis.
#[test]
fn a_redeclared_axis_shadows_the_inherited_one_entire() {
    let m = merged(
        "[axes.theme]\nvalues = [\"ledger\", \"atlas\"]\nfield = \"theme\"\n",
        "[axes.theme]\nvalues = [\"ledger\"]\n",
    );
    let theme = m["axes"]["theme"].as_table().expect("the axis is a table");
    assert_eq!(
        theme["values"].as_array().map(Vec::len),
        Some(1),
        "the site's values: {theme:?}"
    );
    assert!(
        !theme.contains_key("field"),
        "the base's `field` leaked into the site's axis: {theme:?}"
    );
}

/// A marker is a definition under its filename, so redeclaring one says
/// what it means WHOLE, the reason `MarkerDef`
/// exists. The base declares `".noindex" = { noindex = true }`; a site
/// that repurposes the name gets its own payload and nothing else.
///
/// This is the live path, not a stand-in: `base.toml` really does declare
/// the three markers, so `from_toml` reaches the arm.
#[test]
fn a_redeclared_marker_replaces_the_payload_whole() {
    let c = Config::from_toml(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [markers]\n\".noindex\" = { hidden = true }\n",
    )
    .unwrap();
    let payload = &c.markers[".noindex"].0;
    let keys: Vec<&str> = payload.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["hidden"],
        "the base's `noindex = true` composed itself into the site's \
             marker: {payload:?}"
    );
    // The markers the site left alone are untouched, a definition is the
    // unit, and `[markers]` itself is the namespace.
    assert!(c.markers.contains_key(".draft"), "{:?}", c.markers.keys());
}

/// `[links]` is a bag like `[site]`: setting one key keeps the others.
/// `merge_to_depth` is what runs; `reach` stands in for a key not added yet.
#[test]
fn links_keys_merge_one_at_a_time() {
    let m = merged(
        "[links]\npolicy = \"loose\"\nreach = \"site\"\n",
        "[links]\npolicy = \"strict\"\n",
    );
    let links = m["links"].as_table().expect("links is a table");
    assert_eq!(
        links.get("policy").and_then(toml::Value::as_str),
        Some("strict"),
        "the nearer writer wins the key: {links:?}"
    );
    assert_eq!(
        links.get("reach").and_then(toml::Value::as_str),
        Some("site"),
        "a key the site never wrote was dropped: {links:?}"
    );
}

/// Which views the SITE declared, recorded before the merge blurs it,
/// the flag that keeps an inherited route from minting an empty URL.
#[test]
fn declared_views_are_told_apart_from_inherited_ones() {
    let c = Config::from_toml(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [routes.feed]\npath=\"/feed.xml\"\nfrom=\"published\"\nshell=\"atom\"\n",
    )
    .unwrap();
    assert!(!c.views["feed"].inherited, "the site wrote this one");
    assert!(c.views["blog_index"].inherited);
}

#[test]
fn an_unknown_extends_names_the_two_that_exist() {
    let e =
        Config::from_toml("extends = \"ledger\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n")
            .unwrap_err()
            .to_string();
    assert!(e.contains("\"default\"") && e.contains("\"none\""), "{e}");
}

#[test]
fn brace_alternatives_expand_in_order() {
    assert_eq!(
        brace_alternatives("index.{md,html}"),
        ["index.md", "index.html"]
    );
    assert_eq!(brace_alternatives("index.md"), ["index.md"]);
}

/// A view may not both demand a row and offer to take one.
#[test]
fn content_and_default_content_are_exclusive() {
    let e = Config::from_toml(
        "extends=\"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource=\".\"\n\
             [routes.r]\npath=\"/r/\"\nfrom=\"entries\"\ncontent=\"a.md\"\n\
             default_content=\"b.md\"\n",
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("Pick which"), "{e}");
}

/// A `[site]`-only config over an empty tree must name the missing collections.
#[test]
fn a_config_with_no_collections_says_so() {
    let src = "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n";
    let c = Config::from_toml(src).unwrap();
    let e = c.validate().unwrap_err().to_string();
    assert!(e.contains("no collections declared"), "{e}");
    assert!(
        e.contains("[[collections]]"),
        "the error should show the shape: {e}"
    );
}

/// A path scope conjoins along the chain: a child narrows within its
/// parent's subtree and cannot escape it. The glob is a clause of `where`,
/// so the filter conjunction carries it, and the assertion is on the source
/// the type-checker actually sees.
#[test]
fn a_path_scope_conjoins_along_the_chain() {
    let c = cfg(
        "[sets.recipes]\nfrom = \"blog\"\nwhere = 'glob(path, \"recipes/**\")'\n\
             [sets.desserts]\nfrom = \"recipes\"\n\
             where = 'glob(path, \"**/sweet/**\") && !draft'\n",
    );
    assert_eq!(
        c.query("desserts").unwrap().predicate().unwrap(),
        "(glob(path, \"recipes/**\")) && (glob(path, \"**/sweet/**\") && !draft)"
    );
    // The parent keeps only its own.
    assert_eq!(
        c.query("recipes").unwrap().predicate().unwrap(),
        "glob(path, \"recipes/**\")"
    );
}

/// `order_by` is nearest-wins, re-sorting a parent's rows is ordinary.
#[test]
fn order_by_inherits_nearest_wins() {
    let c = cfg("[sets.books]\nfrom = \"blog\"\norder_by = \"-month\"\n\
             [sets.by_title]\nfrom = \"books\"\norder_by = \"title\"\n\
             [sets.newest]\nfrom = \"books\"\nlimit = 1\n");
    assert_eq!(
        c.query("by_title").unwrap().order_by.as_deref(),
        Some("title")
    );
    // Undeclared: inherited from the parent rather than lost.
    assert_eq!(
        c.query("newest").unwrap().order_by.as_deref(),
        Some("-month")
    );
}

#[test]
fn a_set_may_not_declare_a_path() {
    let e = merge_err("[sets.s]\nfrom = \"blog\"\npath = \"/s/\"\n");
    assert!(e.contains("[routes.s]"), "{e}");
}

#[test]
fn a_route_must_declare_a_path() {
    let e = merge_err("[routes.r]\nfrom = \"blog\"\n");
    assert!(e.contains("[sets.r]"), "{e}");
}

/// One namespace: `from` resolves against collections, sets and routes
/// alike, so a shared name is a conflict, not a silent preference.
#[test]
fn a_name_may_not_be_both_a_collection_and_a_query() {
    let e = merge_err("[sets.blog]\nfrom = \"blog\"\n");
    assert!(e.contains("one namespace"), "{e}");
}

#[test]
fn a_name_may_not_be_both_a_set_and_a_route() {
    let e = merge_err("[sets.x]\nfrom = \"blog\"\n[routes.x]\nfrom = \"blog\"\npath = \"/x/\"\n");
    assert!(e.contains("both a set and a route"), "{e}");
}

// ---------------------------------------------------------------- trail
//
// `cfg_unmerged` splices its argument straight after the
// collection's `source`, so a `trail = …` line lands on the collection
// and the `[routes]` after it close the table, which is exactly the
// shape these need.

/// The control, and the shape grack.com really has: a month archive
/// composed `over` a year archive, both routed and both labelled.
#[test]
fn a_grouped_routed_trail_validates() {
    cfg(TRAIL_CHAIN);
}

/// The typo. Also a fixture (`trail-unknown-view`), which is what pins
/// that the SITE fails rather than that the function does.
#[test]
fn a_trail_naming_no_view_is_a_load_error() {
    let e = cfg_err(&TRAIL_CHAIN.replace("monthly_archive\"\n", "montly_archive\"\n"));
    assert!(e.contains("is not a declared view"), "{e}");
    assert!(e.contains("monthly_archive"), "the knowns are listed: {e}");
}

/// A trail is a SUBDIVISION chain, `post_trail` renders each grouped
/// view along the `from` chain from the row's own group keys. A view
/// that groups by nothing, over nothing that groups, is a chain of
/// nothing, and produced a silently empty trail.
#[test]
fn a_trail_over_nothing_grouped_is_a_load_error() {
    let e = cfg_err(
        "trail = \"flat\"\n\
             [routes.flat]\npath = \"/flat/\"\nfrom = \"blog\"\nlayout = \"card\"\ntitle = \"F\"\n",
    );
    assert!(e.contains("subdivision chain"), "{e}");
    assert!(e.contains("Grouped views: "), "the knowns are listed: {e}");
}

/// A level with no `path` has no URL to hang its crumb on, so
/// `post_trail` skips it and the trail comes out with a hole in the
/// middle, Home > December > 16, the year gone.
#[test]
fn a_trail_level_that_lands_nowhere_is_a_load_error() {
    let e = cfg_err(
        &TRAIL_CHAIN
            .replace("[routes.yearly_archive]", "[sets.yearly_archive]")
            .replace("path = \"/blog/{year}/\"\n", ""),
    );
    assert!(e.contains("lands at no single `path`"), "{e}");
    assert!(e.contains("yearly_archive > monthly_archive"), "{e}");
}

/// Same hole, other cause: nothing to write in the crumb.
#[test]
fn a_trail_level_with_no_label_is_a_load_error() {
    let e = cfg_err(&TRAIL_CHAIN.replace("title = \"{year}\"\n", ""));
    assert!(e.contains("neither `crumb` nor `title`"), "{e}");
}

const TRAIL_CHAIN: &str = "trail = \"monthly_archive\"\n\
         [i18n.tables.months]\n\
         1 = \"January\"\n\
         [routes.yearly_archive]\n\
         path = \"/blog/{year}/\"\n\
         from = \"blog\"\n\
         group_by = \"date.year\"\n\
         layout = \"card\"\n\
         title = \"{year}\"\n\
         [routes.monthly_archive]\n\
         path = \"/blog/{year}/{month:02}/\"\n\
         from = \"yearly_archive\"\n\
         group_by = \"date.month\"\n\
         layout = \"card\"\n\
         crumb = \"@months[{month}]\"\n";

/// The field names serde accepts for `T`, read out of its own
/// `deny_unknown_fields` complaint, renames applied, skipped fields
/// absent. This is the list the merge actually keys on.
///
/// Two shapes to read: "expected one of `a`, `b`" for a struct with
/// several fields, and plain "expected `head`" for one with a single
/// field (`HtmlCfg`, `LinksCfg`). Splitting on the shorter prefix takes
/// both, and the invented key sits before it either way.
fn serde_keys<T: serde::de::DeserializeOwned>() -> Vec<String> {
    let e = toml::from_str::<T>("no_such_key = 1")
        .err()
        .expect("deny_unknown_fields should reject an invented key")
        .to_string();
    let listed = e
        .split_once("expected ")
        .expect("the error names the fields it knows")
        .1
        .lines()
        .next()
        .expect("the list is on one line");
    let mut keys: Vec<String> = listed
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    keys.sort();
    assert!(!keys.is_empty(), "no fields parsed out of: {e}");
    keys
}

/// The other half of the completeness check, struct by struct.
/// `every_config_key_has_a_law` pins the FIELDS at compile time; this pins
/// their TOML SPELLINGS, which is what the merge dispatches on, a renamed
/// or skipped field would otherwise leave a key no shape claims, silently,
/// and `law_of` would hand it back whole.
///
/// (A2 wrote this against the law table; B2 points it at the description,
/// which is now the only place a key can be named.)
///
/// The description is in TOML's name space, so `collections` (renamed from
/// `declared_collections`) must appear and `noindex`, `dir`, `views`,
/// `#[serde(skip)]` every one, must not. Only the structs the merge
/// DESCENDS are listed: a definition's fields are nobody's business
/// (see [`Shape::definition`]).
#[test]
fn the_shape_covers_the_config_surface() {
    for (what, shape, serde) in [
        ("Config", Config::shape(), serde_keys::<Config>()),
        (
            "Collection",
            Collection::shape(),
            serde_keys::<Collection>(),
        ),
        ("Site", Site::shape(), serde_keys::<Site>()),
        ("HtmlCfg", HtmlCfg::shape(), serde_keys::<HtmlCfg>()),
        ("HeadCfg", HeadCfg::shape(), serde_keys::<HeadCfg>()),
        ("AttrCfg", AttrCfg::shape(), serde_keys::<AttrCfg>()),
        ("I18nCfg", I18nCfg::shape(), serde_keys::<I18nCfg>()),
        ("LinksCfg", LinksCfg::shape(), serde_keys::<LinksCfg>()),
    ] {
        let mut named: Vec<String> = shape.fields().iter().map(|(k, _)| k.to_string()).collect();
        named.sort();
        assert_eq!(named, serde, "{what}'s shape and its serde keys drifted");
    }
}

/// annotation is the one thing here that is not derived, and there
/// are exactly two of it. B1 shipped a `KNOWN_EXCEPTIONS` list beside the
/// hand tables, one entry, `[markers]`, which Settled and
/// `MarkerDef` retired, and this is what replaces it now that the tables
/// are gone: with the law read off the shape, the only way to write a law
/// by hand is `annotated(…)`, so counting those IS counting the
/// exceptions.
///
/// A third one means someone decided a key does not merge the way its
/// type says. That deserves a entry and probably a question, not a
/// quiet line in a field list, and this fails until it gets one.
#[test]
fn only_the_annotated_keys_have_a_hand_written_law() {
    let hand_written = |shape: &Shape| -> Vec<(String, Law)> {
        shape
            .fields()
            .iter()
            .filter(|(_, s)| matches!(s, Shape::Annotated(..)))
            .map(|(k, s)| (k.to_string(), s.law()))
            .collect()
    };
    assert_eq!(
        hand_written(&Config::shape()),
        [("collections".to_string(), Law::Collections)],
        "a config key merges by a hand-written law"
    );
    assert_eq!(
        hand_written(&Collection::shape()),
        [("rules".to_string(), Law::Prepend)],
        "a collection key merges by a hand-written law"
    );
}

/// The depths table A calls out, each traced back to the type it falls
/// out of. These are the rows the table describes as "falls out", this
/// is where that stops being a claim.
#[test]
fn table_as_depths_fall_out_of_the_types() {
    // `law_of` is the merge's own lookup, not a test-side restatement:
    // this reads the laws that ship.
    let law = |key: &str| law_of(&Config::shape(), key);
    let collection_law = |key: &str| law_of(&Collection::shape(), key);
    // `[site]`: a struct under an engine-chosen name, all scalars.
    assert_eq!(law("site"), Law::Descend(1));
    // `[axes.*]`: a map whose value is a definition, `Axis` is a struct
    // under the axis's own name, so the descent stops above it. A3 fixed
    // this by hand; here it is a consequence of `BTreeMap<String, Axis>`.
    assert_eq!(law("axes"), Law::Descend(1));
    // `[links]`: `LinksCfg` is a struct under an ENGINE-chosen name, so
    // it descends per field however many fields it grows. A3 could only
    // state this with a hypothetical second key; now the type states it.
    assert_eq!(law("links"), Law::Descend(1));
    // `[schema]`: `toml::Table`, a map of values the merge does not type.
    assert_eq!(law("schema"), Law::Descend(1));
    // `[records.<field>.<id>]`: map -> map -> `RecordCfg`, a definition.
    assert_eq!(law("records"), Law::Descend(2));
    // `[i18n]`: the bag, then `names`/`strings` by key. `axis` is a scalar
    // beside the maps and is unharmed, no descent can split a string.
    // (`default` is serde-skipped; synced from the pairing axis at load.)
    assert_eq!(law("i18n"), Law::Descend(2));
    // `[html]`: head tables and element attribute maps. Descend(3) reaches
    // the expression (head.meta.robots, html.attribute.lang, …).
    assert_eq!(law("html"), Law::Descend(3));
    // `[markers.<filename>]`: a map whose value is a `MarkerDef`, a
    // definition under the marker's own filename, so what a marker MEANS
    // is taken whole. Unwrap that newtype back to a bare table
    // and this is the assertion that fails.
    assert_eq!(law("markers"), Law::Descend(1));
    // Arrays and scalars are atoms whatever they hold.
    assert_eq!(law("extends"), Law::Atom);
    // And the annotation is the annotation: structurally `[[collections]]`
    // is an array, and nothing but exception tells collections apart
    // from a plain atom array.
    assert_eq!(law("collections"), Law::Collections);
    assert_eq!(collection_law("rules"), Law::Prepend);
    assert_eq!(
        collection_law("relations"),
        Law::Descend(1),
        "a named relation is a definition"
    );
}

/// One described field: its TOML name, the depth of its own shape, and
/// whether that shape is an atom a descent would SPLIT (`Shape::TableAtom`
/// whether that shape is an atom a descent would SPLIT (`Shape::TableAtom`
type Field = (&'static str, usize, bool);

/// Every struct in `shape`, with whether it sits under an ENGINE-chosen
/// name (a field) or a user-chosen one (a map value).
fn each_struct(shape: &Shape, engine_named: bool, seen: &mut Vec<(Vec<Field>, bool)>) {
    match shape {
        Shape::Atom | Shape::TableAtom => {}
        // The annotation overrides the law, not the description: walk
        // what it wraps, so an annotated field is held to the same
        // invariants as any other.
        Shape::Annotated(_, inner) => each_struct(inner, engine_named, seen),
        Shape::Struct(fields) => {
            seen.push((
                fields
                    .iter()
                    .map(|(k, s)| (*k, s.depth(), s.is_table_atom()))
                    .collect(),
                engine_named,
            ));
            for (_, s) in fields {
                each_struct(s, true, seen);
            }
        }
        Shape::Map(value) => each_struct(value, false, seen),
    }
}

fn config_structs() -> Vec<(Vec<Field>, bool)> {
    let mut seen = Vec::new();
    each_struct(&Config::shape(), true, &mut seen);
    each_struct(&Collection::shape(), true, &mut seen);
    seen
}

/// [`Shape::definition`] leaves a definition's fields undescribed because
/// nothing descends into one. That holds only while every undescribed
/// struct sits under a user-chosen name: a `View`-shaped field of `Site`
/// would be a namespace whose fields this file claims not to have, and
/// would merge as if it had none.
#[test]
fn a_definition_never_sits_under_an_engine_name() {
    for (fields, engine_named) in config_structs() {
        assert!(
            !engine_named || !fields.is_empty(),
            "an undescribed struct sits under an engine-chosen name: {fields:?}"
        );
    }
}

/// The depth invariant, as a function of the shapes rather than as a body
/// of assertions: the only way to mutation-check a tripwire whose whole
/// point is that nothing in the config trips it
/// (`a_localized_string_beside_a_map_would_be_split` fires it at a shape
/// that does).
///
/// A field at the table's deepest level is the one `Descend(n)` was
/// measured from. Anything shallower is descended PAST, which is safe
/// exactly while `merge_to_depth` would then be handed a non-table and
/// hand it back whole, so a scalar or an array at depth 0 is fine, and a
/// table-spelled atom at depth 0 is the case that would be merged key by
/// key by a descent that was measured for its sibling.
fn an_atom_a_deeper_sibling_would_split(structs: &[(Vec<Field>, bool)]) -> Option<String> {
    for (fields, _) in structs {
        let deepest = fields.iter().map(|(_, d, _)| *d).max().unwrap_or(0);
        for (name, depth, table_atom) in fields {
            if *depth == deepest {
                continue;
            }
            if *table_atom {
                return Some(format!(
                    "`{name}` is an atom spelled as a TABLE at depth {depth}, beside a \
                         field at {deepest}: `Descend({deepest})` would merge \
                         into it — the atom is the whole value"
                ));
            }
            if *depth != 0 {
                return Some(format!(
                    "`{name}` sits at depth {depth} beside a field at {deepest}: \
                         one `Descend(n)` cannot serve both"
                ));
            }
        }
    }
    None
}

/// Why one `Descend(n)` can govern a whole table: see
/// [`an_atom_a_deeper_sibling_would_split`], which is this invariant.
/// `[i18n]`'s `LocalizedStr`s are at the bottom of the deepest path, not
/// beside it, so the config has none, and this says so for the next field
/// anyone adds.
#[test]
fn a_nested_struct_ends_at_one_depth() {
    let mut nested = Vec::new();
    for (_, s) in Config::shape().fields() {
        each_struct(s, true, &mut nested);
    }
    for (_, s) in Collection::shape().fields() {
        each_struct(s, true, &mut nested);
    }
    assert_eq!(an_atom_a_deeper_sibling_would_split(&nested), None);
}

/// The tripwire, fired, the mutation check for a guard that nothing in
/// the config can trip today (batch review 2, finding 1).
///
/// `[i18n]` is the table most likely to grow the field: a `LocalizedStr`
/// beside `strings`, a site-wide `title`, say, reads as depth 0 under a
/// `Descend(2)`, passes `a_definition_never_sits_under_an_engine_name`
/// (it is not a struct) and `the_shape_covers_the_config_surface` (serde
/// knows the key), and would be composed out of two writers by the merge.
#[test]
fn a_localized_string_beside_a_map_would_be_split() {
    let i18n_with_a_title = Shape::Struct(vec![
        // The three that are there today: a scalar at depth 0 is
        // descended past harmlessly, which is why the whitelist existed.
        ("default", Shape::Atom),
        ("locales", Shape::Atom),
        ("names", Shape::Map(Box::new(Shape::Atom))),
        // The hypothetical field. Not added to `I18nCfg`, the point is
        // that it never has to be for the guard to speak.
        ("title", LocalizedStr::shape()),
        ("strings", Shape::Map(Box::new(LocalizedStr::shape()))),
    ]);
    assert_eq!(
        i18n_with_a_title.law(),
        Law::Descend(2),
        "the sibling's law"
    );

    let mut nested = Vec::new();
    each_struct(&i18n_with_a_title, true, &mut nested);
    let msg = an_atom_a_deeper_sibling_would_split(&nested)
        .expect("a table-spelled atom beside a map must trip the invariant");
    assert!(msg.contains("`title`"), "{msg}");

    // And what the invariant is protecting, since a shape alone does not
    // say: at `Descend(2)` the base's `en` and the site's `fr` come back
    // as one localized string, written by two files and by no author.
    let base = toml::from_str::<toml::Value>("title = { en = \"Home\" }\n").unwrap();
    let site = toml::from_str::<toml::Value>("title = { fr = \"Accueil\" }\n").unwrap();
    let merged = merge_to_depth(base, site, 2, &mut Vec::new(), &mut Trace::off());
    let title = merged["title"].as_table().expect("a localized string");
    assert_eq!(
        title.keys().collect::<Vec<_>>(),
        ["en", "fr"],
        "the merge composed a LocalizedStr out of two writers"
    );
}

/// Retired spellings must not be silently ignored. View-level stales
/// (`match`, `over`, …) land in `route_fields` and fail validate;
/// everything else is still `deny_unknown_fields` at parse.
#[test]
fn an_unknown_config_key_is_a_parse_error() {
    for stale in [
        "[views.published]\nfrom = \"blog\"\n",
        // A relation's candidate pool is `from` now.
        "[collections.relations.related]\nover = \"published\"\n",
        // `match` on a relation is a `where` clause now.
        "[collections.relations.related]\nmatch = \"recipes/**\"\n",
        // Objects membership is a rule `match` glob now.
        "extensions = [\"png\"]\n",
    ] {
        let src = format!(
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
                 [[collections]]\nsource = \"_posts\"\n{stale}"
        );
        let e = Config::from_toml(&src)
            .expect_err("stale spelling should not parse")
            .to_string();
        assert!(e.contains("unknown field"), "{stale} -> {e}");
    }
    // Stale view keys fail at validate, not parse.
    for stale in [
        "[sets.s]\nover = \"blog\"\n",
        "[sets.s]\nfrom = \"blog\"\nfilter = \"!draft\"\n",
        "[routes.r]\nfrom = \"blog\"\npath = \"/r/\"\nroute = \"/also/\"\n",
        "[sets.s]\nfrom = \"blog\"\nmatch = \"recipes/**\"\n",
    ] {
        let e = cfg_err(stale);
        assert!(e.contains("unknown field"), "{stale} -> {e}");
    }
}

/// The strictness reaches the leaf tables too. Each of these parsed and
/// dropped the key before: `[site] them =` left the site on the base
/// theme, `[i18n] locale =` left i18n off, `[links] strict =` left the
/// policy at its default.
#[test]
fn an_unknown_key_on_a_leaf_table_is_a_parse_error() {
    for stale in [
        "[i18n]\nlocale = \"fr\"\n",
        "[links]\nstrict = true\n",
        "[shells.x]\ncommand = \"c\"\nargs = []\n",
    ] {
        let src = format!(
            "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
                 [[collections]]\nsource = \"_posts\"\n{stale}"
        );
        let e = Config::from_toml(&src)
            .expect_err("stale spelling should not parse")
            .to_string();
        assert!(e.contains("unknown field"), "{stale} -> {e}");
    }
    let e = Config::from_toml(
        "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             them = \"ledger\"\n",
    )
    .expect_err("a misspelled [site] key should not parse")
    .to_string();
    assert!(e.contains("unknown field"), "{e}");
}

/// `[site] noindex` is refused: the field lives in `[schema]` and is
/// forced via `[profiles.*.force]`, never written on the site table.
#[test]
fn site_noindex_is_an_unknown_field() {
    let e = Config::from_toml(
        "root = \".\"\nextends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             noindex = true\n",
    )
    .expect_err("[site] noindex is not a site key")
    .to_string();
    assert!(e.contains("unknown field `noindex`"), "{e}");
}

/// The two views every profile test below writes over: a set that never
/// lands, and a route that does.
const PROFILE_VIEWS: &str = "[schema]\nhidden = { type = \"bool\" }\n\
         [sets.published]\nfrom = \"blog\"\nwhere = \"!hidden\"\n\
         [routes.blog_index]\npath = \"/blog/\"\nfrom = \"published\"\nlayout = \"card\"\n";

/// The fence, law made checkable. A profile may
/// touch what a projection SAYS and SELECTS and never what LOADS, and the
/// two lists beside `Config::shape` are exhaustive over the config surface,
/// so the error can tell "you may not project this" apart from "this is not
/// a config key at all".
///
/// Mutation check: delete `"axes"` from `PROJECTABLE` and the first arm
/// below fails (an axis overlay is refused); delete the `fence` call from
/// `check_profiles` and the `[profiles.p.collections]` half loads in
/// silence, the fixture `profile-projects-collections` is the same
/// sentence at site scale, where the overlay would really have applied.
#[test]
fn the_fence_refuses_what_a_profile_may_not_write() {
    // The control: every projectable key, written by a profile that also
    // forces a field, on a config that loads.
    let ok = format!(
        "{PROFILE_VIEWS}[profiles.p.site]\nurl = \"https://drafts.example.com\"\n\
             [profiles.p.axes.look]\nfield = \"look\"\nvalues = [\"plain\"]\n\
             [profiles.p.widgets]\nnote = \"<aside>{{body}}</aside>\"\n"
    );
    let c = projected(&ok, "p").expect("site, axes and widgets are projectable");
    assert_eq!(c.site.url, "https://drafts.example.com");
    assert!(c.axes.contains_key("look") && c.widgets.contains_key("note"));

    // What loads is not a profile's to change, and the error says so.
    let e = cfg_err(&format!(
        "{PROFILE_VIEWS}[[profiles.p.collections]]\nname = \"x\"\n"
    ));
    assert!(e.contains("[profiles.p.collections]"), "{e}");
    assert!(e.contains("never changes what loads"), "{e}");
    assert!(e.contains("identical under every profile"), "{e}");
    assert!(e.contains("site, html, sets, routes"), "the knowns: {e}");
    // Every non-projectable key says it, not just the interesting one.
    for key in ["schema", "markers", "extends", "root", "links"] {
        let e = cfg_err(&format!("{PROFILE_VIEWS}[profiles.p]\n{key} = \"x\"\n"));
        assert!(e.contains("never changes what loads"), "{key}: {e}");
    }

    // No recursion: an overlay is one layer, not a ladder.
    let e = cfg_err(&format!(
        "{PROFILE_VIEWS}[profiles.p.profiles.q.site]\nurl = \"u\"\n"
    ));
    assert!(e.contains("never contains profiles"), "{e}");

    // And a key that is no config key at all is told that instead.
    let e = cfg_err(&format!("{PROFILE_VIEWS}[profiles.p]\nnosuch = 1\n"));
    assert!(e.contains("names no config key"), "{e}");
}

/// The fence is a decision, so it must be TOTAL over the config surface,
/// which is what makes "names no config key" a true sentence rather than a
/// guess. A field added to `Config` has to be put on one side or the other
/// here, the same discipline `every_config_key_has_a_law` applies to the
/// merge itself.
///
/// Mutation check: delete any key from either list and this fails naming
/// it; move one to the other list and the disjointness assert fires.
#[test]
fn the_fence_classifies_every_top_level_key() {
    let shape = Config::shape();
    let keys: Vec<&str> = shape.fields().iter().map(|(k, _)| *k).collect();
    for k in &keys {
        let projectable = PROJECTABLE.contains(k);
        assert!(
            projectable != NOT_PROJECTABLE.contains(k),
            "{k} is on both sides of the fence, or on neither"
        );
    }
    for k in PROJECTABLE.iter().chain(NOT_PROJECTABLE) {
        assert!(
            keys.contains(k),
            "the fence names {k}, which is no config key"
        );
    }
    // `force` is reserved rather than projectable: it is rung 0, lifted out
    // before the overlay is merged, and it is deliberately NOT a config key.
    assert!(!keys.contains(&FORCE));
}

/// The fence again, and the reason grack.com's drafts profile restates
/// `[sets.published]` in full: the shape decides. `[site]` is a bag, so a
/// profile patches one key of it and the rest survive; a `[sets.*]` entry
/// is a DEFINITION, and a definition is an atom, the profile's entry
/// replaces the site's entire, `order_by` and all.
///
/// Mutation check: give `sets` `Law::Atom`'s depth-0 shape and the bag half
/// fails; annotate `site` as an atom and the definition half stops being
/// the distinction this test is about. The site-scale version of the second
/// half is the parity gate: drop `order_by` from grack.com's restatement
/// and `--profile drafts` lists by path.
#[test]
fn a_bag_patches_per_key_and_a_definition_replaces_whole() {
    let views = "[schema]\nhidden = { type = \"bool\" }\n\
             [sets.published]\nfrom = \"blog\"\nwhere = \"!hidden\"\norder_by = \"-date\"\n";
    let c = projected(
        &format!("{views}[profiles.p.site]\nurl = \"https://drafts.example.com\"\n"),
        "p",
    )
    .expect("a bag patches");
    assert_eq!(c.site.url, "https://drafts.example.com");
    assert_eq!(c.site.title, "t", "the rest of the bag stands");

    let c = projected(
        &format!("{views}[profiles.p.sets.published]\nfrom = \"blog\"\nwhere = \"true\"\n"),
        "p",
    )
    .expect("a restatement is a whole definition");
    assert_eq!(c.views["published"].filter.as_deref(), Some("true"));
    assert_eq!(
        c.views["published"].order_by, None,
        "the site's `order_by` is not half-inherited — the definition is the atom"
    );
}

/// The migrations, both of them: the closed profile vocabulary's two typed
/// keys are ordinary config paths now, and each old spelling names its new
/// one rather than leaving the fence to say "not a config key".
///
/// Mutation check: delete either arm of `fence` and the spelling gets the
/// generic sentence, which is true and says nothing about the fix.
#[test]
fn the_old_profile_spellings_name_the_new_ones() {
    // The tombstone: `[profiles.NAME] noindex = true` meant
    // something materially different, it overwrote the head declaration
    // with a constant, so `noindex = false` is refused too, since it never
    // meant anything either.
    for old in ["noindex = true", "noindex = false"] {
        let e = cfg_err(&format!(
            "[schema]\nnoindex = {{ type = \"bool\" }}\n[profiles.drafts]\n{old}\n"
        ));
        assert!(e.contains("no longer a profile key"), "{e}");
        assert!(e.contains("[profiles.drafts.force]"), "{e}");
        assert!(e.contains("noindex = true"), "the new spelling: {e}");
    }
    // E2's: `url` was the profile's own key and is the site's key now.
    // Serde says nothing here, the body is a partial config, so an
    // unknown top-level key is the fence's to explain, and `url` is live
    // in example.
    let e = cfg_err("[profiles.drafts]\nurl = \"https://drafts.example.com\"\n");
    assert!(e.contains("no longer a profile key of its own"), "{e}");
    assert!(e.contains("[profiles.drafts.site]"), "{e}");
    assert!(e.contains("url = "), "the new spelling: {e}");
}

/// The site every R7 test below leans on: it declares `url` and lets the
/// base supply `title` and `author`, which is the ordinary shape (a site
/// need not restate what `extends` already said) and the shape that breaks
/// a re-parse of the site's text alone.
const BASE_LEANING: &str = "root = \".\"\n[site]\nurl = \"u\"\n";

/// The spanned re-parse is a *second opinion*, and a second
/// opinion that changes the subject must not be published: on this site the
/// text alone is missing base-supplied `[site]` keys, so the re-parse says
/// `missing field`, a fiction, since the merged config had every one of
/// them, while the real error is the retired `match` spelling in the
/// overlay. Post-hard-cutoff `deny_unknown_fields` is the only
/// thing that teaches the three retired spellings, so masking it is the
/// whole cost.
///
/// Mutation check: drop the `message()` comparison (re-parse's error
/// whenever it errors, the pre-R7 `?`) and this reports `missing field
/// title` at line 2 instead.
#[test]
fn a_re_parse_that_changes_the_subject_does_not_speak() {
    // `match` on a view is captured as a route field and refused at validate
    // (View flattens schema stamps), so this uses a site-level unknown so the
    // re-parse path is what speaks for keys that fail deserialize on the
    // merged table.
    let e = Config::from_toml_profile(
        &format!("{BASE_LEANING}[profiles.q.site]\nnosuch = 1\n"),
        Some("q"),
    )
    .expect_err("unknown site key")
    .to_string();
    assert!(e.contains("unknown field `nosuch`"), "the real error: {e}");
    assert!(
        !e.contains("missing field"),
        "the site's own text is short of base-supplied keys; that is not an error: {e}"
    );
}

/// The other half of R7, and B3's original intent: when the re-parse DOES
/// reproduce the failure, its error is the one worth having, because it
/// carries the line and column that deserializing a merged `toml::Value`
/// threw away.
///
/// Mutation check: delete the fallback (return the merged error always) and
/// the sentence survives while the span does not, no `line 4`, no caret.
#[test]
fn a_genuine_error_in_the_sites_own_text_keeps_its_span() {
    let e = Config::from_toml(&format!("{BASE_LEANING}nope = 1\n"))
        .expect_err("`nope` is not a `[site]` key")
        .to_string();
    assert!(e.contains("unknown field `nope`"), "{e}");
    assert!(e.contains("line 4"), "the span is the point: {e}");
}

/// The control that keeps the two above honest: leaning on the base is not
/// itself an error, with a profile or without one. If this ever fails, the
/// other two are passing for the wrong reason.
#[test]
fn a_site_that_leans_on_the_base_for_site_keys_loads() {
    let text = format!("{BASE_LEANING}[profiles.q.site]\ntitle = \"drafts\"\n");
    let plain = Config::from_toml(&text).expect("the base supplies title and author");
    assert_eq!(plain.site.title, "A grackle site");
    assert_eq!(plain.site.author, "");
    let projected =
        Config::from_toml_profile(&text, Some("q")).expect("and the overlay patches one key");
    assert_eq!(projected.site.title, "drafts");
    assert_eq!(projected.site.author, "");
}

/// A profile's `where` is accepted exactly where the `where`
/// it replaces is, the row built-ins AND every declared field, one
/// schema, because that is what `Schemas::row_filter_schema` hands
/// `Base::resolve`.
///
/// The two-shot try this replaces (`row_schema()`, then
/// `route_schema(declared)`, with `?`) could not MIX them: `title` is in
/// the first and not the second, `hidden`, a declared field since,
/// is in the second and not the first, so a filter naming both failed
/// both parses and the profile was refused. Mutation-checked by restoring
/// the two-shot, which fails on `unknown field \`title\``.
#[test]
fn a_profile_filter_may_mix_builtins_and_declared_fields() {
    let c = projected(
        &format!(
            "{PROFILE_VIEWS}[profiles.p.sets.published]\nfrom = \"blog\"\n\
                 where = 'title != \"\" && !hidden'\n"
        ),
        "p",
    )
    .expect("one vocabulary, not two");
    assert_eq!(
        c.views["published"].filter.as_deref(),
        Some("title != \"\" && !hidden")
    );
    // Who wrote it. The overlay replaced the whole definition, so the
    // config cannot recover this from the text, it is recorded as the
    // projection is built, and it is what keeps the profile
    // in an error about a filter the reader cannot find in `[sets]`.
    assert_eq!(c.views["published"].filter_profile.as_deref(), Some("p"));
}

/// The other half of C6a: WHICH vocabulary is the patched view's own, and
/// the three genuinely differ. `kind` is a route column no row has;
/// `title` is a row column no route has; and `dir` is a `Str` on a row and
/// a `Bool` on a route, so "the union of all three", which is what a
/// two-shot try is reaching for, is not a schema anything could
/// type-check against. The dispatch is `build_views`'s, restated nowhere:
/// an all-outputs fold -> routes, otherwise rows plus every declared field,
/// and an object is a row like any other now, so a gallery reads
/// the same row vocabulary a post's view does.
#[test]
fn a_profile_filter_takes_the_patched_views_own_vocabulary() {
    let c = cfg_raw(&format!(
        "{PROFILE_VIEWS}\
             [[collections]]\nname = \"pics\"\n\
             [sets.gallery]\nfrom = \"pics\"\n\
             [routes.sitemap]\npath = \"/sitemap.xml\"\nshell = \"sitemap\"\n"
    ));
    let rows = c.view_filter_schema("published");
    assert!(rows.contains_key("title") && rows.contains_key("hidden"));
    assert!(!rows.contains_key("kind"), "a row has no route kind");

    let routes = c.view_filter_schema("sitemap");
    assert!(routes.contains_key("kind") && routes.contains_key("hidden"));
    assert!(!routes.contains_key("title"), "a route has no title");

    // A gallery reads the one row schema: the image columns (`image.width`)
    // and every declared field (`hidden`) alike.
    let objects = c.view_filter_schema("gallery");
    assert!(
        objects.contains_key("image.width"),
        "an object has dimensions"
    );
    assert!(objects.contains_key("hidden"), "and the declared fields");

    // The collision that rules the union out, stated rather than implied.
    use grackle_db::filter::Type;
    assert_eq!(rows.get("dir"), Some(&Type::Str));
    assert_eq!(routes.get("dir"), Some(&Type::Bool));

    // And the fold's own overlay applies, against route words.
    projected(
        &format!(
            "{PROFILE_VIEWS}[routes.sitemap]\npath = \"/sitemap.xml\"\n\
                 shell = \"sitemap\"\n\
                 [profiles.p.routes.sitemap]\npath = \"/sitemap.xml\"\n\
                 shell = \"sitemap\"\nwhere = 'collection == \"posts\" && !hidden'\n"
        ),
        "p",
    )
    .expect("an all-outputs fold reads routes");
}

/// The deferral C6a's fix rests on, at the unit level: a name this early
/// vocabulary does not have is NOT rejected here, because a positional
/// `.schema.toml` declares fields the tree walk has not read yet and
/// refusing them would make a profile's `where` stricter than the `where`
/// it replaces. What is caught is everything that is wrong however the
/// walk turns out, `a_profile_filter_that_does_not_type_check_is_caught_at_load`
/// is that half. The tree-driven proof of both directions lives in
/// `load::profile_filter_tests`.
#[test]
fn an_unknown_name_in_a_profile_filter_is_deferred_not_rejected() {
    let c = projected(
        &format!(
            "{PROFILE_VIEWS}[profiles.p.sets.published]\nfrom = \"blog\"\nwhere = \"!cover\"\n"
        ),
        "p",
    )
    .expect("`cover` may yet be a positional declaration");
    assert_eq!(c.views["published"].filter.as_deref(), Some("!cover"));
}

/// The projection is a config that `validate` has never seen,
/// so it is validated, and `check_profile_filters` is what makes that
/// required, since it is keyed off the provenance E2 records as the
/// overlay is merged.
///
/// Mutation-checked by deleting the `filter_profile` loop in
/// `from_toml_profile`, after which this config projects happily and the
/// type error surfaces at the pass that evaluates the filter, naming no
/// profile.
#[test]
fn a_profile_filter_that_does_not_type_check_is_caught_at_load() {
    let e = format!(
        "{:#}",
        projected(
            &format!(
                "{PROFILE_VIEWS}[profiles.p.sets.published]\nfrom = \"blog\"\n\
                     where = 'title > 3'\n"
            ),
            "p",
        )
        .expect_err("a string is not an int")
    );
    assert!(e.contains("profile p"), "names the profile: {e}");
    assert!(e.contains("view published"), "names the view: {e}");
}

/// What the retired placement checks were guarding is now
/// said by the ordinary rules, because the overlay produces an ordinary
/// config. C6c refused `[profiles.p.sets.blog_index]` because `blog_index`
/// is a route; today that entry ADDS a `[sets]` definition of that name,
/// which collides in the one namespace `merge_queries` folds the two
/// sections into, the same error a site writing it twice would get, and
/// the reason no third rule is needed.
///
/// Mutation check: delete the `views.insert(...).is_some()` bail in
/// `merge_queries` and the misplaced entry loads, its view patched twice
/// in map order.
#[test]
fn a_misplaced_profile_entry_collides_in_the_one_namespace() {
    // The control: both entries where they belong, both restated whole.
    let ok = projected(
        &format!(
            "{PROFILE_VIEWS}[profiles.p.sets.published]\nfrom = \"blog\"\nwhere = \"!hidden\"\n\
                 [profiles.p.routes.blog_index]\npath = \"/blog/\"\nfrom = \"published\"\n\
                 layout = \"card\"\nwhere = 'title != \"\"'\n"
        ),
        "p",
    )
    .expect("both are where they belong");
    assert_eq!(ok.views["published"].filter.as_deref(), Some("!hidden"));
    assert_eq!(
        ok.views["blog_index"].filter.as_deref(),
        Some("title != \"\"")
    );

    let e = format!(
        "{:#}",
        Config::from_toml(&cfg_source(&format!(
            "{PROFILE_VIEWS}[profiles.p.sets.blog_index]\npath = \"/blog/\"\n\
                 from = \"published\"\nlayout = \"card\"\nwhere = \"!hidden\"\n"
        )))
        .expect_err("`blog_index` is already a route")
    );
    assert!(e.contains("profile p"), "{e}");
    assert!(
        e.contains("declares a path") || e.contains("both a set and a route"),
        "{e}"
    );
}

/// Every declared profile is
/// projected, deserialized and validated at every load, so a broken
/// overlay in a projection nobody is building is a load error today. The
/// config below is loaded the way `grackle build` loads it, with no
/// `--profile` anywhere.
///
/// Its typo is `publised` for `[sets.published]`, the query a drafts-shaped
/// profile relaxes. A profile naming an unknown view adds a definition,
/// which is what a registry does; the addition is then held to the same rules
/// as any other entry, and a set with no `from` is not a set. An absent
/// `from` is legal on a fold shell (it reads every output) and on nothing
/// else, and this entry declares no shell.
///
/// Mutation check: delete the dry-run loop in `from_toml_profile` and both
/// halves load in silence, failing only under `--profile staging`. The
/// site-scale version is the `profile-unknown-view` fixture.
#[test]
fn a_broken_overlay_fails_a_load_that_never_applies_it() {
    let e = format!(
        "{:#}",
        Config::from_toml(&cfg_source(&format!(
            "{PROFILE_VIEWS}[profiles.staging.sets.publised]\nwhere = \"!hidden\"\n"
        )))
        .expect_err("a set with no `from` is not a set")
    );
    assert!(e.contains("profile staging"), "names the profile: {e}");
    assert!(e.contains("checked at every load"), "and why: {e}");
    assert!(e.contains("no `from`"), "{e}");

    // The other direction: a profile ADDING a well-formed view is legal,
    // a registry gains an entry, which is what a registry is for.
    let c = projected(
        &format!(
            "{PROFILE_VIEWS}[profiles.staging.sets.drafts_only]\nfrom = \"blog\"\n\
                 where = \"hidden\"\n"
        ),
        "staging",
    )
    .expect("a profile may add a set");
    assert_eq!(c.views["drafts_only"].filter.as_deref(), Some("hidden"));
    // …and it is the author's, not the base's: an error about it must not
    // send them looking in a config they did not write.
    assert!(!c.views["drafts_only"].inherited);
}

/// A correct profile is unchanged by being checked early.
///
/// `dev` is implicit (`serve` defaults to it; undeclared it changes
/// nothing), so the dry run must not invent a `[profiles.dev]` requirement.
/// It only iterates profiles the config declares.
#[test]
fn checking_every_profile_leaves_the_correct_ones_alone() {
    // A site with no profiles at all, reaching the next line is the
    // assertion, since the dry run runs inside `from_toml`.
    let plain = Config::from_toml(&cfg_source(PROFILE_VIEWS)).expect("no profiles, no checks");
    assert!(plain.profiles.is_empty());

    // grack.com's shape: one profile, correct, never applied.
    let declared = format!(
        "{PROFILE_VIEWS}[profiles.drafts.force]\nhidden = false\n\
             [profiles.drafts.sets.published]\nfrom = \"blog\"\nwhere = \"true\"\n"
    );
    let both = Config::from_toml(&cfg_source(&declared)).expect("declared, not applied");
    assert_eq!(
        both.views["published"].filter.as_deref(),
        Some("!hidden"),
        "the default projection is the config exactly as written"
    );
    assert!(both.forced.is_empty(), "nothing is forced until applied");
    // And applying it still works, which is the same config one flag on.
    let applied = projected(&declared, "drafts").expect("as declared");
    assert_eq!(applied.views["published"].filter.as_deref(), Some("true"));
    assert_eq!(applied.forced["hidden"], toml::Value::Boolean(false));

    // `serve`'s default: undeclared `dev` needs no `[profiles.dev]`, and a
    // config carrying an unrelated profile still loads under it.
    let dev = projected(&declared, "dev").expect("dev is implicit");
    assert!(!dev.profiles.contains_key("dev"));
    assert_eq!(dev.profile.as_deref(), Some("dev"));
    // …and changes nothing: `drafts` was declared, not applied.
    assert_eq!(dev.views["published"].filter.as_deref(), Some("!hidden"));
    assert!(dev.views["published"].filter_profile.is_none());

    // A name that is neither declared nor implicit is a load error naming
    // what exists, rather than a build that ships the wrong projection.
    let e = format!(
        "{:#}",
        Config::from_toml_profile(&cfg_source(&declared), Some("stagin"))
            .expect_err("a typo is not a projection")
    );
    assert!(e.contains("unknown profile \"stagin\""), "{e}");
    assert!(e.contains("declared: dev, drafts"), "{e}");
}

/// A `[routes]` entry whose `default_content` offer was DECLINED loses its
/// path, and what that leaves is not a set. The section an entry was
/// declared under is recorded rather than re-derived for exactly this
/// case: `is_materialized()` would call this view a set, and C7b's error
/// tells the author to "declare your own [sets.home]" over an entry that
/// lives under `[routes]`.
///
/// (C6c's placement check was the other reader and is retired with E2,
/// `whose_from` is what keeps `declared_set` live. Mutation check: derive
/// it from `is_materialized()` in `merge_queries` and this fails.)
#[test]
fn a_declined_default_content_route_is_still_a_route() {
    let mut c = cfg_raw(
        "[routes.home]\npath = \"/\"\nfrom = \"blog\"\nlayout = \"card\"\n\
             default_content = \"index.md\"\n",
    );
    // What `resolve_default_content` does to a route whose offered row
    // exists and does not place `{% view home %}`: the row wants the URL to
    // itself, so the route stands down.
    let v = c.views.get_mut("home").expect("declared");
    v.route = None;
    v.routes.clear();
    assert!(!v.is_materialized());
    assert!(
        !c.views["home"].declared_set,
        "a route with no path left is still a route"
    );
}

/// The whole point of the shape: the profile writes the
/// FIELD, and the site's own `robots` expression is left exactly as its
/// author wrote it. C6d's key overwrote `[html.head.meta] robots` with the
/// constant `"noindex,follow"` on every page of the projection, which is
/// why it needed a warning to be honest; there is nothing left to warn
/// about, and the two configs below, one inheriting the base's
/// expression, one writing its own, now come out saying different things
/// about the same forced fact, which is what "the site's vocabulary"
/// means.
///
/// Mutation check: leave `force` in the overlay (`split_profile` reading it
/// rather than removing it) and the projected table carries a top-level
/// `force` key the `Config` deserializer refuses, rung 0 is reserved, not
/// config surface, and the fence lets it through for exactly that reason.
#[test]
fn a_forced_field_leaves_the_sites_robots_expression_alone() {
    let site = "[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n";
    let inherited = Config::from_toml_profile(
        &format!("root = \".\"\n{site}[profiles.drafts.force]\nnoindex = true\n"),
        Some("drafts"),
    )
    .expect("a base-inheriting config");
    assert_eq!(
        inherited.html.head.meta["robots"], "noindex ? \"noindex,follow\" : \"\"",
        "the base's expression, untouched — it EVALUATES the forced field"
    );

    let own = Config::from_toml_profile(
        &format!(
            "root = \".\"\n{site}[profiles.drafts.force]\nnoindex = true\n\
                 [html.head.meta]\nrobots = 'noindex ? \"noindex,nofollow\" : \"index,follow\"'\n"
        ),
        Some("drafts"),
    )
    .expect("a site may write its own robots expression");
    assert_eq!(
        own.html.head.meta["robots"], "noindex ? \"noindex,nofollow\" : \"index,follow\"",
        "an editorial policy its author spelled out is not a profile's to \
             replace — it answers the forced fact its own way"
    );
    assert_eq!(own.forced["noindex"], toml::Value::Boolean(true));
}

/// Rung 0's names come from the site's own `[schema]`, and they are
/// checked for EVERY declared profile at every load, the same sentence,
/// one table over. `cfg_err` applies no profile at all.
///
/// Mutation-checked three ways: deleting the `declared.get` arm accepts
/// `nosuchfield` (first half); deleting the `schema::typed` call accepts
/// `noindex = "yes"` (second half); and deleting the whole block from
/// `check_profiles` loses both.
#[test]
fn a_forced_field_is_declared_and_typed_for_every_profile() {
    const S: &str = "[schema]\nnoindex = { type = \"bool\" }\n";

    let e = cfg_err(&format!(
        "{S}[profiles.staging.force]\nnosuchfield = true\n"
    ));
    assert!(e.contains("profile staging"), "names the profile: {e}");
    assert!(e.contains("[profiles.staging.force] nosuchfield"), "{e}");
    assert!(e.contains("declared in the site's own [schema]"), "{e}");
    assert!(e.contains("declared fields: noindex"), "the knowns: {e}");

    let e = cfg_err(&format!("{S}[profiles.staging.force]\nnoindex = \"yes\"\n"));
    assert!(e.contains("[profiles.staging.force]"), "{e}");
    assert!(e.contains("declared bool"), "{e}");

    // Rung 0 is not overlay: `force` is lifted out before the merge, so a
    // table under it is never a config path.
    let e = cfg_err(&format!("{S}[profiles.staging]\nforce = 3\n"));
    assert!(e.contains("[profiles.staging.force] is a table"), "{e}");

    // The control: correct, and inert on a load that applies no profile.
    let ok = cfg(&format!("{S}[profiles.staging.force]\nnoindex = true\n"));
    assert!(ok.forced.is_empty(), "nothing is forced until applied");
}

#[test]
fn unknown_from_is_an_error() {
    let c = cfg("[sets.latest]\nfrom = \"pubished\"\nlimit = 3\n");
    let e = c.query("latest").unwrap_err().to_string();
    assert!(
        e.contains("neither a collection, a set nor a route"),
        "unexpected error: {e}"
    );
    // The author wrote this one, so there is nothing to explain about
    // where it came from, the control for the two tests below.
    assert!(!e.contains("inherited from the base config"), "{e}");
    assert!(!e.contains("reached from"), "{e}");
}

/// Renaming the collection at `_posts` retires the name
/// `posts`, and the base's `[sets.published] from = "posts"` then names
/// nothing, on a site whose grackle.toml has no `published` in it.
///
/// Views key on name and survive every rename; collections key on
/// `source` and do not. That asymmetry is the whole of this bug, and it
/// is why an inherited `from` is the one reference a site can break
/// without touching the entry that carries it.
#[test]
fn an_inherited_sets_dangling_from_says_it_came_from_the_base() {
    let c = Config::from_toml(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"notes\"\nsource=\"_posts\"\n",
    )
    .unwrap();
    let e = c.query("published").unwrap_err().to_string();
    assert!(e.starts_with("published: `from = \"posts\"`"), "{e}");
    assert!(
        e.contains("\"published\" is inherited from the base config"),
        "{e}"
    );
    assert!(
        e.contains("declare your own [sets.published]"),
        "the fix, in the table the entry would live in: {e}"
    );
    // The knowns are what show the author their own rename.
    assert!(e.contains("collections: entries, notes, objects"), "{e}");
}

/// The other half of the same blame: `blog_index` composes over
/// `published`, so asking for `blog_index`'s query is what surfaces
/// `published`'s broken `from`, and the old message put `blog_index`'s
/// name in front of a `from` that is not in `blog_index`.
#[test]
fn a_composed_chain_blames_the_view_that_carries_the_from() {
    let c = Config::from_toml(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"notes\"\nsource=\"_posts\"\n",
    )
    .unwrap();
    let e = c.query("blog_index").unwrap_err().to_string();
    assert!(
        e.starts_with("published: `from = \"posts\"`"),
        "the carrier, not the asker: {e}"
    );
    assert!(
        e.contains("(reached from \"blog_index\", which composes over it.)"),
        "{e}"
    );
}

/// The control, and the shape `examples/field-notes` really has: rename
/// the collection AND say what the inherited set means now. One line, and
/// it is the line the error above asks for.
#[test]
fn a_renamed_collection_with_its_own_published_set_resolves() {
    let c = Config::from_toml(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"notes\"\nsource=\"_posts\"\n\
             [sets.published]\nfrom=\"notes\"\nwhere=\"!draft\"\n",
    )
    .unwrap();
    let q = c
        .query("blog_index")
        .expect("the chain terminates at `notes`");
    assert_eq!(q.base, vec!["notes".to_string()]);
}

#[test]
fn cyclic_chain_terminates() {
    let c = cfg("[sets.a]\nfrom = \"b\"\n\n[sets.b]\nfrom = \"a\"\n");
    let e = c.query("a").unwrap_err().to_string();
    assert!(e.contains("cyclic"), "unexpected error: {e}");
}

/// The pairing axis's canonical is `values[0]`; without a declared axis, none.
#[test]
fn axes_locale_syncs_the_display_default() {
    let c = cfg("[axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n");
    assert_eq!(c.pairing_canonical(), Some("en"));
    let (name, axis) = c.pairing_axis().expect("locale axis");
    assert_eq!(name, "locale");
    assert_eq!(axis.values, ["en", "fr"]);

    let off = cfg("");
    assert_eq!(off.pairing_canonical(), None);
    assert!(off.pairing_axis().is_none());
}

/// A site may name the i18n axis anything; the engine follows `[i18n] axis`.
#[test]
fn i18n_axis_need_not_be_named_locale() {
    let c = cfg(
        "[i18n]\naxis = \"lang\"\n\n[axes.lang]\nvalues = [\"en\", \"fr\"]\nfield = \"lang\"\n",
    );
    let (name, axis) = c.pairing_axis().expect("lang axis");
    assert_eq!(name, "lang");
    assert_eq!(axis.field, "lang");
    assert_eq!(c.pairing_canonical(), Some("en"));
    assert_eq!(axis.values, ["en", "fr"]);
}

/// display-name hierarchy: inline beats global beats built-in;
/// "@key" references the global map; "@@" escapes a literal @.
#[test]
fn string_hierarchy_resolves() {
    let c = cfg("[sets.a]\nfrom = \"posts\"\ntitle = \"@kitchen\"\n\n\
             [sets.b]\nfrom = \"posts\"\ntitle = \"Inline wins\"\ncrumb = \"@@literal-at\"\n\n\
             [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\n\
             [i18n.strings]\nkitchen = { en = \"Kitchen\", fr = \"Cuisine\" }\n\
             home = { en = \"Home\", fr = \"Accueil\" }\n");
    let t = c.views["a"].title.as_ref().unwrap();
    assert_eq!(c.i18n_text(t, "en"), "Kitchen");
    assert_eq!(c.i18n_text(t, "fr"), "Cuisine");
    let t = c.views["b"].title.as_ref().unwrap();
    assert_eq!(c.i18n_text(t, "fr"), "Inline wins");
    let t = c.views["b"].crumb.as_ref().unwrap();
    assert_eq!(c.i18n_text(t, "en"), "@literal-at");
    // Declared override; absent key is empty (defaults live in base.toml).
    assert_eq!(c.i18n_string("home", "fr"), "Accueil");
    assert_eq!(c.i18n_string("related", "fr"), "");
}

#[test]
fn i18n_tables_resolve_and_merge() {
    // Base ships months; inherited untouched.
    let c = Config::from_toml(
        "[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
         [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n",
    )
    .unwrap();
    c.validate().unwrap();
    assert_eq!(c.i18n_table("months", "1", "en"), "January");
    assert_eq!(c.i18n_table("months", "7", "en"), "July");
    assert_eq!(c.i18n_table("months", "13", "en"), "");
    assert_eq!(c.i18n_string("home", "en"), "Home");

    // A named table is a TableAtom: the site's months replace base's whole.
    let c = Config::from_toml(
        "[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
         [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\
         [i18n.tables.months]\n7 = { en = \"July\", fr = \"juillet\" }\n",
    )
    .unwrap();
    c.validate().unwrap();
    assert_eq!(c.i18n_table("months", "1", "en"), "");
    assert_eq!(c.i18n_table("months", "7", "fr"), "juillet");
    assert_eq!(c.i18n_table("months", "7", "en"), "July");
}

#[test]
fn render_localized_resolves_table_refs() {
    let c = Config::from_toml(
        "[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
         [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\
         [i18n.tables.months]\n7 = { en = \"July\", fr = \"juillet\" }\n",
    )
    .unwrap();
    let get = |tok: &str| match tok {
        "month" => Some("07".into()),
        "year" => Some("2022".into()),
        _ => None,
    };
    let crumb = LocalizedStr::One("@months[{month}]".into());
    assert_eq!(c.render_localized(&crumb, "fr", &get).unwrap(), "juillet");
    let title = LocalizedStr::One("{year} @months[{month}]".into());
    assert_eq!(c.render_localized(&title, "en", &get).unwrap(), "2022 July");
    // `@@` keeps a literal `@months[…]`.
    let lit = LocalizedStr::One("@@months[{month}]".into());
    assert_eq!(c.render_localized(&lit, "en", &get).unwrap(), "@months[07]");
}

#[test]
fn format_date_expands_medium_date_template() {
    let c = Config::from_toml(
        "[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
         [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n",
    )
    .unwrap();
    let d = chrono::NaiveDate::from_ymd_opt(2022, 3, 16).unwrap();
    assert_eq!(c.format_date(d, "medium_date", "en"), "16 March 2022");
    assert_eq!(c.format_date(d, "short_date", "en"), "16 Mar 2022");
    assert_eq!(c.format_date(d, "long_date", "en"), "March 16, 2022");
    // Site override of the template + French month name.
    let c = Config::from_toml(
        "[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
         [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\
         [i18n.strings]\nmedium_date = \"{day} @months[{month}] {year}\"\n\
         [i18n.tables.months]\n3 = { en = \"March\", fr = \"mars\" }\n",
    )
    .unwrap();
    assert_eq!(c.format_date(d, "medium_date", "fr"), "16 mars 2022");
}

#[test]
fn unknown_i18n_table_ref_is_a_load_error() {
    let e = cfg_err(
        "[axes.locale]\nvalues = [\"en\"]\nfield = \"locale\"\n\
         [routes.x]\npath = \"/x/\"\nfrom = \"blog\"\n\
         group_by = \"date.month\"\ncrumb = \"@nope[{month}]\"\n",
    );
    assert!(e.contains("@nope"), "{e}");
    assert!(
        e.contains("names no table") || e.contains("no table"),
        "{e}"
    );
}

#[test]
fn i18n_table_member_maps_are_checked() {
    let e = cfg_err(
        "[axes.locale]\nvalues = [\"en\"]\nfield = \"locale\"\n\
         [i18n.tables.months]\n1 = { en = \"January\", fr = \"janvier\" }\n",
    );
    assert!(e.contains("i18n.tables.months.1"), "{e}");
    assert!(e.contains("fr"), "{e}");
}

/// `extends = "none"` inherits no vocabulary, Home is empty until declared.
#[test]
fn extends_none_has_no_i18n_vocabulary() {
    let c = cfg("");
    assert_eq!(c.i18n_string("home", "en"), "");
    assert_eq!(c.i18n_table("months", "1", "en"), "");
}

/// A dangling reference and an unused global string are both load
/// errors, the latter is what catches a typo'd engine-key override.
#[test]
fn string_hierarchy_fails_loud() {
    let e = cfg_err("[sets.a]\nfrom = \"posts\"\ntitle = \"@nope\"\n");
    assert!(e.contains("names no string"), "{e}");
    let e = cfg_err("[i18n.strings]\nhom = \"Home\"\n");
    assert!(e.contains("unused string"), "{e}");
    let e = cfg_err(
        "[sets.a]\nfrom = \"posts\"\ntitle = \"@x\"\n\n[i18n.strings]\nx = \"@y\"\ny = \"z\"\n",
    );
    assert!(e.contains("no chains"), "{e}");
}

/// `[i18n.names]` is keyed by locale, so a key naming no declared locale
/// is dead. The error names the default and the declared set.
#[test]
fn an_i18n_name_must_name_a_declared_locale() {
    let c = cfg(
        "[axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\n\
             [i18n.names]\nen = \"English\"\nfr = \"Français\"\n",
    );
    assert_eq!(c.i18n.name_of("fr"), "Français");
    assert_eq!(c.i18n.name_of("en"), "English");
    // The canonical member needs no `names` entry, and a name for it is
    // the shape every live site uses.
    let e =
            cfg_err("[axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\n[i18n.names]\nfr_CA = \"Français canadien\"\n");
    assert!(e.contains("fr_CA"), "{e}");
    assert!(e.contains("\"en\""), "the default is named: {e}");
    assert!(e.contains("\"fr\""), "the knowns are named: {e}");
    // …and with no locale axis, no member may be named.
    let e = cfg_err("[i18n.names]\nfr = \"Français\"\n");
    assert!(e.contains("\"fr\""), "{e}");
    assert!(e.contains("[]"), "no axis ⇒ no known members: {e}");
}

/// enum records: slug and display names default to the id; a
/// per-locale name falls back default-locale, then id. The `intro`
/// rides the same record; the retired [tags.x] spelling errors with
/// the new form.
#[test]
fn enum_records_default_to_id() {
    let c = cfg(
            "[records.tags.contes]\nslug = \"fairy-tales\"\nname = { en = \"Fairy tales\", fr = \"Contes\" }\n\n\
             [records.course.dinner]\nintro = \"Sure to please!\"\n\n[axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n",
        );
    assert_eq!(c.record_slug("tags", "contes"), "fairy-tales");
    assert_eq!(c.record_slug("tags", "rust"), "rust");
    assert_eq!(c.record_name("tags", "contes", "fr"), "Contes");
    assert_eq!(c.record_name("tags", "contes", "en"), "Fairy tales");
    assert_eq!(c.record_name("tags", "contes", "de"), "Fairy tales");
    assert_eq!(c.record_name("tags", "rust", "fr"), "rust");
    assert_eq!(c.record_slug("course", "dinner"), "dinner");
    assert_eq!(c.record_name("course", "dinner", "fr"), "dinner");
    let i = c
        .record("course", "dinner")
        .unwrap()
        .intro
        .as_ref()
        .unwrap();
    assert_eq!(c.i18n_text(i, "en"), "Sure to please!");
}

/// A multi-locale archive route spends `{axis:locale}` beside `{key}`;
/// validate and pill URLs must accept the axis the same way materialize does.
#[test]
fn an_archive_route_may_spend_an_axis() {
    let c = cfg(
        "[[collections.rules]]\nmatch = \"**\"\nroute = \"/{slug}/\"\n\
             [schema]\ntags = { type = \"list\" }\n\
             [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\
             [routes.tag_index]\n\
             paths = [\"/{axis:locale}/blog/tags/{key}/\", \"/blog/tags/{key}/\"]\n\
             from = \"blog\"\ngroup_by = \"tags\"\nlayout = \"card\"\n",
    );
    assert_eq!(
        c.archive_url("tags", "rust", "fr").as_deref(),
        Some("/fr/blog/tags/rust/")
    );
    assert_eq!(
        c.archive_url("tags", "rust", "en").as_deref(),
        Some("/blog/tags/rust/")
    );
}

// ------------------------------------------- `config --effective` (B3)

/// The effective config of a site whose text is `site`, with the preamble
/// stripped so an assertion is about the config and not about the prose.
fn effective(site: &str) -> String {
    let printed =
        Config::effective_toml(site, "test", None).expect("the effective config should print");
    printed
        .lines()
        .skip_while(|l| l.starts_with('#') || l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The line that carries `key`, comment and all.
fn provenance_of(printed: &str, key: &str) -> String {
    printed
        .lines()
        .find(|l| l.trim_start().starts_with(key))
        .unwrap_or_else(|| panic!("no line for {key} in:\n{printed}"))
        .to_string()
}

/// Law 2 at the surface a person reads: a redeclared registry entry says
/// SITE and the base's entry is gone entirely, not merged, not half
/// present. `limit = 20` is the base's `[routes.feed]`, and its absence
/// here is the whole claim.
#[test]
fn a_shadowed_registry_entry_reads_as_one_atom_from_the_site() {
    let out = effective(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [routes.feed]\npath=\"/feed.xml\"\nfrom=\"published\"\nshell=\"atom\"\n",
    );
    assert!(
        provenance_of(&out, "[routes.feed]").contains("# site over base, whole"),
        "{out}"
    );
    assert!(
        !out.contains("limit = 20"),
        "the base's feed entry survived the shadow:\n{out}"
    );
    // And the entry's own keys carry no provenance: the atom said it once.
    assert_eq!(
        provenance_of(&out, "path = \"/feed.xml\""),
        "path = \"/feed.xml\""
    );
    // A neighbour the site never wrote is the point of the command.
    assert!(
        provenance_of(&out, "[routes.home]").contains("# base, whole"),
        "{out}"
    );
}

/// A bag is the other law at the same depth, and reads differently: three
/// keys, three answers, on one table.
#[test]
fn a_merged_bag_shows_its_sources_key_by_key() {
    let out = effective("[site]\ntitle = \"Mine\"\nemail = \"me@example.com\"\n");
    assert!(
        provenance_of(&out, "title =").contains("# site over base"),
        "{out}"
    );
    assert!(provenance_of(&out, "email =").contains("# site"), "{out}");
    assert!(provenance_of(&out, "url =").ends_with("# base"), "{out}");
    assert!(
        provenance_of(&out, "author =").ends_with("# base"),
        "the base's empty author is still the base's:\n{out}"
    );
}

/// A whole table the site never mentioned. `[markers]` is the base's
/// three, and a site that has never heard of `.draft` still has it, the
/// invisible base, made visible, which is the reason this command exists.
#[test]
fn an_untouched_table_is_all_base() {
    let out = effective("[site]\ntitle = \"Mine\"\n");
    for m in ["\".draft\"", "\".hidden\"", "\".noindex\""] {
        assert!(
            provenance_of(&out, m).contains("# base, whole"),
            "{m} in:\n{out}"
        );
    }
    assert!(
        provenance_of(&out, "[sets.published]").contains("# base, whole"),
        "{out}"
    );
    // Never written by either file: serde's default, and it is named as
    // such rather than passed off as the base's.
    assert!(
        provenance_of(&out, "gitignore =").ends_with("# default"),
        "{out}"
    );
    assert!(
        provenance_of(&out, "root =").ends_with("# default"),
        "{out}"
    );
}

/// annotation, read out loud. A site's rules go in front and say
/// `site`; the base's catch-all sits behind them and says `base`, which is
/// how "first writer wins" looks when you can see the list.
#[test]
fn prepended_rules_carry_provenance_per_rule() {
    let out = effective(
        "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource = \"_posts\"\n\
             [[collections.rules]]\nmatch = \"drafts/**\"\nroute = \"/d/{slug}/\"\n",
    );
    // Only the posts collection's rules: the base's other two collections
    // are printed too, and their rules are a different list.
    let posts = out
        .split("\n[[collections]]")
        .find(|c| c.contains("source = \"_posts\""))
        .unwrap_or_else(|| panic!("no posts collection in:\n{out}"));
    let rules: Vec<&str> = posts
        .lines()
        .filter(|l| l.starts_with("[[collections.rules]]"))
        .collect();
    assert_eq!(rules.len(), 2, "site rule + the base's catch-all:\n{out}");
    assert!(rules[0].contains("# site, whole"), "{out}");
    assert!(rules[1].contains("# base, whole"), "{out}");
    assert!(
        out.contains("match = \"drafts/**\""),
        "the site's rule is first:\n{out}"
    );
}

/// `extends = "none"` has no merge to record, so the walk that stands in
/// for one must reach the same atoms: every key the site's own, at the
/// same granularity (`[sets.x]` whole, `[site]` per key).
#[test]
fn an_uninheriting_site_owns_every_key() {
    let out = effective(
        "extends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [sets.mine]\nfrom = \"posts\"\nwhere = \"!draft\"\norder_by = \"-date\"\n\
             [markers]\n\".x\" = { draft = true }\n",
    );
    for (key, want) in [
        ("extends =", "# site"),
        ("url =", "# site"),
        ("[sets.mine]", "# site, whole"),
        ("\".x\"", "# site, whole"),
    ] {
        assert!(provenance_of(&out, key).contains(want), "{key} in:\n{out}");
    }
    assert!(!out.contains("# base"), "nothing was inherited:\n{out}");
}

/// The printer neither drops a key nor invents one: parsed back, the text
/// IS the merged table. Comments are TOML's own, so nothing is stripped,
/// the parser does that.
///
/// This is the test that makes the rest safe to read. Provenance is a
/// comment and a comment cannot be wrong about a value it does not carry;
/// what could go wrong is the VALUE, a definition flattened, an inline
/// table mis-quoted, a key printed under the wrong header, and a
/// round-trip catches every one of those.
#[test]
fn printing_the_merged_config_loses_nothing() {
    for site in [
        "",
        "[site]\ntitle = \"Mine\"\n",
        "extends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n",
        // Every shape the printer distinguishes, in one file: an
        // array-of-tables keyed by identity, its rules, a nested map of
        // definitions, a localized string, a quoted key, an inline table.
        "root = \"..\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource = \"_posts\"\n\
             [collections.schema]\ncover = { type = \"image\" }\n\
             [[collections.rules]]\nmatch = \"**\"\ndefaults = { layout = \"post\" }\n\
             [collections.relations.related]\nfrom = \"published\"\nlimit = 3\n\
             [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\
             [html.head.meta]\n\"apple-title\" = 'site.title'\n\
             [i18n.strings]\nhome = { en = \"Home\", fr = \"Accueil\" }\n\
             [records.course.dinner]\nname = { en = \"Dinner\", fr = \"Dîner\" }\n\
             [widgets]\nnote = \"<aside>{body}</aside>\"\n",
    ] {
        let printed = Config::effective_toml(site, "test", None).expect("prints");
        let back: toml::Value = toml::from_str(&printed)
            .unwrap_or_else(|e| panic!("the printed config is not TOML: {e}\n{printed}"));

        let value: toml::Value = toml::from_str(site).unwrap();
        let mut want = match Config::extends_of(&value).unwrap() {
            true => merge_base(value).unwrap(),
            false => value,
        };
        let t = want.as_table_mut().unwrap();
        for (k, v) in engine_defaults() {
            if !t.contains_key(k) {
                t.insert(k.to_string(), v);
            }
        }
        assert_eq!(back, want, "printed:\n{printed}");
    }
}

/// A key TOML would not accept bare has to be quoted in a HEADER too, not
/// only in a `k = v` line, `[markers.".archive"]`. Found by mutating the
/// base-recording loop away, which turned every inherited marker into a
/// block and printed `[markers..draft]`; the payload here is long enough
/// to take that path without a mutation.
#[test]
fn a_quoted_key_stays_quoted_in_a_table_header() {
    let site = "[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n[markers]\n\
                    \".archive\" = { noindex = true, hidden = true, draft = true, layout = \"post\" }\n";
    let out = effective(site);
    assert!(out.contains("[markers.\".archive\"]"), "{out}");
    toml::from_str::<toml::Value>(&Config::effective_toml(site, "t", None).unwrap())
        .expect("a quoted header must parse");
}

/// The family check, on the view side. A view is a query,
/// so its declared shell folds the collection the query selects, and a
/// MAP shell here is an arity error, not an unknown word: `html` is a
/// perfectly good shell that happens to wrap one output, which is the
/// distinction the old "unknown shell" message could not make because the
/// two vocabularies never met.
///
/// Mutation check: replace `shell::check_view`'s body with the pre-I2
/// membership test (`is_fold(name) || registered.contains(&name)` alone,
/// erroring with "unknown shell") and the map half fails on the message
/// while the control still passes.
#[test]
fn a_map_shell_on_a_view_is_an_arity_error() {
    for map in crate::shell::MAP {
        let e = cfg_err(&format!(
            "[routes.feed]\npath = \"/f.xml\"\nfrom = \"blog\"\nshell = \"{map}\"\n"
        ));
        assert!(e.contains("is a map shell"), "{map}: {e}");
        assert!(e.contains("wraps ONE output"), "{map}: {e}");
        assert!(e.contains("atom, sitemap, search"), "{map}: {e}");
    }
    // The controls: every fold, and a registered script shell beside them.
    for fold in crate::shell::FOLD {
        cfg_raw(&format!(
            "[routes.feed]\npath = \"/f.xml\"\nfrom = \"blog\"\nshell = \"{fold}\"\n"
        ))
        .validate()
        .unwrap_or_else(|e| panic!("{fold} is a fold shell: {e:#}"));
    }
    cfg_raw(
        "[shells.llms]\ncommand = \"c\"\n\
             [routes.feed]\npath = \"/f.txt\"\nfrom = \"blog\"\nshell = \"llms\"\n",
    )
    .validate()
    .expect("a registered script shell is a fold");
    // And the retired spellings are hard cutoffs on this side too.
    for stale in ["none", "light"] {
        let e = cfg_err(&format!(
            "[routes.feed]\npath = \"/f.xml\"\nfrom = \"blog\"\nshell = \"{stale}\"\n"
        ));
        assert!(e.contains("unknown shell"), "{stale}: {e}");
    }
}

/// The per-member half of the arity check. An `[axes.*]` over `shell`
/// declares the serializations its members leave through, and a member is
/// one output, so the values are map shells.
///
/// This is the one path a shell reaches `build.rs` on without passing
/// through a row's cascade, which is why it needs a check of its own:
/// before I2 the axis fixture's `light` was never validated anywhere, and
/// a value outside the vocabulary rendered the fallback tier in silence.
///
/// Mutation check: delete the `a.field == "shell"` loop in `check` and both
/// halves here pass an unchecked value straight through.
#[test]
fn an_axis_over_shell_takes_map_shells_only() {
    let e = cfg_err("[axes.serialization]\nvalues = [\"html\", \"atom\"]\nfield = \"shell\"\n");
    assert!(e.contains("spends the `shell` field"), "{e}");
    assert!(e.contains("fold shell"), "{e}");
    let e = cfg_err("[axes.s]\nvalues = [\"html\", \"light\"]\nfield = \"shell\"\n");
    assert!(e.contains("not a map shell"), "{e}");
    // Controls: the map family passes, and an axis over another field is
    // none of this check's business (a theme value carries subtheme
    // tokens and would fail every shell test there is).
    cfg_raw("[axes.s]\nvalues = [\"html\", \"light_html\"]\nfield = \"shell\"\n")
        .validate()
        .expect("map shells are what a member leaves through");
    cfg_raw("[axes.t]\nvalues = [\"default\", \"ledger:dark\"]\nfield = \"theme\"\n")
        .validate()
        .expect("a theme axis is not a shell axis");
}

/// A script shell may not take a built-in's name, it would be a command
/// nobody could reach, because `check_view` answers from the built-in
/// vocabulary first.
///
/// Mutation check: delete the `check_registered_name` loop and
/// `[shells.atom]` registers a command the atom shell shadows, silently.
#[test]
fn a_script_shell_may_not_take_a_builtins_name() {
    for taken in ["atom", "sitemap", "search", "raw", "html", "light_html"] {
        let e = cfg_err(&format!("[shells.{taken}]\ncommand = \"c\"\n"));
        assert!(e.contains("is a built-in shell"), "{taken}: {e}");
    }
    cfg_raw("[shells.llms]\ncommand = \"c\"\n")
        .validate()
        .expect("a name of its own is fine");
}

/// The cost argument, asserted rather than claimed: the load path merges
/// with a recorder that is off, and an off recorder holds nothing however
/// much config goes past it.
#[test]
fn the_load_path_records_nothing() {
    let mut off = Trace::off();
    let site: toml::Value = toml::from_str("[site]\ntitle = \"Mine\"\n").unwrap();
    merge_base_traced(site, &mut off).unwrap();
    assert_eq!(off.len(), 0);
}
