//! Presentation: head facts + serializations.
//!
//!   schema  -> head facts (computed, never branched; §5a)
//!   layout  -> part maps (parts.rs, §5e)
//!   theme   -> fragments + css (themes/<name>/, theme.rs, §5e)
//!   feed/sitemap -> serializations, no look (below)
//!
//! What remains here is what has no theme: the computed `<head>` facts, the
//! `light` shell (the null theme's minimal wrapper, §5e step 4), and the XML
//! serializations.

use crate::db::Post;
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
    pub title: &'a str,
    pub author: &'a str,
    pub email: Option<&'a str>,
    /// From the profile (§4a): a projection in its own URL space asks
    /// search engines away, site-wide, without touching a row.
    pub noindex: bool,
}

pub fn head_for_post(p: &Post, site: &Site) -> Head {
    let canonical = format!("{}{}", site.url, p.url);
    let published = p.date.map(xmlschema);
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
        noindex: p.noindex || site.noindex,
        og_type: if p.date.is_some() { "article" } else { "website" },
        published,
        author: site.author.to_string(),
        jsonld,
    }
}

pub fn head_simple(title: &str, url: &str, site: &Site, noindex: bool) -> Head {
    let noindex = noindex || site.noindex;
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
}

/// §5g: the engine-owned ROOT HTML SHELL every theme inherits — doctype,
/// `<html>` stamped with the shell kind and any subtheme tokens, `<head>`
/// from the computed facts, `<body>` from the theme's body chrome. Themes
/// never write the skeleton, and a fragmentless (null) theme still
/// produces a valid document.
pub fn root_shell(
    head: &str,
    subtheme: Option<&str>,
    profile: Option<&str>,
    body: &str,
) -> String {
    let sub = subtheme
        .map(|s| format!(" data-subtheme=\"{}\"", esc(s)))
        .unwrap_or_default();
    // §4a: the profile is stamped, not rendered. A dev banner is then a
    // theme CSS rule on `[data-profile="dev"]` — themes opt in, the engine
    // ships no chrome, and a theme that ignores it is unaffected. Same shape
    // as the subtheme token beside it.
    let prof = profile
        .map(|p| format!(" data-profile=\"{}\"", esc(p)))
        .unwrap_or_default();
    format!(
        "<!doctype html>\n<html lang=\"en\" data-kind=\"shell\"{sub}{prof}>\n<head>{head}</head>\n<body>\n{}\n</body>\n</html>\n",
        body.trim_end()
    )
}

/// The `light` head (§5e step 4): title and robots, nothing else — the
/// minimal facts subset, wrapped by the same root shell as everything.
pub fn light_head(head: &Head) -> String {
    let robots = if head.noindex {
        "\n\t<meta name=\"robots\" content=\"noindex,follow\">"
    } else {
        ""
    };
    format!(
        "\n\t<title>{}</title>{robots}\n\t<meta charset=\"utf-8\">\n",
        esc(&head.title)
    )
}

const FAVICONS: &str = r#"
	<link rel="apple-touch-icon" sizes="180x180" href="/resource/favicon/apple-touch-icon-180x180.png">
	<link rel="icon" type="image/png" href="/resource/favicon/favicon-192x192.png">
	<meta name="apple-mobile-web-app-title" content="grack.com">
	<meta name="application-name" content="grack.com">"#;

/// The computed head facts as the shell's `head` part (§5a: a theme renders
/// the subset it wants; today's default takes them all). This fills
/// `<head data-slot="head">` in the theme's shell fragment. `css` is the
/// rendering theme's stylesheet URL — themes are per row (§5a), so the
/// link is too.
pub fn head_html(head: &Head, css: &str) -> String {
    let mut h = String::with_capacity(2048);
    let _ = write!(h, "\n\t<title>{}</title>\n", esc(&head.title));
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
    let _ = write!(h, "\t<link href='{css}' rel='stylesheet' type='text/css'>\n");
    h.push_str(FAVICONS);
    if head.noindex {
        h.push_str("\n\t<meta name=\"robots\" content=\"noindex,follow\">");
    }
    h.push('\n');
    h
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
/// `self_path` is the feed route's own URL (§6f: locale-parallel feeds —
/// /atom.xml and /fr/atom.xml — each claim their own self link).
pub fn feed(site: &Site, self_path: &str, updated: &str, entries: &[(&Post, &str)]) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    s.push_str("\t<feed xmlns=\"http://www.w3.org/2005/Atom\">\n");
    let _ = write!(s, "\t<title><![CDATA[{}]]></title>\n", cdata_escape(site.title));
    let _ = write!(s, "\t<link href=\"{u}{self_path}\" rel=\"self\"/>\n", u = site.url);
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
