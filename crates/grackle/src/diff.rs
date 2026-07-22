//! Golden comparison against the Jekyll build (DESIGN.md §7).
//!
//! Post *bodies* only: comrak against kramdown. Chrome is never compared, and
//! the URL set is `urls.rs` — a clean run here says nothing about either.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// Byte-identical after trimming.
    Identical,
    /// Identical once whitespace/entity noise is normalised away.
    Equivalent,
    /// A real difference in the emitted markup.
    Differs,
    /// No reference rendering to compare against.
    Missing,
}

/// Pull the rendered markdown body out of a Jekyll page.
///
/// The body is the `<section>` inside `<article class="post">`. The reference
/// layouts have since changed (`header.permalink` became `header.post-header`),
/// so this targets the innermost `<section>` rather than the surrounding
/// chrome.
///
/// The article region **must** be bounded before searching: a page has
/// `<section class="content">` outside the article and `<section class="related">`
/// after it, so an unbounded `rfind("</section>")` runs past `</article>` and
/// swallows the whole page.
pub fn extract_body(html: &str) -> Option<String> {
    let a = html.find("<article")?;
    let rest = &html[a..];
    let article_end = rest.find("</article>").unwrap_or(rest.len());
    let region = &rest[..article_end];

    let s = region.find("<section>")? + "<section>".len();
    let end = region.rfind("</section>")?;
    if end <= s {
        return None;
    }
    Some(strip_layout_chrome(&region[s..end]))
}

/// Remove markup the *layout* put inside the body section.
///
/// The reference build's `article.html` appended a "Read full post" link inside
/// `<section>`; the current one does not. Either way it is layout chrome, never
/// markdown output, and counting it as a markdown difference would be wrong.
fn strip_layout_chrome(body: &str) -> String {
    let mut out = body.to_string();
    while let Some(i) = out.find("<a class=\"fullpost\"") {
        let Some(j) = out[i..].find("</a>") else {
            break;
        };
        out.replace_range(i..i + j + "</a>".len(), "");
    }
    out
}

/// Collapse the differences that are not differences.
///
/// Whitespace between tags, entity spellings, and self-closing style are all
/// invisible to a reader; treating them as diffs would bury the real signal.
pub fn normalize(html: &str) -> String {
    let mut s = html.to_string();

    // comrak injects `<a href="#slug" … class="anchor"></a>` inside every
    // heading; kramdown emits only the id. The SHIPPED pipeline keeps these
    // (markdown.rs) — they are stripped here only so the measurement stays
    // pointed at the slug question, which means this comparison is blind to
    // that divergence by construction.
    s = strip_comrak_anchors(&s);

    // Entity spellings that render identically.
    for (from, to) in [
        ("&#8217;", "’"),
        ("&#8216;", "‘"),
        ("&#8220;", "“"),
        ("&#8221;", "”"),
        ("&#8211;", "–"),
        ("&#8212;", "—"),
        ("&#8230;", "…"),
        ("&rsquo;", "’"),
        ("&lsquo;", "‘"),
        ("&ldquo;", "“"),
        ("&rdquo;", "”"),
        ("&ndash;", "–"),
        ("&mdash;", "—"),
        ("&hellip;", "…"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
        (" />", ">"),
        ("<br/>", "<br>"),
        ("<hr/>", "<hr>"),
    ] {
        s = s.replace(from, to);
    }

    // Collapse all runs of whitespace to one space, then drop it between tags.
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            in_ws = true;
            continue;
        }
        if in_ws && !out.is_empty() {
            out.push(' ');
        }
        in_ws = false;
        out.push(ch);
    }
    out.replace("> <", "><").trim().to_string()
}

/// Fold every curly quote to its ASCII form, to isolate smartypants heuristic
/// differences from real markup differences.
pub fn quote_blind(s: &str) -> String {
    s.replace(['\u{2018}', '\u{2019}'], "'")
        .replace(['\u{201C}', '\u{201D}'], "\"")
}

/// Remove comrak's heading anchor links. See `normalize`.
fn strip_comrak_anchors(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<a href=\"#") {
        // Only an anchor comrak generated: it carries class="anchor" and is
        // immediately self-closed. Anything else is the author's own link.
        let Some(close) = rest[i..].find("></a>") else {
            break;
        };
        let tag = &rest[i..i + close + "></a>".len()];
        if tag.contains("class=\"anchor\"") {
            out.push_str(&rest[..i]);
            rest = &rest[i + close + "></a>".len()..];
        } else {
            out.push_str(&rest[..i + 2]);
            rest = &rest[i + 2..];
        }
    }
    out.push_str(rest);
    out
}

/// Best-effort attribution for a real difference, so the report says what to
/// fix rather than just how many are broken.
///
/// Classifies the **window around the first divergence**, not the whole
/// document — otherwise a post that merely contains a code block anywhere gets
/// blamed on code blocks regardless of where it actually differs.
pub fn classify_cause(ref_win: &str, mine_win: &str) -> &'static str {
    let both = format!("{ref_win}\u{0}{mine_win}");
    let has = |n: &str| both.contains(n);
    if has("highlighter-rouge") || has("<div class=\"highlight\">") {
        "code block markup (Rouge wrapper divs)"
    } else if has("<table") || has("<td") || has("<th") {
        "table"
    } else if has("footnote") {
        "footnotes"
    } else if has("<h1") || has("<h2") || has("<h3") || has("<h4") {
        "heading (id / slug rules)"
    } else if has("<img") {
        "image markup"
    } else if has("<pre") || has("<code") {
        "code / preformatted"
    } else if has("<li") || has("<ul") || has("<ol") {
        "list"
    } else if has("<a href") {
        "link"
    } else if has("<blockquote") {
        "blockquote"
    } else {
        "inline / prose"
    }
}

