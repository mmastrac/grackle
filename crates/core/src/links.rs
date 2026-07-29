//! Row links and view links (§6a, Matt's rule): authored links reference
//! what the database OWNS — a row by its source path, a view by name —
//! because final URLs are derived values (locale prefixes, slugs, route
//! templates). The engine renders the URL, exactly as it does for chrome.
//!
//! Resolution, per markdown link destination:
//! - `view:name` / `view:name/key…` → the view's route template rendered
//!   with the keys (tag slugs applied), locale-aware, verified against the
//!   materialized route set — a typo'd key errors LISTING the keys.
//! - a source path (relative to the linking file, or root-relative) → the
//!   row's URL. Unknown source = error with a closest-match suggestion.
//! - a raw internal URL: `loose` leaves it (the legacy-corpus posture);
//!   `strict` errors, suggesting the correct source/`view:` form.
//!   External schemes, fragments and mailto pass through untouched.

use anyhow::{bail, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use crate::config::{Config, LinkPolicy};
use crate::model::SiteDb;
// The materializer's own path selection (§6f, the default-axis case). Shared
// rather than restated: a link that builds a URL by a rule of its own is a link
// that can name a URL the build never issued.
use grackle_source::load::{select_path, spends, Coord};

/// Which FORM of citation is asking (IO.md §4a, I11).
///
/// The design's sentence — *three URLs, three jobs, each citation form knowing
/// its address kind* — as a parameter. An authored **link** is a promise of a
/// bookmarkable address, so it demands a route and errors without one. An
/// **embed** is markup the browser fetches, so it takes the strong address
/// when the target has no route, and is not held to the link policy at all:
/// half the `src`s on a finished page are engine-derived (`/static/` thumbs,
/// the stylesheet) and were never anybody's source path.
/// Link vs embed. Defined in [`grackle_model::Cite`]; re-exported here because
/// resolution policy lives in this module.
pub use grackle_model::Cite;

/// Everything link resolution needs, computed once per build.
pub struct LinkSpace {
    /// Root-relative source path → the row's URL (posts, pages, objects).
    source_to_url: HashMap<String, String>,
    /// Every materialized URL.
    routes: HashSet<String>,
    /// URL → the form a strict-mode error suggests instead.
    url_form: HashMap<String, String>,
    /// q53: source path → the axes the row's rule spends. Read for one question
    /// only — *does this row's rule spend that axis at all* — which is what
    /// separates "you selected a member of an axis this row is not on" from
    /// "that member did not materialize". The member's URL comes from
    /// `member_url`, never from a template — which is why `RowAxis` carries a
    /// name and nothing else (MERGE.md F3).
    source_to_axis: HashMap<String, Vec<grackle_model::RowAxis>>,
    /// q53: (source path, axis, value) → the URL that route ACTUALLY landed at,
    /// for the routes whose every other axis sits at its canonical — which is
    /// what a one-axis selector means.
    ///
    /// A lookup rather than a reconstruction, and that is the whole point
    /// (MERGE.md C5c). Rebuilding the URL meant picking a template — the first
    /// of the rule's list, an arbitrary one — and then blaming the
    /// rule when the guess missed, while `select_path` had chosen a different
    /// template for that very member. DESIGN.md q32's law is that producers take
    /// URLs from the owning view rather than construct them; the materialized
    /// route is the owner here, and it cannot lie.
    member_url: HashMap<(String, String, String), String>,
    /// C5(d): links whose query key looks like an axis selector but names no
    /// declared axis. Collected rather than raised: unknown keys stay literal by
    /// design (`?utm=x`), so this is a warning, and `resolve` runs inside rayon's
    /// render passes with nothing mutable in reach. Sorted and deduped, because
    /// one bad link in a template-shared fragment would otherwise say it once
    /// per page.
    warnings: Mutex<BTreeSet<String>>,
    /// q53 locale axis: source path → the row's logical identity, and
    /// (logical, locale) → URL. A `?locale=` selector picks a SIBLING row — a
    /// different file, not a restyle of the same one — so it resolves through
    /// `by_logical`, not through a template. This is the file-axis half of the
    /// abstraction: a member may add its own content file.
    source_to_logical: HashMap<String, String>,
    sibling: HashMap<(String, String), String>,
    /// IO.md §4a: root-relative source path → the row's STRONG address, for
    /// the rows a rule declined to route (`embed = true`).
    ///
    /// A second map rather than a widening of `source_to_url`, because the two
    /// answer different questions and exactly one citation form may read each:
    /// an embed resolves here, and an authored link resolving here is the
    /// error that says a bookmarkable address was never declared. Merging them
    /// would have made a link to an unrouted asset silently ship a hash URL,
    /// which is the address kind that exists precisely because it is nobody's
    /// to write down.
    source_to_strong: HashMap<String, String>,
}

impl LinkSpace {
    /// The strong address of a root-relative source path, for the generated
    /// affordances that are not markup anyone authored (IO.md §4a).
    ///
    /// `{% image %}` is the one caller today: its `<a href>` is an expansion
    /// affordance rather than an authored link, so it takes the strong address
    /// where an authored link to the same row would be refused. Same map, and
    /// the difference is entirely which citation form is asking.
    pub fn strong_of_source(&self, rel: &str) -> Option<&str> {
        self.source_to_strong.get(rel).map(String::as_str)
    }

    /// Does this URL name a materialized route? The raw-HTML seam (§6d stage
    /// B) asks, because it meets engine-derived URLs it must not police.
    pub fn is_route(&self, url: &str) -> bool {
        self.routes.contains(url)
    }

    /// Drain the C5(d) warnings the render passes accumulated. Called once, by
    /// `render_site`, after every body is resolved.
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut *self.warnings.lock().unwrap())
            .into_iter()
            .collect()
    }

    fn warn(&self, w: String) {
        self.warnings.lock().unwrap().insert(w);
    }

    pub fn new(_cfg: &Config, db: &SiteDb, root: &Path) -> LinkSpace {
        let mut source_to_url = HashMap::new();
        let mut source_to_axis: HashMap<String, Vec<grackle_model::RowAxis>> = HashMap::new();
        let mut source_to_logical = HashMap::new();
        let mut sibling = HashMap::new();
        let mut source_to_strong = HashMap::new();
        // A row that publishes on demand (§4) is a legal link target even
        // though nothing has materialized it yet: the question a link asks is
        // whether the target is PUBLISHABLE, not whether someone else already
        // cited it. Its URL comes from the same rule template either way.
        for p in db.posts().chain(db.pages()).chain(db.objects()) {
            // IO.md §4a: an embed-addressed row has no canonical URL at all,
            // and its strong address goes in the other map — which is what
            // makes a LINK to it an error and an EMBED of it resolve.
            if let Some(strong) = &p.strong_url {
                source_to_strong.insert(p.rel.to_string_lossy().to_string(), strong.clone());
                continue;
            }
            // q45: a claimed locale variant whose partition never
            // materialized has no URL; offering it would rewrite links
            // to "".
            if p.url.is_empty() {
                continue;
            }
            let rel = p.rel.to_string_lossy().to_string();
            source_to_url.insert(rel.clone(), p.url.clone());
            if !p.axis.is_empty() {
                source_to_axis.insert(rel.clone(), p.axis.clone());
            }
            source_to_logical.insert(rel, p.logical.clone());
            sibling.insert((p.logical.clone(), p.locale().to_owned()), p.url.clone());
        }
        let mut routes = HashSet::new();
        let mut url_form = HashMap::new();
        let mut member_url: HashMap<(String, String, String), String> = HashMap::new();
        // §4: `route || row.on_demand`. An on-demand row has no Route yet, so
        // taking the route set alone would call every asset link dangling —
        // and the link is precisely what will materialize it.
        for p in db.rows.iter().filter(|p| p.on_demand && !p.url.is_empty()) {
            routes.insert(p.url.clone());
        }
        for r in &db.routes {
            routes.insert(r.url.clone());
            // A view route names itself by its view (and group key); every
            // other output names itself by its source path. "Is this a view
            // route" is the `view` column being non-empty (IO.md §3, I13) —
            // which is also the value this arm needs, so matching on the
            // column binds it instead of asking `kind` and then unwrapping.
            let form = match &r.view {
                Some(v) => Some(match &r.key {
                    Some(k) => format!("view:{v}/{k}"),
                    None => format!("view:{v}"),
                }),
                None => r.source.as_ref().and_then(|s| {
                    s.strip_prefix(root)
                        .ok()
                        .map(|rel| format!("/{}", rel.to_string_lossy()))
                }),
            };
            if let Some(f) = form {
                url_form.insert(r.url.clone(), f);
            }
            // q53: index this route under each axis it is a non-default member
            // of, but only while every OTHER axis of the tuple sits at its
            // canonical — a `?theme=ledger` link names one member along one axis
            // and leaves the rest alone, so that is the only tuple it can mean.
            if r.axis.is_empty() {
                continue;
            }
            let Some(rel) = r
                .row
                .as_ref()
                .and_then(|k| db.rows.get(k))
                .map(|p| p.rel.to_string_lossy().to_string())
            else {
                continue;
            };
            for m in &r.axis {
                if r.axis.iter().any(|o| o.axis != m.axis && !o.canonical) {
                    continue;
                }
                member_url.insert(
                    (rel.clone(), m.axis.clone(), m.value.clone()),
                    r.url.clone(),
                );
            }
        }
        LinkSpace {
            source_to_url,
            routes,
            url_form,
            source_to_axis,
            member_url,
            source_to_logical,
            sibling,
            source_to_strong,
            warnings: Mutex::new(BTreeSet::new()),
        }
    }
}

