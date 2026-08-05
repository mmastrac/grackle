//! Rendering passes over the route table.
//!
//! One walk of `db.routes`. Aggregate (layouted, non-landing) routes go through
//! the listing pass; `layout` / `variant` only pick the member face. Everything
//! a pass may read is in [`Ctx`], which is built once and borrowed immutably.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::{Config, View};
use crate::markdown::Doc;
use crate::model::{Route, SiteDb};
use crate::pipeline::{Backlink, PageBody, SiteOutput, Stats};
use crate::render::Site;
use crate::theme;

pub mod listing;
pub mod preview;

/// Everything a pass may read. Immutable by construction: a pass renders from
/// the database and the prepasses, never from another pass's output.
pub struct Ctx<'a> {
    pub cfg: &'a Config,
    pub db: &'a SiteDb,
    pub site: &'a Site<'a>,
    /// Every theme. A pass resolves the row's own (§5a) — there is
    /// deliberately no default-theme handle here, because reaching for one
    /// is how a route ends up rendered by two themes at once.
    pub themes: &'a theme::Themes,
    pub thumbs: &'a crate::thumbs::Renditions,
    /// Post bodies, held from the loader; tree bodies are re-read into
    /// `page_bodies`. A listing over tree rows finds them there.
    pub bodies: &'a HashMap<&'a grackle_db::Key, Doc>,
    pub page_bodies: &'a HashMap<String, PageBody>,
    pub linkspace: &'a crate::links::LinkSpace,
    pub backlinks: &'a HashMap<String, Vec<Backlink>>,
    /// Theme-to-URL map for stylesheets, resolved once after they compile.
    pub css_urls: &'a crate::assets::CssUrls,
    pub root: PathBuf,
    pub profile: Option<&'a str>,
    /// `[html.head.meta]`, compiled once (§4e).
    pub metas: &'a crate::render::Metas,
    /// `[html.html.attribute]` / `[html.body.attribute]`, compiled once (§4e).
    pub attrs: &'a crate::render::HtmlAttrs,
    /// Picture rows (`width` set): a listing asks per member whether the row IS
    /// the picture, and the membership list is a Vec.
    pub objects: std::collections::HashSet<&'a crate::model::Key>,
    /// Chrome-part facts, computed once per build.
    pub chrome: &'a preview::ChromeFacts,
}

impl<'a> Ctx<'a> {
    /// The URL a theme's stylesheet is linked at, resolved per
    /// `[assets] addressing`: `stable` keeps `/css/main.css`, `hashed` a
    /// content address.
    pub fn css_of(&self, theme: Option<&str>) -> String {
        self.css_urls.of(&self.cfg.site.baseurl, theme)
    }

    /// The route's pairing-axis member, or the site default.
    pub fn pairing_member(&self, r: &Route) -> String {
        self.cfg.pairing_member(r)
    }

    pub fn root_path(&self) -> &Path {
        &self.root
    }

    /// A listing wears its members' theme when they unanimously name one
    /// (§5h). Subtheme tokens are one row's dress and never lift; mixed or
    /// theme-less members keep the default.
    pub fn unanimous_theme(&self, r: &Route) -> Option<&'a str> {
        let mut names = r.members.iter().map(|k| {
            self.db
                .rows
                .get(k)
                .and_then(|row| row.theme.as_deref())
                .map(|s| theme::split_spec(s).0)
        });
        match names.next().flatten() {
            Some(first) if names.all(|n| n == Some(first)) => Some(first),
            _ => None,
        }
    }
}

/// One rendering pass over aggregate (layouted, non-landing) routes.
pub trait Pass {
    fn render(
        &self,
        ctx: &Ctx,
        r: &Route,
        view: &str,
        v: &View,
        out: &mut SiteOutput,
        stats: &mut Stats,
    ) -> Result<()>;
}

pub fn all() -> Vec<Box<dyn Pass>> {
    vec![Box::new(listing::Listing)]
}

/// Walk the route table once. Every layouted route that is not a landing
/// goes to the aggregate pass — `layout` / `variant` only pick the member
/// face the theme must ship.
pub fn run(
    ctx: &Ctx,
    passes: &[Box<dyn Pass>],
    out: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    for r in &ctx.db.routes {
        let Some(view) = &r.view else { continue };
        let Some(v) = ctx.cfg.views.get(view) else {
            continue;
        };
        // §5h: a route that claims a content row renders in the landing pass —
        // view-level for a literal `content`, or per-route (`Route.content`)
        // when a templated `content`/`default_content` resolved to a row for
        // THIS route. A templated `default_content` offer declined by a route
        // leaves `Route.content` unset, so that route lists here as usual.
        if v.content.is_some() || r.content.is_some() {
            continue;
        }
        if v.layout.is_none() {
            continue;
        }
        for p in passes {
            p.render(ctx, r, view, v, out, stats)?;
        }
    }
    Ok(())
}
