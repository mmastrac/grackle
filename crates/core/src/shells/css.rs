//! Theme stylesheet compilation (the megacss, IO.md §6).

use anyhow::{Context, Result};
use std::path::Path;

use crate::pipeline::types::Stats;
use crate::store::split_front_matter;

/// `@charset` is only legal as the very first thing in a stylesheet, and
/// grass emits one per compilation unit — two of which now go INSIDE layer
/// blocks. Strip them there and write one at the top of the file.
pub(crate) fn strip_charset(css: &str) -> &str {
    css.strip_prefix("@charset \"UTF-8\";\n").unwrap_or(css)
}

/// A theme's stylesheet is the ENGINE BASE plus whatever the theme adds
/// (§5e) — compiled to the URL the theme's pages link (`default` keeps
/// /css/main.css for parity).
///
/// **The theme's own sheet is `theme.scss`, or failing that `_tokens.scss`
/// alone.** The second case is the smallest theme worth having: retune the
/// palette and the measure, inherit every rule. Compiling it needs saying
/// out loud, because the alternative is the failure mode it replaced — a
/// directory holding one `_tokens.scss` shipped a stylesheet that never
/// read it, and nothing said so. A file that is silently ignored is worse
/// than one that errors.
///
/// The two sheets arrive in declared layers, the cascade order §5e states.
/// That buys what plain concatenation cannot: a theme's rule wins over the
/// base's whatever the selectors say, so a theme writing `.crumb` is never
/// outranked by the base's `[data-kind="crumb"] + [data-kind="crumb"]`.
///
/// **These per-theme sheets ARE the megacss** (IO.md §6, item I5). The model
/// is one CSS artifact — engine base, theme, site overlay, extracted
/// `root.html` styles, eventually per-post styles — and chunking it per theme
/// is an optimization of that one artifact, not a competing design: a page
/// links exactly one sheet, and the sheet it links is the whole cascade for
/// that page. Nothing about the URLs or the assembly changed when the model
/// said so; what changed is that "the megacss" now names something that
/// exists.
///
/// `head_style` is the theme root's `<head>` CSS (`Theme::head_style`), and
/// it lands in the THEME layer **after** `theme.scss`. Two reasons, and the
/// second is why it is not merely arbitrary: a theme's files are read top to
/// bottom by whoever maintains it, and `root.html` is the file that states
/// the theme's own frame — so a rule it writes about its own chrome should
/// win against the general sheet, not lose to it. It is also the reading that
/// preserves I4's inline emission: a `<style>` last in a `<head>` outranked
/// the stylesheet link above it, and staying last keeps the same rule
/// winning after the move.
/// Returns the compiled sheet's bytes. The caller chooses the URL it lands at
/// and inserts it (the stable convention or a content address, see
/// [`crate::assets`]), because in `hashed` mode the URL is a function of these
/// bytes.
pub(crate) fn css_pass(
    theme_dir: &Path,
    head_style: &str,
    overlay: Option<&str>,
    stats: &mut Stats,
) -> Result<Vec<u8>> {
    // `theme.scss` if the theme wrote one, else `_tokens.scss` on its own.
    // `wants_skin` is the ONLY thing a sheet's presence now decides: the
    // heading ladder and block rhythm always apply (measured inert under a
    // theme that has its own — see `base::css`), and only the decorative
    // skins wait to be asked for. That shrinks the ladder's one
    // discontinuity from "the whole page changes" to "the code panel and
    // blockquote rule are missing".
    let full = theme_dir.join("theme.scss");
    let tokens = theme_dir.join("_tokens.scss");
    let (own, wants_skin) = match (full.exists(), tokens.exists()) {
        (true, _) => (Some(full), false),
        (false, true) => (Some(tokens.clone()), true),
        (false, false) => (None, true),
    };

    // The full cascade order §5e declares, not just the two layers used
    // today. `overlay` (§5b subtree styles) and `post` (§6c per-post
    // styles) are unbuilt — declaring them now is free and makes this
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
    // something reached it — a theme with neither writes no `@layer theme`
    // at all, exactly as before this item.
    let mut theme_layer: Vec<String> = Vec::new();
    // Every partial ANY of the theme's CSS sources pulled in, both passes
    // pooled. The orphaned-tokens question below is about the theme as a
    // whole — "does anything the theme compiles read this file?" — and a
    // per-pass list can only answer it for one file (IR5).
    let mut imported: Vec<String> = Vec::new();
    // A tokens-only theme (`_tokens.scss`, no `theme.scss`) reads its tokens
    // by BEING them: `own` is the partial itself, so no `@import` names it
    // and none could. It is the one shape where the file is fully alive and
    // the import list is empty.
    let tokens_only = own.as_deref() == Some(tokens.as_path());
    if let Some(src) = own {
        let text = std::fs::read_to_string(&src)?;
        let (_, _, body) = split_front_matter(&text);
        let mut seen = Vec::new();
        let flat = inline_imports(body, theme_dir, &mut seen)?;
        imported.append(&mut seen);

        let opts = grass::Options::default().load_path(theme_dir);
        match grass::from_string(flat, &opts) {
            Ok(theme_css) => theme_layer.push(strip_charset(&theme_css).to_string()),
            // Reported here so `serve` shows it immediately, and RECORDED so
            // `build` can refuse: the binder treats a malformed fragment as
            // a build error with file:line, and the CSS half of the same
            // theme should not be the lenient one. Publishing a site whose
            // stylesheet silently failed to compile is the worst outcome
            // available — it looks deployable and is wrong.
            Err(e) => {
                eprintln!("scss: {}: {e}", src.display());
                stats.css_errors.push(format!("{}: {e}", src.display()));
            }
        }
    }
    // The theme root's head styles (IO.md §6), through the SAME pipeline as
    // `theme.scss`: `@import` inlining, then grass, with the theme directory
    // on the load path.
    //
    // **Compiled, not passed through** — decided at I5. A `root.html` head is
    // authored as CSS in an HTML file, and plain CSS is valid SCSS, so
    // compiling costs a pass over a few lines and buys the author the two
    // things the rest of the theme already has: nesting, and
    // `@import "tokens";` reaching the theme's own partial or the engine
    // base's. The alternative — verbatim — would have made one file in a
    // theme the file where the theme's own vocabulary does not work, which is
    // the kind of exception that is only ever discovered by hitting it.
    // A style that does not compile is the same event as a `theme.scss` that
    // does not: reported, recorded, and a publishing build refuses.
    if !head_style.is_empty() {
        let root_html = theme_dir.join("root.html");
        let mut seen = Vec::new();
        let flat = inline_imports(head_style, theme_dir, &mut seen)?;
        imported.append(&mut seen);
        let opts = grass::Options::default().load_path(theme_dir);
        match grass::from_string(flat, &opts) {
            Ok(head_css) => theme_layer.push(strip_charset(&head_css).to_string()),
            Err(e) => {
                eprintln!("scss: {}: {e}", root_html.display());
                stats
                    .css_errors
                    .push(format!("{}: {e}", root_html.display()));
            }
        }
    }
    // A `_tokens.scss` nobody imports is the dead-file trap again, one arm
    // along: the sheet compiles, the tokens are simply never read, and the
    // only symptom is a theme that ignores its own palette. Worth a word, not
    // a failure, because a theme may legitimately keep a partial it does not
    // use yet.
    //
    // **Asked here, of the whole theme** (IR5). It used to be asked inside the
    // `theme.scss` pass, of that pass's imports alone, and was therefore false
    // in the two shapes where the tokens are read by something else: a
    // tokens-only theme (they ARE the sheet — a wart of this warning's own
    // vintage), and a theme whose `root.html` head imports them while
    // `theme.scss` does not (I5 gave the head its own pass and its own list).
    // What survives is the case the warning was written for: a `theme.scss`
    // beside a `_tokens.scss` that nothing in the theme pulls in.
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
    // §5b rung 1: the site's own sheet, above every theme's. Appended to each
    // theme's stylesheet rather than served separately, because it must apply
    // whichever theme is active — that is the whole guarantee, that a knob set
    // here survives a theme SWITCH and not merely a theme update.
    if let Some(o) = overlay {
        css.push_str(&format!("@layer overlay {{\n{}\n}}\n", strip_charset(o)));
    }
    stats.css += css.len();
    Ok(css.into_bytes())
}

