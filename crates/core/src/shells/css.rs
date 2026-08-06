//! Theme stylesheet compilation (the megacss).

use anyhow::{Context, Result};
use std::path::Path;

use crate::pipeline::types::Stats;
use crate::store::split_front_matter;

/// `@charset` is only legal as the very first thing in a stylesheet, and
/// grass emits one per compilation unit, two of which now go INSIDE layer
/// blocks. Strip them there and write one at the top of the file.
pub(crate) fn strip_charset(css: &str) -> &str {
    css.strip_prefix("@charset \"UTF-8\";\n").unwrap_or(css)
}

/// Inline `@import`s, compile with grass, and push the result into the theme
/// layer, or, on failure, report and record it. `ctx_path` names the source
/// in the error. Reported so `serve` shows it immediately, and recorded so
/// `build` can refuse: the CSS half of a theme is not the lenient one, and a
/// stylesheet that silently failed to compile looks deployable and is wrong.
fn compile_theme_layer_fragment(
    text: &str,
    ctx_path: &Path,
    theme_dir: &Path,
    theme_layer: &mut Vec<String>,
    imported: &mut Vec<String>,
    stats: &mut Stats,
) -> Result<()> {
    let mut seen = Vec::new();
    let flat = inline_imports(text, theme_dir, &mut seen)?;
    imported.append(&mut seen);
    match grass::from_string(flat, &grass::Options::default().load_path(theme_dir)) {
        Ok(css) => theme_layer.push(strip_charset(&css).to_string()),
        Err(e) => {
            eprintln!("scss: {}: {e}", ctx_path.display());
            stats
                .css_errors
                .push(format!("{}: {e}", ctx_path.display()));
        }
    }
    Ok(())
}

/// Theme stylesheet: engine base plus theme additions, compiled to the URL
/// theme pages link (`default` keeps /css/main.css).
///
/// Theme sheet is `theme.scss`, or else `_tokens.scss` alone. Sheets land in
/// declared layers so theme rules win over the base. `head_style` from the
/// theme root follows `theme.scss` in the theme layer (chrome rules should
/// beat the general sheet).
///
/// Returns compiled bytes. The caller picks the URL (`crate::assets`).
pub(crate) fn css_pass(
    theme_dir: &Path,
    head_style: &str,
    overlay: Option<&str>,
    stats: &mut Stats,
) -> Result<Vec<u8>> {
    // `theme.scss` if present, else `_tokens.scss`. `wants_skin` gates only
    // decorative skins; heading ladder and block rhythm always apply.
    let full = theme_dir.join("theme.scss");
    let tokens = theme_dir.join("_tokens.scss");
    let (own, wants_skin) = match (full.exists(), tokens.exists()) {
        (true, _) => (Some(full), false),
        (false, true) => (Some(tokens.clone()), true),
        (false, false) => (None, true),
    };

    // The full cascade order declares, not just the two layers used
    // today. `overlay` and `post`
    // are unbuilt, declaring them now is free and makes this
    // statement the authority on the order, so whoever builds them slots
    // in rather than discovering that an undeclared layer sorts last by
    // accident. `reset` is the base's own reset partial, which currently
    // ships inside `base`.
    let mut css = format!(
        "@charset \"UTF-8\";\n@layer reset, base, theme, overlay, post;\n\
         @layer base {{\n{}\n}}\n",
        strip_charset(crate::base::css(wants_skin))
    );
    // The theme layer's contents, in order: the theme's own sheet, then the
    // CSS its `root.html` head declared. Collected rather than appended
    // straight to `css` so the layer block is emitted once, and only when
    // something reached it, a theme with neither writes no `@layer theme`
    // at all, exactly as before this item.
    let mut theme_layer: Vec<String> = Vec::new();
    // Every partial ANY of the theme's CSS sources pulled in, both passes
    // pooled. The orphaned-tokens question below is about the theme as a
    // whole, "does anything the theme compiles read this file?", and a
    // per-pass list can only answer it for one file.
    let mut imported: Vec<String> = Vec::new();
    // A tokens-only theme (`_tokens.scss`, no `theme.scss`) reads its tokens
    // by BEING them: `own` is the partial itself, so no `@import` names it
    // and none could. It is the one shape where the file is fully alive and
    // the import list is empty.
    let tokens_only = own.as_deref() == Some(tokens.as_path());
    if let Some(src) = own {
        let text = std::fs::read_to_string(&src)?;
        let (_, _, body) = split_front_matter(&text);
        compile_theme_layer_fragment(
            body,
            &src,
            theme_dir,
            &mut theme_layer,
            &mut imported,
            stats,
        )?;
    }
    // The theme root's head styles, through the SAME pipeline as
    // `theme.scss`: `@import` inlining, then grass, with the theme directory
    // on the load path.
    //
    // **Compiled, not passed through**, by decision. A `root.html` head is
    // authored as CSS in an HTML file, and plain CSS is valid SCSS, so
    // compiling costs a pass over a few lines and buys the author the two
    // things the rest of the theme already has: nesting, and
    // `@import "tokens";` reaching the theme's own partial or the engine
    // base's. The alternative, verbatim, would have made one file in a
    // theme the file where the theme's own vocabulary does not work, which is
    // the kind of exception that is only ever discovered by hitting it.
    // A style that does not compile is the same event as a `theme.scss` that
    // does not: reported, recorded, and a publishing build refuses.
    if !head_style.is_empty() {
        let root_html = theme_dir.join("root.html");
        compile_theme_layer_fragment(
            head_style,
            &root_html,
            theme_dir,
            &mut theme_layer,
            &mut imported,
            stats,
        )?;
    }
    // A `_tokens.scss` nobody imports is the dead-file trap again, one arm
    // along: the sheet compiles, the tokens are simply never read, and the
    // only symptom is a theme that ignores its own palette. Worth a word, not
    // a failure, because a theme may legitimately keep a partial it does not
    // use yet.
    //
    // Asked here, of the whole theme's imports (theme sheet and head alike),
    // so a tokens-only theme and a head-only `@import` are not false
    // alarms. The case that fires is a `theme.scss` beside a `_tokens.scss`
    // that nothing in the theme pulls in.
    if tokens.exists() && !tokens_only && !imported.iter().any(|s| s == "tokens") {
        let w = format!(
            "{} has a _tokens.scss that nothing imports — add `@import \
             \"tokens\";` to theme.scss, or the palette is dead weight",
            theme_dir.display()
        );
        eprintln!("grackle: {w}");
        stats.css_warnings.push(w);
    }
    if !theme_layer.is_empty() {
        css.push_str(&format!(
            "@layer theme {{\n{}\n}}\n",
            theme_layer.join("\n")
        ));
    }
    // rung 1: the site's own sheet, above every theme's. Appended to each
    // theme's stylesheet rather than served separately, because it must apply
    // whichever theme is active, that is the whole guarantee, that a knob set
    // here survives a theme SWITCH and not merely a theme update.
    if let Some(o) = overlay {
        css.push_str(&format!("@layer overlay {{\n{}\n}}\n", strip_charset(o)));
    }
    stats.css += css.len();
    Ok(css.into_bytes())
}