/// Normalize `.`/`..` without touching the filesystem.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The closest source path to a miss, for the error's suggestion: same
/// file name first, then same stem anywhere in the path.
fn closest_source<'a>(space: &'a LinkSpace, wanted: &str) -> Option<&'a str> {
    let name = Path::new(wanted).file_name()?.to_string_lossy().to_string();
    let stem = Path::new(wanted).file_stem()?.to_string_lossy().to_string();
    space
        .source_to_url
        .keys()
        .find(|k| Path::new(k).file_name().is_some_and(|f| *f == *name))
        .or_else(|| space.source_to_url.keys().find(|k| k.contains(&stem)))
        .map(String::as_str)
}

/// C5(d): does an unrecognized link query key *look like* an axis selector?
///
/// A warning and never an error, deliberately. Only a declared axis name is read
/// as a selector (q53), so `?utm=x` must keep meaning what it has always meant —
/// which is exactly what makes a typo silent, since a misspelled axis is
/// indistinguishable from an ordinary query string except by intent. The three
/// legible intents: the right name in the wrong case (`?Theme=`), a name one or
/// two edits away (`?thmee=`), and the axis's FIELD in place of the axis
/// (`[axes.look] field = "theme"` selected as `?theme=`). Nothing else warns.
///
/// A pure function, so the sentence is testable without a site — `slots.rs`'s
/// `unknown_stems` is the precedent.
pub fn misspelled_axis(cfg: &Config, source: &str, href: &str, key: &str) -> Option<String> {
    // `locale` is an axis only where i18n is on; elsewhere it is an ordinary
    // query key and must not be suggested.
    let names: Vec<&str> = cfg
        .axes
        .keys()
        .map(String::as_str)
        .chain(cfg.locale_enabled().then_some("locale"))
        .collect();
    if names.contains(&key) {
        return None; // a selector, not a miss — the caller already handled it
    }
    let say = |hint: String| {
        format!(
            "{source}: link {href:?} — {key:?} names no axis, so it ships as a literal \
             query string{hint}\n  declared axes: {}",
            if names.is_empty() {
                "(none)".to_string()
            } else {
                names.join(", ")
            }
        )
    };
    // Case first: an exact match but for case is the one wrong spelling an
    // author is staring at, the way `.slots/Nav.md` is (C4b).
    let lowered = key.to_ascii_lowercase();
    if let Some(n) = names.iter().find(|n| n.to_ascii_lowercase() == lowered) {
        return Some(say(format!(
            " (did you mean `?{n}=`? axis names are matched exactly, so case counts)"
        )));
    }
    // Then the field: an axis sets a named row field, and the two are commonly
    // the same word (`[axes.theme] field = "theme"`), which makes selecting by
    // the field a natural mistake when they differ.
    if let Some((n, _)) = cfg
        .axes
        .iter()
        .find(|(_, a)| a.field.eq_ignore_ascii_case(key))
    {
        return Some(say(format!(
            " ({key:?} is the FIELD the {n:?} axis sets — select the axis: `?{n}=`)"
        )));
    }
    // Then distance, at `filter.rs`'s threshold — and the edit must be SMALLER
    // than the shorter of the two words, or a short deliberate key (`?id=`)
    // matches a short axis name while sharing nothing with it.
    names
        .iter()
        .map(|n| (crate::filter::levenshtein(key, n), *n))
        .filter(|(d, n)| *d <= 2 && *d < key.len().min(n.len()))
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| say(format!(" (did you mean `?{n}=`?)")))
}

