//! Filesystem walk: claim files, build rows.

use super::*;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// **One walk of the site, one ordered rule sequence over it, first rule
/// wins** (IO.md I7d). Every row of every table comes from here.
///
/// Two laws decide membership, and between them they retire the hardcoded
/// posts → objects → tree precedence (DESIGN.md §3) that the two loaders were:
///
/// - **First rule wins.** A file is offered to every scope in [`scopes`]'
///   order — each reading the path its own source makes of the file, and
///   skipping the file entirely when it is not under that source — with each
///   scope's rules in its own order. The first rule past both gates claims it,
///   and the scope that rule belongs to is the row's collection. That scope's
///   rules then cascade defaults exactly as they always did: membership is
///   what the sequence answers, and everything after it is §4's per-key law
///   inside the one scope.
/// - **A scope owns its source.** When a scope whose source CONTAINS the file
///   is asked and claims nothing, the search stops there: the file is **not
///   content** and leaves the walk. It does not fall through to the scopes
///   below, which is what makes the law a law rather than an ordering. This
///   is what `store::load_dir`'s `.md`-only argument list did by accident,
///   said out loud — and it is what keeps `_drafts/caret/`'s bundle of images,
///   an `.rtf` and an `.xcf` invisible rather than letting the objects
///   catch-all and the tree's passthrough take eighteen files nobody asked
///   for. (Two scopes on one source own it jointly, so the stop waits for
///   them both; they are adjacent in the order by construction.)
///
///   **The root scope is where the second law reads differently, and
///   deliberately.** Its source is the whole site, so "the owning scope
///   claimed nothing" is not a narrowing that missed — it is a site with no
///   rule for a file, which the engine already refuses by name (*no rule
///   supplies a route*). A proper subtree IS a narrowing: `source = "_posts"`
///   with `match = "**/*.{md,markdown}"` is one statement written in two keys,
///   and a `.png` beside a draft was never being refused, it was never being
///   asked about.
///
/// The walk keeps Jekyll's dot/underscore skip, with the declared sources
/// punching through it (`store::walk_tree`).
///
/// **One constructor** (IO.md I7e). Every row this function returns is built by
/// the one block below: rule defaults, marker defaults, schema validation and
/// rung 0, then the rendering law, then routing. The objects branch that used
/// to build a binary row from `Default::default()` — no cascade, no markers, no
/// declared fields, a Null shell — is gone, and with it the last place where
/// a row's origin decided what a row could carry. What survives of
/// "object" is a fact about the FILE (the extension fact, below), which keys
/// the three things that were ever really about pictures: the objects index,
/// the name index and the header read.
pub(crate) fn walk_site(
    cfg: &Config,
    scopes: &[Scope],
    markers: &Markers,
    // Found by the declaration walk (IO.md I8). Identity from a sidecar is the
    // same fact identity from a block is — it just says nothing about the
    // row's bytes.
    sidecars: &Sidecars,
    schemas: &Schemas,
    // Compiled by `load` from the tree collection, and shared with the marker
    // and vocabulary walks so all three agree on what is not content (§4c).
    not_content: &store::NotContent,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Row>, Vec<Row>, Vec<Row>)> {
    let root = cfg.root();
    // IO.md §4a's policy, compiled once. `None` is "no subset declared" —
    // every row an `embed` rule marked — which is not the same as an empty
    // set, and is why this is an Option rather than a GlobSet that matches
    // everything.
    let embed_subset = cfg.embeds.compiled()?;
    // The sources that punch through the dot/underscore skip: a scope naming a
    // directory has declared it to be content, in the one key that means that.
    let sources: Vec<PathBuf> = scopes
        .iter()
        .filter_map(|s| s.owned())
        .map(|p| p.to_path_buf())
        .collect();
    // The one contradiction the one walk creates, refused rather than
    // suffered. `exclude` is the site saying "this is not content" (§4c) and
    // `source` is a scope saying "this is"; before I7d they governed different
    // walks and could disagree in silence, and the disagreement was even
    // harmless — the dot/underscore skip kept `_posts` out of the tree anyway,
    // so an `exclude = ["_posts/**"]` was a redundant line that did nothing.
    // With one walk it does something: it empties the blog, and nothing says
    // so. A load error naming both keys is the house answer, and the fix is to
    // delete the line.
    for s in scopes {
        let Some(src) = s.owned() else { continue };
        if !not_content.keeps_dir(src) {
            bail!(
                "collection {}: `source = {:?}` declares that directory to be \
                 content, and the tree's `exclude` takes it back out of the \
                 walk — so this scope would load nothing. There is one walk \
                 now: a declared source is content, and the dot/underscore \
                 skip already keeps it out of the tree. Delete the `exclude` \
                 entry that names it.",
                s.name,
                src.display()
            );
        }
    }
    let files = store::walk_tree(&root, not_content, cfg.gitignore, &sources)?;

    // A file claimed as a view's template is not independently routable: the
    // view owns its routes. (`blog/index.html` is rendered once per paginated
    // page; `atom.xml` is the feed.)
    let templates: Vec<PathBuf> = cfg
        .views
        .values()
        .filter_map(|v| v.template.as_ref())
        .map(PathBuf::from)
        .collect();
    let files: Vec<_> = files
        .into_iter()
        .filter(|f| !templates.contains(&f.rel))
        // A marker declares defaults; it is not itself content.
        .filter(|f| !markers.is_marker(&f.path))
        // Nor is a sidecar (IO.md I8), for the same reason and by the same
        // kind of test: it declares the identity of the file beside it. The
        // set comes from the declaration walk, so what makes a `.toml` a
        // declaration is the file it sits beside — not its name, which is why
        // an ordinary `netlify.toml` is still published like any other file.
        .filter(|f| !sidecars.is_sidecar(&f.rel))
        // Nor is the config that declared all of this. Matched by identity,
        // not by glob, so a site needs no `exclude` entry to avoid
        // publishing its own grackle.toml.
        .filter(|f| {
            std::fs::canonicalize(&f.path)
                .map(|p| p != cfg.config_file)
                .unwrap_or(true)
        })
        // Nor are theme SOURCES (IO.md I7b). A site-root `themes/` is engine
        // vocabulary by POSITION, the class `.slots/`, `.section` and
        // `.schema.toml` already occupy: the build reads themes from exactly
        // one place (`root.join("themes")`), so what sits there is input to
        // the build in the same sense the config file is, and publishing a
        // theme's `root.html` at `/themes/mine/root.html` is the same
        // accident as publishing `grackle.toml`.
        //
        // `include` stays the escape hatch — asked the way `NotContent::keeps`
        // asks it, so a site that deliberately publishes something underneath
        // says so in the one key that already means that.
        .filter(|f| not_content.included(&f.rel) || !under_themes(&f.rel))
        .collect();

    // q45: rows named by a view's `content` — claimed landings. Matched
    // by logical identity so every file-axis twin is claimed with its
    // original.
    let claims = cfg.content_claims();

    // **The extension fact** (IO.md §7: "name index and dimensions keyed off
    // extension") — the objects scopes' globs, asked on their own. It is I7a's
    // `is_obj`, unchanged in what it computes, and since I7e it is a fact about
    // the FILE that stands beside the sequence rather than a preview of it:
    // the sequence answers which scope claims a row, and this answers whether
    // the row is a picture. Three readers, none of which the sequence can
    // serve:
    //
    //   - the PEEK. Whether a file was peeked is what the front-matter gate
    //     reads, so it cannot itself be gated. Skipping the ~800 binaries is
    //     what keeps the peek off the build's critical path.
    //   - the i18n file axis. An image is shared across members (§6f), so an
    //     objects rule does not spend that axis in `file`. Pinned in
    //     both directions by `io_dissolve.rs` — a `photo.fr.png` keeps its
    //     literal name while a `notes.fr.md` beside it is the French variant.
    //   - the OBJECTS index (I7e). `object_ix`, `by_name` and the header read
    //     key off this rather than off which scope claimed the row, which is
    //     what lets the row constructor be one constructor: a former-object
    //     row is built like every other row and is an object because of what
    //     it IS.
    //
    // The three agree with the sequence on every corpus site because no objects
    // rule of any site gates on front matter — one that did would take the
    // glob's answer here and the gate's answer there, and its rows would be
    // indexed as images while belonging to whichever scope claimed them, which
    // is the honest reading of an index keyed off extension. Stated rather than
    // guarded (I7a recorded the same shape: such a rule claimed nothing before
    // either).
    //
    // Bare matchers rather than the rules themselves: this closure runs inside
    // the parallel peek, and a `CompiledRule` carries the `Cell`s the walk
    // writes.
    let obj_globs: Vec<&GlobMatcher> = scopes
        .iter()
        .filter(|s| s.source.is_none())
        .flat_map(|s| s.rules.iter().map(|r| &r.matcher))
        .collect();
    let is_obj = |rel: &Path| obj_globs.iter().any(|m| m.is_match(rel));

    // Only text rows can carry front matter, and only non-objects need the
    // page/static decision — so skip the peek for the ~800 binaries and run the
    // rest in parallel. (Sequential-over-everything cost ~140ms.)
    let mut files = files;
    files.par_iter_mut().for_each(|f| {
        if !is_obj(&f.rel) {
            f.has_front_matter = store::peek_front_matter(&f.path);
        }
    });

    let mut posts: Vec<Row> = Vec::new();
    let mut pages: Vec<Row> = Vec::new();
    let mut objects: Vec<Row> = Vec::new();

    for f in files {
        // The extension fact (above), asked once per file and read twice: by
        // the peek that already ran, and by the partition at the bottom of
        // this loop. Locale is not special here — a rule's `file` patterns
        // either spend the i18n file axis or they don't. An objects rule that
        // does not leaves `photo.fr.png` as a literal name (one picture
        // serves every i18n member; `io_dissolve.rs` pins both halves).
        let object_shaped = is_obj(&f.rel);

        // **Identity: a block, or a sidecar** (IO.md §1, I8). The two are peers
        // — neither is nearer than the other, and there is no ladder between
        // them — so a file carrying both is a load error rather than a
        // precedence rule nobody could predict. It is the marker-collision
        // shape (MERGE.md A5) and it gets the same answer: an unrankable
        // disagreement is refused, and the fix is to write one of them.
        let sidecar = sidecars.get(&f.rel);
        if sidecar.is_some() && f.has_front_matter {
            bail!(
                "{}: identity twice — the file carries a front-matter block and \
                 {}.toml is a sidecar for it. A sidecar exists for files that \
                 CANNOT carry a block; nothing ranks the two, so pick one: \
                 delete the sidecar, or delete the block.",
                f.rel.display(),
                f.rel.display()
            );
        }
        // The fact §3's table names, widened by I8 without changing its
        // meaning. What it does NOT widen is `renders` below: a block is IN
        // the file and so says the file is a document, while a sidecar says
        // only that someone wrote a row's fields down somewhere.
        let has_identity = f.has_front_matter || sidecar.is_some();

        // **The ordered rule sequence, first rule wins.** Every scope in turn,
        // each reading the path its own source makes of the file, until one of
        // their rules claims it.
        let mut claim: Option<(&Scope, Routing, PathBuf)> = None;
        // Set when a scope that OWNS this path has been asked and passed. What
        // it means is "not content" — see this function's doc — and the loop
        // keeps going only for the scopes that share that same source, because
        // two scopes on one source own it jointly. Nothing below them looks.
        let mut owner_passed: Option<&Path> = None;
        for s in scopes {
            if let Some(o) = owner_passed {
                if s.owned() != Some(o) {
                    break;
                }
            }
            // Rule globs and route tokens read the path this scope's source
            // makes of it — collection-relative in `_posts`, root-relative in
            // the tree — so a rule's `match` and its `route` spell the same
            // words (IO.md I6). `None` is a file outside this scope's source,
            // which the scope never sees.
            let Some(scope_rel) = s.relative(&f.rel) else {
                continue;
            };
            // Offered: this scope's source contains the file and the sequence
            // reached it, so its rules are about to be asked. Counted here
            // rather than from the file list because "under the source" and
            // "asked" are the same event only at this line — a nearer scope
            // that claimed first, or an owner that already stopped the search,
            // means the scopes below never saw the file at all.
            s.offered.set(s.offered.get() + 1);
            // Match on the logical path: `file` patterns strip spent axes
            // (suffix or prefix) before the glob sees the path.
            let r = apply_rules(&s.rules, &s.formats, &scope_rel, has_identity);
            if r.claimed.is_some() {
                claim = Some((s, r, scope_rel));
                break;
            }
            if let Some(o) = s.owned() {
                owner_passed = Some(o);
            }
        }
        let Some((scope, routing, scope_rel)) = claim else {
            // A scope owns its source: what its rules did not claim is not
            // content, and it leaves without a word — that silence is
            // `load_dir`'s extension filter, which never said anything either.
            // Under the root scope there is no narrowing to have missed, so
            // the site is simply missing a rule, which is the error it was.
            if owner_passed.is_some() {
                continue;
            }
            bail!("no rule supplies a route for {}", f.path.display());
        };
        let extracted = filename::extract(routing.formats, &path_key(&scope_rel));
        let pairing = cfg.pairing_axis();
        let pairing_default = pairing
            .and_then(|(_, a)| a.canonical().map(str::to_owned))
            .unwrap_or_default();
        let (logical_rel, pairing_value) = match &extracted {
            Some(m) => {
                let value = pairing
                    .and_then(|(n, _)| m.axes.get(n).cloned())
                    .unwrap_or_else(|| pairing_default.clone());
                (with_logical(&scope_rel, &m.logical_stem), value)
            }
            None => (scope_rel.clone(), pairing_default.clone()),
        };
        // Root-relative again for everything that is about the FILE rather
        // than about a rule: schema governance, `logical` identity, the claim.
        let logical_root: PathBuf = match scope.owned() {
            Some(src) => src.join(&logical_rel),
            None => logical_rel.clone(),
        };
        scope.found.set(scope.found.get() + 1);
        check_on_demand_cover(&logical_rel, &routing)?;
        let on_demand = routing.on_demand;
        let marker_defaults = markers.defaults_for(&f.rel);
        let defaults = merged_defaults(&marker_defaults, routing.defaults);

        // STORED rather than re-derived later: recomputing the stem from
        // `logical` via `file_stem()` returns `v1` for `v1.2-release.md`.
        let stem = logical_rel
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let key = extracted.as_ref().map(|m| m.key.clone());
        let from_name = match key.as_ref().and_then(|k| k.ymd()) {
            Some((y, m, d)) => Some(NaiveDate::from_ymd_opt(y, m, d).with_context(|| {
                format!(
                    "{} has an impossible date in its filename",
                    f.path.display()
                )
            })?),
            None => None,
        };
        let slug = key
            .as_ref()
            .and_then(|k| k.slug.clone())
            .unwrap_or_else(|| stem.clone());

        // Front matter, read once. The posts loader read every file whole and
        // the tree loader read only the front-mattered ones; the one walk reads
        // a file that has a block to parse, and reads a blockless one below
        // only if it turns out to render.
        // A sidecar is the same struct read from the other file (IO.md I8), so
        // everything below this line — the title rung, `permalink:`, the date
        // precedence, the declared fields — is one code path for both
        // spellings of identity. A sidecar'd row's `body_bytes` is 0 until the
        // rendering law says otherwise, because the sidecar is not the row's
        // body and the row's own bytes have not been read.
        let (fm, mut body_bytes) = match (f.has_front_matter, sidecar) {
            (true, _) => read_front_matter(&f.path)?,
            (false, Some(sc)) => (sc.front.clone(), 0),
            (false, None) => Default::default(),
        };
        // §5b: a governed row's extra front matter is validated — an undeclared
        // key or wrong type fails the load naming the file. Ungoverned rows
        // stay as tolerant as they always were. Governance follows the LOGICAL
        // path (§6f): a translation is governed by the same `.schema.toml` as
        // its original — and the ROOT-relative one, because that walk keys them
        // root-relative (a `_posts/.schema.toml` is registered under `_posts`,
        // and resolving a bare filename would never find it).
        let parent = logical_root.parent().unwrap_or(Path::new("")).to_path_buf();
        // Every row is governed (§4e): declare a field before you use it.
        let schema = schemas.resolve(scope.name, &parent);
        // Identity's own errors name the file identity was WRITTEN in, which
        // is the file the author has to edit: the row for a block, the sidecar
        // for a sidecar. Every rung below (markers, rules, the profile) keeps
        // naming the row, because that is what those rungs are about.
        let identity_path = sidecar.map_or(f.path.as_path(), |sc| sc.path.as_path());
        let mut checked = match has_identity {
            true => schema::validate(&schema, &fm.extra, identity_path)?,
            false => Default::default(),
        };
        // The engine's own four arrive on named front-matter fields rather
        // than in `extra`, so they are seeded here — nearest writer first.
        schema::cascade_front(&schema, &fm, &mut checked, identity_path)?;
        // Markers and rules fill whatever front matter left unset (§4b).
        schema::apply_defaults(&schema, &defaults, &mut checked, &f.path)?;
        // …and rung 0 overrules all three (§2, MERGE.md E1). Above `cascade`,
        // so a forced `theme` or `toc` is what the row wears.
        schema::force(&cfg.forced, &schema, &mut checked, &f.path)?;
        let worn = cascade(&checked, &f.rel)?;

        // The law (IO.md I7c), asked once now that there is one walk to ask it
        // in. `rendered: true` stood in the posts loader and
        // `rendered: has_front_matter` in the tree's — each the config's answer
        // read off the wrong thing.
        //
        // **It reads the BLOCK, not identity** (IO.md I8), and that is the
        // whole of what a sidecar splits: a block is in the file, so a file
        // with one is a document whose remainder is a body; a sidecar is a
        // second file, and says nothing about the first one's bytes. So a
        // sidecar'd `.png` answers `front_mattered true` and `rendered false`,
        // which is §3's sentence — "a `.png` with a sidecar is a governed row
        // whose bytes are never parsed" — as two columns.
        let rendered = crate::shell::renders(f.has_front_matter, worn.shell.as_deref());
        // **A picture is not a document, and the description page is not
        // built.** IO.md §4a says an image with a sidecar *can* wear an html
        // output — the object's description page. It needs an output whose
        // content is not the row's bytes, which the model has (facts at
        // planning, content at materialization) and the engine does not.
        //
        // **No item owns it.** I8 wrote "one line to delete when I11/I12
        // lands"; both landed and neither built it — I11 gave an input a
        // second ADDRESS and I12 gave it derived BYTES, and a description page
        // is neither: it is an output whose content is a rendered template
        // over the row's fields. Nothing in the ledger is going to build that,
        // so this refusal is not an interim, and the honest thing for the next
        // reader is a shape rather than an item number: the day something
        // materializes an html output from a row's FIELDS instead of its
        // bytes, this check is what has to move. Until then the shape is
        // refused where the author wrote it. Deleting the check does not make
        // the page work — measured: the render path reads the row's file as
        // text and the load dies on `stream did not contain valid UTF-8`,
        // naming a file and no reason.
        if rendered && object_shaped {
            bail!(
                "{}: shell = {:?} would render this file as a document, and its \
                 bytes are a picture. An image's own outputs are its bytes (IO.md \
                 §4a); a description PAGE for one is a second output and is not \
                 built yet. Route it `raw`{}.",
                f.rel.display(),
                worn.shell.as_deref().unwrap_or("-"),
                match sidecar.is_some() {
                    true =>
                        " — the sidecar still gives it a title, fields and a \
                             place in the link graph",
                    false => "",
                }
            );
        }
        // A blockless row that renders is the one shape whose body was not
        // already in hand: it is ALL body. The posts loader read every post
        // whole and so had it; the tree loader read nothing and reported zero;
        // one walk has to pick one answer, and `body_bytes` is a fact about
        // the row rather than about which loader found it.
        if rendered && !f.has_front_matter {
            body_bytes = read_front_matter(&f.path)?.1;
        }
        // Front matter beats the filename, the precedence every other field
        // has (§4b) — and it is read ABOVE routing now, so a `date:` reaches a
        // dated route template on any row rather than only on a post. That is
        // I6's recorded other half of "one supplier", and the seam it named.
        let date = match &fm.date {
            Some(s) => Some(front_matter_date(s, identity_path)?),
            None => from_name,
        };
        // The engine-fallback rung, below front matter and every default
        // (§4b). A row that is not a document has no title to imply: its
        // content is its bytes.
        let title = match (fm.title, rendered) {
            (Some(t), _) => Some(t),
            (None, true) => Some(implied_title(&slug)),
            (None, false) => None,
        };
        // A degenerate row carries no front matter, so its title IS the
        // implied one — the warning states the derivation rather than reading
        // back a value it would have to prove is there.
        //
        // This one asks IDENTITY, where `renders` above asks the block: the
        // warning exists to nudge an unnamed row towards a name, and a sidecar
        // is a name. Two questions, two inputs, one shell (IO.md I8).
        if let Some(sh) = crate::shell::degenerate(has_identity, worn.shell.as_deref()) {
            warnings.push(degenerate_warning(&f.rel, sh, &implied_title(&slug)));
        }

        // The embed policy's half of the address question (IO.md §4a, I11).
        // A rule said `embed = true`, so this row has NO canonical URL; what it
        // has instead is the hash address the policy publishes, computed here —
        // at planning, from the input bytes and the identity transform's
        // parameters — because §1's law says an output's facts exist before
        // anything renders and this address is one of them.
        //
        // Below the `permalink` branch on purpose: front matter beats a rule
        // (§4b), and a `permalink:` is a canonical address written by hand, so
        // a row that carries one is routed however its rule was written.
        let mut strong_url: Option<String> = None;
        if routing.embed && fm.permalink.is_none() {
            strong_url = Some(embed_address(cfg, &embed_subset, &f, routing.pattern)?);
        }
        // A `permalink` is a literal URL, spending no axis; otherwise each of
        // the rule's template(s) is rendered by the one supplier — path tokens,
        // the extractor's, axis placeholders preserved for
        // per-member selection.
        let route_templates: Vec<String> = if let Some(p) = &fm.permalink {
            vec![p.clone()]
        } else if strong_url.is_some() {
            Vec::new()
        } else {
            if routing.templates.is_empty() {
                bail!("no rule supplies a route for {}", f.path.display());
            }
            RouteTokens {
                cfg,
                rel: &logical_rel,
                path: &f.path,
                hash: Default::default(),
                date,
                key: key.as_ref(),
                slug: &slug,
            }
            .render_all(routing.templates, routing.pattern, &f.path)?
        };
        // Empty for an embed-addressed row, and that is the shape the two
        // slots take today: `url` is the canonical address and this row has
        // none, so every reader that asks "where does this land canonically"
        // gets the honest answer rather than a hash dressed as a route.
        let url = match route_templates.is_empty() {
            true => String::new(),
            false => canonical_url(cfg, &route_templates, &pairing_value, routing.formats)?,
        };

        let logical = logical_root.to_string_lossy().to_string();
        // q45: a row named by some view's `content` is claimed — every logical twin
        // variant of it (the claim is on the logical identity).
        let claimed = claims.contains_key(logical.as_str());
        if claimed && !f.has_front_matter {
            bail!(
                "view {}: content {logical:?} has no front-matter block, so it \
                 is a static file, not a claimable row",
                claims[logical.as_str()]
            );
        }
        let row = Row {
            axis: row_axes(cfg, &route_templates, routing.formats),
            route_templates,
            width: None,
            height: None,
            // Assigned by `insert_rows`, which is where rows become the
            // database's rather than the loader's.
            key: Default::default(),
            on_demand,
            collection: scope.name.to_string(),
            // Which rule of which scope claimed this row: the ordering law's
            // one observable (IO.md I7d), printed by `grackle explain`.
            rule: routing.claimed.map(str::to_string),
            slug,
            stem,
            body_bytes,
            path: f.path,
            // ROOT-relative, so `path`/`dir` mean one thing on every row. Rule
            // globs match the SCOPE-relative form: `match = "hidden/**"` is
            // relative to `_posts`.
            rel: f.rel,
            // A row's identity may live in a second file, so its change stamp
            // has to (IO.md I8). Without the fold, editing a sidecar changes a
            // row's title and nothing notices: `version` is what the
            // incremental machinery compares.
            version: match sidecar {
                Some(sc) => f.version ^ sc.version,
                None => f.version,
            },
            url,
            strong_url,
            rendered,
            // The tree's old page/static gate IS this fact, and since I7c it is
            // no longer the whole of the gate: the fact is one clause of the
            // law above, which the shell can also satisfy. Since I8 it is also
            // no longer the same question as "does this file open with `---`" —
            // a sidecar answers it too, and `sidecar` says which.
            front_mattered: has_identity,
            sidecar: sidecar.is_some(),
            size: f.size,
            title,
            order: fm.order,
            date,
            theme: worn.theme,
            shell: worn.shell,
            fields: {
                // §6f: stamp file-axis fields after schema validation so the
                // filename wins over any front-matter for those axes — the
                // file is which member the row IS.
                let mut values = checked.values;
                if let Some(m) = &extracted {
                    for (axis_name, value) in &m.axes {
                        if let Some(axis) = cfg.axes.get(axis_name) {
                            values.insert(
                                axis.field.clone(),
                                grackle_db::Value::Str(value.clone()),
                            );
                        }
                    }
                }
                if let Some((_, axis)) = pairing {
                    values
                        .entry(axis.field.clone())
                        .or_insert_with(|| grackle_db::Value::Str(pairing_value.clone()));
                }
                values
            },
            images: checked.images,
            logical,
            claimed,
            // IO.md §2's join: filled by `join_outputs`/`join_arrangement`
            // once the routes exist. Not derivable here — a row cannot say
            // whether it lands until the route table says so.
            output: None,
            alternates: Vec::new(),
            viewed_by: Vec::new(),
        };
        // **The partition, keyed off the fact** (IO.md I7e). The three vectors
        // are the three key lists (`post_ix`, `page_ix`, `object_ix`) and
        // nothing else — there is one constructor above them, so which one a
        // row joins is a question about the row rather than about how it was
        // built. A posts scope's rows are its own by role; an image is an image
        // because the extension fact says so, whichever scope claimed it. A
        // posts scope is the one that OWNS a proper source — the role read off
        // `source` now that `kind` is gone.
        if scope.owned().is_some() {
            posts.push(row);
        } else if object_shaped {
            objects.push(row);
        } else {
            pages.push(row);
        }
    }

    // Every claim must have found its row — a typo'd content path is a
    // load error naming the view, not a silently bare landing.
    for (path, view) in &claims {
        if !pages
            .iter()
            .chain(posts.iter())
            .any(|p| p.claimed && p.logical == *path)
        {
            bail!("view {view}: content {path:?} names no row in the tree");
        }
    }
    // Dimensions are a property of the FILE, so they belong on the row where
    // a query can reach them rather than in a build-time side map. One header
    // read each, in parallel — sequentially this is ~200ms on a corpus with
    // 850 images, which is a third of the whole build. Keyed off the extension
    // fact like the two indexes (IO.md I7e): the reason to open a file looking
    // for a width is that it is a picture.
    objects.par_iter_mut().for_each(|o| {
        if let Ok((w, h)) = image::image_dimensions(&o.path) {
            o.width = Some(w);
            o.height = Some(h);
        }
    });

    for s in scopes {
        warnings.extend(dead_rules(s.name, &s.rules, s.found.get()));
        // The other side of `dead_rules`' `found == 0` (IO.md IR8): where it
        // falls silent because a whole scope came up empty, this asks whether
        // the source was empty or the globs were wrong.
        warnings.extend(empty_source(s));
    }
    Ok((posts, pages, objects))
}

