//! comrak, configured to stand in for kramdown (DESIGN.md §8).
//!
//! kramdown is *not* CommonMark, so this can never be exact by construction.
//! The job is to get close enough that the remaining diffs are countable, and
//! to know precisely where the dialects part ways.

use comrak::nodes::{AstNode, NodeHtmlBlock, NodeValue};
use comrak::{format_html, parse_document, Arena, Options};

/// Options chosen to match Jekyll's kramdown defaults where comrak can.
pub fn options() -> Options<'static> {
    let mut o = Options::default();

    // kramdown's `auto_ids` is ON by default: `## Foo` -> <h2 id="foo">.
    // Without this every heading in the corpus diffs.
    //
    // Caveat: comrak also injects an `<a class="anchor">` inside the heading,
    // which kramdown does not. The real pipeline strips it in the AST pass;
    // the spike strips it in `diff::normalize` so the measurement isolates the
    // question that actually matters — do the two slug algorithms agree?
    o.extension.header_id_prefix = Some(String::new());

    // kramdown runs smartypants by default: quotes -> curly, -- -> en dash.
    o.parse.smart = true;

    // Present in kramdown, absent from bare CommonMark.
    o.extension.table = true;
    o.extension.strikethrough = true;
    o.extension.footnotes = true;
    o.extension.description_lists = true;

    // 20 years of hand-written HTML in markdown; kramdown passes it through.
    o.render.r#unsafe = true;

    o
}

/// Jekyll's `syntax_highlighter_opts.default_lang` (_config.yml:46). Every
/// code block gets a language, so `language-text` is by far the common case:
/// 2236 of 2534 blocks in the reference build.
const DEFAULT_LANG: &str = "text";

/// Rouge escapes `&`, `<` and `>` — and, unlike comrak, leaves `"` alone.
/// Using comrak's escaper here yields `&quot;` and a diff on every code block
/// containing a double quote.
fn escape_code(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            _ => o.push(c),
        }
    }
    o
}

/// Rewrite every code block into Jekyll's Rouge markup.
///
/// This is the AST escape hatch from §9a: we replace the node with a raw
/// `HtmlBlock` rather than teach comrak's formatter a new shape. Neither of
/// comrak's two relevant adapters can do this job:
///
///   * `CodefenceRendererAdapter` is a map keyed by language and only fires
///     when the info string is non-empty (html.rs:513). Our corpus is 88%
///     *indented* code blocks, whose info is "" — only 7 posts use fences at
///     all — so it would never fire where it matters.
///   * `SyntaxHighlighterAdapter` does fire for empty info and could open the
///     two wrapper divs, but comrak then hardcodes `</code></pre>`
///     (html.rs:566) with no hook to close them.
///
/// Highlighting itself is still absent: Rouge emits Pygments token spans
/// (`<span class="c1">`) for the ~12% of blocks with a real language. Those
/// keep their wrapper but not their spans, so they stay on §8's known-inexact
/// list until we have a Rouge-compatible highlighter.
pub fn rouge_code_blocks<'a>(root: &'a AstNode<'a>) {
    for node in root.descendants() {
        let repl = {
            let data = node.data.borrow();
            match &data.value {
                NodeValue::CodeBlock(ncb) => {
                    let lang = match ncb.info.split_whitespace().next() {
                        Some(l) if !l.is_empty() => l,
                        _ => DEFAULT_LANG,
                    };
                    Some(NodeValue::HtmlBlock(NodeHtmlBlock {
                        block_type: 6,
                        literal: format!(
                            "<div class=\"language-{lang} highlighter-rouge\"><div class=\"highlight\"><pre class=\"highlight\"><code>{}</code></pre></div></div>",
                            escape_code(&ncb.literal)
                        ),
                    }))
                }
                // Backtick spans get the same treatment, minus the wrappers.
                // A hand-written `<code>` in the source is an HtmlInline node,
                // not this one, so it passes through untouched — which is what
                // kramdown does with it too.
                NodeValue::Code(nc) => Some(NodeValue::HtmlInline(format!(
                    "<code class=\"language-{DEFAULT_LANG} highlighter-rouge\">{}</code>",
                    escape_code(&nc.literal)
                ))),
                _ => continue,
            }
        };
        if let Some(v) = repl {
            node.data.borrow_mut().value = v;
        }
    }
}

