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
    // comrak also injects an `<a class="anchor">` inside each heading, which
    // kramdown does not. **We keep it, deliberately** (2026-07-21): 226 of
    // them across 44 posts, each carrying an aria-label, which is a heading
    // affordance the Jekyll site never had and we want.
    //
    // It is a real divergence from the reference, and worth knowing that the
    // body oracle CANNOT see it: `diff::normalize` calls
    // `strip_comrak_anchors` before comparing, so the 90% parity figure is
    // computed with these removed. That normalizer was written to isolate the
    // slug question — do the two algorithms agree — and in doing so it hides
    // this one permanently. An earlier version of this comment claimed "the
    // real pipeline strips it in the AST pass"; nothing ever did, and the
    // harness being blind is exactly why no one noticed for a month. §8a's
    // rule, again: agreement is not evidence unless it can disagree.
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

// ------------------------------------------------------------------ blocks

/// A rendered document as its top-level block sequence (§6d). `whole` is the
/// exact `render()` output — documents and the feed use it unchanged, so the
/// byte oracle survives. `blocks` is the same tree formatted per top-level
/// child; a summary is a literal *prefix* of the document.
///
/// The invariant `concat(blocks) == whole` holds for 326/327 posts — the
/// exception is footnotes, whose definitions comrak relocates at parse time
/// (they are annotations addressed by identity, not blocks; §6d models them
/// as a second stream, deferred until sidenotes give it a consumer). The
/// corpus test below pins the exception set.
pub struct Doc {
    pub whole: String,
    /// Each top-level block's HTML, in order.
    pub blocks: Vec<String>,
}

pub fn render_doc(src: &str) -> Doc {
    render_doc_with(src, &|_| Ok(None)).expect("no-op resolver cannot fail")
}

/// `render_doc` with a link resolver (§6a row/view links): every markdown
/// link's destination is offered to `resolve`; `Ok(Some(url))` rewrites it,
/// `Ok(None)` leaves it, and an error aborts the render — a broken source
/// reference is a build error naming the file, not a shipped 404.
pub fn render_doc_with(
    src: &str,
    resolve: &dyn Fn(&str) -> anyhow::Result<Option<String>>,
) -> anyhow::Result<Doc> {
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, src, &opts);
    rouge_code_blocks(root);
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        if let comrak::nodes::NodeValue::Link(ref mut link) = data.value {
            if let Some(url) = resolve(&link.url)? {
                link.url = url;
            }
        }
    }
    let mut whole = String::new();
    format_html(root, &opts, &mut whole).expect("writing to a String cannot fail");
    let mut blocks = Vec::new();
    for child in root.children() {
        let mut html = String::new();
        format_html(child, &opts, &mut html).expect("writing to a String cannot fail");
        blocks.push(html);
    }
    Ok(Doc { whole, blocks })
}

impl Doc {
    /// A truncated projection of the document: blocks kept until either
    /// budget runs out, plus whether anything was cut (the `truncated` fact
    /// the theme gates the ★ on).
    ///
    /// This is **mechanism only** — the numbers are policy, and policy lives
    /// in the view that asks for the projection (`summary = { max_blocks,
    /// max_chars }` in config, §6d). `max_chars` counts *visible text*, not
    /// markup, so a rouge-wrapped code block doesn't blow the budget with
    /// spans; the block that would exceed it is dropped whole (block
    /// granularity), but at least one block is always kept.
    pub fn truncate(&self, max_blocks: Option<usize>, max_chars: Option<usize>) -> (String, bool) {
        let mut cut = self.blocks.len();
        let mut chars = 0usize;
        for (i, b) in self.blocks.iter().enumerate() {
            if let Some(mb) = max_blocks {
                if i >= mb {
                    cut = i;
                    break;
                }
            }
            if let Some(mc) = max_chars {
                chars += text_len(b);
                if chars > mc && i > 0 {
                    cut = i;
                    break;
                }
            }
        }
        let html: String = self.blocks[..cut].concat();
        (html, cut < self.blocks.len())
    }
}

/// Visible text length of an HTML fragment: characters outside tags.
/// Entity-naive (`&amp;` counts as 5), which errs on the side of keeping
/// summaries short — fine for a budget, wrong for typography.
fn text_len(html: &str) -> usize {
    let mut n = 0;
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => n += 1,
            _ => {}
        }
    }
    n
}

// ---------------------------------------------------------------- headings

/// One heading of a rendered document — §6e's heading axis.
#[derive(Debug, PartialEq)]
pub struct Heading {
    pub level: u8,
    /// The anchor target comrak emitted on the element.
    pub id: String,
    /// The heading's visible text, entities folded back.
    pub text: String,
}

impl Doc {
    /// The document's headings, extracted from the same rendered bytes
    /// that ship — link and target cannot desync (§6e), the search-wasm
    /// argument applied to anchors.
    pub fn headings(&self) -> Vec<Heading> {
        self.blocks.iter().filter_map(|b| heading_of(b)).collect()
    }
}

fn heading_of(html: &str) -> Option<Heading> {
    let h = html.trim_start();
    let rest = h.strip_prefix("<h")?;
    let level = rest.bytes().next().filter(u8::is_ascii_digit)? - b'0';
    if !(1..=6).contains(&level) {
        return None;
    }
    let i = h.find("id=\"")? + 4;
    let id = h[i..].split('"').next()?.to_string();
    let text = unescape(&visible_text(h));
    Some(Heading { level, id, text })
}

