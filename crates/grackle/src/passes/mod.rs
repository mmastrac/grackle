//! Rendering passes over the route table.
//!
//! One walk of `db.routes`, dispatching each route to the pass that claims it.
//! A pass is two questions: does this route belong to me, and what bytes does
//! it produce. Everything a pass may read is in [`Ctx`], which is built once
//! and borrowed immutably, so passes cannot see each other's work and their
//! order cannot matter.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::build::{Backlink, PageBody, SiteOutput, Stats};
use crate::config::{Config, View};
use crate::db::{Route, SiteDb};
use crate::markdown::Doc;
use crate::render::Site;
use crate::theme;

pub mod listing;

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
    pub thumbs: &'a HashMap<String, crate::thumbs::Thumb>,
    /// Post bodies, held from the loader; tree bodies are re-read into
    /// `page_bodies`. A listing over tree rows finds them there.
    pub bodies: &'a HashMap<&'a grackle_db::Key, Doc>,
    pub page_bodies: &'a HashMap<String, PageBody>,
    pub linkspace: &'a crate::links::LinkSpace,
    pub backlinks: &'a HashMap<String, Vec<Backlink>>,
    pub root: PathBuf,
    pub profile: Option<&'a str>,
    /// `[html.head.meta]`, compiled once (§4e).
    pub metas: &'a crate::render::Metas,
    /// `db.object_ix` as a set: a listing asks per member whether the row IS
    /// the picture, and the membership list is a Vec.
    pub objects: std::collections::HashSet<&'a crate::db::Key>,
}

impl<'a> Ctx<'a> {
    /// Each theme compiles its own stylesheet; `default` keeps
    /// `/css/main.css` for URL parity with the reference.
    pub fn css_of(&self, theme: Option<&str>) -> String {
        match theme {
            None | Some("default") => format!("{}/css/main.css", self.cfg.site.baseurl),
            Some(n) => format!("{}/css/{n}.css", self.cfg.site.baseurl),
        }
    }

    /// The route's locale, or the site default.
    pub fn locale_of<'r>(&'r self, r: &'r Route) -> &'r str {
        r.locale.as_deref().unwrap_or(&self.cfg.i18n.default)
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

/// One rendering pass.
pub trait Pass {
    /// The `layout` this pass renders. A route claims it when its view
    /// declares this layout and does not claim a content row — a claimed view
    /// renders in the landing pass, which owns the arrangement (§5h).
    fn layout(&self) -> &'static str;

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

/// Every layout-dispatched pass, in no significant order — a route matches at
/// most one, so the order is not a precedence.
pub fn all() -> Vec<Box<dyn Pass>> {
    vec![Box::new(listing::Listing)]
}

/// Walk the route table once, dispatching to whichever pass claims each route.
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
        let Some(layout) = v.layout.as_deref() else {
            continue;
        };
        for p in passes {
            if p.layout() == layout {
                p.render(ctx, r, view, v, out, stats)?;
                break;
            }
        }
    }
    Ok(())
}