/// The site's own stylesheet: `.style.scss` at the root, compiled once and
/// handed to every theme's sheet (§5b, rung 1 of themes/DESIGN.md §2).
///
/// The cheapest real customization there is, and the one the ladder promised
/// and could not deliver: `:root { --accent: … }` in a file the site owns,
/// landing in the `overlay` layer above theme CSS. Because the token names are
/// a cross-theme contract, an override written here survives switching themes,
/// not just updating one — which is what makes this a rung below "derive a
/// theme" rather than a worse way to do it.
///
/// Positional `.style.scss` (§5b's other half — a file per subtree, scoped by
/// `data-scope`) is NOT this. It needs every rendered row to carry its scope
/// chain, and nothing emits one yet.
pub(crate) fn site_overlay(root: &Path, stats: &mut Stats) -> Option<String> {
    let src = root.join(".style.scss");
    let text = std::fs::read_to_string(&src).ok()?;
    // Unscoped, so `:root` works here — which is the point of the root file and
    // exactly what §5b warns is impossible in a SCOPED one, where a `:root`
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
/// but `_sass/_post.scss:240` has one — `pre > code { @import "rouge"; }` — to
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
    /// integration tests — a unit test gets the system temp dir or nothing.)
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
    /// rule. It regressed once — a directory holding only `_tokens.scss`
    /// shipped a stylesheet that never read the file, silently — so the
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

    /// The heading ladder is unconditional — a theme never loses it by
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
    /// fight them — which is exactly what the ladder does NOT do.
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

    /// §5e's cascade order, declared in full even though `overlay` and
    /// `post` have nothing to emit into them yet: the declaration is what
    /// makes the order authoritative rather than an accident of which
    /// layers happen to exist.
    #[test]
    fn the_full_cascade_order_is_declared() {
        let css = compile_as("layer-order", &[("theme.scss", ".x { color: red; }")]);
        assert!(
            css.contains("@layer reset, base, theme, overlay, post;"),
            "the sheet declares §5e's order: {}",
            &css[..css.len().min(120)]
        );
    }

    /// The reset must keep a long code line from scrolling the whole page,
    /// even for a theme that imports no typography at all — same class of
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
