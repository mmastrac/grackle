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
//! External schemes, fragments and mailto pass through untouched.

use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use crate::config::{Config, LinkPolicy};
use crate::db::{RouteKind, SiteDb};

/// Everything link resolution needs, computed once per build.
pub struct LinkSpace {
    /// Root-relative source path → the row's URL (posts, pages, objects).
    source_to_url: HashMap<String, String>,
    /// Every materialized URL.
    routes: HashSet<String>,
    /// URL → the form a strict-mode error suggests instead.
    url_form: HashMap<String, String>,
    /// q53: source path → the axes the row's rule spends. A member's URL is the
    /// shared template with each axis substituted — the selected one by the
    /// link's value, the rest by their canonical — while `source_to_url` holds
    /// the all-canonical form, which is what a plain link wants.
    source_to_axis: HashMap<String, Vec<grackle_model::RowAxis>>,
}

impl LinkSpace {
    /// Does this URL name a materialized route? The raw-HTML seam (§6d stage
    /// B) asks, because it meets engine-derived URLs it must not police.
    pub fn is_route(&self, url: &str) -> bool {
        self.routes.contains(url)
    }

    pub fn new(_cfg: &Config, db: &SiteDb, root: &Path) -> LinkSpace {
        let mut source_to_url = HashMap::new();
        let mut source_to_axis: HashMap<String, Vec<grackle_model::RowAxis>> = HashMap::new();
        // A row that publishes on demand (§4) is a legal link target even
        // though nothing has materialized it yet: the question a link asks is
        // whether the target is PUBLISHABLE, not whether someone else already
        // cited it. Its URL comes from the same rule template either way.
        for p in db.posts().chain(db.pages()).chain(db.objects()) {
            // q45: a claimed locale variant whose partition never
            // materialized has no URL; offering it would rewrite links
            // to "".
            if p.url.is_empty() {
                continue;
            }
            source_to_url.insert(p.rel.to_string_lossy().to_string(), p.url.clone());
            if !p.axis.is_empty() {
                source_to_axis.insert(p.rel.to_string_lossy().to_string(), p.axis.clone());
            }
        }
        let mut routes = HashSet::new();
        let mut url_form = HashMap::new();
        // §4: `route || row.on_demand`. An on-demand row has no Route yet, so
        // taking the route set alone would call every asset link dangling —
        // and the link is precisely what will materialize it.
        for p in db.rows.iter().filter(|p| p.on_demand && !p.url.is_empty()) {
            routes.insert(p.url.clone());
        }
        for r in &db.routes {
            routes.insert(r.url.clone());
            let form = match r.kind {
                RouteKind::View => r.view.as_ref().map(|v| match &r.key {
                    Some(k) => format!("view:{v}/{k}"),
                    None => format!("view:{v}"),
                }),
                _ => r.source.as_ref().and_then(|s| {
                    s.strip_prefix(root)
                        .ok()
                        .map(|rel| format!("/{}", rel.to_string_lossy()))
                }),
            };
            if let Some(f) = form {
                url_form.insert(r.url.clone(), f);
            }
        }
        LinkSpace {
            source_to_url,
            routes,
            url_form,
            source_to_axis,
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

    // Source-path resolution: relative to the linking file, then
    // root-relative. A hit that is ALSO a route URL (passthrough files)
    // resolves to the identical string, so trying sources first is safe.
    let mut candidates: Vec<(String, bool)> = Vec::new(); // (source path, was relative)
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
            // q53: the selector picks a member of the row's axis. Checked
            // against the route set, so a selector on a row the axis does not
            // cover is a load error rather than a link to nothing — the same
            // standard every other link here is held to.
            if let Some((_axis, value)) = axis_sel {
                let sel_name = suffix
                    .strip_prefix('?')
                    .and_then(|q| q.split_once('='))
                    .map(|(k, _)| k)
                    .unwrap_or_default();
                // The row's own template is what a member's URL is made of, so
                // a selector on a row whose rule never spent that axis has
                // nothing to substitute into — which is the error below. A link
                // picks one member along one axis; any OTHER axis the row spends
                // stays at its canonical, so the result is a route that exists.
                let member = match space.source_to_axis.get(c) {
                    Some(axes) if axes.iter().any(|a| a.name == sel_name) => {
                        let mut url = axes[0].template.clone();
                        for a in axes {
                            let fill = if a.name == sel_name {
                                value
                            } else {
                                cfg.axes.get(&a.name).and_then(|x| x.canonical()).unwrap_or("")
                            };
                            url = url.replace(&format!("{{{}}}", a.name), fill);
                        }
                        url
                    }
                    _ => String::new(),
                };
                if member.is_empty() || !space.routes.contains(&member) {
                    bail!(
                        "{source}: link {href:?} selects an axis member that does not \
                         exist — {url:?} is not on that axis, because the rule that routed \
                         it does not spend a {{{sel_name}}} segment"
                    );
                }
                return Ok(Some(member));
            }
            // §6f, same invariant as view links: a translated row's source
            // link lands in its own locale's variant when that variant
            // materialized, and falls back to the target row's own URL.
            if locale != cfg.i18n.default && !url.starts_with(&format!("/{locale}/")) {
                let prefixed = format!("/{locale}{url}");
                if space.routes.contains(&prefixed) {
                    return Ok(Some(format!("{prefixed}{suffix}")));
                }
            }
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
    let mut axis_subs: Vec<(String, String)> = Vec::new();
    for (k, val) in &selectors {
        let Some(axis) = cfg.axes.get(*k) else {
            let known: Vec<&str> = cfg.axes.keys().map(String::as_str).collect();
            bail!(
                "{source}: view:{name}?{k}= names no axis\n  declared axes: {}",
                if known.is_empty() { "(none)".into() } else { known.join(", ") }
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
        axis_subs.push((format!("{{{k}}}"), val.to_string()));
    }
    // Every axis the view lands on must be named; an unpinned one leaves several
    // URLs and no honest default among them.
    for a in &v.axis {
        if !selectors.iter().any(|(k, _)| k == a) {
            bail!(
                "{source}: view:{name} is materialized across the {a:?} axis, so it \
                 lands at several URLs — name one with view:{name}?{a}=<value>{}",
                if v.axis.len() > 1 {
                    format!(" (this view spends {} axes: {})", v.axis.len(), v.axis.join(", "))
                } else {
                    String::new()
                }
            );
        }
    }
    let sub = |t: &str| {
        let mut t = t.to_string();
        for (ph, val) in &axis_subs {
            t = t.replace(ph.as_str(), val);
        }
        t
    };
    let chain = cfg.group_specs(name);
    let url = if !chain.is_empty() {
        if keys.len() != chain.len() {
            bail!(
                "{source}: view:{rest} — {name} groups by {} and needs {} key(s)",
                chain.join(", "),
                chain.len()
            );
        }
        let tmpl = v
            .route
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("{source}: view {name} has no route"))?;
        // The same params grouped_routes exposes: each level's field name,
        // `key` = the leaf, tag slugs on the URL (§6f).
        let mut params: Vec<(String, String)> = Vec::new();
        for (spec, key) in chain.iter().zip(&keys) {
            let field = crate::db::spec_field(spec);
            // §6f enum records: the URL wears any grouped field's slug.
            let value = cfg.record_slug(field, key).to_string();
            params.push((field.to_string(), value.clone()));
            params.push(("key".to_string(), value));
        }
        crate::template::render(&sub(tmpl), |tok| {
            // Bare or `group:`-qualified name the same group param.
            match crate::template::classify(tok) {
                (None | Some("group"), k) => crate::template::param(&params, k),
                _ => None,
            }
        })?
    } else {
        if !keys.is_empty() {
            bail!("{source}: view:{rest} — {name} is not grouped; drop the key");
        }
        sub(v.route
            .as_deref()
            .or_else(|| v.routes.first().map(String::as_str))
            .ok_or_else(|| {
                anyhow::anyhow!("{source}: view {name} has no route (is it embed-only?)")
            })?)
    };
    // Locale-parallel views (§6f): a translated row links into its own
    // locale's archive when that variant materialized.
    let url = if locale != cfg.i18n.default {
        let prefixed = format!("/{locale}{url}");
        if space.routes.contains(&prefixed) {
            prefixed
        } else {
            url
        }
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
    use crate::db::{Route, RouteKind};

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
        db.rows.push(crate::db::Row {
            key: grackle_db::Key::new("writing/saturn/index.md"),
            path: PathBuf::from("writing/saturn/index.md"),
            rel: PathBuf::from("writing/saturn/index.md"),
            version: 0,
            url: "/writing/saturn/".into(),
            rendered: true,
            size: 0,
            title: None,
            layout: None,
            description: None,
            order: None,
            date: None,
            tags: Vec::new(),
            toc: false,
            theme: None,
            shell: None,
            fields: Default::default(),
            images: Default::default(),
            locale: "en".into(),
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
             [[collections]]\nname = \"blog\"\nkind = \"posts\"\nsource = \"_posts\"\n\
             [i18n]\nlocales = [\"fr\"]\n\
             [sets.published]\nfrom = \"blog\"\n\
             [routes.tag_index]\nfrom = \"published\"\ngroup_by = \"tags\"\n\
             path = \"/blog/tags/{key}/\"\nlayout = \"listing\"\n",
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