/// The site's own stylesheet: `.style.scss` at the root, compiled once and
/// handed to every theme's sheet.
///
/// The cheapest real customization there is, and the one the ladder promised
/// and could not deliver: `:root { --accent: … }` in a file the site owns,
/// landing in the `overlay` layer above theme CSS. Because the token names are
/// a cross-theme contract, an override written here survives switching themes,
/// not just updating one, which is what makes this a rung below "derive a
/// theme" rather than a worse way to do it.
///
/// Positional `.style.scss`
/// is NOT this. It needs every rendered row to carry its scope
/// chain, and nothing emits one yet.
pub(crate) fn site_overlay(root: &Path, stats: &mut Stats) -> Option<String> {
    let src = root.join(".style.scss");
    let text = std::fs::read_to_string(&src).ok()?;
    // Unscoped, so `:root` works here, which is the point of the root file and
    // exactly what warns is impossible in a scoped one, where a `:root`
    // block would be nested inside a selector and silently never apply.
    match grass::from_string(text, &grass::Options::default().load_path(root)) {
        Ok(css) => Some(css),
        Err(e) => {
            eprintln!("scss: {}: {e}", src.display());
            stats.css_errors.push(format!("{}: {e}", src.display()));
            None
        }
    }
}

/// Resolve `@import "name"` textually against `_sass/_name.scss`, recursively.
///
/// `grass` rejects a **nested** `@import` ("this at-rule is not allowed here"),
/// but `_sass/_post.scss:240` has one, `pre > code { @import "rouge"; }`, to
/// scope Rouge's syntax classes. libsass (what Jekyll uses) allows it, so the
/// site is legal input that grass will not take. Inlining first gives grass the
/// flattened source it wants without touching the site's sass.
pub(crate) fn inline_imports(src: &str, load: &Path, seen: &mut Vec<String>) -> Result<String> {
    let mut out = String::with_capacity(src.len() * 2);
    for line in src.lines() {
        let t = line.trim();
        let name = t
            .strip_prefix("@import")
            .map(|r| r.trim().trim_end_matches(';').trim())
            .filter(|r| r.starts_with('"') && r.ends_with('"') && !r.contains("url("))
            .map(|r| r.trim_matches('"'));
        let Some(name) = name else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        if seen.iter().any(|s| s == name) {
            continue; // already inlined; sass imports are idempotent here
        }
        // The theme's own partial wins; failing that, the engine base's, so
        // a theme can `@import "tokens"` to build on the base vocabulary
        // without carrying a copy of it. Neither: leave the line for grass,
        // which will say so.
        let path = load.join(format!("_{name}.scss"));
        let inner = if path.exists() {
            std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
        } else if let Some(src) = crate::base::partial(name) {
            src.to_string()
        } else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        seen.push(name.to_string());
        // Preserve the indentation so nested imports stay inside their block.
        let indent = &line[..line.len() - line.trim_start().len()];
        for l in inline_imports(&inner, load, seen)?.lines() {
            out.push_str(indent);
            out.push_str(l);
            out.push('\n');
        }
    }
    Ok(out)
}

