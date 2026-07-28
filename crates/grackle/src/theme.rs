//! A theme is a directory of data (§5e): binder fragments + CSS. This module
//! loads one and renders full pages: the layout kind's part map through its
//! fragment, the result into the root chrome, identity slots filled from the
//! tree.
//!
//! The chrome file is **`root.html`** and the kind it binds is `root` (IO.md
//! §6). It may be a bare fragment — which is the body chrome, and what every
//! theme here writes — or document-shaped, with a `<head>` fenced to `<style>`
//! and a `<body>`; `binder::split_root` is that split, and the engine keeps
//! owning `<html>` and the computed head either way. The head half leaves as
//! CSS: `head_style()` is read by the CSS assembly, never by a page (I5).
//!
//! The root's engine-provided parts are `site_title` (config), `axes` (the
//! language/theme switcher) and `main` (the rendered kind). **Every other
//! `html`-typed root slot is an identity slot**, resolved from `.slots/` up
//! the source path — the theme places `<p data-slot="copyright">`; the tree
//! owns the words. A fill is markup, so the part type is what decides
//! (`from_sources`); a fill naming no identity slot of any loaded theme is a
//! load-time warning (`build::render_site`).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::base;
use crate::binder::{self, Fragments};
use crate::parts::{Part, PartMap, PartType};
use crate::slots::SlotFills;

pub struct Theme {
    pub fragments: Fragments,
    fills: SlotFills,
    root: PathBuf,
    /// Root identity slots: (schema name, element is phrasing-only).
    /// Leaked: a slot name is decided at load and lives as long as the
    /// process, and `PartMap` keys are `&'static str`.
    identity: Vec<(&'static str, bool)>,
    /// The CSS a document-shaped `root.html` declared in its `<head>` —
    /// `<style>` and nothing else, fenced at load (IO.md §6), and handed to
    /// the CSS assembly rather than to any page (I5). Empty for a body-only
    /// root, which is every theme in the repository today.
    style: String,
}

/// Split a row's theme spec: the directory name before the first `:`,
/// subtheme tokens after it, space-joined — `"recipes:spicy"` renders
/// through the `recipes` theme with `subtheme = "spicy"` on the root,
/// which CSS subselects via `[data-subtheme~="spicy"]` (the same
/// whitespace-token trick as §5b's data-scope).
pub fn split_spec(spec: &str) -> (&str, Option<String>) {
    match spec.split_once(':') {
        Some((name, rest)) => {
            let toks: Vec<&str> = rest.split(':').filter(|t| !t.is_empty()).collect();
            (name, (!toks.is_empty()).then(|| toks.join(" ")))
        }
        None => (spec, None),
    }
}

/// Every theme under `themes/`, keyed by directory name. Theme is chosen
/// per row (§5a): a row names one (`theme:` front matter, cascadable via
/// rule defaults); failing that the site default (`[site] theme`, else the
/// `default` directory); a site with no themes at all gets the null theme —
/// §5e's "needs no directory" made literal.
pub struct Themes {
    map: std::collections::BTreeMap<String, Theme>,
    null: Theme,
    /// `[site] theme`, split. `None` keeps the historical behaviour exactly:
    /// `get(None)` finds the `default` directory or the base, and the
    /// stylesheet URL stays `/css/main.css`.
    site_name: Option<String>,
    site_sub: Option<String>,
}

impl Themes {
    pub fn load_all(
        themes_dir: &Path,
        site_root: &Path,
        schemas: &crate::parts::Schemas,
        site_theme: Option<&str>,
    ) -> Result<Themes> {
        let mut map = std::collections::BTreeMap::new();
        if let Ok(rd) = std::fs::read_dir(themes_dir) {
            for e in rd.filter_map(|e| e.ok()) {
                let name = e.file_name().to_string_lossy().to_string();
                // Underscore-prefixed directories are not themes, the same
                // convention `_posts` and `_includes` already use — room for
                // a site to keep working files beside its themes.
                if e.path().is_dir() && !name.starts_with('_') {
                    map.insert(name, Theme::load(&e.path(), site_root, schemas)?);
                }
            }
        }
        // `default` is the one name that resolves without a directory (it is
        // the base), so it is spellable even by a site with no `themes/`.
        let (site_name, site_sub) = match site_theme {
            Some(spec) => {
                let (n, sub) = split_spec(spec);
                if n != "default" && !map.contains_key(n) {
                    let known: Vec<&str> = map.keys().map(String::as_str).collect();
                    anyhow::bail!(
                        "[site] theme = {spec:?} names no theme — themes in {}: {}",
                        themes_dir.display(),
                        if known.is_empty() {
                            "(none)".into()
                        } else {
                            known.join(", ")
                        }
                    );
                }
                (Some(n.to_string()), sub)
            }
            None => (None, None),
        };
        Ok(Themes {
            map,
            null: Theme::null(site_root, schemas)?,
            site_name,
            site_sub,
        })
    }

