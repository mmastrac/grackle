//! The page render chain (THEME.md §1, §4): root_shell → root → row → body.
//!
//! `slot` names the rung whose `content` hole receives the body:
//! - absent — full stack (default)
//! - `root` — skip row furniture; body fills theme chrome

use anyhow::Result;
use grackle_model::AxisMember;
use std::path::Path;

use super::parts::{self, PartMap};
use super::theme::Theme;
use crate::model::Row;
use crate::render;

/// Where in the chain the row's body is spliced in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Document,
    Root,
}

impl Slot {
    pub fn parse(s: Option<&str>) -> Slot {
        match s {
            Some("root") => Slot::Root,
            _ => Slot::Document,
        }
    }

    pub fn of_row(row: Option<&Row>) -> Slot {
        let Some(row) = row else {
            return Slot::Document;
        };
        match row.fields.get("slot") {
            Some(grackle_db::Value::Str(s)) => Self::parse(Some(s)),
            _ => Slot::Document,
        }
    }
}

pub struct Page<'a> {
    pub theme: &'a Theme,
    pub head_html: String,
    pub site_title: &'a str,
    pub source_dir: &'a Path,
    /// Pairing-axis member for `.slots/<name>.{member}` resolution.
    pub lang: &'a str,
    /// Evaluated `[html.html.attribute]` / `[html.body.attribute]`.
    pub html_attrs: Vec<(String, String)>,
    pub body_attrs: Vec<(String, String)>,
    #[allow(clippy::type_complexity)]
    pub resolve_link: &'a dyn Fn(crate::links::Cite, &Path, &str) -> Result<Option<String>>,
    pub subtheme: Option<&'a str>,
    pub profile: Option<&'a str>,
    pub axis: &'a [AxisMember],
    pub axes: Vec<PartMap>,
}

/// Wrap already-rendered inner HTML in theme chrome + root_shell.
pub fn wrap(page: Page<'_>, inner: String) -> Result<String> {
    page.theme.page(
        page.head_html,
        page.site_title,
        inner,
        page.source_dir,
        page.lang,
        &page.html_attrs,
        &page.body_attrs,
        page.resolve_link,
        page.subtheme,
        page.profile,
        page.axis,
        page.axes,
    )
}

/// Render a row-shaped page: honor `slot`, then wrap.
pub fn document_page(
    page: Page<'_>,
    cfg: &crate::config::Config,
    row: Option<&Row>,
    doc: PartMap,
    body: &str,
    resolve_asset: &dyn Fn(&str) -> String,
) -> Result<String> {
    let mut doc = doc;
    if let Some(row) = row {
        parts::fill_from_fields(cfg, &mut doc, row, page.theme.schemas(), resolve_asset)?;
    }
    let inner = match Slot::of_row(row) {
        Slot::Root => body.to_string(),
        Slot::Document => page.theme.fragments.render(&doc),
    };
    wrap(page, inner)
}

/// Light tier: minimal head, no theme chrome.
pub fn light_page(
    head: &render::Head,
    html_attrs: &[(String, String)],
    body_attrs: &[(String, String)],
    profile: Option<&str>,
    axis: &[AxisMember],
    body: &str,
) -> String {
    render::root_shell(
        &render::light_head(head),
        html_attrs,
        body_attrs,
        None,
        profile,
        axis,
        &parts::canonical(&parts::raw(body)),
    )
}

/// Resolve `layout`/`variant` to a theme face and concatenate member rows
/// (THEME.md §3). Callers attach view context on the error.
pub fn member_faces(
    theme: &Theme,
    layout: &str,
    variant: Option<&str>,
    items: &[PartMap],
) -> Result<String> {
    let face = parts::member_face(&theme.fragments, layout, variant)?;
    let mut out = String::new();
    for m in items {
        out.push_str(&theme.fragments.render_with(m, Some(face)));
    }
    Ok(out)
}
