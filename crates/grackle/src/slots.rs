//! Tree-filled slots (§5e): `.slots/` directories fill named slots for every
//! row beneath them. Filename = slot name = key; content = fill; resolution
//! is nearest-wins up the source path — the same ascent as §6a asset names
//! and §4b markers.
//!
//! Extension picks the pipeline: `.md` renders through comrak into an `Html`
//! part; `.html` is trusted markup verbatim (binder holes in fills are a
//! later step). The **block-arity rule** applies at load: a fill destined
//! for a phrasing-only element (`<p>`, `<h2>`, …) must render to exactly one
//! block — hard error otherwise — and unwraps to its inline content; flow
//! elements (`<div>`, `<footer>`, …) take any number of blocks verbatim.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One fill, RAW: rendering happens per consuming page (§6a row/view
/// links resolve against the page's locale, so `view:blog_index` in a nav
/// fill lands on /blog/ or /fr/blog/ depending on who is asking).
#[derive(Debug)]
pub struct Fill {
    /// The authored source, unrendered.
    pub raw: String,
    /// "md" renders through comrak (links resolved); "html" is verbatim.
    pub ext: String,
    /// The directory whose `.slots/` owns this fill — relative source
    /// links in the fill resolve from here.
    pub owner: PathBuf,
    /// Source path, for error messages.
    pub file: PathBuf,
}

/// A fill rendered for one consumer, in both shapes the arity rule can
/// demand. `inline` is present iff the rendered fill is exactly one block.
#[derive(Debug)]
pub struct RenderedFill {
    /// The rendered fill, blocks intact — what a flow element receives.
    pub blocks: String,
    /// The single block's inner HTML — what a phrasing element receives.
    /// `None` when the fill has zero or multiple blocks.
    pub inline: Option<String>,
    /// Top-level block count, for arity errors.
    pub block_count: usize,
    /// Source path, for error messages.
    pub file: PathBuf,
}

impl Fill {
    /// Render this fill for one consumer. `resolve` sees each markdown
    /// link destination (§6a); `.html` fills are verbatim — their seam is
    /// the §6d rewrite stage.
    pub fn render(
        &self,
        resolve: &dyn Fn(&str) -> anyhow::Result<Option<String>>,
    ) -> Result<RenderedFill> {
        let html = match self.ext.as_str() {
            "md" => {
                crate::markdown::render_doc_with(self.raw.trim(), resolve)
                    .with_context(|| format!("slot fill {}", self.file.display()))?
                    .whole
            }
            _ => self.raw.trim().to_string(),
        };
        let (block_count, inline) = block_shape(&html);
        Ok(RenderedFill {
            blocks: html,
            inline,
            block_count,
            file: self.file.clone(),
        })
    }
}

/// All `.slots/` fills in the tree, keyed by (directory, slot name).
#[derive(Debug, Default)]
pub struct SlotFills {
    by_dir: BTreeMap<PathBuf, BTreeMap<String, Fill>>,
}

impl SlotFills {
    /// Scan the site tree for `.slots/` directories. The walk skips
    /// underscore- and dot-prefixed directories (except `.slots` itself) and
    /// the usual build/VCS artifacts — fills are content, but they live in
    /// dot-directories precisely so the route walk never sees them.
    pub fn load(root: &Path) -> Result<SlotFills> {
        let mut fills = SlotFills::default();
        walk(root, root, &mut fills)?;
        Ok(fills)
    }

    /// Resolve a slot for a row whose source lives in `dir`: nearest
    /// `.slots/<name>.*` ascending from `dir` to the root wins. Within a
    /// directory, the row's locale wins (§6f): `nav.fr.md` beside `nav.md`
    /// is the same suffix convention rows use, needing no config — the
    /// dotted stem simply IS the localized slot name. The base file is the
    /// default locale, so a page's chrome follows its language exactly as
    /// its trail does, and a locale with no localized fill inherits the
    /// default one.
    pub fn resolve(&self, root: &Path, dir: &Path, slot: &str, locale: &str) -> Option<&Fill> {
        let localized = format!("{slot}.{locale}");
        let mut cur = dir;
        loop {
            if let Some(m) = self.by_dir.get(cur) {
                if let Some(f) = m.get(&localized).or_else(|| m.get(slot)) {
                    return Some(f);
                }
            }
            if cur == root {
                return None;
            }
            cur = cur.parent()?;
        }
    }

