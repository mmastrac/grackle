//! The page render chain (THEME.md §1, §4): root_shell → root → row → body.
//!
//! `slot` names the rung whose `content` hole receives the body:
//! - absent — full stack (default)
//! - `root` — skip row furniture; body fills theme chrome

use anyhow::Result;
use grackle_model::AxisMember;
use std::path::Path;

use super::binder::Fragments;
use super::parts::{self, PartMap, Preview};
use super::theme::Theme;
use crate::db::Row;
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
    pub locale: &'a str,
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
        page.locale,
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
    row: Option<&Row>,
    doc: PartMap,
    body: &str,
    resolve_asset: &dyn Fn(&str) -> String,
) -> Result<String> {
    let mut doc = doc;
    if let Some(row) = row {
        parts::fill_from_fields(&mut doc, row, page.theme.schemas(), resolve_asset)?;
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
    locale: &str,
    profile: Option<&str>,
    axis: &[AxisMember],
    body: &str,
) -> String {
    render::root_shell(
        &render::light_head(head),
        locale,
        None,
        profile,
        axis,
        &parts::canonical(&parts::raw(body)),
    )
}

/// Resolve `layout`/`variant` to a theme face and concatenate member rows
/// (THEME.md §3). Callers attach view context on the error.
pub fn member_faces(
    fragments: &Fragments,
    layout: &str,
    variant: Option<&str>,
    items: Vec<Preview<'_>>,
    featured: bool,
) -> Result<String> {
    let face = parts::require_face(fragments, parts::member_face(layout, variant))?;
    Ok(concat_rows(fragments, face, items, featured))
}

/// Concatenate member row faces (THEME.md §3). When `featured`, the first
/// member prefers face `featured` if the theme ships it.
fn concat_rows(
    fragments: &Fragments,
    face: &str,
    items: Vec<Preview<'_>>,
    featured: bool,
) -> String {
    let mut out = String::new();
    let mut items = items;
    if featured && !items.is_empty() {
        let first = items.remove(0);
        let feat = if fragments.has("row--featured") {
            "featured"
        } else {
            face
        };
        out.push_str(&fragments.render_with(&parts::preview(first), Some(feat)));
    }
    for p in items {
        out.push_str(&fragments.render_with(&parts::preview(p), Some(face)));
    }
    out
}
