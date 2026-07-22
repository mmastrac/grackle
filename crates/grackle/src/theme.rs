//! A theme is a directory of data (§5e): binder fragments + CSS. This module
//! loads one and renders full pages: the layout kind's part map through its
//! fragment, the result into the shell, identity slots filled from the tree.
//!
//! The shell's engine-provided parts are `head` (computed facts, §5a),
//! `site_title` (config) and `main` (the rendered kind). **Every other shell
//! slot is an identity slot**, resolved from `.slots/` up the source path —
//! the theme places `<p data-slot="copyright">`; the tree owns the words.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::binder::{self, Fragments};
use crate::parts::{Part, PartMap};
use crate::slots::SlotFills;

pub struct Theme {
    pub fragments: Fragments,
    fills: SlotFills,
    root: PathBuf,
    /// Shell identity slots: (schema name, element is phrasing-only).
    /// Leaked: a slot name is decided at load and lives as long as the
    /// process, and `PartMap` keys are `&'static str`.
    identity: Vec<(&'static str, bool)>,
}

/// Split a row's theme spec: the directory name before the first `:`,
/// subtheme tokens after it, space-joined — `"recipes:spicy"` renders
/// through the `recipes` theme with `subtheme = "spicy"` on the shell,
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
/// rule defaults); the site default is `default`; a site with no themes at
/// all gets the null theme — §5e's "needs no directory" made literal.
pub struct Themes {
    map: std::collections::BTreeMap<String, Theme>,
    null: Theme,
}

impl Themes {
    pub fn load_all(
        themes_dir: &Path,
        site_root: &Path,
        schemas: &crate::parts::Schemas,
    ) -> Result<Themes> {
        let mut map = std::collections::BTreeMap::new();
        if let Ok(rd) = std::fs::read_dir(themes_dir) {
            for e in rd.filter_map(|e| e.ok()) {
                if e.path().is_dir() {
                    let name = e.file_name().to_string_lossy().to_string();
                    map.insert(name, Theme::load(&e.path(), site_root, schemas)?);
                }
            }
        }
        Ok(Themes {
            map,
            null: Theme::null(site_root)?,
        })
    }

    /// Resolve a row's theme. None = the site default (`default`, or the
    /// null theme when no theme directory exists at all); a *named* theme
    /// that doesn't exist is an error listing the knowns — a row asked for
    /// it explicitly.
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
}

impl Theme {
    /// The null theme as a value: no fragments, identity fills still
    /// resolved from the tree.
    pub fn null(site_root: &Path) -> Result<Theme> {
        Ok(Theme {
            fragments: Fragments::default(),
            fills: SlotFills::load(site_root)?,
            root: site_root.to_path_buf(),
            identity: Vec::new(),
        })
    }

    pub fn load(
        theme_dir: &Path,
        site_root: &Path,
        schemas: &crate::parts::Schemas,
    ) -> Result<Theme> {
        let fragments = Fragments::load_dir(theme_dir, schemas)
            .with_context(|| format!("loading theme {}", theme_dir.display()))?;
        let fills = SlotFills::load(site_root)?;
        // Identity slots = shell slots the engine does not provide, matched
        // back to the schema so the names stay 'static and checked.
        let engine = ["main", "site_title"];
        let mut identity = Vec::new();
        for (slot, tag) in fragments.slot_tags("shell") {
            if engine.contains(&slot.as_str()) {
                continue;
            }
            let name = schemas
                .get("shell")
                .and_then(|s| s.iter().find(|(n, _)| **n == *slot).map(|(n, _)| *n))
                .with_context(|| format!("shell fragment slots unknown part `{slot}`"))?;
            identity.push((name, binder::is_phrasing_only(&tag)));
        }
        Ok(Theme {
            fragments,
            fills,
            root: site_root.to_path_buf(),
            identity,
        })
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
        resolve_link: &dyn Fn(&Path, &str) -> Result<Option<String>>,
        subtheme: Option<&str>,
        profile: Option<&str>,
    ) -> Result<String> {
        let mut m = PartMap::new("shell");
        m.set("site_title", Part::Text(site_title.to_string()));
        for (name, phrasing) in &self.identity {
            if let Some(fill) = self.fills.resolve(&self.root, source_dir, name, locale) {
                let rendered = fill.render(&|href| resolve_link(&fill.owner, href))?;
                let html = if *phrasing {
                    SlotFills::inline_or_err(&rendered)?.to_string()
                } else {
                    rendered.blocks
                };
                m.set(name, Part::Html(html));
            }
        }
        m.set("main", Part::Html(main));
        // The theme renders BODY chrome; the engine's root shell (§5g)
        // supplies doctype/<html>/<head>/<body> around it — so even a
        // theme with no shell fragment yields a valid document.
        let body = self.fragments.render_body(&m);
        Ok(crate::render::root_shell(
            &head_html, locale, subtheme, profile, &body,
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
}
