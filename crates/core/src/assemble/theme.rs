//! A theme is a directory of data (§5e): binder fragments + CSS. This module
//! loads one and renders full pages: the layout kind's part map through its
//! fragment, the result into the root chrome, identity slots filled from the
//! tree.
//!
//! The chrome file is **`root.html`** and the kind it binds is `root`. It may be a bare fragment — which is the body chrome, and what every
//! theme here writes — or document-shaped, with a `<head>` fenced to `<style>`
//! and a `<body>`; `binder::split_root` is that split, and the engine keeps
//! owning `<html>` and the computed head either way. The head half leaves as
//! CSS: `head_style()` is read by the CSS assembly, never by a page.
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

use super::binder::{self, Fragments};
use super::parts::{Part, PartMap, PartType, Schemas};
use crate::base;
use crate::slots::SlotFills;

pub struct Theme {
    pub fragments: Fragments,
    /// Part vocabulary for this theme: derived from fragments + field schemas.
    schemas: Schemas,
    fills: SlotFills,
    root: PathBuf,
    identity: Vec<(&'static str, bool)>,
    style: String,
    manifest: Manifest,
    /// Positional `.slots/chrome.html` overrides: owner directory → the
    /// fragment name each registered under. Resolved nearest-wins per page,
    /// the same ascent as every other fill.
    chrome_overrides: std::collections::BTreeMap<PathBuf, String>,
}

/// A theme's `theme.toml`: today only `[subthemes]`, the declared token
/// vocabulary a spec may name after the colon. A token may carry scheme
/// semantics. A theme with no manifest declares nothing: its tokens stay
/// unvalidated (the pre-manifest behavior) and it offers no scheme choice.
#[derive(Debug, Default)]
pub struct Manifest {
    declared: bool,
    subthemes: std::collections::BTreeMap<String, Option<Scheme>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Dark,
    Light,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    name: Option<String>,
    extends: Option<toml::Value>,
    contract: Option<toml::Value>,
    #[serde(default)]
    subthemes: std::collections::BTreeMap<String, SubthemeDef>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SubthemeDef {
    scheme: Option<String>,
}

impl Manifest {
    /// `what` names the theme in errors; `dir_name` is the identity the
    /// optional `name` key must agree with (None for the base).
    fn parse(src: &str, what: &str, dir_name: Option<&str>) -> Result<Manifest> {
        let file: ManifestFile =
            toml::from_str(src).with_context(|| format!("{what}: theme.toml"))?;
        if file.extends.is_some() || file.contract.is_some() {
            anyhow::bail!(
                "{what}: theme.toml declares `extends`/`contract`, which are specced and \
                 unbuilt (TODO-1.0.md, the theme ladder) — a derived theme cannot load \
                 yet; copy the parent's files instead"
            );
        }
        if let (Some(name), Some(dir)) = (file.name.as_deref(), dir_name) {
            if name != dir {
                anyhow::bail!(
                    "{what}: theme.toml says name = {name:?} but the directory is \
                     {dir:?} — the directory name is a theme's identity"
                );
            }
        }
        let mut subthemes = std::collections::BTreeMap::new();
        for (token, def) in file.subthemes {
            let scheme = match def.scheme.as_deref() {
                None => None,
                Some("dark") => Some(Scheme::Dark),
                Some("light") => Some(Scheme::Light),
                Some(other) => anyhow::bail!(
                    "{what}: theme.toml subtheme {token:?} declares scheme = {other:?} — \
                     a scheme is \"dark\" or \"light\""
                ),
            };
            subthemes.insert(token, scheme);
        }
        Ok(Manifest {
            declared: true,
            subthemes,
        })
    }

    fn scheme_of(&self, token: &str) -> Option<Scheme> {
        self.subthemes.get(token).copied().flatten()
    }

