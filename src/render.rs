//! Presentation: the four layers of DESIGN.md §5a.
//!
//!   schema  -> head facts (computed, never branched)
//!   render  -> semantic fragment (comrak)
//!   layout  -> document | listing | feed | raw   (fills `main`)
//!   theme   -> shell + css                       (default | light)
//!
//! The layout kinds emit the *existing* BEM hooks, so `_sass` works unchanged
//! as the `default` theme. Stable class names are the contract (§5a).

use crate::db::{Post, RouteKind, SiteDb};
use std::fmt::Write as _;

// ---------------------------------------------------------------- escaping

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

// ------------------------------------------------------------- head facts

/// Typed facts derived from a row's schema. A theme renders the subset it
/// wants; nobody branches on "am I a post" (§5a).
#[derive(Debug, Default)]
pub struct Head {
    pub title: String,
    pub description: Option<String>,
    pub canonical: String,
    pub noindex: bool,
    /// "article" when the row has a date, else "website" — a fact, not a branch.
    pub og_type: &'static str,
    pub published: Option<String>,
    pub author: String,
    pub jsonld: Option<String>,
}

pub struct Site<'a> {
    pub url: &'a str,
    pub baseurl: &'a str,
    pub title: &'a str,
    pub author: &'a str,
    pub email: Option<&'a str>,
    pub css: &'a str,
}

pub fn head_for_post(p: &Post, site: &Site) -> Head {
    let canonical = format!("{}{}", site.url, p.url);
    let published = p.date.map(|d| format!("{}T00:00:00+00:00", d.format("%Y-%m-%d")));
    let jsonld = published.as_ref().map(|ts| {
        let mut j = String::new();
        let _ = write!(
            j,
            r#"{{"@context":"https://schema.org","@type":"BlogPosting","headline":{},"mainEntityOfPage":{{"@type":"WebPage","@id":{}}},"url":{},"datePublished":"{ts}","dateModified":"{ts}","author":{{"@type":"Person","name":{},"url":{}}},"publisher":{{"@type":"Person","name":{}}}"#,
            json_str(&p.title),
            json_str(&canonical),
            json_str(&canonical),
            json_str(site.author),
            json_str(&format!("{}/", site.url)),
            json_str(site.author),
        );
        if let Some(d) = &p.description {
            let _ = write!(j, r#","description":{}"#, json_str(d));
        }
        j.push('}');
        j
    });
    Head {
        title: p.title.clone(),
        description: p.description.clone(),
        canonical,
        noindex: p.noindex,
        og_type: if p.date.is_some() { "article" } else { "website" },
        published,
        author: site.author.to_string(),
        jsonld,
    }
}

pub fn head_simple(title: &str, url: &str, site: &Site, noindex: bool) -> Head {
    Head {
        title: title.into(),
        description: None,
        canonical: format!("{}{}", site.url, url),
        noindex,
        og_type: "website",
        published: None,
        author: site.author.to_string(),
        jsonld: None,
    }
}

// ----------------------------------------------------------------- themes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Default,
    Light,
}

impl Theme {
    pub fn parse(s: Option<&str>) -> Theme {
        match s {
            Some("light") => Theme::Light,
            _ => Theme::Default,
        }
    }

    /// A theme owns the shell *and* the css, and renders whichever head facts
    /// it cares about. `light` deliberately takes almost none — it is the
    /// falsifier for the layer boundary (§5a).
    pub fn shell(self, head: &Head, main: &str, site: &Site, body_class: &str) -> String {
        match self {
            Theme::Light => light_shell(head, main),
            Theme::Default => default_shell(head, main, site, body_class),
        }
    }
}

fn light_shell(head: &Head, main: &str) -> String {
    let robots = if head.noindex {
        "\n\t<meta name=\"robots\" content=\"noindex,follow\">"
    } else {
        ""
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\t<title>{}</title>{robots}\n\t<meta charset=\"utf-8\">\n</head>\n<body>\n{main}\n</body>\n</html>\n",
        esc(&head.title)
    )
}

