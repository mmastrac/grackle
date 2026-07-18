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
use crate::parts::{self, Part, PartMap};
use crate::slots::SlotFills;

pub struct Theme {
    pub fragments: Fragments,
    fills: SlotFills,
    root: PathBuf,
    /// Shell identity slots: (schema name, element is phrasing-only).
    identity: Vec<(&'static str, bool)>,
}

impl Theme {
    pub fn load(theme_dir: &Path, site_root: &Path) -> Result<Theme> {
        let fragments = Fragments::load_dir(theme_dir)
            .with_context(|| format!("loading theme {}", theme_dir.display()))?;
        let fills = SlotFills::load(site_root)?;
        // Identity slots = shell slots the engine does not provide, matched
        // back to the schema so the names stay 'static and checked.
        let engine = ["head", "main", "site_title"];
        let mut identity = Vec::new();
        for (slot, tag) in fragments.slot_tags("shell") {
            if engine.contains(&slot.as_str()) {
                continue;
            }
            let name = parts::schema("shell")
                .and_then(|s| s.iter().find(|(n, _)| *n == slot).map(|(n, _)| *n))
                .with_context(|| format!("shell fragment slots unknown part `{slot}`"))?;
            identity.push((name, binder::is_phrasing_only(&tag)));
        }
        Ok(Theme { fragments, fills, root: site_root.to_path_buf(), identity })
    }

    /// Render one full page: `main` is the already-rendered layout kind;
    /// `source_dir` anchors identity-slot resolution (rows deeper in the
    /// tree can override the site's identity, nearest wins).
    pub fn page(
        &self,
        head_html: String,
        site_title: &str,
        main: String,
        source_dir: &Path,
    ) -> Result<String> {
        let mut m = PartMap::new("shell");
        m.set("head", Part::Html(head_html));
        m.set("site_title", Part::Text(site_title.to_string()));
        for (name, phrasing) in &self.identity {
            if let Some(fill) = self.fills.resolve(&self.root, source_dir, name) {
                let html = if *phrasing {
                    SlotFills::inline_or_err(fill)?.to_string()
                } else {
                    fill.blocks.clone()
                };
                m.set(name, Part::Html(html));
            }
        }
        m.set("main", Part::Html(main));
        Ok(self.fragments.render(&m))
    }
}