    /// The site default, split: `[site] theme` when there is one, else
    /// `None` — which `get` reads as the `default` directory and `css_of` as
    /// `/css/main.css`, exactly as before this key existed.
    pub fn site_default(&self) -> (Option<&str>, Option<String>) {
        (self.site_name.as_deref(), self.site_sub.clone())
    }

    /// A row's `theme:` spec resolved against the site default — the whole
    /// cascade in one place, so the five render paths cannot drift on what
    /// "this row named nothing" means. A row that names a theme states its
    /// own subtheme tokens; the site's tokens are the site's, and reach only
    /// the rows that asked for nothing.
    pub fn resolve<'a>(&'a self, spec: Option<&'a str>) -> (Option<&'a str>, Option<String>) {
        match spec {
            Some(s) => {
                let (n, sub) = split_spec(s);
                (Some(n), sub)
            }
            None => self.site_default(),
        }
    }

    /// Look up a theme by directory name. `None` and `"default"` both mean
    /// the `default` directory, or the base theme when there is none — so
    /// callers pass the output of `resolve`, which has already spent
    /// `[site] theme`. A *named* theme that doesn't exist is an error listing
    /// the knowns: somebody asked for it explicitly.
    pub fn get(&self, name: Option<&str>) -> Result<&Theme> {
        match name {
            None | Some("default") => Ok(self.map.get("default").unwrap_or(&self.null)),
            Some(n) => self.map.get(n).ok_or_else(|| {
                let known: Vec<&str> = self.map.keys().map(String::as_str).collect();
                anyhow::anyhow!(
                    "no theme named {n:?} — themes: {}",
                    if known.is_empty() {
                        "(none)".into()
                    } else {
                        known.join(", ")
                    }
                )
            }),
        }
    }

    /// The theme directories, for per-theme stylesheet compilation.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(String::as_str)
    }

    /// Every slot the tree may fill, over every theme that can RENDER —
    /// sorted and deduped.
    ///
    /// The union is the point (C4b). Themes ship their own roots and may
    /// place different identity slots, so a fill is dead only when NO theme
    /// would read it — a site that switches between two themes keeps both
    /// sets of words, and neither is a typo. The base theme joins the union
    /// on exactly the condition `get` reaches it: there is no
    /// `themes/default` for it to stand behind.
    pub fn identity_slots(&self) -> Vec<&'static str> {
        let base = (!self.map.contains_key("default")).then_some(&self.null);
        let mut out: Vec<&'static str> = base
            .into_iter()
            .flat_map(|t| t.identity_slots())
            .chain(self.map.values().flat_map(|t| t.identity_slots()))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The tree's `.slots/` fills. Every theme scans the same tree at load,
    /// so the null theme's copy IS the tree's — this is a reader for that
    /// scan, not a second one.
    pub fn fills(&self) -> &SlotFills {
        &self.null.fills
    }
}

impl Theme {
    /// The null theme as a value — which is the BASE theme, not an empty one.
    /// A site with no `themes/` directory renders through the fragments the
    /// engine carries, so "no theme" means plain, never broken.
    pub fn null(site_root: &Path, schemas: &crate::parts::Schemas) -> Result<Theme> {
        Theme::from_sources(Vec::new(), site_root, schemas, "the base theme")
    }

    pub fn load(
        theme_dir: &Path,
        site_root: &Path,
        schemas: &crate::parts::Schemas,
    ) -> Result<Theme> {
        // IO.md §6: the chrome file is `root.html`, and the part kind it
        // binds renamed with it. A theme still carrying `shell.html` would
        // otherwise fail with the generic "fragment names no layout kind
        // `shell`", which sends its reader hunting for a kind when the fix is
        // a rename — the one case §10's precedent allows a targeted sentence.
        if theme_dir.join("shell.html").exists() && !theme_dir.join("root.html").exists() {
            anyhow::bail!(
                "{}: `shell.html` is `root.html` now (IO.md §6) — the chrome part kind \
                 renamed shell → root, and a theme root may carry a <head> as well as \
                 a body. Rename the file; its contents are the body chrome, unchanged.",
                theme_dir.display()
            );
        }
        let own = binder::dir_sources(theme_dir)
            .with_context(|| format!("loading theme {}", theme_dir.display()))?;
        let what = theme_dir.display().to_string();
        Theme::from_sources(own, site_root, schemas, &what)
    }