#[allow(clippy::too_many_arguments)]
/// Resolve one markdown link destination. `Ok(None)` = leave untouched.
///
/// `url_dir` is the linking PAGE's URL directory: when a relative source
/// link resolves to the same URL the browser would reach anyway, the href
/// is left byte-identical — the engine rewrites only where the browser
/// would get it wrong (`.md` links, source-dir ≠ url-dir references).
pub fn resolve(
    cfg: &Config,
    space: &LinkSpace,
    linking_dir: &Path,
    url_dir: &str,
    locale: &str,
    source: &str,
    form: Cite,
    href: &str,
) -> Result<Option<String>> {
    // Not ours: external schemes, in-page fragments, protocol-relative.
    if href.is_empty()
        || href.starts_with('#')
        || href.starts_with("//")
        || href.contains("://")
        || href.starts_with("mailto:")
        || href.starts_with("data:")
        // A bookmarklet is code, not a path — strict mode would otherwise
        // read `javascript:(function(){…})` as a dangling source path.
        || href.starts_with("javascript:")
    {
        return Ok(None);
    }

    if let Some(rest) = href.strip_prefix("view:") {
        return view_link(cfg, space, locale, source, rest).map(Some);
    }

    // IO.md §4a's embed branch, and it is deliberately the SMALLEST thing that
    // can be true.
    //
    // An embed rewrites in exactly one case — the target is a row a rule
    // declined to route, so the only address it has is the strong one — and
    // otherwise hands the author's markup back untouched. Two refusals fall
    // out of that, both of them right:
    //
    //   * an embed of a ROUTED row keeps what the author wrote. "A routed
    //     output wins — citations link the declared address" is what a
    //     relative `src` beside its page already says, and rewriting those to
    //     canonical URLs would move bytes on a corpus no item asked to move.
    //   * an embed is not held to `[links] policy`. Most `src`s on a finished
    //     page are engine-derived — `/static/` thumbnails, the stylesheet, the
    //     favicon — and were never a source path to dangle.
    //
    // So the whole seam is: two candidate spellings of a source path, one
    // lookup, no index fallback (an embed of a directory means nothing) and no
    // axis selectors (an image has one form; renditions are I12's).
    if form == Cite::Embed {
        return Ok(embed_target(space, linking_dir, href));
    }

    // Split an anchor/query suffix off the path part.
    let cut = href.find(['#', '?']).unwrap_or(href.len());
    let (path_part, suffix) = href.split_at(cut);

    // An axis selector (q53): `page.md?theme=ledger` links to a specific
    // MEMBER. Without it an axis member is unreachable from prose — a link
    // names a row and a row answers with its canonical URL, so "the ledger
    // rendering of this page" had no spelling. It reads as a query string and
    // resolves to a PATH, which is the point: the member's address is derived,
    // exactly like every other URL here.
    //
    // Only a DECLARED axis name is read this way. Any other `?k=v` stays the
    // literal suffix it has always been, so this cannot change what an existing
    // link means.
    let axis_sel: Option<(&crate::config::Axis, &str)> = suffix
        .strip_prefix('?')
        .and_then(|q| q.split_once('='))
        .and_then(|(k, v)| cfg.axes.get(k).map(|a| (a, v)));
    if let Some((axis, value)) = axis_sel {
        if !axis.values.iter().any(|x| x == value) {
            bail!(
                "{source}: link {href:?} names no member of that axis\n  \
                 members: {}",
                axis.values.join(", ")
            );
        }
    }
    // q53 locale axis: `?locale=fr` picks a SIBLING row — a translation, a
    // different file, not a restyle — the same spelling `?theme=` uses. The
    // filename suffix (`index.fr.md`) gives the implicit value; the selector
    // overrides it, so `index.md?locale=fr` and `index.fr.md` name one member,
    // and `index.fr.md?locale=en` names the base. Read only when i18n is on, so
    // `?locale=` on a monolingual site stays the literal suffix it always was.
    let locale_sel: Option<&str> = cfg
        .locale_enabled()
        .then(|| {
            suffix
                .strip_prefix('?')
                .and_then(|q| q.split_once('='))
                .filter(|(k, _)| *k == "locale")
                .map(|(_, v)| v)
        })
        .flatten();
    if let Some(v) = locale_sel {
        if !cfg.is_locale(v) {
            bail!(
                "{source}: link {href:?} names no locale {v:?}\n  declared: {}",
                cfg.locales().join(" ")
            );
        }
    }
    // C5(d): `page.md?thmee=ledger` is not an axis selector — only a DECLARED
    // name is read as one — so it ships as the literal query string it always
    // was, which is right and silent. Say something when the intent is legible:
    // a key that is a declared axis's name in the wrong case, a typo one or two
    // edits away from one, or the axis's FIELD rather than the axis. Behaviour
    // is unchanged; the link still ships literally, and `?utm=x` says nothing.
    if axis_sel.is_none() && locale_sel.is_none() {
        if let Some(k) = suffix.strip_prefix('?').and_then(|q| q.split_once('=')) {
            if let Some(w) = misspelled_axis(cfg, source, href, k.0) {
                space.warn(w);
            }
        }
    }

    // Source-path resolution: relative to the linking file, then
    // root-relative. A hit that is ALSO a route URL (passthrough files)
    // resolves to the identical string, so trying sources first is safe.
    let mut candidates: Vec<(String, bool)> = Vec::new(); // (source path, was relative)
                                                          // A self-pivot: `.?locale=fr` / `.?theme=ledger` names THIS page's member on
                                                          // that axis — the switcher an author writes by hand. `.` (or a bare `?…`)
                                                          // resolves to the linking row itself, so the `?axis=` branch below pivots the
                                                          // source rather than a directory index. Only when a selector is present, so a
                                                          // plain `.` still means the directory.
    if (path_part.is_empty() || path_part == "." || path_part == "./")
        && (locale_sel.is_some() || axis_sel.is_some())
    {
        candidates.push((source.to_string(), false));
    }
    if !path_part.starts_with('/') {
        candidates.push((
            normalize(&linking_dir.join(path_part))
                .to_string_lossy()
                .to_string(),
            true,
        ));
    }
    candidates.push((path_part.trim_start_matches('/').to_string(), false));
    // A link to a DIRECTORY means its index, the oldest convention on the
    // web (`saturn/` is `saturn/index.md`). Without this, strict mode calls
    // 35 perfectly good links in this corpus dangling — and they resolve to
    // the URL the browser would have reached anyway, so the rewrite is
    // usually a no-op and the real work is the verification.
    for (c, was_relative) in candidates.clone() {
        for index in ["index.md", "index.html"] {
            let joined = if c.is_empty() {
                index.to_string()
            } else {
                format!("{}/{index}", c.trim_end_matches('/'))
            };
            candidates.push((joined, was_relative));
        }
    }
    for (c, was_relative) in &candidates {
        // IO.md §4a: **an authored link demands a route.** The target is a
        // known row — the resolver found it — and the config declined to give
        // it a canonical address, so there is nothing here a reader could
        // bookmark. Answering with the strong address would be worse than
        // failing: a hash URL is the content store made public, it changes the
        // day the bytes do, and a link is a promise that it will not.
        if let Some(strong) = space.source_to_strong.get(c) {
            bail!(
                "{source}: link {href:?} names {c:?}, which no rule routes — \
                 the embed policy gives it a content address ({strong}) and \
                 nothing else, and that address is not a link's to make. Add a \
                 rule routing it (e.g. `route = \"/{{path}}\"`), or embed it \
                 instead."
            );
        }
        if let Some(url) = space.source_to_url.get(c) {
            if *was_relative {
                let browser = format!(
                    "/{}",
                    normalize(&Path::new(url_dir.trim_matches('/')).join(path_part))
                        .to_string_lossy()
                );
                if browser == *url {
                    return Ok(None); // the browser already gets it right
                }
            }
            // q53 locale axis: resolve to the sibling row for the named locale,
            // via by_logical rather than a template — the member is a different
            // file (a file axis, where the theme axis reuses one row). No such
            // sibling is the untranslated case: a member with no file does not
            // materialize, so this is a link to nothing, held to the same
            // standard as every other selector.
            if let Some(v) = locale_sel {
                let url = space
                    .source_to_logical
                    .get(c)
                    .and_then(|lg| space.sibling.get(&(lg.clone(), v.to_string())));
                match url {
                    Some(u) => return Ok(Some(u.clone())),
                    None => bail!(
                        "{source}: link {href:?} selects locale {v:?}, but {c:?} \
                         has no such translation"
                    ),
                }
            }
            // q53: the selector picks a member of the row's axis, and the answer
            // is LOOKED UP in the materialized routes rather than rebuilt from a
            // template (MERGE.md C5c). The reconstruction that used to live here
            // chose the first of the rule's path list — an arbitrary member of
            // it — so a rule written `route = ["/{look}/{axis:locale}/",
            // "/{look}/", "/"]` produced a URL `select_path` never issued, and
            // then blamed the rule for not spending a segment it plainly spent.
            if let Some((_axis, value)) = axis_sel {
                let sel_name = suffix
                    .strip_prefix('?')
                    .and_then(|q| q.split_once('='))
                    .map(|(k, _)| k)
                    .unwrap_or_default();
                // Two failures, two sentences. Whether the row's RULE spends the
                // axis is the row's own property and the honest thing to blame;
                // whether that member MATERIALIZED is a different question, and
                // conflating them is what made the old error misleading.
                let spent = space
                    .source_to_axis
                    .get(c)
                    .is_some_and(|axes| axes.iter().any(|a| a.name == sel_name));
                if !spent {
                    bail!(
                        "{source}: link {href:?} selects an axis member that does not \
                         exist — {url:?} is not on that axis, because the rule that routed \
                         it does not spend a {{{sel_name}}} segment"
                    );
                }
                let key = (c.clone(), sel_name.to_string(), value.to_string());
                if let Some(u) = space.member_url.get(&key) {
                    return Ok(Some(u.clone()));
                }
                // The canonical member's address IS the row's own URL — that is
                // what `Row.url` is, `select_path` run with every coord at its
                // canonical. Rows with no Route of their own (on demand until
                // something cites them, or claimed by a landing view) are absent
                // from `member_url` entirely, and this is the one member of
                // theirs that still has an honest answer.
                if cfg.axes.get(sel_name).and_then(|a| a.canonical()) == Some(value) {
                    return Ok(Some(url.clone()));
                }
                bail!(
                    "{source}: link {href:?} selects {sel_name}={value:?}, which is not \
                     materialized — {c:?} spends the {sel_name:?} axis but publishes no \
                     route for that member"
                );
            }
            // Hold every other axis at the current member (q53). `Row.url`
            // already spent locale when the templates declare it.
            return Ok(Some(format!("{url}{suffix}")));
        }
    }

    match cfg.links.policy {
        LinkPolicy::Loose => Ok(None),
        LinkPolicy::Strict => {
            if space.routes.contains(path_part) {
                match space.url_form.get(path_part) {
                    Some(form) => bail!(
                        "{source}: link {href:?} is a raw URL to routable content — \
                         URLs are derived; link the source instead: {form:?}"
                    ),
                    None => Ok(None), // a route with no better form to offer
                }
            } else {
                match closest_source(space, path_part) {
                    Some(s) => bail!(
                        "{source}: link {href:?} matches no source file or route \
                         (closest source: {s:?})"
                    ),
                    None => bail!("{source}: link {href:?} matches no source file or route"),
                }
            }
        }
    }
}