    /// Both schemes declared — the scheme chrome part's capability fact.
    fn covers_both_schemes(&self) -> bool {
        let mut dark = false;
        let mut light = false;
        for s in self.subthemes.values().flatten() {
            match s {
                Scheme::Dark => dark = true,
                Scheme::Light => light = true,
            }
        }
        dark && light
    }
}

/// Localized inputs for the chrome parts, built per page by
/// `preview::chrome_input`. `None` means the capability's fact is absent
/// (no route wears that shell) and the part stays empty, so the theme's
/// hole deletes. The scheme fill decision is the theme's
/// (`scheme_choice_offered`) and only its labels arrive here.
#[derive(Debug, Default, Clone)]
pub struct ChromeInput {
    /// (label, loader URL) when a `shell = "search"` route materialized.
    pub search: Option<(String, String)>,
    /// (label, feed URL) when a `shell = "atom"` route materialized.
    pub feed: Option<(String, String)>,
    pub scheme: SchemeLabels,
    /// The skip link's label (`@skip`). Not a capability — the link is
    /// always correct — but the label is words, and words localize. Empty
    /// (an `extends = "none"` site that declares no string) drops the link.
    pub skip: String,
}

#[derive(Debug, Default, Clone)]
pub struct SchemeLabels {
    pub auto: String,
    pub light: String,
    pub dark: String,
}

/// Applies the reader's stored scheme choice before any content paints.
/// Emitted as the first thing in `<body>` whenever the scheme control is
/// offered, so a page never flashes the wrong palette. Touches only the two
/// scheme tokens, so every other subtheme token survives.
const SCHEME_BOOT: &str = "<script>try{var s=localStorage.getItem('grackle:scheme');\
if(s=='dark'||s=='light'){var h=document.documentElement,\
t=(h.getAttribute('data-subtheme')||'').split(' ')\
.filter(function(x){return x&&x!='dark'&&x!='light'});t.push(s);\
h.setAttribute('data-subtheme',t.join(' '))}}catch(e){}</script>";

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

/// Stylesheet URL for a theme name (`None` / `default` keep `/css/main.css`).
pub fn css_url(baseurl: &str, theme: Option<&str>) -> String {
    match theme {
        None | Some("default") => format!("{baseurl}/css/main.css"),
        Some(n) => format!("{baseurl}/css/{n}.css"),
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
        fields: &[(String, grackle_source::schema::FieldType)],
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
                    map.insert(name, Theme::load(&e.path(), site_root, fields)?);
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
                        crate::util::join_or_none(&known)
                    );
                }
                (Some(n.to_string()), sub)
            }
            None => (None, None),
        };
        let themes = Themes {
            map,
            null: Theme::null(site_root, fields)?,
            site_name,
            site_sub,
        };
        if let Some(spec) = site_theme {
            themes
                .check_spec(spec)
                .with_context(|| format!("[site] theme = {spec:?}"))?;
        }
        Ok(themes)
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
                    crate::util::join_or_none(&known)
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
    /// The union is the point. Themes ship their own roots and may
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

    /// Validate a full theme spec — name and subtheme tokens. The name half
    /// is `get`'s existing error; the token half is the manifest's
    /// declaration, and a theme without one validates nothing.
    pub fn check_spec(&self, spec: &str) -> Result<()> {
        let (name, tokens) = split_spec(spec);
        let thm = self.get(Some(name))?;
        if let Some(tokens) = tokens {
            for t in tokens.split(' ') {
                thm.check_token(t, name)?;
            }
        }
        Ok(())
    }
}

impl Theme {
    /// The null theme as a value — which is the BASE theme, not an empty one.
    /// A site with no `themes/` directory renders through the fragments the
    /// engine carries, so "no theme" means plain, never broken.
    pub fn null(
        site_root: &Path,
        fields: &[(String, grackle_source::schema::FieldType)],
    ) -> Result<Theme> {
        let manifest = Manifest::parse(base::manifest(), "the base theme", None)?;
        Theme::from_sources(
            Vec::new(),
            site_root,
            fields,
            None,
            "the base theme",
            manifest,
        )
    }

    pub fn load(
        theme_dir: &Path,
        site_root: &Path,
        fields: &[(String, grackle_source::schema::FieldType)],
    ) -> Result<Theme> {
        if theme_dir.join("shell.html").exists() && !theme_dir.join("root.html").exists() {
            anyhow::bail!(
                "{}: `shell.html` is `root.html` now — the chrome part kind renamed \
                 shell → root, and a theme root may carry a <head> as well as a body. \
                 Rename the file; its contents are the body chrome, unchanged.",
                theme_dir.display()
            );
        }
        let own = binder::dir_sources(theme_dir)
            .with_context(|| format!("loading theme {}", theme_dir.display()))?;
        let what = theme_dir.display().to_string();
        let manifest = match std::fs::read_to_string(theme_dir.join("theme.toml")) {
            Ok(src) => {
                let dir_name = theme_dir.file_name().and_then(|n| n.to_str());
                Manifest::parse(&src, &what, dir_name)?
            }
            Err(_) => Manifest::default(),
        };
        Theme::from_sources(own, site_root, fields, Some(theme_dir), &what, manifest)
    }