    /// Build a theme from its OWN fragment sources, layered over the base's
    /// (§5e). A source of the same name replaces the base's outright; every
    /// kind the theme declines keeps the base arrangement, which is what
    /// makes a theme "only its differences" rather than a whole vocabulary.
    ///
    /// The merge happens before `Fragments::load` deliberately: that function
    /// parses the whole set and only then validates, so a theme's
    /// `document.html` may name a stream child that lives in the base.
    fn from_sources(
        mut own: Vec<(String, String, String)>,
        site_root: &Path,
        schemas: &crate::parts::Schemas,
        what: &str,
    ) -> Result<Theme> {
        // IO.md §6: `root.html` may be document-shaped. Split it before the
        // merge, so the body half is an ordinary fragment source and the head
        // half never reaches the binder's part vocabulary at all — it is
        // presentation, not an arrangement of parts.
        //
        // A head-only root drops OUT of `own` here, which is the whole of
        // "and inherits the base's chrome": the merge below is by name, so a
        // theme that contributes no `root` body keeps the base's.
        let mut style = String::new();
        if let Some(i) = own.iter().position(|(n, _, _)| n == "root") {
            let split = binder::split_root(&own[i].1, &own[i].2)?;
            if let Some(s) = split.style {
                style = s.trim().to_string();
            }
            match split.body {
                Some(body) => own[i].1 = body,
                None => {
                    own.remove(i);
                }
            }
        }
        let mut sources: Vec<(String, String, String)> = base::fragments()
            .iter()
            .filter(|(n, _)| !own.iter().any(|(o, _, _)| o == n))
            .map(|(n, src)| (n.to_string(), src.to_string(), format!("<base>/{n}.html")))
            .collect();
        sources.extend(own);
        let fragments =
            Fragments::load(sources, schemas).with_context(|| format!("loading theme {what}"))?;
        let fills = SlotFills::load(site_root)?;
        // Identity slots = root slots the TREE fills, matched back to the
        // schema so the names stay 'static and checked.
        //
        // Which ones those are is read off the declared PART TYPE rather than
        // off a list of names (MERGE.md C4c). A `.slots/` fill is HTML by
        // construction — `Fill::render` produces markup and `page` sets it as
        // `Part::Html` — so a root slot declared as anything else can never
        // take one. That answers `site_title` (`text`) and, the case this was
        // written for, `axes` (`stream:axis`, the engine's own language/theme
        // switcher): without the type test, `.slots/axes.md` lands in a slot
        // the binder validated as a stream and replaces it with prose.
        //
        // `main` is the one NAME left, because no type can say it: it is HTML
        // the engine renders into itself.
        let engine = ["main"];
        let mut identity = Vec::new();
        for (slot, tag) in fragments.slot_tags("root") {
            if engine.contains(&slot.as_str()) {
                continue;
            }
            let (name, ty) = schemas
                .get("root")
                .and_then(|s| s.iter().find(|(n, _)| **n == *slot).copied())
                .with_context(|| format!("root fragment slots unknown part `{slot}`"))?;
            if ty != PartType::Html {
                continue;
            }
            identity.push((name, binder::is_phrasing_only(&tag)));
        }
        Ok(Theme {
            fragments,
            fills,
            root: site_root.to_path_buf(),
            identity,
            style,
        })
    }

    /// The root slots this theme leaves for the tree to fill — what a
    /// `.slots/` file may be named for this theme (§5e).
    pub fn identity_slots(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.identity.iter().map(|(n, _)| *n)
    }

    /// The CSS this theme's `root.html` declared in its `<head>` (IO.md §6),
    /// empty for the body-only roots every theme in the repository writes.
    ///
    /// Its one reader is `build::css_pass`, which compiles it into the
    /// theme's sheet (I5). **No page ever sees it**: the head fence exists so
    /// that a theme's presentation can join the one CSS artifact, and a page
    /// carrying an inline `<style>` as well as the stylesheet link would be
    /// the second artifact the model says does not exist.
    pub fn head_style(&self) -> &str {
        &self.style
    }

