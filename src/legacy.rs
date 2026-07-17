//! The legacy composer: part maps → the pre-§5e BEM markup, byte-for-byte.
//!
//! **This module is scheduled to die.** It exists so the part-map extraction
//! (§5e step 1) can be verified against the byte-diff oracle before any markup
//! changes: layout kinds now *produce* parts, and this composer replays
//! exactly the strings `render.rs` used to build inline. When the theme
//! directory + fragment binder land (step 3), the by-eye chrome budget gets
//! spent once and this file is deleted with it.
//!
//! Everything here is knowingly diseased in the ways §5e catalogues: it
//! branches on the `tree` fact to pick between the two document shapes (§8b),
//! it emits both breadcrumb markup forms, and it names theme geometry
//! (`post-full__below-title`) from the wrong layer. Preserving the disease *is
//! the job* — the oracle is the point.

use crate::parts::PartMap;
use crate::render::{esc, Site};
use std::fmt::Write as _;

/// Compose `main` for any part map the legacy markup knows how to arrange.
pub fn compose(m: &PartMap, site: &Site) -> String {
    match m.kind {
        "document" if m.flag("tree") => page_html(m, site),
        "document" => post_html(m, site),
        "listing" => listing_html(m, site),
        "link_list" => link_list_html(m, site),
        "raw" => m.html("content").unwrap_or_default().to_string(),
        k => unreachable!("legacy composer has no arrangement for kind `{k}`"),
    }
}

// ------------------------------------------------------------ document/post

fn post_html(m: &PartMap, site: &Site) -> String {
    let content = m.html("content").unwrap_or_default();
    let mut s = String::with_capacity(content.len() + 2048);
    s.push_str("<div class=\"post-full\">\n\t<div class=\"post-full__main\">\n");
    s.push_str("<article class=\"post\">\n");
    let _ = write!(
        s,
        "\t<header class=\"post-header\">\n\t\t<h2>{}</h2>\n\t\t<a href=\"{}{}\" class=\"permalink\">permalink</a>\n\t</header>\n",
        esc(m.text("title").unwrap_or_default()),
        site.baseurl,
        m.text("url").unwrap_or_default()
    );
    s.push_str("\t<div class=\"post-full__below-title\">\n");
    s.push_str(&margin_html(m, site));
    let _ = write!(s, "\t\t<section>\n{content}\n\t\t</section>\n");
    s.push_str("\t</div>\n</article>\n");
    s.push_str(&neighbors_html(m, site));
    s.push_str("\t</div>\n</div>\n");
    s
}

/// The full-post margin: the `nav`/`span` breadcrumb form + tags.
fn margin_html(m: &PartMap, site: &Site) -> String {
    let mut s = String::new();
    s.push_str("\t\t<aside class=\"post-full__margin\" aria-label=\"Post navigation\">\n");
    s.push_str("\t\t\t<nav class=\"breadcrumbs\">\n\t\t\t\t");
    s.push_str(&crumb_spans(m, site));
    s.push_str("\n\t\t\t</nav>\n");
    s.push_str(&tags_html(m, site));
    s.push_str("\t\t</aside>\n");
    s
}

/// Crumbs as `breadcrumbs__part` spans joined by ` > ` separators — the form
/// shared by the post margin and the listing header (differing only in the
/// whitespace their containers wrap around this).
fn crumb_spans(m: &PartMap, site: &Site) -> String {
    let sep = "<span class=\"breadcrumbs__sep\" aria-hidden=\"true\"> &gt; </span>";
    let mut s = String::new();
    for (i, c) in m.stream("crumbs").iter().enumerate() {
        if i > 0 {
            s.push_str(sep);
        }
        match c.text("url") {
            Some(u) => {
                let _ = write!(
                    s,
                    "<span class=\"breadcrumbs__part\"><a href=\"{}{u}\">{}</a></span>",
                    site.baseurl,
                    esc(c.text("label").unwrap_or_default())
                );
            }
            None => {
                let _ = write!(
                    s,
                    "<span class=\"breadcrumbs__part\">{}</span>",
                    esc(c.text("label").unwrap_or_default())
                );
            }
        }
    }
    s
}

fn tags_html(m: &PartMap, site: &Site) -> String {
    let tags = m.stream("tags");
    if tags.is_empty() {
        return String::new();
    }
    let mut s = String::from("\t\t\t<div class=\"post-tags\" aria-label=\"Tags\">\n");
    for t in tags {
        let _ = write!(
            s,
            "\t\t\t\t<a class=\"post-tags__pill\" href=\"{}{}\">{}</a>\n",
            site.baseurl,
            t.text("url").unwrap_or_default(),
            t.text("name").unwrap_or_default()
        );
    }
    s.push_str("\t\t\t</div>\n");
    s
}

