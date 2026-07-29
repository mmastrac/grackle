//! Tree-filled slots (§5e): `.slots/` directories fill named slots for every
//! row beneath them. Filename = slot name = key; content = fill; resolution
//! is nearest-wins up the source path — the same ascent as §6a asset names
//! and §4b markers.
//!
//! Extension picks the pipeline: `.md` renders through comrak into an `Html`
//! part; `.html` is trusted markup with links resolved (§6d stage B). The
//! **block-arity rule** applies at load: a fill destined
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
    /// Render this fill for one consumer. `resolve` sees each link
    /// destination (§6a) — in markdown via the comrak AST, in `.html` fills
    /// via the §6d stage-B rewriter.
    pub fn render(
        &self,
        resolve: &dyn Fn(crate::links::Cite, &str) -> anyhow::Result<Option<String>>,
    ) -> Result<RenderedFill> {
        let markdown = self.ext == "md";
        let (html, _) = crate::markdown::render_source(self.raw.trim(), markdown, resolve)
            .with_context(|| format!("slot fill {}", self.file.display()))?;
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
        walk(root, &mut fills)?;
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

    /// Every fill in the tree, sorted: (stem as authored, source file).
    /// `nav.fr` is one stem — that is what `resolve` compares against.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Path)> + '_ {
        self.by_dir
            .values()
            .flat_map(|m| m.iter().map(|(stem, f)| (stem.as_str(), f.file.as_path())))
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

/// Fills nothing will ever read (MERGE.md C4b): stem not in any theme's
/// identity slots. Warning not error — spare fills for uninstalled themes
/// should not fail a build.
pub fn unknown_stems(fills: &SlotFills, known: &[&str], locales: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for (stem, file) in fills.iter() {
        // `nav.fr` is `nav` in French — but only where `fr` is declared;
        // `nav.frr` is its own stem, and its own dead name.
        let slot = match stem.rsplit_once('.') {
            Some((base, loc)) if locales.contains(&loc) => base,
            _ => stem,
        };
        if known.contains(&slot) {
            continue;
        }
        // Case variants are unknown stems BY CONSTRUCTION (batch review 2,
        // finding 7): `resolve` compares stems byte for byte, so `Nav.md`
        // fills nothing even on a filesystem that would call it `nav.md`.
        // That makes it the one wrong spelling worth naming outright — the
        // author is looking at a file the filesystem says is there.
        let lowered = slot.to_ascii_lowercase();
        let hint = known
            .iter()
            .find(|k| **k == lowered)
            .map(|k| {
                format!(" (did you mean `{k}`? slot names are matched exactly, so case counts)")
            })
            .unwrap_or_default();
        out.push(format!(
            "{}: fills slot {slot:?}, which no loaded theme's root places{hint} — \
             slots the tree may fill: {}",
            file.display(),
            if known.is_empty() {
                "(none)".to_string()
            } else {
                known.join(", ")
            }
        ));
    }
    out
}

fn walk(dir: &Path, fills: &mut SlotFills) -> Result<()> {
    const SKIP: &[&str] = &[
        "node_modules",
        "vendor",
        "docker",
        "scripts",
        "CHANGES",
        // Site root may be the parent repo (skip the engine tree) or the
        // grackle workspace itself (skip crates — fixtures live there).
        "grackle",
        "crates",
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
        walk(&p, fills)?;
    }
    Ok(())
}

/// "nav.html is verbatim markup, nav.md renders through comrak" — the two
/// files claiming one slot, ordered by filename so the message reads the same
/// however `read_dir` happened to hand them over (the scan is unsorted).
fn conflict(a: &Path, b: &Path) -> String {
    let mut both = [a, b];
    both.sort();
    format!(
        "{} {}, {} {}",
        both[0].display(),
        pipeline(both[0]),
        both[1].display(),
        pipeline(both[1])
    )
}

/// What a fill's extension makes of it — the half of §5e that says two files
/// with one stem can never be the same statement.
fn pipeline(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()) {
        Some("md") => "renders through comrak",
        _ => "is verbatim markup",
    }
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
        // Asked before the file is read, so a fill the engine cannot run never
        // reaches the map and can never be one half of a collision below.
        if !matches!(ext.as_str(), "md" | "html") {
            bail!(
                "{}: unknown slot fill extension .{ext} (md renders, html is verbatim)",
                p.display()
            );
        }
        let text = std::fs::read_to_string(&p)
            .with_context(|| format!("reading slot fill {}", p.display()))?;
        let slot = fills.by_dir.entry(owner.to_path_buf()).or_default();
        // Two files in ONE `.slots/` resolving to one key are unordered peers:
        // `resolve` walks *directory levels*, and nearness ranks levels, not
        // files at one level — so the winner used to be whichever `read_dir`
        // handed over last (MERGE.md A6).
        //
        // The only reachable shape is two extensions, since a directory cannot
        // hold two files of one name — and A5's "agreement is not a conflict"
        // exemption cannot follow it here: `.md` renders and `.html` is
        // trusted verbatim (§5e), so even byte-identical files are two
        // different fills. There is no equality to test.
        if let Some(prev) = slot.get(&stem) {
            bail!(
                "{} — two files in {} fill slot {stem:?}. Extension picks the \
                 pipeline, so these are different fills of one name, and \
                 nothing ranks two files in one directory: nearest-wins ranks \
                 directory levels. Delete one, or give them different names — \
                 a locale suffix is a different name ({stem}.fr.md beside \
                 {stem}.md is §6f, not a collision).",
                conflict(&prev.file, &p),
                slots_dir.display()
            );
        }
        slot.insert(
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
                if depth == 0 && count == 1 && first_inner.is_none() {
                    first_inner = Some((open_end, i));
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
    super::binder::VOID.contains(&name.as_str())
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

    /// Write a site tree under the system temp dir and scan it. `who` names
    /// the caller: unit tests run in parallel threads, and a shared scratch
    /// directory means one test loads another's fills.
    fn tree(who: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grackle-slots-{who}"));
        let _ = std::fs::remove_dir_all(&dir);
        for (rel, body) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().expect("a fill has a directory")).unwrap();
            std::fs::write(&p, body).unwrap();
        }
        dir
    }

    /// The guard (MERGE.md A6). Delete the `bail!` in `load_dir` and this
    /// loads silently, with the slot filled by whichever file `read_dir`
    /// handed over last.
    #[test]
    fn two_fills_of_one_stem_in_one_directory_is_an_error() {
        let dir = tree(
            "same-stem",
            &[
                (".slots/nav.md", "[home](/)"),
                (".slots/nav.html", "<p><a href=\"/\">home</a></p>"),
            ],
        );
        let e = SlotFills::load(&dir)
            .expect_err("one stem, two pipelines, one directory: nothing ranks them")
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(e.contains("nav.html"), "{e}");
        assert!(e.contains("nav.md"), "{e}");
        assert!(e.contains("fill slot \"nav\""), "{e}");
        // Sorted by filename so read_dir order does not reach the message.
        assert!(e.find("nav.html") < e.find("nav.md"), "{e}");
        assert_eq!(
            conflict(Path::new(".slots/nav.md"), Path::new(".slots/nav.html")),
            conflict(Path::new(".slots/nav.html"), Path::new(".slots/nav.md"))
        );
    }

    /// A locale suffix IS a different slot name; the base file is the default locale.
    #[test]
    fn a_locale_suffix_is_a_different_slot_not_a_collision() {
        let dir = tree(
            "locale",
            &[
                (".slots/nav.md", "default"),
                (".slots/nav.fr.md", "français"),
            ],
        );
        let fills = SlotFills::load(&dir).expect("nav.md beside nav.fr.md is the designed shape");
        assert_eq!(
            fills.resolve(&dir, &dir, "nav", "fr").map(|f| &f.raw[..]),
            Some("français")
        );
        assert_eq!(
            fills.resolve(&dir, &dir, "nav", "en").map(|f| &f.raw[..]),
            Some("default")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The guard (MERGE.md C4b). Delete the body of `unknown_stems` and this
    /// goes silent: `.slots/copyrite.md` is read, keyed, and never looked at
    /// by anything, on every build, forever.
    #[test]
    fn a_fill_naming_no_slot_is_reported_with_the_knowns() {
        let dir = tree(
            "unknown-stem",
            &[
                (".slots/copyright.md", "© 1998"),
                (".slots/copyrite.md", "© 1998, misspelt"),
            ],
        );
        let fills = SlotFills::load(&dir).expect("both stems load; only one is read");
        let w = unknown_stems(&fills, &["copyright", "nav"], &["en"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(w.len(), 1, "only the misspelt one is dead: {w:?}");
        assert!(w[0].contains("copyrite.md"), "{}", w[0]);
        assert!(w[0].contains("\"copyrite\""), "{}", w[0]);
        // The knowns, so the fix is readable off the message.
        assert!(w[0].contains("copyright, nav"), "{}", w[0]);
    }

    /// Batch review 2, finding 7: a case variant is an unknown stem BY
    /// CONSTRUCTION — `resolve` compares stems byte for byte, so `Nav.md`
    /// fills nothing on every filesystem, including the ones that would call
    /// it `nav.md`. That is the one dead name worth spelling out.
    #[test]
    fn a_case_variant_of_a_known_slot_says_so() {
        let dir = tree("case-variant", &[(".slots/Nav.md", "[home](/)")]);
        let fills = SlotFills::load(&dir).expect("a stem is a stem");
        let w = unknown_stems(&fills, &["copyright", "nav"], &["en"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("did you mean `nav`?"), "{}", w[0]);
        assert!(w[0].contains("case counts"), "{}", w[0]);
    }

    /// The control §6f needs, and the shape the live corpus has: a localized
    /// fill is `{slot}.{locale}`, so `nav.fr` is `nav` where `fr` is
    /// declared — and only there. `nav.frr` is its own dead name.
    #[test]
    fn a_localized_fill_is_known_when_its_locale_is() {
        let dir = tree(
            "localized-stem",
            &[
                (".slots/nav.md", "nav"),
                (".slots/nav.fr.md", "navigation"),
                (".slots/nav.frr.md", "typo"),
            ],
        );
        let fills = SlotFills::load(&dir).expect("locale suffixes are stems");
        let w = unknown_stems(&fills, &["nav"], &["en", "fr"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(w.len(), 1, "only the undeclared locale is dead: {w:?}");
        assert!(w[0].contains("nav.frr.md"), "{}", w[0]);
    }

    /// The law that stays: directory levels ARE ordered, so the same stem
    /// deeper in the tree is nearest-wins, not a collision (table C).
    #[test]
    fn the_same_stem_at_two_levels_is_still_nearest_wins() {
        let dir = tree(
            "two-levels",
            &[
                (".slots/nav.md", "site nav"),
                ("blog/.slots/nav.md", "blog nav"),
            ],
        );
        let fills = SlotFills::load(&dir).expect("different levels are ranked by nearness");
        assert_eq!(
            fills
                .resolve(&dir, &dir.join("blog"), "nav", "en")
                .map(|f| &f.raw[..]),
            Some("blog nav")
        );
        assert_eq!(
            fills.resolve(&dir, &dir, "nav", "en").map(|f| &f.raw[..]),
            Some("site nav")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