    pub fn schemas(&self) -> &Schemas {
        &self.schemas
    }

    fn from_sources(
        mut own: Vec<(String, String, String)>,
        site_root: &Path,
        fields: &[(String, grackle_source::schema::FieldType)],
        theme_dir: Option<&Path>,
        what: &str,
        manifest: Manifest,
    ) -> Result<Theme> {
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
        let base_sources = base::base_sources();
        let mut fragments =
            Fragments::parse(base_sources).with_context(|| format!("parsing theme {what}"))?;
        // Overlay after base parse so inline defaults from a replaced parent
        // (e.g. base `row` → crumb/tag) remain unless the theme ships that name.
        if !own.is_empty() {
            fragments
                .overlay(own)
                .with_context(|| format!("parsing theme {what}"))?;
        }
        // The site's cluster overrides: each `.slots/chrome.html` is a binder
        // fragment shadowing `chrome` for its subtree over EVERY theme — the
        // tree overlay rung of the precedence law applied to a fragment-
        // bearing slot, and the place a site author mints chrome of their own
        // (literal markup may sit beside the engine holes). Positional like
        // every other fill: each registers as a variant of the `chrome` kind,
        // and `page` resolves the nearest one up the source path.
        let fills = SlotFills::load(site_root)?;
        let mut chrome_overrides = std::collections::BTreeMap::new();
        for (i, (owner, file, src)) in fills.chrome_sources().into_iter().enumerate() {
            let name = format!("chrome--{i}");
            fragments
                .overlay(vec![(name.clone(), src, file.display().to_string())])
                .with_context(|| format!("parsing {}", file.display()))?;
            chrome_overrides.insert(owner, name);
        }
        let mut schemas = Schemas::derive(&fragments, fields);
        if let Some(dir) = theme_dir {
            schemas = schemas.extend_theme_dir(dir)?;
        }
        fragments
            .validate_against(&schemas)
            .with_context(|| format!("loading theme {what}"))?;
        let engine = ["content"];
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
            schemas,
            fills,
            root: site_root.to_path_buf(),
            identity,
            style,
            manifest,
            chrome_overrides,
        })
    }

    /// Validate one subtheme token against the manifest. A theme with no
    /// `theme.toml` declares nothing and validates nothing — the pre-manifest
    /// behavior, kept so undeclared themes keep loading.
    fn check_token(&self, token: &str, theme_name: &str) -> Result<()> {
        if !self.manifest.declared || self.manifest.subthemes.contains_key(token) {
            return Ok(());
        }
        let known: Vec<&str> = self.manifest.subthemes.keys().map(String::as_str).collect();
        anyhow::bail!(
            "theme {theme_name:?} declares no subtheme {token:?} — declared in its \
             theme.toml: {}",
            crate::util::join_or_none(&known)
        )
    }

    /// The scheme chrome part's fill rule: the theme declares both schemes,
    /// and no rung of the cascade forced one. A spec wearing a scheme token
    /// has decided, so the offered control stands down.
    pub fn scheme_choice_offered(&self, subtheme: Option<&str>) -> bool {
        self.manifest.covers_both_schemes()
            && !subtheme.is_some_and(|toks| {
                toks.split(' ')
                    .any(|t| self.manifest.scheme_of(t).is_some())
            })
    }

    /// The root slots this theme leaves for the tree to fill — what a
    /// `.slots/` file may be named for this theme (§5e).
    pub fn identity_slots(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.identity.iter().map(|(n, _)| *n)
    }

    /// The CSS this theme's `root.html` declared in its `<head>`,
    /// empty for the body-only roots every theme in the repository writes.
    ///
    /// Its one reader is `shells::css::css_pass`, which compiles it into the
    /// theme's sheet. **No page ever sees it**: the head fence exists so
    /// that a theme's presentation can join the one CSS artifact, and a page
    /// carrying an inline `<style>` as well as the stylesheet link would be
    /// the second artifact the model says does not exist.
    pub fn head_style(&self) -> &str {
        &self.style
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    /// Render one full page: `main` is the already-rendered layout kind;
    /// `source_dir` anchors identity-slot resolution (rows deeper in the
    /// tree can override the site's identity, nearest wins); `subtheme`
    /// is the row's `theme:` colon suffix, if any.
    /// `resolve_link` sees every markdown link in a fill, with the fill's
    /// OWNER directory as its relative base (§6a) — this is how one nav.md
    /// with `view:` links serves every pairing member: resolution runs per page.
    pub fn page(
        &self,
        head_html: String,
        site_title: &str,
        main: String,
        source_dir: &Path,
        lang: &str,
        html_attrs: &[(String, String)],
        body_attrs: &[(String, String)],
        resolve_link: &dyn Fn(crate::links::Cite, &Path, &str) -> Result<Option<String>>,
        subtheme: Option<&str>,
        profile: Option<&str>,
        axis: &[grackle_model::AxisMember],
        axes: Vec<PartMap>,
        chrome: &ChromeInput,
    ) -> Result<String> {
        let mut m = PartMap::new("root");
        m.set("site_title", Part::Text(site_title.to_string()));
        // Chrome parts: each fills iff its fact holds, at whichever level the
        // resolved root reads it — a slot the root places directly wins, the
        // `chrome` cluster takes the rest, and a root placing neither drops
        // the part. Placement is the theme's option, presence is the fact's.
        let root_slots: std::collections::BTreeSet<String> = self
            .fragments
            .slot_tags("root")
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        // The skip link is not a widget — no fact gates it — but its label
        // is words, and words localize. Root-level only.
        if !chrome.skip.is_empty() && root_slots.contains("skip") {
            m.set("skip", Part::Text(chrome.skip.clone()));
        }
        let mut cluster = PartMap::new("chrome");
        // The nearest positional override up the source path renders the
        // cluster for this page — the fills' ascent, applied to a fragment.
        let chrome_face = {
            let mut cur = source_dir;
            loop {
                if let Some(name) = self.chrome_overrides.get(cur) {
                    break Some(name.clone());
                }
                if cur == self.root {
                    break None;
                }
                match cur.parent() {
                    Some(p) => cur = p,
                    None => break None,
                }
            }
        };
        if let Some(name) = &chrome_face {
            cluster.set_face(name.clone());
        }
        let mut place = |name: &'static str, part: Part| {
            if root_slots.contains(name) {
                m.set(name, part);
            } else if root_slots.contains("chrome") {
                cluster.set(name, part);
            }
        };
        if !axes.is_empty() {
            place("axes", Part::Stream(axes));
        }
        if let Some((label, js)) = &chrome.search {
            let mut b = PartMap::new("search_button");
            b.set("label", Part::Text(label.clone()));
            b.set("js", Part::Text(js.clone()));
            place("search", Part::Map(b));
        }
        if let Some((label, url)) = &chrome.feed {
            let mut f = PartMap::new("feed_link");
            f.set("url", Part::Text(url.clone()));
            f.set("label", Part::Text(label.clone()));
            place("feed", Part::Map(f));
        }
        let scheme_on = self.scheme_choice_offered(subtheme);
        if scheme_on {
            let mut s = PartMap::new("scheme_button");
            s.set("label", Part::Text(chrome.scheme.auto.clone()));
            s.set("label_auto", Part::Text(chrome.scheme.auto.clone()));
            s.set("label_light", Part::Text(chrome.scheme.light.clone()));
            s.set("label_dark", Part::Text(chrome.scheme.dark.clone()));
            place("scheme", Part::Map(s));
        }
        // An override with only literal markup still renders: the author's
        // chrome is content even when no engine hole fills.
        if !cluster.is_empty() || (chrome_face.is_some() && root_slots.contains("chrome")) {
            m.set("chrome", Part::Map(cluster));
        }
        for (name, phrasing) in &self.identity {
            if let Some(fill) = self.fills.resolve(&self.root, source_dir, name, lang) {
                let rendered = fill.render(&|form, href| resolve_link(form, &fill.owner, href))?;
                let html = if *phrasing {
                    SlotFills::inline_or_err(&rendered)?.to_string()
                } else {
                    rendered.blocks
                };
                m.set(name, Part::Html(html));
            }
        }
        m.set("content", Part::Html(main));
        let mut body = self.fragments.render_body(&m);
        if scheme_on {
            body = format!("{SCHEME_BOOT}\n{body}");
        }
        Ok(crate::render::root_shell(
            &head_html, html_attrs, body_attrs, subtheme, profile, axis, &body,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{split_spec, Theme, Themes};
    use crate::parts::first_dropped;
    use std::path::{Path, PathBuf};

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

    // `[site] theme`: row-less resolve rewrites; tokens stay with the named theme.
    #[test]
    fn site_theme_resolve() {
        let root = crate::workspace_root();
        let fields: &[(String, grackle_source::schema::FieldType)] = &[];

        let none = Themes::load_all(&gallery(), &root, fields, None).unwrap();
        assert_eq!(none.resolve(None), (None, None));
        assert_eq!(none.site_default(), (None, None));

        let site = Themes::load_all(&gallery(), &root, fields, Some("ledger:dark")).unwrap();
        assert_eq!(site.resolve(None), (Some("ledger"), Some("dark".into())));
        assert!(std::ptr::eq(
            site.get(site.resolve(None).0).unwrap(),
            site.get(Some("ledger")).unwrap()
        ));
        assert_eq!(site.resolve(Some("terminal")), (Some("terminal"), None));
        assert_eq!(
            site.resolve(Some("terminal:wide")),
            (Some("terminal"), Some("wide".into()))
        );

        let err = Themes::load_all(&gallery(), &root, fields, Some("legder"))
            .map(|_| ())
            .expect_err("misspelled site theme")
            .to_string();
        assert!(err.contains("legder") && err.contains("ledger"), "{err}");

        let empty = root.join("themes-that-do-not-exist");
        let base = Themes::load_all(&empty, &root, fields, Some("default")).unwrap();
        assert_eq!(base.resolve(None), (Some("default"), None));
        base.get(Some("default")).expect("base answers");
        let err = Themes::load_all(&empty, &root, fields, Some("ledger"))
            .map(|_| ())
            .expect_err("no directory")
            .to_string();
        assert!(err.contains("(none)"), "{err}");
    }

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

    /// Base theme may decline listed parts; anything else must survive.
    #[test]
    fn the_base_theme_drops_only_what_it_means_to() {
        // (kind, part, why)
        const EXEMPT: &[(&str, &str, &str)] = &[
            (
                "row",
                "url",
                "self-link is chrome; themes that want a permalink place it",
            ),
            ("row", "date", "member faces place dates"),
            ("row", "date_pretty", "rides with date"),
            ("row", "description", "member faces place the blurb"),
            ("row", "truncated", "card CSS fact; default face has no cue"),
            (
                "chrome",
                "feed",
                "the base places feed in the footer; the cluster hole exists \
                 for a chrome.html that wants it in the widget row",
            ),
        ];

        let thm = Theme::null(&crate::workspace_root(), &[]).expect("base loads");
        let schemas = thm.schemas();
        for kind in schemas.kind_names() {
            // `root` is filled by `Theme::page`, not by a kind renderer.
            if kind == "root" {
                continue;
            }
            // `relation.items` holds full rows; `row--neighbor` chops them.
            // Depth 0 leaves items unset so this test measures the relation
            // fragment itself, not every row part the face declines.
            let depth = if kind == "relation" { 0 } else { 2 };
            let full = crate::parts::populate(&schemas, kind, depth);
            let out = thm.fragments.render(&full);
            // Exemptions hold at every depth: nested rows reuse the same
            // part names, so a summary's exempt parts are exempt inside it too.
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
    /// `row--card` is a jacket and drops the prose. An arrangement selects.
    ///
    /// What no arrangement may do is lose the thing the row is CALLED. A
    /// fragment that renders to nothing, or renders a summary with no title
    /// in it anywhere — content slot, or an `alt` attribute, which is how
    /// `summary--figure` legitimately carries one — is broken in every theme
    /// anyone would write, so it is worth failing the build over.
    #[test]
    fn every_gallery_theme_keeps_a_rows_name() {
        let root = crate::workspace_root();
        let themes = Themes::load_all(&gallery(), &root, &[], None).expect("gallery loads");
        let schemas = themes.get(None).unwrap().schemas();
        let names: Vec<String> = themes.names().map(str::to_string).collect();
        assert!(names.len() >= 6, "expected the gallery, found {names:?}");

        for name in &names {
            let thm = themes.get(Some(name)).unwrap();
            for kind in schemas.kind_names() {
                if kind == "root" {
                    continue;
                }
                let m = crate::parts::populate(schemas, kind, 2);
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

    // ------------------------------------------------- identity slots

    /// The identity set is derived from the declared PART TYPE, not from
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
        let base = Theme::null(&crate::workspace_root(), &[]).expect("base loads");
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
        for engine in ["chrome", "content", "site_title"] {
            assert!(placed.contains(&engine.to_string()), "the root places it");
            assert!(!slots.contains(&engine), "but the tree does not fill it");
        }
    }

    /// The union, and why it is one: themes ship their own roots and
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
                     <main data-slot=\"content\"></main>"
                ),
            )
            .unwrap();
        }
        std::fs::create_dir_all(dir.join(".slots")).unwrap();
        for stem in ["nav", "copyright", "copyrite"] {
            std::fs::write(dir.join(".slots").join(format!("{stem}.md")), "words").unwrap();
        }
        let themes =
            Themes::load_all(&dir.join("themes"), &dir, &[], None).expect("two roots load");
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
            super::base::fragments()
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
            let missing: Vec<&String> = base
                .difference(&own)
                // Capability slots are a theme's option: dropping one loses
                // a widget, not the site's words, and the capability-without-
                // slot warning is the guard there.
                .filter(|s| !["search", "scheme", "feed"].contains(&s.as_str()))
                .collect();
            assert!(
                missing.is_empty(),
                "{}: its root drops {missing:?} — every identity slot the \
                 base's root places is a place the TREE fills, so dropping one \
                 loses the site's own words with no error anywhere",
                root.display()
            );
        }
    }

    /// The `data-slot="…"` names a fragment places on its own holes (after
    /// inline fragment defaults are extracted — nested child slots do not
    /// count as the parent's).
    fn slots_of(src: &str) -> std::collections::HashSet<String> {
        let f = crate::assemble::Fragments::parse(vec![(
            "root".into(),
            src.to_string(),
            "root.html".into(),
        )])
        .expect("root parses");
        f.slot_tags("root").into_iter().map(|(s, _)| s).collect()
    }

    // ------------------------------------------------- chrome parts

    /// The manifest's refusals, each naming its rule: `extends` is specced
    /// and unbuilt; a scheme is dark or light; `name` must agree with the
    /// directory; unknown keys are unknown.
    #[test]
    fn a_manifest_refuses_what_it_cannot_honor() {
        let err = |src: &str| {
            format!(
                "{:#}",
                super::Manifest::parse(src, "themes/t", Some("t")).expect_err("should refuse")
            )
        };
        assert!(err("extends = \"ledger\"").contains("unbuilt"));
        assert!(err("contract = 1").contains("unbuilt"));
        assert!(err("name = \"other\"").contains("identity"));
        assert!(err("[subthemes]\nd = { scheme = \"drak\" }").contains("\"dark\" or \"light\""));
        assert!(err("wat = 1").contains("wat"));

        let m = super::Manifest::parse(
            "name = \"t\"\n[subthemes]\ndark = { scheme = \"dark\" }\nwide = { }\n",
            "themes/t",
            Some("t"),
        )
        .expect("a good manifest parses");
        assert!(m.declared && !m.covers_both_schemes());
        assert_eq!(m.scheme_of("dark"), Some(super::Scheme::Dark));
        assert_eq!(m.scheme_of("wide"), None);
    }

    /// Spec tokens validate against the declaration — and only against one:
    /// a theme with no theme.toml declares nothing and keeps accepting any
    /// token, which is what lets undeclared themes load unchanged.
    #[test]
    fn subtheme_tokens_validate_against_the_manifest() {
        let dir = std::env::temp_dir().join("grackle-subtheme-validation");
        let _ = std::fs::remove_dir_all(&dir);
        for (theme, manifest) in [("declared", true), ("grandfathered", false)] {
            let d = dir.join("themes").join(theme);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("root.html"),
                "<a data-slot=\"site_title\"></a><main data-slot=\"content\"></main>",
            )
            .unwrap();
            if manifest {
                std::fs::write(
                    d.join("theme.toml"),
                    "[subthemes]\ndark = { scheme = \"dark\" }\n",
                )
                .unwrap();
            }
        }
        let themes = Themes::load_all(&dir.join("themes"), &dir, &[], None).expect("load");
        themes.check_spec("declared:dark").expect("declared token");
        themes
            .check_spec("grandfathered:anything")
            .expect("no manifest, no validation");
        let err = themes
            .check_spec("declared:drak")
            .expect_err("undeclared token")
            .to_string();
        assert!(err.contains("drak") && err.contains("dark"), "{err}");

        // The same check guards `[site] theme` at load.
        let err = Themes::load_all(&dir.join("themes"), &dir, &[], Some("declared:drak"))
            .map(|_| ())
            .expect_err("site spec validates")
            .to_string();
        assert!(err.contains("drak"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The chrome fill rules end to end through `Theme::page`: a fact fills
    /// its part, an absent fact leaves nothing, and a forced scheme token
    /// stands the offered control down.
    #[test]
    fn chrome_parts_fill_from_facts_and_stand_down() {
        let root = crate::workspace_root();
        let thm = Theme::null(&root, &[]).expect("base loads");
        let no_link = |_: crate::links::Cite, _: &Path, _: &str| Ok(None);
        let page = |subtheme: Option<&str>, chrome: &super::ChromeInput| {
            thm.page(
                String::new(),
                "T",
                "<p>hi</p>".into(),
                &root,
                "en",
                &[],
                &[],
                &no_link,
                subtheme,
                None,
                &[],
                Vec::new(),
                chrome,
            )
            .expect("page renders")
        };

        let full = super::ChromeInput {
            search: Some(("Search".into(), "/search.js".into())),
            feed: Some(("Feed".into(), "/atom.xml".into())),
            scheme: super::SchemeLabels {
                auto: "Colors: auto".into(),
                light: "Colors: light".into(),
                dark: "Colors: dark".into(),
            },
            skip: "Skip to content".into(),
        };
        let html = page(None, &full);
        assert!(html.contains("data-search-js=\"/search.js\""), "{html}");
        assert!(html.contains("href=\"/atom.xml\""), "{html}");
        assert!(html.contains("Feed"), "{html}");
        // The base declares both schemes, so the control and its boot
        // script ship; the boot precedes the body chrome.
        assert!(html.contains("data-l-dark=\"Colors: dark\""), "{html}");
        assert!(html.contains("grackle:scheme"), "{html}");

        // No facts: no search button, no feed link — the parts are empty and
        // rule 2 deletes their elements. The scheme control still ships
        // (the theme capability is the fact).
        let bare = page(None, &super::ChromeInput::default());
        assert!(!bare.contains("data-search-js"), "{bare}");
        assert!(!bare.contains("/atom.xml"), "{bare}");

        // A forced scheme token stands the control down: no button, no boot.
        let forced = page(Some("dark"), &full);
        assert!(!forced.contains("grackle:scheme"), "{forced}");
        assert!(!forced.contains("data-l-dark"), "{forced}");
        // A non-scheme token does not.
        let wide = page(Some("wide"), &full);
        assert!(wide.contains("grackle:scheme"), "{wide}");
    }

    /// First writer per part: a slot the root places directly wins, and the
    /// cluster's copy of that part empties — nothing renders twice. The rest
    /// of the widgets keep arriving through the cluster.
    #[test]
    fn an_individually_placed_slot_beats_the_clusters_copy() {
        let dir = std::env::temp_dir().join("grackle-chrome-split");
        let _ = std::fs::remove_dir_all(&dir);
        let d = dir.join("themes").join("split");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("root.html"),
            "<a data-slot=\"site_title\"></a>\
             <span data-slot=\"search\" data-fragment=\"search_button\"></span>\
             <div data-slot=\"chrome\" data-fragment=\"chrome\"></div>\
             <main data-slot=\"content\"></main>",
        )
        .unwrap();
        std::fs::write(
            d.join("theme.toml"),
            "[subthemes]\ndark = { scheme = \"dark\" }\nlight = { scheme = \"light\" }\n",
        )
        .unwrap();
        let themes = Themes::load_all(&dir.join("themes"), &dir, &[], None).expect("loads");
        let thm = themes.get(Some("split")).unwrap();
        let html = render_chrome_page(thm, &dir, &dir, None, &full_chrome());
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            html.matches("data-search-js").count(),
            1,
            "one search button, not two:\n{html}"
        );
        assert!(
            html.find("data-search-js").unwrap() < html.find("data-slot=\"chrome\"").unwrap(),
            "the direct placement renders where the root put it:\n{html}"
        );
        assert!(
            html.contains("data-l-dark"),
            "cluster keeps scheme:\n{html}"
        );
    }

    /// The site's `.slots/chrome.html` shadows the `chrome` fragment across
    /// every theme: the author reorders the widgets and mints chrome of
    /// their own beside the engine holes, no theme touched.
    #[test]
    fn a_site_chrome_html_reorders_and_mints() {
        let dir = std::env::temp_dir().join("grackle-chrome-site");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".slots")).unwrap();
        std::fs::write(
            dir.join(".slots/chrome.html"),
            "<span data-slot=\"scheme\" data-fragment=\"scheme_button\"></span>\
             <details data-chrome=\"dropdown\"><summary>Elsewhere</summary>\
             <a href=\"/x/\">x</a></details>\
             <span data-slot=\"search\" data-fragment=\"search_button\"></span>",
        )
        .unwrap();
        let thm = Theme::null(&dir, &[]).expect("base loads with the override");
        let html = render_chrome_page(&thm, &dir, &dir, None, &full_chrome());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(html.contains("Elsewhere"), "author chrome renders:\n{html}");
        assert!(
            html.find("data-l-dark").unwrap() < html.find("Elsewhere").unwrap()
                && html.find("Elsewhere").unwrap() < html.find("data-search-js").unwrap(),
            "the override's order stands: scheme, author markup, search:\n{html}"
        );
    }

    /// Positional overrides resolve nearest-wins per page, the fills'
    /// ascent applied to a fragment: the root's file answers for the site,
    /// a subtree's file answers beneath it, and each may drop holes the
    /// other places.
    #[test]
    fn positional_chrome_resolves_nearest_per_page() {
        let dir = std::env::temp_dir().join("grackle-chrome-positional");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".slots")).unwrap();
        std::fs::create_dir_all(dir.join("docs/.slots")).unwrap();
        std::fs::write(
            dir.join(".slots/chrome.html"),
            "<i>ROOTMARK</i><span data-slot=\"search\" data-fragment=\"search_button\"></span>",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/.slots/chrome.html"),
            "<i>DOCSMARK</i><span data-slot=\"scheme\" data-fragment=\"scheme_button\"></span>",
        )
        .unwrap();
        let thm = Theme::null(&dir, &[]).expect("base loads");

        let at_root = render_chrome_page(&thm, &dir, &dir, None, &full_chrome());
        assert!(
            at_root.contains("ROOTMARK") && !at_root.contains("DOCSMARK"),
            "{at_root}"
        );
        assert!(at_root.contains("data-search-js"), "{at_root}");

        let deep = dir.join("docs/deep");
        let under_docs = render_chrome_page(&thm, &dir, &deep, None, &full_chrome());
        assert!(
            under_docs.contains("DOCSMARK") && !under_docs.contains("ROOTMARK"),
            "{under_docs}"
        );
        assert!(
            !under_docs.contains("data-search-js") && under_docs.contains("data-l-dark"),
            "the docs override dropped the search hole and kept scheme:\n{under_docs}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An override that is only author markup renders even when no engine
    /// part fills — the author's chrome is content in its own right.
    #[test]
    fn an_author_only_override_renders_without_facts() {
        let dir = std::env::temp_dir().join("grackle-chrome-authoronly");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".slots")).unwrap();
        std::fs::write(dir.join(".slots/chrome.html"), "<b>ELSEMARK</b>").unwrap();
        let thm = Theme::null(&dir, &[]).expect("base loads");
        // No facts, and a forced scheme stands the control down: the cluster
        // map is empty, and only the resolved override keeps it rendering.
        let html = render_chrome_page(
            &thm,
            &dir,
            &dir,
            Some("dark"),
            &super::ChromeInput::default(),
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert!(html.contains("ELSEMARK"), "{html}");
    }

    fn full_chrome() -> super::ChromeInput {
        super::ChromeInput {
            search: Some(("Search".into(), "/search.js".into())),
            feed: Some(("Feed".into(), "/atom.xml".into())),
            scheme: super::SchemeLabels {
                auto: "Colors: auto".into(),
                light: "Colors: light".into(),
                dark: "Colors: dark".into(),
            },
            skip: "Skip to content".into(),
        }
    }

    fn render_chrome_page(
        thm: &Theme,
        _root: &Path,
        source_dir: &Path,
        subtheme: Option<&str>,
        chrome: &super::ChromeInput,
    ) -> String {
        let no_link = |_: crate::links::Cite, _: &Path, _: &str| Ok(None);
        thm.page(
            String::new(),
            "T",
            "<p>hi</p>".into(),
            source_dir,
            "en",
            &[],
            &[],
            &no_link,
            subtheme,
            None,
            &[],
            Vec::new(),
            chrome,
        )
        .expect("page renders")
    }

    /// The token contract (themes/README.md): a theme may add names, but it
    /// may not USE one that neither it nor the base defines. An undefined
    /// custom property is not an error in CSS — it silently resolves to
    /// nothing — so this is the only place a typo can be caught.
    #[test]
    fn no_theme_uses_a_token_nothing_defines() {
        let base = super::base::partial("tokens").expect("base tokens");
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