fn neighbors_html(m: &PartMap, site: &Site) -> String {
    // Absence of the part (row not in the posts table) means no nav at all;
    // an empty or half-empty stream still renders both section headings.
    if m.get("neighbors").is_none() {
        return String::new();
    }
    let row = |rel: &str| -> String {
        let Some(n) = m.stream("neighbors").iter().find(|n| n.text("rel") == Some(rel)) else {
            return String::new();
        };
        format!(
            "\t\t\t<a class=\"post-neighbors__row\" href=\"{}{}\">\n\t\t\t\t<time class=\"post-neighbors__date\" datetime=\"{}\">{}</time>\n\t\t\t\t<span class=\"post-neighbors__title\">{}</span>\n\t\t\t</a>\n",
            site.baseurl,
            n.text("url").unwrap_or_default(),
            n.text("date").unwrap_or_default(),
            n.text("date_pretty").unwrap_or_default(),
            esc(n.text("title").unwrap_or_default())
        )
    };
    format!(
        "\n\t\t<nav class=\"post-neighbors\" aria-label=\"More posts\">\n\t\t\t<section class=\"post-neighbors__block\">\n\t\t\t\t<h2 class=\"post-neighbors__heading\">Later post</h2>\n{}\t\t\t</section>\n\n\t\t\t<section class=\"post-neighbors__block\">\n\t\t\t\t<h2 class=\"post-neighbors__heading\">Earlier post</h2>\n{}\t\t\t</section>\n\t\t</nav>\n",
        row("newer"),
        row("older")
    )
}

// ------------------------------------------------------------ document/tree

/// The page shape: bare-`div` breadcrumbs above a single column — the *other*
/// breadcrumb form (§8b's drift, preserved for the oracle).
fn page_html(m: &PartMap, site: &Site) -> String {
    let content = m.html("content").unwrap_or_default();
    let title = m.text("title").unwrap_or_default();
    let mut s = String::with_capacity(content.len() + 1024);
    s.push_str("<div class=\"breadcrumbs\">\n");
    let crumbs = m.stream("crumbs");
    for c in crumbs {
        match c.text("url") {
            Some(u) => {
                let _ = write!(s, "\t<a href=\"{}{u}\">{}</a> &gt;\n", site.baseurl, esc(c.text("label").unwrap_or_default()));
            }
            None => {
                let _ = write!(s, "\t<span class=\"active\">{}</span>\n", esc(c.text("label").unwrap_or_default()));
            }
        }
    }
    s.push_str("</div>\n\n");
    s.push_str("<article class=\"page\">\n");
    let _ = write!(
        s,
        "\t<header class=\"post-header\">\n\t\t<h2>{}</h2>\n\t\t<a href=\"{}{}\" class=\"permalink\">permalink</a>\n\t</header>\n",
        esc(title),
        site.baseurl,
        m.text("url").unwrap_or_default()
    );
    let _ = write!(s, "\t<section>\n{content}\n\t</section>\n");
    s.push_str("</article>\n");
    s
}

// ---------------------------------------------------------------- listings

fn listing_html(m: &PartMap, site: &Site) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<header class="multipost-listing">
	<div class="multipost-listing__below-title">
		<aside class="post-full__margin" aria-label="Navigation">
			<nav class="breadcrumbs">{crumbs}</nav>
		</aside>
		<div class="multipost-listing__main">
			<header class="post-header"><h2>{t}</h2></header>
		</div>
	</div>
</header>
"#,
        crumbs = crumb_spans(m, site),
        t = esc(m.text("title").unwrap_or_default()),
    );
    for item in m.stream("items") {
        s.push_str(&summary_html(item, site));
    }
    if let Some(p) = m.map("pagination") {
        s.push_str(&pagination_html(p, site));
    }
    s
}

fn summary_html(m: &PartMap, site: &Site) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "<article class=\"post post-summary\">\n\t<a class=\"post-link\" href=\"{b}{u}\" aria-label=\"Read {t}\"></a>\n\t<div class=\"post-summary__body\">\n\t\t<aside class=\"post-summary__margin\" aria-label=\"Post date and tags\">\n",
        b = site.baseurl,
        u = m.text("url").unwrap_or_default(),
        t = esc(m.text("title").unwrap_or_default())
    );
    if let Some(d) = m.text("date") {
        let _ = write!(
            s,
            "\t\t\t<time class=\"post-date\" datetime=\"{d}T00:00:00+00:00\">{}</time>\n",
            m.text("date_pretty").unwrap_or_default()
        );
    }
    s.push_str(&tags_html(m, site));
    let _ = write!(
        s,
        "\t\t</aside>\n\t\t<div class=\"post-summary__main\">\n\t\t\t<header class=\"post-header\"><h2>{}</h2></header>\n\t\t\t<section>\n{}\n\t\t\t</section>\n\t\t</div>\n\t</div>\n</article>\n",
        esc(m.text("title").unwrap_or_default()),
        m.html("content").unwrap_or_default()
    );
    s
}