/// The strong address an EMBED of `href` resolves to, or `None` to leave the
/// markup exactly as written (IO.md §4a).
///
/// `href` is offered in the two spellings the corpus writes: relative to the
/// embedding file, and root-relative. Anything with a query or a fragment
/// keeps them — an `<img src="a.png?v=2">` names the same file — so the suffix
/// is split off the path and put back.
fn embed_target(space: &LinkSpace, linking_dir: &Path, href: &str) -> Option<String> {
    let cut = href.find(['#', '?']).unwrap_or(href.len());
    let (path_part, suffix) = href.split_at(cut);
    if path_part.is_empty() {
        return None;
    }
    let mut candidates: Vec<String> = Vec::new();
    if !path_part.starts_with('/') {
        candidates.push(
            normalize(&linking_dir.join(path_part))
                .to_string_lossy()
                .to_string(),
        );
    }
    candidates.push(path_part.trim_start_matches('/').to_string());
    candidates
        .iter()
        .find_map(|c| space.source_to_strong.get(c))
        .map(|s| format!("{s}{suffix}"))
}

/// `view:name[/key…]` → the view's route, rendered and verified.
fn view_link(
    cfg: &Config,
    space: &LinkSpace,
    locale: &str,
    source: &str,
    rest: &str,
) -> Result<String> {
    // q53: `view:name?axis=value` picks a member, the same spelling a row link
    // uses — a view materialized across an axis lands at several URLs, and
    // naming one of them should not need a second syntax. The group-key form
    // (`view:name/key`) stays what it is; they compose, an axis segment and a
    // group segment being different parts of the path.
    let (rest, query) = match rest.split_once('?') {
        Some((r, q)) => (r, Some(q)),
        None => (rest, None),
    };
    let mut parts = rest.split('/').filter(|s| !s.is_empty());
    let name = parts.next().unwrap_or_default();
    let keys: Vec<&str> = parts.collect();
    let Some(v) = cfg.views.get(name) else {
        let mut known: Vec<&str> = cfg.views.keys().map(String::as_str).collect();
        known.sort_unstable();
        bail!(
            "{source}: view:{name} names no view (views: {})",
            known.join(", ")
        );
    };
    // The members substitute into the template before the group keys do, so the
    // two halves of a `/{theme}/{key}/` path are filled by the two things that
    // own them. A view on several axes is named `view:x?a=1&b=2` — every axis
    // it lands on must be pinned, or there is no single URL to mean.
    let selectors: Vec<(&str, &str)> = query
        .map(|q| q.split('&').filter_map(|s| s.split_once('=')).collect())
        .unwrap_or_default();
    let mut pinned: Vec<(String, String, bool)> = Vec::new(); // (axis, value, canonical)
    for (k, val) in &selectors {
        let Some(axis) = cfg.axes.get(*k) else {
            let known: Vec<&str> = cfg.axes.keys().map(String::as_str).collect();
            bail!(
                "{source}: view:{name}?{k}= names no axis\n  declared axes: {}",
                if known.is_empty() {
                    "(none)".into()
                } else {
                    known.join(", ")
                }
            );
        };
        if !v.axis.iter().any(|a| a == k) {
            bail!("{source}: view:{name}?{k}= — {name} is not materialized across {k:?}");
        }
        if !axis.values.iter().any(|x| x == val) {
            bail!(
                "{source}: view:{name}?{k}={val} names no member of that axis\n  members: {}",
                axis.values.join(", ")
            );
        }
        pinned.push((
            (*k).to_string(),
            (*val).to_string(),
            axis.canonical() == Some(val),
        ));
    }
    // Every axis the view lands on must be named; an unpinned one leaves several
    // URLs and no honest default among them.
    for a in &v.axis {
        if !selectors.iter().any(|(k, _)| k == a) {
            bail!(
                "{source}: view:{name} is materialized across the {a:?} axis, so it \
                 lands at several URLs — name one with view:{name}?{a}=<value>{}",
                if v.axis.len() > 1 {
                    format!(
                        " (this view spends {} axes: {})",
                        v.axis.len(),
                        v.axis.join(", ")
                    )
                } else {
                    String::new()
                }
            );
        }
    }
    // Every template this view lands on, read exactly as `build_view` reads
    // them: `paths` when it has them, else `path`, minus the paginating ones —
    // a link names page one. Reading only the FIRST was the second half of
    // C5's bug (batch review 2): with a default-axis list the canonical member
    // drops its segment to a SHORTER template, so `view:hub?look=plain` rendered
    // `/plain/all/` while the route it names is `/all/`.
    let tmpls: Vec<String> = if v.routes.is_empty() {
        v.route.iter().cloned().collect()
    } else {
        v.routes.clone()
    };
    let page1: Vec<String> = tmpls.into_iter().filter(|t| !t.contains("{n}")).collect();
    if page1.is_empty() {
        bail!("{source}: view {name} has no route (is it embed-only?)");
    }
    let chain = cfg.group_specs(name);
    // The same params grouped_routes exposes: each level's field name, `key` =
    // the leaf, tag slugs on the URL (§6f).
    let mut params: Vec<(String, String)> = Vec::new();
    if chain.is_empty() {
        if !keys.is_empty() {
            bail!("{source}: view:{rest} — {name} is not grouped; drop the key");
        }
    } else {
        if keys.len() != chain.len() {
            bail!(
                "{source}: view:{rest} — {name} groups by {} and needs {} key(s)",
                chain.join(", "),
                chain.len()
            );
        }
        for (spec, key) in chain.iter().zip(&keys) {
            let field = crate::model::spec_field(spec);
            // §6f enum records: the URL wears any grouped field's slug.
            let value = cfg.record_slug(field, key).to_string();
            params.push((field.to_string(), value.clone()));
            params.push(("key".to_string(), value));
        }
    }
    // Fill the group keys and PRESERVE the axis and locale tokens, then let
    // `select_path` spend those — `build_view`'s own two stages, in its order,
    // so the two halves of a `/{look}/{key}/` path are filled by the two things
    // that own them and a link cannot pick a template the materializer did not.
    let rendered: Vec<String> = page1
        .iter()
        .map(|t| {
            crate::template::render(t, |tok| {
                let (ns, k) = crate::template::classify(tok);
                match ns {
                    Some("axis") => Some(format!("{{{tok}}}")),
                    None if cfg.axes.contains_key(k) => Some(format!("{{{k}}}")),
                    None if k == "locale" && cfg.locale_enabled() => Some("{locale}".to_string()),
                    None | Some("group") => crate::template::param(&params, k),
                    _ => None,
                }
            })
        })
        .collect::<Result<_>>()?;
    let pick = |loc: &str| -> Result<String> {
        let mut coords: Vec<Coord> = pinned
            .iter()
            .map(|(axis, value, canonical)| Coord {
                axis,
                value,
                canonical: *canonical,
            })
            .collect();
        // Locale is a coord only when a template spends it (same law as
        // select_path). A gallery at `/photos/` is one URL in every language;
        // inventing a required locale would refuse the link.
        if rendered.iter().any(|t| spends(t, "locale")) {
            coords.push(Coord {
                axis: "locale",
                value: loc,
                canonical: loc == cfg.i18n.default,
            });
        }
        select_path(&rendered, &coords)
    };
    // Locale-parallel views (§6f): a translated row links into its own
    // locale's archive when that variant materialized, and falls back to the
    // default one when it did not.
    let url = pick(locale)?;
    let url = if locale != cfg.i18n.default && !space.routes.contains(&url) {
        pick(&cfg.i18n.default)?
    } else {
        url
    };
    if !space.routes.contains(&url) {
        let mut have: Vec<String> = space
            .url_form
            .iter()
            .filter(|(_, f)| f.starts_with(&format!("view:{name}")))
            .map(|(_, f)| f.clone())
            .collect();
        have.sort_unstable();
        have.dedup();
        bail!(
            "{source}: view:{rest} renders {url:?}, which is not materialized \
             (have: {})",
            if have.is_empty() {
                "none".to_string()
            } else {
                have.join(", ")
            }
        );
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Route, RouteKind};

    /// Every case below is about an authored LINK, so the form is bound once
    /// here rather than repeated at thirty call sites. The embed form has its
    /// own tests, in `io_embeds.rs`, where a site with an unrouted asset can
    /// be built.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve(
        cfg: &Config,
        space: &LinkSpace,
        linking_dir: &Path,
        url_dir: &str,
        locale: &str,
        source: &str,
        href: &str,
    ) -> Result<Option<String>> {
        super::resolve(
            cfg,
            space,
            linking_dir,
            url_dir,
            locale,
            source,
            Cite::Link,
            href,
        )
    }

    /// A link to a DIRECTORY resolves to its index; strict mode must not
    /// call those dangling.
    #[test]
    fn a_directory_link_resolves_to_its_index() {
        let cfg: Config =
            Config::from_toml("root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n")
                .unwrap();
        let mut db = SiteDb::seed(Vec::new(), false);
        db.page_ix
            .push(grackle_db::Key::new("writing/saturn/index.md"));
        db.rows.push(crate::model::Row {
            key: grackle_db::Key::new("writing/saturn/index.md"),
            path: PathBuf::from("writing/saturn/index.md"),
            rel: PathBuf::from("writing/saturn/index.md"),
            version: 0,
            url: "/writing/saturn/".into(),
            rendered: true,
            size: 0,
            title: None,
            description: None,
            order: None,
            date: None,
            theme: None,
            shell: None,
            fields: std::collections::BTreeMap::from([(
                "locale".into(),
                grackle_db::Value::Str("en".into()),
            )]),
            images: Default::default(),
            logical: "writing/saturn/index".into(),
            claimed: false,
            ..Default::default()
        });
        let space = LinkSpace::new(&cfg, &db, Path::new("."));

        // From the site root, `writing/saturn/` finds the index and the
        // browser would NOT have got there on its own, so it is rewritten.
        let got = resolve(
            &cfg,
            &space,
            Path::new(""),
            "/",
            "en",
            "index.md",
            "writing/saturn/",
        )
        .unwrap();
        assert_eq!(got.as_deref(), Some("/writing/saturn/"));

        // Trailing slash is not required, and an anchor rides along.
        let got = resolve(
            &cfg,
            &space,
            Path::new(""),
            "/",
            "en",
            "index.md",
            "/writing/saturn#rings",
        )
        .unwrap();
        assert_eq!(got.as_deref(), Some("/writing/saturn/#rings"));

        // A directory with no index is still dangling — the convention
        // resolves indexes, it does not invent them. And since strict is
        // the default, that is a load error naming the file rather than a
        // link quietly left to 404.
        let e = resolve(
            &cfg,
            &space,
            Path::new(""),
            "/",
            "en",
            "i.md",
            "writing/pluto/",
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("matches no source file or route"), "{e}");
    }

    /// §6f × §6a, pinned: a view link resolves to the LINKING ROW's locale
    /// when that locale's variant materialized, and falls back to the
    /// default when it didn't.
    #[test]
    fn view_links_are_locale_aware() {
        let cfg: Config = Config::from_toml(
            "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"blog\"\nsource = \"_posts\"\n\
             [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\
             [sets.published]\nfrom = \"blog\"\n\
             [routes.tag_index]\nfrom = \"published\"\ngroup_by = \"tags\"\n\
             paths = [\"/{axis:locale}/blog/tags/{key}/\", \"/blog/tags/{key}/\"]\nlayout = \"card\"\n",
        )
        .unwrap();
        let mut db = SiteDb::default();
        for url in [
            "/blog/tags/meta/",
            "/fr/blog/tags/meta/",
            "/blog/tags/rust/",
        ] {
            db.routes.push(Route::new(url.to_string(), RouteKind::View));
        }
        let space = LinkSpace::new(&cfg, &db, Path::new("."));
        let go = |locale: &str, href: &str| {
            resolve(&cfg, &space, Path::new(""), "/", locale, "test.md", href)
        };

        // A French row's view link lands in the French archive…
        let url = go("fr", "view:tag_index/meta").unwrap().unwrap();
        assert_eq!(url, "/fr/blog/tags/meta/");
        // …an English row's in the default one…
        let url = go("en", "view:tag_index/meta").unwrap().unwrap();
        assert_eq!(url, "/blog/tags/meta/");
        // …and a locale with no materialized variant falls back to the
        // default archive rather than linking a 404.
        let url = go("fr", "view:tag_index/rust").unwrap().unwrap();
        assert_eq!(url, "/blog/tags/rust/");
        // A key that exists in NO locale errors, listing what does.
        let err = format!("{:#}", go("en", "view:tag_index/nope").unwrap_err());
        assert!(err.contains("not materialized"), "{err}");
    }
}