pub fn render(src: &str) -> String {
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, src, &opts);
    rouge_code_blocks(root);
    let mut out = String::new();
    format_html(root, &opts, &mut out).expect("writing to a String cannot fail");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_punctuation_matches_kramdown_defaults() {
        assert!(render("it's").contains('\u{2019}'));
        assert!(render("a -- b").contains('\u{2013}'));
    }

    #[test]
    fn headings_get_ids() {
        assert!(render("## Foo Bar").contains(r#"id="foo-bar""#));
    }

    #[test]
    fn raw_html_passes_through() {
        assert!(render("<div class=\"x\">hi</div>").contains(r#"<div class="x">"#));
    }
}

// ---- spike: is a document the concatenation of its top-level blocks?
#[cfg(test)]
mod block_spike {
    use super::*;

    /// Render each top-level child on its own, exactly as `render()` would.
    fn split(src: &str) -> (Vec<String>, String) {
        let arena = Arena::new();
        let opts = options();
        let root = parse_document(&arena, src, &opts);
        rouge_code_blocks(root);
        let mut blocks = Vec::new();
        for child in root.children() {
            let mut out = String::new();
            format_html(child, &opts, &mut out).unwrap();
            blocks.push(out);
        }
        (blocks, render(src))
    }

    /// Direct translation of the CSS rule in `_sass/_style.scss:34`:
    ///   `> p:nth-of-type(2), > :nth-of-type(4) { ~ * { display: none } }`
    /// Note `:nth-of-type(4)` counts per TAG NAME, not "4th child".
    fn visible_cut(kinds: &[String]) -> usize {
        let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        let mut cut = kinds.len();
        for (i, k) in kinds.iter().enumerate() {
            let n = seen.entry(k.as_str()).or_insert(0);
            *n += 1;
            if (k == "p" && *n == 2) || *n == 4 {
                cut = i + 1;
                break;
            }
        }
        cut
    }

    fn tag_of(html: &str) -> String {
        let h = html.trim_start();
        if !h.starts_with('<') {
            return "#text".into();
        }
        h[1..]
            .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .next()
            .unwrap_or("")
            .to_lowercase()
    }

    #[test]
    fn concat_equals_whole_over_corpus() {
        let root = std::path::Path::new("..");
        let mut n = 0;
        let mut bad: Vec<(String, usize, usize)> = Vec::new();
        let mut counts = Vec::new();
        let mut cuts: Vec<usize> = Vec::new();
        let (mut kept_bytes, mut total_bytes) = (0usize, 0usize);
        let (mut untouched, mut star) = (0usize, 0usize);
        for e in walkdir::WalkDir::new(root.join("_posts")).into_iter().flatten() {
            if !e.file_type().is_file() { continue; }
            if e.path().extension().is_none_or(|x| x != "md") { continue; }
            let text = std::fs::read_to_string(e.path()).unwrap();
            let body = match text.strip_prefix("---") {
                Some(r) => match r.find("\n---") { Some(i) => &r[i + 4..], None => &text },
                None => &text,
            };
            let (blocks, whole) = split(body);
            n += 1;
            counts.push(blocks.len());
            let kinds: Vec<String> = blocks.iter().map(|b| tag_of(b)).collect();
            let cut = visible_cut(&kinds);
            kept_bytes += blocks[..cut].iter().map(|b| b.len()).sum::<usize>();
            total_bytes += whole.len();
            cuts.push(cut);
            if cut == blocks.len() { untouched += 1; }
            if kinds.get(cut - 1).is_some_and(|k| k == "p") { star += 1; }
            let cat = blocks.concat();
            if cat != whole {
                bad.push((e.path().display().to_string(), cat.len(), whole.len()));
            }
        }
        counts.sort();
        cuts.sort();
        eprintln!("cut blocks: min {} median {} max {}", cuts[0], cuts[cuts.len()/2], cuts[cuts.len()-1]);
        eprintln!("summary bytes if truncated at build: {} of {} ({:.1}% saved)",
            kept_bytes, total_bytes, 100.0 - 100.0 * kept_bytes as f64 / total_bytes as f64);
        eprintln!("summaries not truncated at all (star visible today): {untouched} of {n}");
        eprintln!("summaries whose last KEPT block is a <p> (star appears if we truncate): {star} of {n}");
        eprintln!("posts: {n}, mismatched: {}", bad.len());
        eprintln!("blocks/post: min {} median {} max {}", counts[0], counts[counts.len()/2], counts[counts.len()-1]);
        for (p, a, b) in bad.iter().take(8) {
            eprintln!("  MISMATCH {p}: concat {a} bytes vs whole {b}");
        }
        assert!(n > 100, "corpus not found");
    }
}
