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
    // A real divergence the body oracle CANNOT see: `diff::normalize` calls
    // `strip_comrak_anchors` before comparing, so the parity figure is
    // computed with these removed. §8a: agreement is not evidence unless it
    // can disagree.
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
/// A block with a real language gets token spans from `crate::highlight` — the
/// four classes the theme colours (`k`/`s`/`c1`/`n`), not Rouge's full Pygments
/// set. Inline code and `text` blocks stay plain, as they are on the reference.
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
                            crate::highlight::render(lang, &ncb.literal)
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
    render_doc_with(src, &|_, _| Ok(None)).expect("no-op resolver cannot fail")
}

/// `render_doc` with a citation resolver (§6a row/view links, IO.md §4a):
/// every markdown link's and image's destination is offered to `resolve`;
/// `Ok(Some(url))` rewrites it, `Ok(None)` leaves it, and an error aborts the
/// render — a broken source reference is a build error naming the file, not a
/// shipped 404.
///
/// **Links and images are offered separately**, tagged with which they are.
/// They were not before: the AST pass visited `Link` and skipped `Image`, so
/// `![](x.png)` reached no resolver at all, and §6d stage B's raw-HTML twin
/// declined `img[src]` to keep the two paths equal. Both now offer both, and
/// what the two forms MEAN is `links::resolve`'s to say — an authored link
/// demands a route, an embed takes the strong address.
pub fn render_doc_with(
    src: &str,
    resolve: &dyn Fn(crate::markup::Cite, &str) -> anyhow::Result<Option<String>>,
) -> anyhow::Result<Doc> {
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, src, &opts);
    rouge_code_blocks(root);
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        match data.value {
            comrak::nodes::NodeValue::Link(ref mut link) => {
                if let Some(url) = resolve(crate::markup::Cite::Link, &link.url)? {
                    link.url = url;
                }
            }
            comrak::nodes::NodeValue::Image(ref mut img) => {
                if let Some(url) = resolve(crate::markup::Cite::Embed, &img.url)? {
                    img.url = url;
                }
            }
            // Raw HTML embedded in markdown (`<img src=…>`, `<a href=…>`, an
            // `<iframe>`) is an opaque Html node comrak never descends into, so
            // its citations bypassed the resolver — a relative `<img src>` in a
            // markdown body shipped a 404. Run the literal through the same
            // raw-HTML rewriter the non-markdown path uses (`render_source`),
            // so both paths offer both forms (the doc above).
            comrak::nodes::NodeValue::HtmlBlock(ref mut block) => {
                block.literal = crate::rewrite::resolve_links(&block.literal, resolve)?;
            }
            comrak::nodes::NodeValue::HtmlInline(ref mut html) => {
                *html = crate::rewrite::resolve_links(html, resolve)?;
            }
            _ => {}
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

/// Markdown or raw HTML through the same link resolver.
pub fn render_source(
    src: &str,
    markdown: bool,
    resolve: &dyn Fn(crate::markup::Cite, &str) -> anyhow::Result<Option<String>>,
) -> anyhow::Result<(String, Option<Doc>)> {
    if markdown {
        let doc = render_doc_with(src, resolve)?;
        Ok((doc.whole.clone(), Some(doc)))
    } else {
        Ok((crate::rewrite::resolve_links(src, resolve)?, None))
    }
}

impl Doc {
    /// A truncated projection of the document: blocks kept until either
    /// budget runs out, plus whether anything was cut (the `truncated` fact
    /// the theme gates the ★ on).
    ///
    /// Mechanism only: policy is the field expression on the view
    /// (`truncate_chars(truncate_blocks(content, n), m)`, §5f / §6d).
    /// Composes the two wrappers the expression language exposes.
    pub fn truncate(&self, max_blocks: Option<usize>, max_chars: Option<usize>) -> (String, bool) {
        let mut c = grackle_db::Content::new(self.blocks.clone());
        if let Some(n) = max_blocks {
            c = c.truncate_blocks(n).unwrap_or(c);
        }
        if let Some(n) = max_chars {
            c = c.truncate_chars(n).unwrap_or(c);
        }
        (c.html_string(), c.truncated)
    }
}

/// One heading of a rendered document — defined in [`grackle_model::Heading`].
pub use grackle_model::Heading;

impl Doc {
    /// The document's headings, extracted from the same rendered bytes
    /// that ship — link and target cannot desync (§6e), the search-wasm
    /// argument applied to anchors.
    pub fn headings(&self) -> Vec<Heading> {
        grackle_db::Content::new(self.blocks.clone())
            .headings()
            .into_iter()
            .map(|h| Heading {
                level: h.level,
                id: h.id,
                text: h.text,
            })
            .collect()
    }
}

#[cfg(test)]
mod link_tests {
    use super::*;

    /// §6a row/view links: the resolver rewrites destinations, leaves
    /// what it returns None for, and its errors abort the render.
    #[test]
    fn resolver_rewrites_and_fails_loud() {
        let doc = render_doc_with("[a](x.md) and [b](https://e.com/)", &|_, href| {
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
        let err = render_doc_with("[a](nope.md)", &|_, _| anyhow::bail!("no such source"));
        let Err(e) = err else {
            panic!("a resolver error must fail the render")
        };
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
            let (_, _, body) = crate::store::split_front_matter(&text);
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