#[cfg(test)]
mod axis_tests {
    use super::tests::resolve;
    use super::*;
    use crate::model::{Route, RouteKind};
    use grackle_model::AxisMember;

    /// A site with one axis, `look = [plain, fancy]`, and a `hub` view on a
    /// default-axis path list — the canonical member drops its segment. `spell`
    /// is how the axis token is written; the two spellings must mean one thing.
    fn cfg_with(spell: &str) -> Config {
        Config::from_toml(&format!(
            "root = \".\"\nextends = \"none\"\n\
             [site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [axes.look]\nvalues = [\"plain\", \"fancy\"]\nfield = \"look\"\n\
             [[collections]]\nname = \"blog\"\nsource = \"_posts\"\n\
             [sets.published]\nfrom = \"blog\"\n\
             [routes.hub]\nfrom = \"published\"\nlayout = \"card\"\naxis = \"look\"\n\
             paths = [\"/{{{spell}}}/all/\", \"/all/\"]\n"
        ))
        .unwrap()
    }

    /// C5(b): a `view:` link must read `{axis:look}` as the same segment
    /// `{look}` names, and must pick the template the MATERIALIZER picked —
    /// the shortest one covering the non-canonical members. Reading the first
    /// path and substituting the bare form only sent the canonical member to
    /// `/plain/all/`, a URL the build never issued.
    #[test]
    fn a_view_link_reads_both_axis_spellings_and_the_whole_path_list() {
        for spell in ["look", "axis:look"] {
            let cfg = cfg_with(spell);
            let mut db = SiteDb::default();
            for url in ["/all/", "/fancy/all/"] {
                db.routes.push(Route::new(url.to_string(), RouteKind::View));
            }
            let space = LinkSpace::new(&cfg, &db, Path::new("."));
            let go = |href: &str| {
                resolve(&cfg, &space, Path::new(""), "/", "en", "test.md", href)
                    .unwrap()
                    .unwrap()
            };
            assert_eq!(go("view:hub?look=plain"), "/all/", "spelled {{{spell}}}");
            assert_eq!(
                go("view:hub?look=fancy"),
                "/fancy/all/",
                "spelled {{{spell}}}"
            );
        }
    }