/// Front matter of one row, and the size of its body.
///
/// A parse failure is a LOAD ERROR naming the file, never an empty schema —
/// an unquoted `title: A: B` must not ship a silently titleless page (§4). An
/// EMPTY block is not a failure, though: `---\n---` is a file that carries
/// identity and says nothing with it, which is what the posts loader always
/// read it as, and one walk keeps the more permissive of the two readings
/// because the other one was an accident of never being asked.
///
/// `body_bytes` comes from the same read, so the field means the same thing on
/// every row.
pub(crate) fn read_front_matter(path: &Path) -> Result<(store::FrontMatter, usize)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (yaml, body) = store::split_front_matter(&text);
    let fm = match yaml.trim().is_empty() {
        true => store::FrontMatter::default(),
        false => serde_yaml_ng::from_str(yaml)
            .with_context(|| format!("front matter of {}", path.display()))?,
    };
    Ok((fm, body.len()))
}

/// Read the site named by `cfg` and return the database it describes.
/// An image field names a ROW, so check that it does (§5b × §6a).
///
/// `cover: books/covers/x.png` is a foreign key: an objects collection already
/// claimed that file, and its row key IS that path. Nothing said so, so a typo
/// shipped a broken `<img>` — the same silent 404 that strict link policy
/// exists to prevent for prose links. Runs after `insert_rows`, because the
/// row it names may load after the row that names it.
///
/// An ABSOLUTE url is left alone: it names something outside the site, which
/// no row can vouch for.
pub(crate) fn resolve_image_fields(db: &SiteDb, schemas: &Schemas) -> Result<()> {
    for row in db.rows.iter() {
        let dir = row.rel.parent().unwrap_or(Path::new("")).to_path_buf();
        let declared = schemas.resolve(&row.collection, &dir);
        for (name, ty) in &declared {
            if *ty != crate::schema::FieldType::Image {
                continue;
            }
            let Some(grackle_db::Value::Str(target)) = row.fields.get(*name) else {
                continue;
            };
            if target.contains("://") || target.starts_with("//") {
                continue; // outside the site; no row to check it against
            }
            if db
                .rows
                .get(&grackle_db::Key::new(target.as_str()))
                .is_none()
            {
                anyhow::bail!(
                    "{}: field `{name}` names {target:?}, which is not a file this site \
                     loads. An image field is a reference to a row — check the path, and \
                     that an objects collection claims that extension.",
                    row.rel.display()
                );
            }
        }
    }
    Ok(())
}