    /// Render one full page: `main` is the already-rendered layout kind;
    /// `source_dir` anchors identity-slot resolution (rows deeper in the
    /// tree can override the site's identity, nearest wins); `subtheme`
    /// is the row's `theme:` colon suffix, if any.
    /// `resolve_link` sees every markdown link in a fill, with the fill's
    /// OWNER directory as its relative base (§6a) — this is how one nav.md
    /// with `view:` links serves every locale: resolution runs per page.
    pub fn page(
        &self,
        head_html: String,
        site_title: &str,
        main: String,
        source_dir: &Path,
        locale: &str,
        resolve_link: &dyn Fn(crate::links::Cite, &Path, &str) -> Result<Option<String>>,
        subtheme: Option<&str>,
        profile: Option<&str>,
        axis: &[grackle_model::AxisMember],
        axes: Vec<PartMap>,
    ) -> Result<String> {
        let mut m = PartMap::new("root");
        m.set("site_title", Part::Text(site_title.to_string()));
        if !axes.is_empty() {
            m.set("axes", Part::Stream(axes));
        }
        for (name, phrasing) in &self.identity {
            if let Some(fill) = self.fills.resolve(&self.root, source_dir, name, locale) {
                let rendered = fill.render(&|form, href| resolve_link(form, &fill.owner, href))?;
                let html = if *phrasing {
                    SlotFills::inline_or_err(&rendered)?.to_string()
                } else {
                    rendered.blocks
                };
                m.set(name, Part::Html(html));
            }
        }
        m.set("main", Part::Html(main));
        // A document-shaped root's `<style>` does NOT appear here (I5). It is
        // CSS, so it goes where the site's CSS goes — the theme layer of the
        // theme's own sheet, which `build::css_pass` assembles from
        // `head_style()`. I4 emitted it inline in the computed head as a
        // declared interim; the head now carries no theme styles at all, and
        // a page keeps exactly one stylesheet link.
        //
        // The theme renders BODY chrome; the engine's root shell (§5g)
        // supplies doctype/<html>/<head>/<body> around it — so even a
        // theme with no root fragment yields a valid document.
        let body = self.fragments.render_body(&m);
        Ok(crate::render::root_shell(
            &head_html, locale, subtheme, profile, axis, &body,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::split_spec;

    #[test]
    fn theme_specs_split_on_colons() {
        assert_eq!(split_spec("recipes"), ("recipes", None));
        assert_eq!(
            split_spec("recipes:spicy"),
            ("recipes", Some("spicy".into()))
        );
        // Multiple tokens space-join for [data-subtheme~="…"] matching.
        assert_eq!(
            split_spec("recipes:spicy:festive"),
            ("recipes", Some("spicy festive".into()))
        );
        assert_eq!(split_spec("recipes:"), ("recipes", None));
    }

    // ------------------------------------------------- the site default
    //
    // `[site] theme` is rung 0 of the customization ladder (themes/DESIGN.md
    // §2): the cheapest way to change how a site looks, and until it existed
    // the answer was "rename a directory to `default`". It is one rewrite —
    // a row that named nothing becomes a row that named the site's theme —
    // which is why these tests are about `resolve` and not about rendering.

    /// Absent, nothing moves. This is the test that protects URL parity:
    /// `None` is what `css_of` reads as `/css/main.css`, so a site that never
    /// writes the key must keep producing the byte-identical `None`.
    #[test]
    fn no_site_theme_resolves_exactly_as_before() {
        let schemas = Schemas::engine_only();
        let root = crate::workspace_root();
        let themes = Themes::load_all(&gallery(), &root, &schemas, None).unwrap();
        assert_eq!(themes.resolve(None), (None, None));
        assert_eq!(themes.site_default(), (None, None));
    }

    /// The whole feature: a row that names nothing wears the site's spec,
    /// tokens included — so `[site] theme = "ledger:dark"` is a one-line
    /// site-wide dark mode, which is rung 2 reached from rung 0.
    #[test]
    fn a_themeless_row_wears_the_site_theme_and_its_tokens() {
        let schemas = Schemas::engine_only();
        let root = crate::workspace_root();
        let themes = Themes::load_all(&gallery(), &root, &schemas, Some("ledger:dark")).unwrap();
        assert_eq!(themes.resolve(None), (Some("ledger"), Some("dark".into())));
        // And it is the theme that actually renders — not merely a name.
        let thm = themes.get(themes.resolve(None).0).unwrap();
        assert!(std::ptr::eq(thm, themes.get(Some("ledger")).unwrap()));
    }

    /// A row that names a theme states its own tokens. The site's `dark` must
    /// not follow `terminal` around: a subtheme is a dress, and the row
    /// changed clothes. (Merging the two would make `theme: terminal` mean
    /// something different on every site that set a token.)
    #[test]
    fn a_rows_own_theme_beats_the_site_and_takes_no_tokens_with_it() {
        let schemas = Schemas::engine_only();
        let root = crate::workspace_root();
        let themes = Themes::load_all(&gallery(), &root, &schemas, Some("ledger:dark")).unwrap();
        assert_eq!(themes.resolve(Some("terminal")), (Some("terminal"), None));
        assert_eq!(
            themes.resolve(Some("terminal:wide")),
            (Some("terminal"), Some("wide".into()))
        );
    }

    /// Named explicitly, so it fails at load with the knowns — the same
    /// bargain a row's `theme:` already gets. Checking it here rather than at
    /// render time is the point: a typo in `[site] theme` would otherwise
    /// surface as an error on the first page that happened to be themeless.
    #[test]
    fn an_unknown_site_theme_is_a_load_error_naming_the_knowns() {
        let schemas = Schemas::engine_only();
        let root = crate::workspace_root();
        let err = Themes::load_all(&gallery(), &root, &schemas, Some("legder"))
            .map(|_| ())
            .expect_err("a misspelled site theme must not load")
            .to_string();
        assert!(err.contains("legder"), "{err}");
        assert!(err.contains("ledger"), "{err}");
    }

    /// `default` resolves without a directory — it is the base theme (§5e) —
    /// so a site with no `themes/` at all may still say so, and gets the
    /// floor rather than an error listing nothing.
    #[test]
    fn default_is_spellable_with_no_themes_directory() {
        let schemas = Schemas::engine_only();
        let root = crate::workspace_root();
        let none = root.join("themes-that-do-not-exist");
        let themes = Themes::load_all(&none, &root, &schemas, Some("default")).unwrap();
        assert_eq!(themes.resolve(None), (Some("default"), None));
        themes.get(Some("default")).expect("the base answers");

        // Any other name has nothing to resolve to, and says so.
        let err = Themes::load_all(&none, &root, &schemas, Some("ledger"))
            .map(|_| ())
            .expect_err("no directory, no ledger")
            .to_string();
        assert!(err.contains("(none)"), "{err}");
    }

    // ------------------------------------------------------------ gallery
    //
    // Three properties over `themes/`, the real gallery. They are the reason
    // the gallery is in the repository rather than in a gist: a theme that
    // drops a part, invents a token or hardcodes a colour should fail a
    // build, not be noticed six months later by eye.

    use super::{Theme, Themes};
    use crate::parts::{Part, PartMap, PartType, Schemas};
    use std::path::{Path, PathBuf};

    fn gallery() -> PathBuf {
        crate::workspace_root().join("themes")
    }

    fn theme_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(theme_files(&p, ext));
            } else if p.extension().is_some_and(|x| x == ext) {
                out.push(p);
            }
        }
        out.sort();
        out
    }