fn pagination_html(m: &PartMap, site: &Site) -> String {
    let b = site.baseurl;
    let mut s = String::new();
    s.push_str("<nav class=\"pagination\" aria-label=\"Blog pagination\">\n");
    match m.text("prev") {
        Some(u) => {
            let _ = write!(s, "\t<a rel=\"prev\" class=\"pagination__prev\" href=\"{b}{u}\">&#8592; Prev</a>\n");
        }
        None => s.push_str("\t<span class=\"pagination__prev is-disabled\">&#8592; Prev</span>\n"),
    }
    s.push_str("\t<ol class=\"pagination__pages\">\n");
    for p in m.stream("pages") {
        let n = p.text("n").unwrap_or_default();
        s.push_str("\t\t<li>");
        match p.text("url") {
            Some(u) => {
                let _ = write!(s, "<a class=\"pagination__num\" href=\"{b}{u}\">{n}</a>");
            }
            None => {
                let _ = write!(s, "<span class=\"pagination__num\" aria-current=\"page\">{n}</span>");
            }
        }
        s.push_str("</li>\n");
    }
    s.push_str("\t</ol>\n");
    match m.text("next") {
        Some(u) => {
            let _ = write!(s, "\t<a rel=\"next\" class=\"pagination__next\" href=\"{b}{u}\">Next &#8594;</a>\n");
        }
        None => s.push_str("\t<span class=\"pagination__next is-disabled\">Next &#8594;</span>\n"),
    }
    s.push_str("</nav>\n");
    s
}

fn link_list_html(m: &PartMap, site: &Site) -> String {
    let mut s = String::new();
    for l in m.stream("items") {
        let _ = write!(
            s,
            "<p>{} <a href=\"{}{}\">(read)</a></p>\n",
            esc(l.text("title").unwrap_or_default()),
            site.baseurl,
            l.text("url").unwrap_or_default()
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parts;

    fn site() -> Site<'static> {
        Site { url: "https://x", baseurl: "", title: "t", author: "a", email: None, css: "/c" }
    }

    fn pag(current: usize, total: usize) -> String {
        match parts::pagination(current, total) {
            Some(m) => pagination_html(&m, &site()),
            None => String::new(),
        }
    }

    // The pagination tests that used to live against `render::pagination` —
    // same assertions, now through parts + composer.
    #[test]
    fn single_page_has_no_nav() {
        assert_eq!(pag(1, 1), "");
    }

    #[test]
    fn first_page_disables_prev_and_links_page_two() {
        let s = pag(1, 3);
        assert!(s.contains(r#"<span class="pagination__prev is-disabled">"#), "{s}");
        assert!(s.contains(r#"aria-current="page">1</span>"#), "{s}");
        assert!(s.contains(r#"href="/blog/page/2">Next"#), "{s}");
    }

    #[test]
    fn last_page_disables_next() {
        let s = pag(3, 3);
        assert!(s.contains(r#"class="pagination__prev" href="/blog/page/2">"#), "{s}");
        assert!(s.contains(r#"<span class="pagination__next is-disabled">"#), "{s}");
    }

    /// Page 1 is `/blog/`, never `/blog/page/1` — from both the "1" tile and prev.
    #[test]
    fn page_one_link_has_no_page_segment() {
        let s = pag(2, 3);
        assert!(s.contains(r#"class="pagination__num" href="/blog/">1</a>"#), "{s}");
        assert!(s.contains(r#"class="pagination__prev" href="/blog/">"#), "{s}");
        assert!(!s.contains("/blog/page/1"), "{s}");
    }

    #[test]
    fn tree_document_uses_the_div_breadcrumb_form() {
        let m = parts::document_tree(
            "RomTool",
            "/code/legacy/romtool/",
            &[("/code/".into(), "Code".into()), ("/code/legacy/".into(), "Legacy".into())],
            "<p>hi</p>",
        );
        let s = compose(&m, &site());
        assert!(s.starts_with("<div class=\"breadcrumbs\">\n"), "{s}");
        assert!(s.contains("\t<a href=\"/code/\">Code</a> &gt;\n"), "{s}");
        assert!(s.contains("\t<span class=\"active\">RomTool</span>\n"), "{s}");
        assert!(s.contains("<article class=\"page\">"), "{s}");
    }
}