#[cfg(test)]
mod css_pass_tests {
    use super::*;
    use crate::pipeline::types::Stats;

    /// `who` names the caller: these run in parallel threads, and a shared
    /// scratch directory means one test compiles another's theme.
    /// (`CARGO_TARGET_TMPDIR` would be tidier, but Cargo defines it only for
    /// integration tests, a unit test gets the system temp dir or nothing.)
    fn compile_as(who: &str, files: &[(&str, &str)]) -> String {
        let dir = std::env::temp_dir().join(format!("grackle-css-pass-{who}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            std::fs::write(dir.join(name), body).unwrap();
        }
        let mut stats = Stats::default();
        let bytes = css_pass(&dir, "", None, &mut stats).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        String::from_utf8(bytes).unwrap()
    }

    /// The smallest theme worth having: retune the palette, inherit every
    /// rule. It regressed once, a directory holding only `_tokens.scss`
    /// shipped a stylesheet that never read the file, silently, so the
    /// property is worth an assertion rather than a convention.
    #[test]
    fn a_theme_of_only_tokens_is_compiled() {
        let css = compile_as(
            "tokens-only",
            &[("_tokens.scss", ":root { --measure: 33rem; }")],
        );
        assert!(
            css.contains("33rem"),
            "a tokens-only theme must reach the stylesheet: {css}"
        );
        // It never wrote a sheet, so it never declined the decorative half.
        assert!(css.contains("border-left"), "and inherits the skins too");
    }

    /// The heading ladder is unconditional, a theme never loses it by
    /// writing a stylesheet. This is the fix for the ladder's one growth
    /// cliff, and it is safe because the ladder reads only tokens (a theme
    /// retunes it through `--size`/`--scale`) and was measured inert under
    /// the one theme with a complete type sheet of its own.
    #[test]
    fn every_theme_keeps_the_heading_ladder() {
        for files in [
            vec![("theme.scss", ".x { color: var(--fg); }")],
            vec![("_tokens.scss", ":root { --measure: 33rem; }")],
            vec![],
        ] {
            let css = compile_as(&format!("ladder-{}", files.len()), &files);
            assert!(
                css.contains("text-wrap: balance"),
                "the ladder is not a thing a theme can lose by accident: {css}"
            );
        }
    }

    /// The decorative half still waits to be asked for. Measured: applied
    /// under grack.com the skins move a paragraph 19px and the blog listing
    /// 61px, because a theme with its own opinions about a blockquote will
    /// fight them, which is exactly what the ladder does NOT do.
    #[test]
    fn a_theme_with_a_sheet_is_not_given_the_skins() {
        let css = compile_as(
            "no-imposed-skin",
            &[("theme.scss", ".x { color: var(--fg); }")],
        );
        assert!(css.contains(".x"), "the theme's own rules ship");
        assert!(
            !css.contains("border-left: calc(var(--border) * 3)"),
            "but the base does not impose a blockquote rule: {css}"
        );
    }

    /// cascade order, declared in full even though `overlay` and
    /// `post` have nothing to emit into them yet: the declaration is what
    /// makes the order authoritative rather than an accident of which
    /// layers happen to exist.
    #[test]
    fn the_full_cascade_order_is_declared() {
        let css = compile_as("layer-order", &[("theme.scss", ".x { color: red; }")]);
        assert!(
            css.contains("@layer reset, base, theme, overlay, post;"),
            "the sheet declares order: {}",
            &css[..css.len().min(120)]
        );
    }

    /// The reset must keep a long code line from scrolling the whole page,
    /// even for a theme that imports no typography at all, same class of
    /// bug as an image that overflows its column, and `vanilla` is the
    /// theme that would otherwise ship it.
    #[test]
    fn a_wide_code_block_never_scrolls_the_page() {
        let bare = compile_as("pre-bare", &[]);
        assert!(
            bare.contains("overflow-x: auto"),
            "the base reset scrolls `pre` itself: {bare}"
        );
        let themed = compile_as("pre-themed", &[("theme.scss", ".x { color: red; }")]);
        assert!(
            themed.contains("overflow-x: auto"),
            "and so does a theme that imports no typography"
        );
    }
}