    /// A part map with every part of a kind filled with a traceable value.
    /// Generated from the schema rather than hand-written, so a new part in
    /// `parts.toml` is covered the day it lands. `depth` bounds the
    /// self-recursive kinds (`outline_entry.children` is a stream of itself).
    fn populate(schemas: &Schemas, kind: &str, depth: usize) -> PartMap {
        // `PartMap` keys are &'static str, and so are the schema's names.
        let name: &'static str = schemas
            .kind_names()
            .into_iter()
            .find(|k| *k == kind)
            .map(|k| Box::leak(k.to_string().into_boxed_str()) as &'static str)
            .expect("kind exists");
        let mut m = PartMap::new(name);
        let Some(parts) = schemas.get(kind) else {
            return m;
        };
        for (part, ty) in parts {
            match ty {
                PartType::Text => m.set(part, Part::Text(format!("text-{kind}-{part}"))),
                PartType::Url => m.set(part, Part::Text(format!("/url-{kind}-{part}/"))),
                PartType::Html => m.set(part, Part::Html(format!("<p>html-{kind}-{part}</p>"))),
                PartType::Flag => m.set(part, Part::Flag(true)),
                PartType::Stream(child) => {
                    if depth > 0 {
                        m.set(
                            part,
                            Part::Stream(vec![populate(schemas, child, depth - 1)]),
                        );
                    }
                }
                PartType::Map(child) => {
                    if depth > 0 {
                        m.set(part, Part::Map(populate(schemas, child, depth - 1)));
                    }
                }
            }
        }
        m
    }

    /// The first `kind.part` whose bytes did not survive, ignoring the
    /// exempt pairs at whatever depth they occur. Mirrors `parts::complete`,
    /// which is the same walk without the exemptions.
    fn first_dropped(m: &PartMap, out: &str, exempt: &[(&str, &str, &str)]) -> Option<String> {
        for (name, part) in m.iter() {
            if exempt.iter().any(|(k, p, _)| *k == m.kind && *p == name) {
                continue;
            }
            let here = format!("{}.{name}", m.kind);
            let kept = match part {
                Part::Text(v) => out.contains(crate::render::esc(v).as_str()),
                Part::Html(v) => out.contains(v.as_str()),
                Part::Flag(true) => out.contains(&format!("data-{name}")),
                Part::Flag(false) => true,
                Part::Stream(items) => {
                    if let Some(d) = items.iter().find_map(|c| first_dropped(c, out, exempt)) {
                        return Some(d);
                    }
                    true
                }
                Part::Map(sub) => {
                    if let Some(d) = first_dropped(sub, out, exempt) {
                        return Some(d);
                    }
                    true
                }
            };
            if !kept {
                return Some(here);
            }
        }
        None
    }

    /// The base is the null theme now, so it inherits the null theme's
    /// obligation: nothing the parts layer carries may silently vanish
    /// (§5e step 4, and `parts.rs`'s version of this against `canonical()`).
    ///
    /// It does not inherit it *absolutely* — an arrangement is allowed to
    /// decline a part, which is what rule 2 and partiality mean. So the
    /// exemptions are listed here, each with a reason, and the test's real
    /// job is to make adding a sixth one a deliberate act.
    #[test]
    fn the_base_theme_drops_only_what_it_means_to() {
        // (kind, part, why the base declines it)
        const EXEMPT: &[(&str, &str, &str)] = &[
            (
                "document",
                "url",
                "the page's own address — a self-link is chrome, not content, \
                 and a theme that wants a permalink places it itself",
            ),
            (
                "summary",
                "src",
                "a picture needs `summary--figure` or a theme's card: rule 2 \
                 deletes an element with an EMPTY CONTENT SLOT, and an <img> \
                 carries only attribute holes — so a plain summary that tried \
                 to show a cover would emit a broken <img> on every text row",
            ),
            ("summary", "width", "rides with `src`"),
            ("summary", "height", "rides with `src`"),
            (
                "listing",
                "featured",
                "the base has one listing shape; lifting the first item out of \
                 the stream is what `listing--cards` is for",
            ),
        ];

        let schemas = Schemas::engine_only();
        let thm = Theme::null(&crate::workspace_root(), &schemas).expect("base loads");
        for kind in schemas.kind_names() {
            // `root` is filled by `Theme::page`, not by a kind renderer.
            if kind == "root" {
                continue;
            }
            let full = populate(&schemas, kind, 2);
            let out = thm.fragments.render(&full);
            // Exemptions hold at every depth: `document.hero` is a summary,
            // so a summary's exempt parts are exempt inside it too.
            if let Some(missing) = first_dropped(&full, &out, EXEMPT) {
                panic!(
                    "the base drops `{missing}` and does not say why — either \
                     place it, or add it to EXEMPT with a reason:\n{out}"
                );
            }
        }
    }

    /// Every theme renders every kind, stamps it, and keeps its name.
    ///
    /// Deliberately weaker than completeness, because completeness is false
    /// for themes and should be: `terminal`'s summary drops tags on purpose,
    /// `summary--card` is a jacket and drops the prose, `listing--gallery`
    /// has no featured slot. An arrangement selects.
    ///
    /// What no arrangement may do is lose the thing the row is CALLED. A
    /// fragment that renders to nothing, or renders a summary with no title
    /// in it anywhere — content slot, or an `alt` attribute, which is how
    /// `summary--figure` legitimately carries one — is broken in every theme
    /// anyone would write, so it is worth failing the build over.
    #[test]
    fn every_gallery_theme_keeps_a_rows_name() {
        let schemas = Schemas::engine_only();
        let root = crate::workspace_root();
        let themes = Themes::load_all(&gallery(), &root, &schemas, None).expect("gallery loads");
        let names: Vec<String> = themes.names().map(str::to_string).collect();
        assert!(names.len() >= 6, "expected the gallery, found {names:?}");

        for name in &names {
            let thm = themes.get(Some(name)).unwrap();
            for kind in schemas.kind_names() {
                if kind == "root" {
                    continue;
                }
                let m = populate(&schemas, kind, 2);
                let out = thm.fragments.render(&m);
                assert!(
                    out.contains(&format!("data-kind=\"{kind}\"")),
                    "theme {name} rendered kind `{kind}` as `{out}`"
                );
                // Whatever this kind calls its name: title, label, or the
                // `n` a page number wears.
                for part in ["title", "label", "name", "n"] {
                    if m.get(part).is_some() {
                        let expect = format!("text-{kind}-{part}");
                        assert!(
                            out.contains(&expect),
                            "theme {name} rendered `{kind}` without its {part}:\n{out}"
                        );
                        break;
                    }
                }
            }
        }
    }

    // ------------------------------------------------- identity slots (C4)

    /// C4c: the identity set is derived from the declared PART TYPE, not from
    /// a list of names. `axes` is `stream:axis` — the engine's own
    /// language/theme switcher — so `.slots/axes.md` can no longer land in a
    /// slot the binder validated as a stream. `site_title` falls out the same
    /// way (`text`), which leaves `main` as the one hand-written name.
    ///
    /// Delete the `ty != PartType::Html` test and this fails on `axes`, and
    /// the `locale-axis` fixture panics with the switcher's stream and a
    /// prose fill set on one part.
    #[test]
    fn the_identity_slots_are_the_html_ones_the_engine_does_not_fill() {
        let schemas = Schemas::engine_only();
        let base = Theme::null(&crate::workspace_root(), &schemas).expect("base loads");
        let slots: Vec<&str> = base.identity_slots().collect();
        assert_eq!(
            slots,
            ["nav", "copyright"],
            "canonical order, from the schema"
        );
        // Stated as the thing the base's root DOES place, so this test fails
        // if someone reads the exclusion off a name list again.
        let placed: Vec<String> = base
            .fragments
            .slot_tags("root")
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        for engine in ["axes", "main", "site_title"] {
            assert!(placed.contains(&engine.to_string()), "the root places it");
            assert!(!slots.contains(&engine), "but the tree does not fill it");
        }
    }

    /// The union, and why it is one (C4b): themes ship their own roots and
    /// may place different identity slots, so a fill is dead only when NO
    /// loaded theme would read it.
    ///
    /// Two claims in one tree. `nav` is placed by `other` alone and is live
    /// anyway — that is the union. `copyright` is placed by neither, and is
    /// dead **even though the base's root places it**, because a site with
    /// a `themes/default` can never reach the base's root: the union takes
    /// the base on exactly the condition `get` does. This is the case
    /// `no_theme_root_drops_an_identity_slot` calls a live hazard, reported
    /// rather than merely linted.
    #[test]
    fn a_stem_one_loaded_theme_places_is_not_dead() {
        let dir = std::env::temp_dir().join("grackle-theme-union");
        let _ = std::fs::remove_dir_all(&dir);
        for (theme, places) in [("default", ""), ("other", "<nav data-slot=\"nav\"></nav>")] {
            let d = dir.join("themes").join(theme);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("root.html"),
                format!(
                    "<header><a data-slot=\"site_title\"></a>{places}</header>\
                     <main data-slot=\"main\"></main>"
                ),
            )
            .unwrap();
        }
        std::fs::create_dir_all(dir.join(".slots")).unwrap();
        for stem in ["nav", "copyright", "copyrite"] {
            std::fs::write(dir.join(".slots").join(format!("{stem}.md")), "words").unwrap();
        }
        let schemas = Schemas::engine_only();
        let themes =
            Themes::load_all(&dir.join("themes"), &dir, &schemas, None).expect("two roots load");
        assert_eq!(
            themes.identity_slots(),
            ["nav"],
            "one theme's slot, and the base's are out of reach"
        );
        let w = crate::slots::unknown_stems(themes.fills(), &themes.identity_slots(), &["en"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(w.len(), 2, "one theme placing `nav` is enough: {w:?}");
        assert!(w[0].contains("copyright.md"), "{}", w[0]);
        assert!(w[1].contains("copyrite.md"), "{}", w[1]);
    }

    /// Edge 2 of §3's back-tested list, as a lint rather than a hope: a
    /// theme that writes its own `root.html` takes over the root's slots
    /// too, and dropping one puts a site file — its copyright, its nav —
    /// silently dark (or, for `main`, the page itself). Seven gallery themes
    /// ship their own root, so this is a live hazard rather than a
    /// hypothetical one. Until `theme check` exists, the gallery is the
    /// corpus and this is the check.
    #[test]
    fn no_theme_root_drops_an_identity_slot() {
        let base: std::collections::HashSet<String> = slots_of(
            crate::base::fragments()
                .iter()
                .find(|(n, _)| *n == "root")
                .map(|(_, s)| *s)
                .expect("the base ships a root"),
        );
        assert!(base.contains("copyright"), "sanity: the base places it");

        for dir in std::fs::read_dir(gallery()).unwrap().flatten() {
            let root = dir.path().join("root.html");
            if !root.exists() {
                continue; // inherits the base's, nothing to drop
            }
            let own = slots_of(&std::fs::read_to_string(&root).unwrap());
            let missing: Vec<&String> = base.difference(&own).collect();
            assert!(
                missing.is_empty(),
                "{}: its root drops {missing:?} — every slot the base's root \
                 places is a place the TREE fills, so dropping one loses the \
                 site's own words with no error anywhere",
                root.display()
            );
        }
    }

    /// The `data-slot="…"` names a fragment places, content slots only.
    fn slots_of(src: &str) -> std::collections::HashSet<String> {
        let mut out = std::collections::HashSet::new();
        for (i, _) in src.match_indices("data-slot=\"") {
            let rest = &src[i + 11..];
            if let Some(end) = rest.find('"') {
                out.insert(rest[..end].to_string());
            }
        }
        out
    }

    /// The token contract (themes/README.md): a theme may add names, but it
    /// may not USE one that neither it nor the base defines. An undefined
    /// custom property is not an error in CSS — it silently resolves to
    /// nothing — so this is the only place a typo can be caught.
    #[test]
    fn no_theme_uses_a_token_nothing_defines() {
        let base = crate::base::partial("tokens").expect("base tokens");
        let defined = |src: &str| -> std::collections::HashSet<String> {
            src.lines()
                .filter_map(|l| l.trim().strip_prefix("--"))
                .filter_map(|l| l.split_once(':'))
                .map(|(n, _)| format!("--{n}"))
                .collect()
        };
        let base_tokens = defined(base);

        for dir in std::fs::read_dir(gallery()).unwrap().flatten() {
            if !dir.path().is_dir() {
                continue;
            }
            let files = theme_files(&dir.path(), "scss");
            let src: String = files
                .iter()
                .filter_map(|p| std::fs::read_to_string(p).ok())
                .collect();
            let mut known = base_tokens.clone();
            known.extend(defined(&src));
            for (i, _) in src.match_indices("var(--") {
                let rest = &src[i + 4..];
                let end = rest.find([')', ',', ' ']).unwrap_or(rest.len());
                let token = &rest[..end];
                assert!(
                    known.contains(token),
                    "{}: uses `{token}`, which neither the theme nor the base defines",
                    dir.path().display()
                );
            }
        }
    }

    /// The gallery's headline claim, as an assertion: `_tokens.scss` holds
    /// every literal a theme owns, and nothing below it names a colour or a
    /// length. Breaking this is how a theme stops being retunable by editing
    /// one file — which is the whole reason to build a gallery this way.
    #[test]
    fn no_literals_outside_a_themes_token_file() {
        let colour = regex_lite_colour;
        for dir in std::fs::read_dir(gallery()).unwrap().flatten() {
            if !dir.path().is_dir() {
                continue;
            }
            for file in theme_files(&dir.path(), "scss") {
                if file.file_name().is_some_and(|f| f == "_tokens.scss") {
                    continue;
                }
                let src = std::fs::read_to_string(&file).unwrap();
                for (n, line) in src.lines().enumerate() {
                    let code = line.split("//").next().unwrap_or("");
                    assert!(
                        !colour(code),
                        "{}:{}: names a colour below the token file — `{}`",
                        file.display(),
                        n + 1,
                        line.trim()
                    );
                }
            }
        }
    }

    /// `#abc`, `#aabbcc`, `rgb(` or `hsl(` in a declaration. Deliberately not
    /// a regex crate: this is the only pattern matching in the test suite and
    /// it is cheaper to spell out than to take a dependency for.
    fn regex_lite_colour(code: &str) -> bool {
        if code.contains("rgb(") || code.contains("hsl(") {
            return true;
        }
        let b = code.as_bytes();
        b.iter().enumerate().any(|(i, c)| {
            *c == b'#'
                && b[i + 1..]
                    .iter()
                    .take_while(|x| x.is_ascii_hexdigit())
                    .count()
                    >= 3
        })
    }
}