    /// The fill for a phrasing-only element: exactly one block, unwrapped.
    /// Zero or several blocks is the hard error the rule promises.
    pub fn inline_or_err(fill: &RenderedFill) -> Result<&str> {
        fill.inline.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "{}: fills a phrasing-only slot element, so it must be exactly one \
                 block — it has {}",
                fill.file.display(),
                fill.block_count
            )
        })
    }
}

fn walk(root: &Path, dir: &Path, fills: &mut SlotFills) -> Result<()> {
    const SKIP: &[&str] = &[
        "node_modules",
        "vendor",
        "docker",
        "scripts",
        "CHANGES",
        "grackle",
        "themes",
        "_site",
        "_cache",
        "_log",
        "target",
    ];
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if name == ".slots" {
            load_dir(dir, &p, fills)?;
            continue;
        }
        if name.starts_with('.') || name.starts_with('_') || SKIP.contains(&name.as_str()) {
            continue;
        }
        walk(root, &p, fills)?;
    }
    Ok(())
}

fn load_dir(owner: &Path, slots_dir: &Path, fills: &mut SlotFills) -> Result<()> {
    for e in std::fs::read_dir(slots_dir)?.filter_map(|e| e.ok()) {
        let p = e.path();
        let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        let Some(ext) = p.extension().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        let text = std::fs::read_to_string(&p)
            .with_context(|| format!("reading slot fill {}", p.display()))?;
        if !matches!(ext.as_str(), "md" | "html") {
            bail!(
                "{}: unknown slot fill extension .{ext} (md renders, html is verbatim)",
                p.display()
            );
        }
        fills.by_dir.entry(owner.to_path_buf()).or_default().insert(
            stem,
            Fill {
                raw: text,
                ext,
                owner: owner.to_path_buf(),
                file: p,
            },
        );
    }
    Ok(())
}

/// Count top-level blocks in well-formed HTML and, when there is exactly one
/// element, return its inner HTML. Comrak output is well-formed by
/// construction; a hand-written `.html` fill that isn't will miscount and
/// the arity error names the file.
fn block_shape(html: &str) -> (usize, Option<String>) {
    let s = html.trim();
    let mut depth = 0usize;
    let mut count = 0usize;
    let mut first_inner: Option<(usize, usize)> = None; // inner span of first top-level element
    let mut i = 0;
    let b = s.as_bytes();
    let mut open_end = 0usize;
    while i < b.len() {
        if b[i] == b'<' {
            let rest = &s[i..];
            if rest.starts_with("<!--") {
                i += rest.find("-->").map(|e| e + 3).unwrap_or(rest.len());
                continue;
            }
            let close = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
            let tag = &rest[..close];
            if tag.starts_with("</") {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if count == 1 && first_inner.is_none() {
                        first_inner = Some((open_end, i));
                    }
                }
            } else if !tag.ends_with("/>") && !is_void_tag(tag) {
                if depth == 0 {
                    count += 1;
                    open_end = i + close;
                }
                depth += 1;
            } else if depth == 0 {
                // void element at top level is its own block
                count += 1;
            }
            i += close;
        } else {
            if depth == 0 && !s[i..].chars().next().is_some_and(char::is_whitespace) {
                // bare text at top level counts as a block once
                count += 1;
                // consume the text run
                let end = s[i..].find('<').map(|e| i + e).unwrap_or(s.len());
                i = end;
                continue;
            }
            i += 1;
        }
    }
    let inline = if count == 1 {
        first_inner.map(|(a, z)| s[a..z].to_string())
    } else {
        None
    };
    (count, inline)
}

fn is_void_tag(tag: &str) -> bool {
    let name: String = tag
        .trim_start_matches('<')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    crate::binder::VOID.contains(&name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_paragraph_unwraps() {
        let (n, inline) = block_shape("<p>© 1998 <a href=\"/c/\">contact</a></p>\n");
        assert_eq!(n, 1);
        assert_eq!(
            inline.as_deref(),
            Some("© 1998 <a href=\"/c/\">contact</a>")
        );
    }

    #[test]
    fn multiple_blocks_do_not_unwrap() {
        let (n, inline) = block_shape("<p>a</p>\n<p>b</p>");
        assert_eq!(n, 2);
        assert!(inline.is_none());
    }

    #[test]
    fn list_is_one_block() {
        let (n, inline) = block_shape("<ul>\n<li>x</li>\n<li>y</li>\n</ul>");
        assert_eq!(n, 1);
        assert_eq!(inline.as_deref(), Some("\n<li>x</li>\n<li>y</li>\n"));
    }

    #[test]
    fn nested_divs_are_one_block() {
        let (n, _) = block_shape("<div><div>a</div><p>b</p></div>");
        assert_eq!(n, 1);
    }
}