const FAVICONS: &str = r#"
	<link rel="apple-touch-icon" sizes="180x180" href="/resource/favicon/apple-touch-icon-180x180.png">
	<link rel="icon" type="image/png" href="/resource/favicon/favicon-192x192.png">
	<meta name="apple-mobile-web-app-title" content="grack.com">
	<meta name="application-name" content="grack.com">"#;

fn default_shell(head: &Head, main: &str, site: &Site, body_class: &str) -> String {
    let mut h = String::with_capacity(main.len() + 4096);
    h.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    let _ = write!(h, "\t<title>{}</title>\n", esc(&head.title));
    let _ = write!(h, "\t<meta property=\"og:title\" content=\"{}\">\n", esc(&head.title));
    if let Some(d) = &head.description {
        let _ = write!(h, "\t<meta name=\"description\" content=\"{}\">\n", esc(d));
        let _ = write!(h, "\t<meta property=\"og:description\" content=\"{}\">\n", esc(d));
    }
    let _ = write!(h, "\t<link rel=\"canonical\" href=\"{}\">\n", esc(&head.canonical));
    let _ = write!(h, "\t<meta name=\"author\" content=\"{}\">\n", esc(&head.author));
    let _ = write!(h, "\t<meta property=\"og:url\" content=\"{}\">\n", esc(&head.canonical));
    let _ = write!(h, "\t<meta property=\"og:type\" content=\"{}\">\n", head.og_type);
    if let Some(ts) = &head.published {
        let _ = write!(h, "\t<meta property=\"article:published_time\" content=\"{ts}\">\n");
        let _ = write!(h, "\t<meta property=\"article:author\" content=\"{}\">\n", esc(&head.author));
    }
    if let Some(j) = &head.jsonld {
        let _ = write!(h, "\t<script type=\"application/ld+json\">\n\t{j}\n\t</script>\n");
    }
    h.push_str("\t<meta charset=\"utf-8\">\n");
    h.push_str("\t<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = write!(h, "\t<link href='{}' rel='stylesheet' type='text/css'>\n", site.css);
    h.push_str(FAVICONS);
    if head.noindex {
        h.push_str("\n\t<meta name=\"robots\" content=\"noindex,follow\">");
    }
    h.push_str("\n</head>\n");
    let _ = write!(h, "<body{}>\n\n<div class=\"layout\">\n\n", body_class);
    let _ = write!(
        h,
        r#"<header class="header" data-swiftype-index="false">
	<nav class="subheader" aria-label="Site sections">
		<h2><a href="{b}/blog">Blog</a></h2>
		<h2><a href="{b}/writing">Writing</a></h2>
		<h2><a href="{b}/code">Code</a></h2>
	</nav>
	<h1><a id="root" href="{b}/">grack.com</a></h1>
</header>

<div class="body">

<main class="content">

{main}

</main>

</div>

<footer class="footer" data-swiftype-index="false">
	<p class="copyright">&copy; 1998-2026 Matt Mastracci &mdash; <a href="{b}/contact/">contact</a></p>
</footer>

</div>

</body>
</html>
"#,
        b = site.baseurl,
        main = main
    );
    h
}

// ----------------------------------------------------------- layout kinds

/// One row, full content + whatever relations its schema affords (§5a).
pub fn document(db: &SiteDb, p: &Post, content: &str, site: &Site) -> String {
    let mut m = String::with_capacity(content.len() + 2048);
    m.push_str("<div class=\"post-full\">\n\t<div class=\"post-full__main\">\n");
    m.push_str("<article class=\"post\">\n");
    let _ = write!(
        m,
        "\t<header class=\"post-header\">\n\t\t<h2>{}</h2>\n\t\t<a href=\"{}{}\" class=\"permalink\">permalink</a>\n\t</header>\n",
        esc(&p.title),
        site.baseurl,
        p.url
    );
    m.push_str("\t<div class=\"post-full__below-title\">\n");
    m.push_str(&margin(p, site));
    let _ = write!(m, "\t\t<section>\n{content}\n\t\t</section>\n");
    m.push_str("\t</div>\n</article>\n");
    m.push_str(&neighbors(db, p, site));
    m.push_str("\t</div>\n</div>\n");
    m
}

/// The margin of a full document: breadcrumbs + tags.
///
/// Note there is deliberately **no `<time class="post-date">`** here — on a
/// full post the date is expressed *by* the breadcrumb trail
/// (`… > 2022 December > 16`). `post-date` belongs to the summary layout only.
/// Adding one here was a real diff against the live site.
///
/// Which parts appear is schema-driven: a row with a date gets the date trail,
/// a draft gets the drafts trail, a row with tags gets tags (§5a).
fn margin(p: &Post, site: &Site) -> String {
    let mut s = String::new();
    s.push_str("\t\t<aside class=\"post-full__margin\" aria-label=\"Post navigation\">\n");
    let sep = "<span class=\"breadcrumbs__sep\" aria-hidden=\"true\"> &gt; </span>";
    let _ = write!(
        s,
        "\t\t\t<nav class=\"breadcrumbs\">\n\t\t\t\t<span class=\"breadcrumbs__part\"><a href=\"{b}/\">Home</a></span>{sep}<span class=\"breadcrumbs__part\"><a href=\"{b}/blog\">Blog</a></span>",
        b = site.baseurl
    );
    if p.draft {
        let _ = write!(
            s,
            "{sep}<span class=\"breadcrumbs__part\"><a href=\"{b}/drafts\">Drafts</a></span>{sep}<span class=\"breadcrumbs__part\">{t}</span>",
            b = site.baseurl,
            t = esc(&p.title)
        );
    } else if let Some(d) = p.date {
        let _ = write!(
            s,
            "{sep}<span class=\"breadcrumbs__part\"><a href=\"{b}/blog/{ym}\">{pretty}</a></span>{sep}<span class=\"breadcrumbs__part\">{day}</span>",
            b = site.baseurl,
            ym = d.format("%Y/%m"),
            pretty = d.format("%Y %B"),
            day = d.format("%-d")
        );
    }
    s.push_str("\n\t\t\t</nav>\n");
    s.push_str(&tags(p, site));
    s.push_str("\t\t</aside>\n");
    s
}

fn tags(p: &Post, site: &Site) -> String {
    if p.tags.is_empty() {
        return String::new();
    }
    let mut s = String::from("\t\t\t<div class=\"post-tags\" aria-label=\"Tags\">\n");
    for t in &p.tags {
        let _ = write!(
            s,
            "\t\t\t\t<a class=\"post-tags__pill\" href=\"{}/blog/tags/{t}/\">{t}</a>\n",
            site.baseurl
        );
    }
    s.push_str("\t\t\t</div>\n");
    s
}

/// Temporal relations. Present because the schema has a date, not because the
/// template asked "am I a post" (§5a).
fn neighbors(db: &SiteDb, p: &Post, site: &Site) -> String {
    let Some(&i) = db.posts.by_url.get(&p.url) else {
        return String::new();
    };
    let (newer, older) = db.posts.neighbors(i);
    let link = |idx: Option<usize>| -> String {
        match idx {
            Some(j) => {
                let n = &db.posts.rows[j];
                let d = n.date.map(|d| d.format("%-d %B %Y").to_string()).unwrap_or_default();
                let iso = n.date.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
                format!(
                    "\t\t\t<a class=\"post-neighbors__row\" href=\"{}{}\">\n\t\t\t\t<time class=\"post-neighbors__date\" datetime=\"{iso}\">{d}</time>\n\t\t\t\t<span class=\"post-neighbors__title\">{}</span>\n\t\t\t</a>\n",
                    site.baseurl,
                    n.url,
                    esc(&n.title)
                )
            }
            None => String::new(),
        }
    };
    format!(
        "\n\t\t<nav class=\"post-neighbors\" aria-label=\"More posts\">\n\t\t\t<section class=\"post-neighbors__block\">\n\t\t\t\t<h2 class=\"post-neighbors__heading\">Later post</h2>\n{}\t\t\t</section>\n\n\t\t\t<section class=\"post-neighbors__block\">\n\t\t\t\t<h2 class=\"post-neighbors__heading\">Earlier post</h2>\n{}\t\t\t</section>\n\t\t</nav>\n",
        link(newer),
        link(older)
    )
}

/// N rows, summarised. One kind — the view supplies the query, filter and
/// title, so `tag_index`/`monthly_archive`/`blog_index` are all this (§5a).
pub fn listing(
    rows: &[(&Post, String)],
    title: &str,
    breadcrumb_tail: Option<&str>,
    site: &Site,
    pagination: Option<&str>,
) -> String {
    let mut m = String::new();
    let tail = match breadcrumb_tail {
        Some(t) => format!(
            "<span class=\"breadcrumbs__sep\" aria-hidden=\"true\"> &gt; </span><span class=\"breadcrumbs__part\">{}</span>",
            esc(t)
        ),
        None => String::new(),
    };
    let _ = write!(
        m,
        r#"<header class="multipost-listing">
	<div class="multipost-listing__below-title">
		<aside class="post-full__margin" aria-label="Navigation">
			<nav class="breadcrumbs"><span class="breadcrumbs__part"><a href="{b}/">Home</a></span><span class="breadcrumbs__sep" aria-hidden="true"> &gt; </span><span class="breadcrumbs__part"><a href="{b}/blog">Blog</a></span>{tail}</nav>
		</aside>
		<div class="multipost-listing__main">
			<header class="post-header"><h2>{t}</h2></header>
		</div>
	</div>
</header>
"#,
        b = site.baseurl,
        t = esc(title),
        tail = tail
    );
    for (p, content) in rows {
        m.push_str(&summary(p, content, site));
    }
    if let Some(nav) = pagination {
        m.push_str(nav);
    }
    m
}

fn summary(p: &Post, content: &str, site: &Site) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "<article class=\"post post-summary\">\n\t<a class=\"post-link\" href=\"{b}{u}\" aria-label=\"Read {t}\"></a>\n\t<div class=\"post-summary__body\">\n\t\t<aside class=\"post-summary__margin\" aria-label=\"Post date and tags\">\n",
        b = site.baseurl,
        u = p.url,
        t = esc(&p.title)
    );
    if let Some(d) = p.date {
        let _ = write!(
            s,
            "\t\t\t<time class=\"post-date\" datetime=\"{}T00:00:00+00:00\">{}</time>\n",
            d.format("%Y-%m-%d"),
            d.format("%-d %B %Y")
        );
    }
    s.push_str(&tags(p, site));
    let _ = write!(
        s,
        "\t\t</aside>\n\t\t<div class=\"post-summary__main\">\n\t\t\t<header class=\"post-header\"><h2>{}</h2></header>\n\t\t\t<section>\n{content}\n\t\t\t</section>\n\t\t</div>\n\t</div>\n</article>\n",
        esc(&p.title)
    );
    s
}