    /// One row, published at two looks by a rule with a path list. The row says
    /// only WHICH axis its rule spends — since C5(c) a member's URL comes from
    /// the routes the build issued, and F3 dropped the template the row used to
    /// carry beside the name (an arbitrary pick out of the rule's list).
    fn axis_db() -> SiteDb {
        let mut db = SiteDb::seed(Vec::new(), false);
        let key = grackle_db::Key::new("note.md");
        db.page_ix.push(key.clone());
        db.rows.push(crate::model::Row {
            key: key.clone(),
            path: PathBuf::from("note.md"),
            rel: PathBuf::from("note.md"),
            url: "/note/".into(),
            rendered: true,
            fields: std::collections::BTreeMap::from([(
                "locale".into(),
                grackle_db::Value::Str("en".into()),
            )]),
            logical: "note".into(),
            axis: vec![grackle_model::RowAxis {
                name: "look".into(),
            }],
            ..Default::default()
        });
        let member = |value: &str, canonical: bool, url: &str| Route {
            row: Some(key.clone()),
            source: Some(PathBuf::from("note.md")),
            axis: vec![AxisMember {
                axis: "look".into(),
                value: value.into(),
                field: "look".into(),
                canonical,
            }],
            ..Route::new(url.to_string(), RouteKind::Page)
        };
        db.routes.push(member("plain", true, "/note/"));
        db.routes.push(member("fancy", false, "/fancy/note/"));

        // A row the axis does not cover: its rule spends no `{look}` segment,
        // so it has one form and a selector on it names nothing.
        let flat = grackle_db::Key::new("flat.md");
        db.page_ix.push(flat.clone());
        db.rows.push(crate::model::Row {
            key: flat.clone(),
            path: PathBuf::from("flat.md"),
            rel: PathBuf::from("flat.md"),
            url: "/flat/".into(),
            rendered: true,
            fields: std::collections::BTreeMap::from([(
                "locale".into(),
                grackle_db::Value::Str("en".into()),
            )]),
            logical: "flat".into(),
            ..Default::default()
        });
        db.routes.push(Route {
            row: Some(flat),
            source: Some(PathBuf::from("flat.md")),
            ..Route::new("/flat/".to_string(), RouteKind::Page)
        });

        // §4 on demand: the axis IS spent, but nothing has materialized a
        // route yet — the row knows its canonical URL and nothing else.
        let later = grackle_db::Key::new("later.md");
        db.page_ix.push(later.clone());
        db.rows.push(crate::model::Row {
            key: later,
            path: PathBuf::from("later.md"),
            rel: PathBuf::from("later.md"),
            url: "/later/".into(),
            rendered: true,
            on_demand: true,
            fields: std::collections::BTreeMap::from([(
                "locale".into(),
                grackle_db::Value::Str("en".into()),
            )]),
            logical: "later".into(),
            axis: vec![grackle_model::RowAxis {
                name: "look".into(),
            }],
            ..Default::default()
        });
        db
    }

