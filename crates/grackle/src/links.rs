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
}

impl LinkSpace {
    pub fn new(_cfg: &Config, db: &SiteDb, root: &Path) -> LinkSpace {
        let mut source_to_url = HashMap::new();
        // One loop over both tables. It was two only because a post's `rel`
        // was collection-relative, so the post arm had to re-derive the
        // root-relative form from `path`; `rel` means one thing now (q51).
        for p in db.posts().chain(db.pages()) {
            // q45: a claimed locale variant whose partition never
            // materialized has no URL; offering it would rewrite links
            // to "".
            if p.url.is_empty() {
                continue;
            }
            source_to_url.insert(p.rel.to_string_lossy().to_string(), p.url.clone());
        }
        for o in &db.objects.rows {
            source_to_url.insert(o.rel.to_string_lossy().to_string(), o.url.clone());
        }
        let mut routes = HashSet::new();
        let mut url_form = HashMap::new();
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
        // A bookmarklet is code, not a path. Only strict noticed: it read
        // `javascript:(function(){…})` as a relative source path and called
        // it dangling.
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
        crate::template::render(tmpl, |k| crate::template::param(&params, k))?
    } else {
        if !keys.is_empty() {
            bail!("{source}: view:{rest} — {name} is not grouped; drop the key");
        }
        v.route
            .as_deref()
            .or_else(|| v.routes.first().map(String::as_str))
            .ok_or_else(|| {
                anyhow::anyhow!("{source}: view {name} has no route (is it embed-only?)")
            })?
            .to_string()
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

    /// A link to a DIRECTORY is a link to its index — the oldest convention
    /// on the web, and the resolver did not know it. Strict mode called 35
    /// good links in the main corpus dangling because of this.
    #[test]
    fn a_directory_link_resolves_to_its_index() {
        let cfg: Config =
            Config::from_toml("root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n")
                .unwrap();
        let mut db = SiteDb::seed(Vec::new(), false);
        db.page_ix.push(db.rows.len());
        db.rows.push(crate::db::Row {
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
            draft: false,
            hidden: false,
            noindex: false,
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
    /// default when it didn't. This is the invariant, not an accident.
    #[test]
    fn view_links_are_locale_aware() {
        let cfg: Config = Config::from_toml(
            "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
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