/// The blog pagination nav — §5d's one genuine "component": a range loop over
/// pages and a three-way conditional (current / first / other). Empty when
/// there is a single page (`total_pages > 1` in the template).
///
/// Page 1 lives at `/blog/`; page N>1 **links** `/blog/page/N` with no trailing
/// slash, matching jekyll-paginate's `paginate_path` — even though the page
/// itself is served from `/blog/page/N/`. Faithful to the live site.
pub fn pagination(current: usize, total: usize, site: &Site) -> String {
    if total <= 1 {
        return String::new();
    }
    let b = site.baseurl;
    let path = |n: usize| {
        if n <= 1 {
            format!("{b}/blog/")
        } else {
            format!("{b}/blog/page/{n}")
        }
    };
    let mut s = String::new();
    s.push_str("<nav class=\"pagination\" aria-label=\"Blog pagination\">\n");
    if current > 1 {
        let _ = write!(
            s,
            "\t<a rel=\"prev\" class=\"pagination__prev\" href=\"{}\">&#8592; Prev</a>\n",
            path(current - 1)
        );
    } else {
        s.push_str("\t<span class=\"pagination__prev is-disabled\">&#8592; Prev</span>\n");
    }
    s.push_str("\t<ol class=\"pagination__pages\">\n");
    for n in 1..=total {
        s.push_str("\t\t<li>");
        if n == current {
            let _ = write!(s, "<span class=\"pagination__num\" aria-current=\"page\">{n}</span>");
        } else {
            let _ = write!(s, "<a class=\"pagination__num\" href=\"{}\">{n}</a>", path(n));
        }
        s.push_str("</li>\n");
    }
    s.push_str("\t</ol>\n");
    if current < total {
        let _ = write!(
            s,
            "\t<a rel=\"next\" class=\"pagination__next\" href=\"{}\">Next &#8594;</a>\n",
            path(current + 1)
        );
    } else {
        s.push_str("\t<span class=\"pagination__next is-disabled\">Next &#8594;</span>\n");
    }
    s.push_str("</nav>\n");
    s
}