    /// The two failures the lookup keeps apart. Whether the row's RULE spends
    /// the axis is the row's own property; whether a member MATERIALIZED is a
    /// different question, and the old single message blamed the rule for both.
    #[test]
    fn the_two_axis_link_failures_say_different_things() {
        let cfg = cfg_with("axis:look");
        let db = axis_db();
        let space = LinkSpace::new(&cfg, &db, Path::new("."));
        let err = |href: &str| {
            format!(
                "{:#}",
                resolve(&cfg, &space, Path::new(""), "/", "en", "index.md", href).unwrap_err()
            )
        };

        let e = err("flat.md?look=fancy");
        assert!(e.contains("does not spend a {look} segment"), "{e}");
        let e = err("later.md?look=fancy");
        assert!(e.contains("is not materialized"), "{e}");
        assert!(!e.contains("does not spend"), "{e}");

        // …but the CANONICAL member of an on-demand row has an honest answer:
        // the row's own URL is `select_path` with every coord at canonical,
        // which is exactly what a reference will materialize.
        let got = resolve(
            &cfg,
            &space,
            Path::new(""),
            "/",
            "en",
            "index.md",
            "later.md?look=plain",
        )
        .unwrap();
        assert_eq!(got.as_deref(), Some("/later/"));
    }

    /// C5(c): the member's address is LOOKED UP in the routes the build issued,
    /// not rebuilt. Rebuilding it from the row's template would answer
    /// `/{axis:look}/note/` for one member and `/plain/note/` for the other —
    /// neither of which exists — and then blame the rule for not spending a
    /// segment it spends.
    #[test]
    fn a_row_link_takes_the_members_url_from_the_route_it_landed_at() {
        let cfg = cfg_with("axis:look");
        let db = axis_db();
        let space = LinkSpace::new(&cfg, &db, Path::new("."));
        let go = |href: &str| resolve(&cfg, &space, Path::new(""), "/", "en", "index.md", href);

        assert_eq!(go("note.md?look=plain").unwrap().as_deref(), Some("/note/"));
        assert_eq!(
            go("note.md?look=fancy").unwrap().as_deref(),
            Some("/fancy/note/")
        );
        // A self-pivot from the row itself resolves the same way.
        let self_pivot = resolve(
            &cfg,
            &space,
            Path::new(""),
            "/note/",
            "en",
            "note.md",
            ".?look=fancy",
        )
        .unwrap();
        assert_eq!(self_pivot.as_deref(), Some("/fancy/note/"));
    }