/// Visible text of a fragment: characters outside tags, trimmed. The
/// heading's trailing anchor link is empty, so it contributes nothing.
fn visible_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// Fold comrak's text escapes back to characters — the label re-escapes at
/// fill time (`Part::Text`), so leaving entities would double-escape.
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod link_tests {
    use super::*;

    /// §6a row/view links: the resolver rewrites destinations, leaves
    /// what it returns None for, and its errors abort the render.
    #[test]
    fn resolver_rewrites_and_fails_loud() {
        let doc = render_doc_with("[a](x.md) and [b](https://e.com/)", &|href| {
            Ok(match href {
                "x.md" => Some("/x/".to_string()),
                _ => None,
            })
        })
        .unwrap();
        assert!(doc.whole.contains(r#"href="/x/""#), "{}", doc.whole);
        assert!(
            doc.whole.contains(r#"href="https://e.com/""#),
            "{}",
            doc.whole
        );
        let err = render_doc_with("[a](nope.md)", &|_| anyhow::bail!("no such source"));
        let Err(e) = err else { panic!("a resolver error must fail the render") };
        // The resolver's own message must survive, not just some error.
        assert!(format!("{e:#}").contains("no such source"), "{e:#}");
    }
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

    /// The §6e sync property: every extracted heading's id appears in the
    /// block it came from — links target what actually shipped.
    #[test]
    fn headings_extract_in_sync_with_emitted_ids() {
        let d = render_doc("## Alpha & Beta\n\ntext\n\n### Gamma \"quoted\"\n\n## Delta");
        let hs = d.headings();
        assert_eq!(hs.len(), 3);
        assert_eq!((hs[0].level, hs[0].text.as_str()), (2, "Alpha & Beta"));
        assert_eq!((hs[1].level, hs[1].text.as_str()), (3, "Gamma “quoted”"));
        assert_eq!(hs[2].level, 2);
        for h in &hs {
            let target = format!("id=\"{}\"", h.id);
            assert!(
                d.blocks.iter().any(|b| b.contains(&target)),
                "{target} not emitted"
            );
        }
    }

    #[test]
    fn non_headings_extract_nothing() {
        let d = render_doc("plain paragraph\n\n    code block\n\n<hr>");
        assert!(d.headings().is_empty());
    }
}

// The §6d spike, promoted: the invariant it measured is now pinned over the
// corpus on every test run.
#[cfg(test)]
mod block_tests {
    use super::*;

    #[test]
    fn truncate_by_block_budget() {
        let d = render_doc("one\n\ntwo\n\nthree");
        let (html, truncated) = d.truncate(Some(2), None);
        assert!(html.contains("two") && !html.contains("three"), "{html}");
        assert!(truncated);
    }

    #[test]
    fn truncate_by_char_budget_at_block_granularity() {
        let d = render_doc("aaaa aaaa\n\nbbbb bbbb\n\ncccc");
        // ~9 visible chars per paragraph: a 12-char budget keeps one block.
        let (html, truncated) = d.truncate(None, Some(12));
        assert!(html.contains("aaaa") && !html.contains("bbbb"), "{html}");
        assert!(truncated);
    }

    #[test]
    fn char_budget_counts_text_not_markup() {
        // A rouge code block is markup-heavy; the budget sees only its text.
        let d = render_doc("    code\n\nnext");
        let (html, _) = d.truncate(None, Some(30));
        assert!(
            html.contains("next"),
            "markup counted against budget: {html}"
        );
    }

    #[test]
    fn first_block_survives_any_char_budget() {
        let d = render_doc("a paragraph longer than the tiny budget");
        let (html, truncated) = d.truncate(None, Some(1));
        assert_eq!(html, d.whole);
        assert!(!truncated);
    }

    #[test]
    fn no_budgets_means_no_truncation() {
        let d = render_doc("one\n\ntwo");
        let (html, truncated) = d.truncate(None, None);
        assert_eq!(html, d.whole);
        assert!(!truncated);
    }

    /// §6d's load-bearing invariant, pinned: every post's block concatenation
    /// is byte-identical to its whole render — a summary is a literal prefix
    /// of the document — with footnote posts as the *only* tolerated
    /// exception (comrak relocates their definitions at parse time; §6d
    /// models notes as a second stream, deferred to the sidenote pass).
    #[test]
    fn concat_equals_whole_over_corpus() {
        let root = crate::workspace_root().join("..");
        let mut n = 0;
        let mut mismatched: Vec<String> = Vec::new();
        for e in walkdir::WalkDir::new(root.join("_posts"))
            .into_iter()
            .flatten()
        {
            if !e.file_type().is_file() {
                continue;
            }
            if e.path().extension().is_none_or(|x| x != "md") {
                continue;
            }
            let text = std::fs::read_to_string(e.path()).unwrap();
            let (_, body) = crate::store::split_front_matter(&text);
            let d = render_doc(body);
            n += 1;
            let cat: String = d.blocks.concat();
            if cat != d.whole {
                mismatched.push(e.path().file_name().unwrap().to_string_lossy().into());
            }
        }
        assert!(n > 300, "corpus not found ({n} posts)");
        assert_eq!(
            mismatched,
            vec!["2026-06-11-life-before-main.md".to_string()],
            "the footnote post is the only tolerated concat mismatch"
        );
    }
}