pub struct Row {
    pub url: String,
    pub verdict: Verdict,
    pub cause: Option<&'static str>,
    /// Differs only in which curly quote was chosen — the smartypants
    /// heuristic, not the markup.
    pub quotes_only: bool,
    pub reference: String,
    pub mine: String,
}

/// Compare one post body against its reference rendering.
pub fn compare_post(url: &str, body_md: &str, site: &Path) -> Result<Row> {
    let rel = url.trim_start_matches('/').trim_end_matches('/');
    let path = site.join(rel).join("index.html");
    if !path.exists() {
        return Ok(Row {
            url: url.into(),
            verdict: Verdict::Missing,
            cause: None,
            quotes_only: false,
            reference: String::new(),
            mine: String::new(),
        });
    }
    let html =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let reference = extract_body(&html).unwrap_or_default();
    let mine = crate::markdown::render(body_md);

    let (rn, mn) = (normalize(&reference), normalize(&mine));
    let verdict = if reference.trim() == mine.trim() {
        Verdict::Identical
    } else if rn == mn {
        Verdict::Equivalent
    } else {
        Verdict::Differs
    };
    let cause = (verdict == Verdict::Differs).then(|| {
        let (a, b) = first_delta(&rn, &mn);
        classify_cause(&a, &b)
    });
    let quotes_only = verdict == Verdict::Differs && quote_blind(&rn) == quote_blind(&mn);
    Ok(Row {
        url: url.into(),
        verdict,
        cause,
        quotes_only,
        reference: rn,
        mine: mn,
    })
}

/// First differing window, for eyeballing a single post.
pub fn first_delta(reference: &str, mine: &str) -> (String, String) {
    let a: Vec<char> = reference.chars().collect();
    let b: Vec<char> = mine.chars().collect();
    let mut i = 0;
    while i < a.len() && i < b.len() && a[i] == b[i] {
        i += 1;
    }
    let start = i.saturating_sub(60);
    let win = |v: &Vec<char>| -> String {
        v[start.min(v.len())..(i + 90).min(v.len())]
            .iter()
            .collect()
    };
    (win(&a), win(&b))
}

pub type Tally = BTreeMap<Verdict, usize>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The real page shape: a `<section class="content">` wraps the article and
    /// a `<section class="related">` follows it. An earlier version passed only
    /// because the test pre-sliced the input to `</article>` — the function must
    /// bound the region itself.
    #[test]
    fn extracts_only_the_article_body() {
        let h = r#"<section class="content"><article class="post"><header><h2>T</h2></header><section><p>hi</p></section><div class="date">d</div></article><section class="related">nope</section></section>"#;
        assert_eq!(extract_body(h).unwrap().trim(), "<p>hi</p>");
    }

    #[test]
    fn strips_layout_injected_chrome() {
        let h = r#"<article class="post"><section><p>hi</p>
    <a class="fullpost" href="/x/">Read full post</a>
    </section></article>"#;
        let b = extract_body(h).unwrap();
        assert!(b.contains("<p>hi</p>"));
        assert!(
            !b.contains("fullpost"),
            "layout chrome leaked into the body: {b}"
        );
    }

    #[test]
    fn strip_comrak_anchors_spares_author_links() {
        // comrak's generated anchor goes...
        let s = strip_comrak_anchors(
            r##"<h2 id="x">X<a href="#x" class="anchor"></a></h2><p><a href="#x">mine</a></p>"##,
        );
        assert!(!s.contains("class=\"anchor\""));
        // ...but a hand-written same-target link stays.
        assert!(s.contains(r##"<a href="#x">mine</a>"##), "{s}");
    }

    #[test]
    fn normalize_collapses_only_invisible_differences() {
        assert_eq!(
            normalize("<p>a</p>\n\n<p>b</p>"),
            normalize("<p>a</p><p>b</p>")
        );
        assert_eq!(normalize("<p>it&#8217;s</p>"), normalize("<p>it’s</p>"));
        assert_eq!(normalize("<br/>"), normalize("<br>"));
        assert_eq!(normalize("<p>a  b</p>"), "<p>a b</p>");
    }

    #[test]
    fn normalize_keeps_real_differences() {
        assert_ne!(normalize("<p>a</p>"), normalize("<p>b</p>"));
        assert_ne!(normalize("<em>a</em>"), normalize("<strong>a</strong>"));
        // whitespace inside text is collapsed, not deleted
        assert_ne!(normalize("<p>ab</p>"), normalize("<p>a b</p>"));
    }

    #[test]
    fn finds_the_first_delta() {
        // Inputs must exceed the window, or `start`/`end` clamp to the whole
        // string and returning the input unchanged would pass.
        let pad = "x".repeat(200);
        let (a, b) = first_delta(&format!("{pad}world"), &format!("{pad}there"));
        assert_eq!(a, format!("{}world", "x".repeat(60)));
        assert_eq!(b, format!("{}there", "x".repeat(60)));
    }
}