    /// A link picks ONE member along ONE axis; every other axis the row spends
    /// stays at its canonical (q53's composition). With two axes the row has
    /// four routes and `?look=fancy` names exactly one of them — the index must
    /// not answer with whichever of the two the route list happened to end on.
    #[test]
    fn a_one_axis_selector_leaves_the_other_axes_canonical() {
        let cfg = Config::from_toml(
            "root = \".\"\nextends = \"none\"\n\
             [site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [axes.look]\nvalues = [\"plain\", \"fancy\"]\nfield = \"look\"\n\
             [axes.flavor]\nvalues = [\"sweet\", \"salty\"]\nfield = \"flavor\"\n",
        )
        .unwrap();
        let mut db = SiteDb::seed(Vec::new(), false);
        let key = grackle_db::Key::new("page.md");
        db.page_ix.push(key.clone());
        db.rows.push(crate::model::Row {
            key: key.clone(),
            path: PathBuf::from("page.md"),
            rel: PathBuf::from("page.md"),
            url: "/plain/sweet/page/".into(),
            rendered: true,
            fields: std::collections::BTreeMap::from([(
                "locale".into(),
                grackle_db::Value::Str("en".into()),
            )]),
            logical: "page".into(),
            axis: ["look", "flavor"]
                .into_iter()
                .map(|n| grackle_model::RowAxis { name: n.into() })
                .collect(),
            ..Default::default()
        });
        for look in ["plain", "fancy"] {
            for flavor in ["sweet", "salty"] {
                db.routes.push(Route {
                    row: Some(key.clone()),
                    source: Some(PathBuf::from("page.md")),
                    axis: vec![
                        AxisMember {
                            axis: "look".into(),
                            value: look.into(),
                            field: "look".into(),
                            canonical: look == "plain",
                        },
                        AxisMember {
                            axis: "flavor".into(),
                            value: flavor.into(),
                            field: "flavor".into(),
                            canonical: flavor == "sweet",
                        },
                    ],
                    ..Route::new(format!("/{look}/{flavor}/page/"), RouteKind::Page)
                });
            }
        }
        let space = LinkSpace::new(&cfg, &db, Path::new("."));
        let go = |href: &str| {
            resolve(&cfg, &space, Path::new(""), "/", "en", "index.md", href)
                .unwrap()
                .unwrap()
        };
        assert_eq!(go("page.md?look=fancy"), "/fancy/sweet/page/");
        assert_eq!(go("page.md?flavor=salty"), "/plain/salty/page/");
    }

    /// C5(d): an unknown query key stays literal — that is the design, since
    /// only a declared name is a selector — and says so once, naming the file.
    #[test]
    fn a_misspelled_axis_ships_literally_and_warns() {
        let cfg = cfg_with("axis:look");
        let db = axis_db();
        let space = LinkSpace::new(&cfg, &db, Path::new("."));
        let go =
            |href: &str| resolve(&cfg, &space, Path::new(""), "/", "en", "index.md", href).unwrap();

        // The href is UNCHANGED in meaning: the suffix rides along verbatim.
        assert_eq!(go("note.md?lok=fancy").as_deref(), Some("/note/?lok=fancy"));
        assert_eq!(go("note.md?utm=x").as_deref(), Some("/note/?utm=x"));

        let w = space.take_warnings();
        assert_eq!(w.len(), 1, "{w:?}"); // `utm` says nothing
        assert!(
            w[0].starts_with("index.md: link \"note.md?lok=fancy\""),
            "{}",
            w[0]
        );
        assert!(w[0].contains("did you mean `?look=`"), "{}", w[0]);
        // Drained, so `serve`'s next rebuild starts clean.
        assert!(space.take_warnings().is_empty());
    }

    /// The shapes that warn and the ones that must not. A pure function, so the
    /// sentence is testable without a site (`slots::unknown_stems`'s precedent).
    #[test]
    fn only_a_legible_axis_typo_warns() {
        let cfg = Config::from_toml(
            "root = \".\"\nextends = \"none\"\n\
             [site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [axes.palette]\nvalues = [\"plain\"]\nfield = \"theme\"\n\
             [axes.ab]\nvalues = [\"x\"]\nfield = \"ab\"\n",
        )
        .unwrap();
        let say = |k: &str| misspelled_axis(&cfg, "p.md", "x.md?k=v", k);

        // Case: the one spelling an author is looking at while it does nothing.
        assert!(say("Palette").unwrap().contains("case counts"));
        // Distance, at filter.rs's threshold of two.
        assert!(say("palete").unwrap().contains("did you mean `?palette=`"));
        // The FIELD an axis sets, in place of the axis.
        assert!(say("theme").unwrap().contains("is the FIELD"));
        // A declared name is a selector and was handled by the caller.
        assert_eq!(say("palette"), None);
        // An ordinary query string keeps meaning what it always meant.
        assert_eq!(say("utm"), None);
        assert_eq!(say("page"), None);
        // Short keys do not match by accident: `id` is two edits from the `ab`
        // axis, which is the whole of both words — a distance that says nothing.
        assert_eq!(say("id"), None);
        // One edit out of two still says something.
        assert!(say("ib").unwrap().contains("did you mean `?ab=`"));
    }

    /// `locale` is an axis only where i18n is on; elsewhere it is an ordinary
    /// query key and suggesting it would be a lie.
    #[test]
    fn locale_is_only_suggested_where_it_is_declared() {
        let head = "root = \".\"\nextends = \"none\"\n\
                    [site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n";
        let mono = Config::from_toml(head).unwrap();
        assert_eq!(misspelled_axis(&mono, "p.md", "x?locle=fr", "locle"), None);

        let multi = Config::from_toml(&format!(
            "{head}[axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n"
        ))
        .unwrap();
        assert!(misspelled_axis(&multi, "p.md", "x?locle=fr", "locle")
            .unwrap()
            .contains("did you mean `?locale=`"));
    }
}