/// One tree row, full content. The same `document` kind as a post — the
/// relations differ because the *schema* differs (§5a): a post has a date, so
/// it gets a date trail; a tree page has ancestors, so it gets those.
///
/// The **structure** differs too, and that is theme-imposed rather than
/// schema-driven: the theme styles `.post:not(.post-summary)` as a two-column
/// margin layout (`post-full__below-title` + `post-full__margin`) and `.page`
/// as a single column with breadcrumbs above. Emitting the post shape here made
/// the header 800px wide against live's 640. So `document` has two shapes for
/// now — a real tension with §5a's "one document kind", resolvable only by
/// changing the theme, which is a separate decision.
pub fn document_page(
    title: &str,
    content: &str,
    url: &str,
    ancestors: &[(String, String)],
    site: &Site,
) -> String {
    let mut m = String::with_capacity(content.len() + 1024);
    m.push_str("<div class=\"breadcrumbs\">\n");
    let _ = write!(m, "\t<a href=\"{}/\">Home</a> &gt;\n", site.baseurl);
    for (u, t) in ancestors {
        let _ = write!(m, "\t<a href=\"{}{u}\">{}</a> &gt;\n", site.baseurl, esc(t));
    }
    let _ = write!(m, "\t<span class=\"active\">{}</span>\n</div>\n\n", esc(title));
    m.push_str("<article class=\"page\">\n");
    let _ = write!(
        m,
        "\t<header class=\"post-header\">\n\t\t<h2>{}</h2>\n\t\t<a href=\"{}{url}\" class=\"permalink\">permalink</a>\n\t</header>\n",
        esc(title),
        site.baseurl
    );
    let _ = write!(m, "\t<section>\n{content}\n\t</section>\n");
    m.push_str("</article>\n");
    m
}

/// N rows as bare titled links. The smallest listing kind: no dates, no tags,
/// no bodies — just the relation (§5a).
///
/// Unlike `listing`, this one emits no header or margin, because it is embedded
/// in a host page that already provides them (`/`'s "Latest Posts" block).
pub fn link_list(rows: &[&Post], site: &Site) -> String {
    let mut s = String::new();
    for p in rows {
        let _ = write!(
            s,
            "<p>{} <a href=\"{}{}\">(read)</a></p>\n",
            esc(&p.title),
            site.baseurl,
            p.url
        );
    }
    s
}

/// The row's content *is* `main` — it builds its own structure (§5a).
pub fn raw(content: &str) -> String {
    content.to_string()
}

pub fn body_class(kind: RouteKind, multipost: bool) -> String {
    let _ = kind;
    if multipost {
        " class=\"multipost\"".into()
    } else {
        String::new()
    }
}

// --------------------------------------------------------- feed serialization

/// A date as Atom/sitemap `date_to_xmlschema`: `2026-06-25T00:00:00+00:00`.
pub fn xmlschema(d: chrono::NaiveDate) -> String {
    format!("{}T00:00:00+00:00", d.format("%Y-%m-%d"))
}

/// `expand_urls: site.url` (expand_urls.rb): make root-relative `href`/`src`
/// absolute. Protocol-relative `//host` is left alone (the `[^/>]` guard).
fn expand_urls(html: &str, url: &str) -> String {
    let re = regex::Regex::new(r#"(\s+(?:href|src)\s*=\s*["'])(/[^/>][^"'>]*)"#).unwrap();
    re.replace_all(html, |c: &regex::Captures| format!("{}{}{}", &c[1], url, &c[2]))
        .into_owned()
}

/// `feed_images` (feed_images.rb): float images get `align`/`width` so feed
/// readers, which ignore our CSS, still flow text around them. The plugin
/// selects `a.floatright > img`; `{% image right %}` is the only thing that
/// emits that shape (see tags::image), so a targeted rewrite matches it.
fn feed_images(html: &str) -> String {
    let inject = |html: String, class: &str, align: &str| -> String {
        let re = regex::Regex::new(&format!(
            r#"(<a class='image {class}'[^>]*><img [^>]*?)>"#
        ))
        .unwrap();
        re.replace_all(&html, format!(r#"$1 align="{align}" width="200">"#).as_str())
            .into_owned()
    };
    let s = inject(html.to_string(), "floatright", "right");
    inject(s, "floatleft", "left")
}

/// Escape a `]]>` that would otherwise close the surrounding CDATA section.
fn cdata_escape(s: &str) -> String {
    s.replace("]]>", "]]]]><![CDATA[>")
}

/// The Atom feed (atom.xml). `updated` is the build timestamp already in
/// xmlschema form; entries are `(post, rendered_body)`, newest first.
pub fn feed(site: &Site, updated: &str, entries: &[(&Post, &str)]) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    s.push_str("\t<feed xmlns=\"http://www.w3.org/2005/Atom\">\n");
    let _ = write!(s, "\t<title><![CDATA[{}]]></title>\n", cdata_escape(site.title));
    let _ = write!(s, "\t<link href=\"{u}/atom.xml\" rel=\"self\"/>\n", u = site.url);
    let _ = write!(s, "\t<link href=\"{u}/\"/>\n", u = site.url);
    let _ = write!(s, "\t<icon>{u}/resource/favicon/favicon-160x160.png</icon>\n", u = site.url);
    let _ = write!(s, "\t<logo>{u}/resource/favicon/favicon-160x160.png</logo>\n", u = site.url);
    let _ = write!(s, "\t<updated>{updated}</updated>\n");
    let _ = write!(s, "\t<id>{u}/</id>\n", u = site.url);
    s.push_str("\t<author>\n");
    let _ = write!(s, "\t\t<name><![CDATA[{}]]></name>\n", site.author);
    if let Some(email) = site.email {
        let _ = write!(s, "\t\t<email><![CDATA[{email}]]></email>\n");
    }
    s.push_str("\t</author>\n");
    s.push_str("\t<generator uri=\"http://jekyllrb.com/\">Jekyll</generator>\n");
    for (p, body) in entries {
        let content = cdata_escape(&feed_images(&expand_urls(body, site.url)));
        let updated = p.date.map(xmlschema).unwrap_or_default();
        s.push_str("\t<entry>\n");
        let _ = write!(
            s,
            "\t\t<title type=\"html\"><![CDATA[{}]]></title>\n",
            cdata_escape(&p.title)
        );
        let _ = write!(s, "\t\t<link href=\"{}{}\"/>\n", site.url, p.url);
        let _ = write!(s, "\t\t<updated>{updated}</updated>\n");
        let _ = write!(s, "\t\t<id>{}{}</id>\n", site.url, p.url.trim_end_matches('/'));
        let _ = write!(s, "\t\t<content type=\"html\"><![CDATA[{content}]]></content>\n");
        s.push_str("\t</entry>\n");
    }
    s.push_str("</feed>\n");
    s
}

#[cfg(test)]
mod feed_tests {
    use super::*;

    #[test]
    fn expand_urls_makes_root_relative_absolute() {
        let out = expand_urls(r#"<a href="/blog/x/"><img src="/a.png"></a>"#, "https://grack.com");
        assert_eq!(out, r#"<a href="https://grack.com/blog/x/"><img src="https://grack.com/a.png"></a>"#);
    }

    #[test]
    fn expand_urls_leaves_absolute_and_protocol_relative_alone() {
        // Already absolute, protocol-relative, and fragment refs must not move.
        let src = r##"<a href="https://x.com/y"><img src="//cdn/z.png"><a href="#top">"##;
        assert_eq!(expand_urls(src, "https://grack.com"), src);
    }

    #[test]
    fn feed_images_injects_align_and_width() {
        let src = "<a class='image floatright' href='https://grack.com/a.jpg'><img src='https://grack.com/a.jpg' alt=''></a>";
        let out = feed_images(src);
        assert!(out.contains(r#"alt='' align="right" width="200">"#), "{out}");
    }

    #[test]
    fn feed_images_ignores_non_float_images() {
        let src = "<a class='image standard' href='x'><img src='x' alt=''></a>";
        assert_eq!(feed_images(src), src);
    }

    #[test]
    fn cdata_escape_splits_terminator() {
        assert_eq!(cdata_escape("a]]>b"), "a]]]]><![CDATA[>b");
    }

    #[test]
    fn xmlschema_is_utc_midnight() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 6, 25).unwrap();
        assert_eq!(xmlschema(d), "2026-06-25T00:00:00+00:00");
    }
}

/// The XML sitemap (sitemap.xml). Entries are `(absolute loc, optional
/// lastmod)`, already in the order they should appear.
pub fn sitemap(entries: &[(String, Option<String>)]) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<urlset xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://www.sitemaps.org/schemas/sitemap/0.9 http://www.sitemaps.org/schemas/sitemap/0.9/sitemap.xsd\" xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    for (loc, lastmod) in entries {
        s.push_str("<url>\n");
        let _ = write!(s, "<loc>{loc}</loc>\n");
        if let Some(lm) = lastmod {
            let _ = write!(s, "<lastmod>{lm}</lastmod>\n");
        }
        s.push_str("</url>\n");
    }
    s.push_str("</urlset>\n");
    s
}

#[cfg(test)]
mod pagination_tests {
    use super::*;

    fn site() -> Site<'static> {
        Site { url: "https://x", baseurl: "", title: "t", author: "a", email: None, css: "/c" }
    }

    #[test]
    fn single_page_has_no_nav() {
        assert_eq!(pagination(1, 1, &site()), "");
    }

    #[test]
    fn first_page_disables_prev_and_links_page_two() {
        let s = pagination(1, 3, &site());
        assert!(s.contains(r#"<span class="pagination__prev is-disabled">"#), "{s}");
        assert!(s.contains(r#"aria-current="page">1</span>"#), "{s}");
        assert!(s.contains(r#"href="/blog/page/2">Next"#), "{s}");
    }

    #[test]
    fn last_page_disables_next() {
        let s = pagination(3, 3, &site());
        assert!(s.contains(r#"class="pagination__prev" href="/blog/page/2">"#), "{s}");
        assert!(s.contains(r#"<span class="pagination__next is-disabled">"#), "{s}");
    }

    /// Page 1 is `/blog/`, never `/blog/page/1` — from both the "1" tile and prev.
    #[test]
    fn page_one_link_has_no_page_segment() {
        let s = pagination(2, 3, &site());
        assert!(s.contains(r#"class="pagination__num" href="/blog/">1</a>"#), "{s}");
        assert!(s.contains(r#"class="pagination__prev" href="/blog/">"#), "{s}");
        assert!(!s.contains("/blog/page/1"), "{s}");
    }
}
