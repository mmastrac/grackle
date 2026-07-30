//! The `atom` fold shell: Atom XML serialization of a view's member rows.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::config::Config;
use crate::markdown::Doc;
use crate::model::{Row, SiteDb};
use crate::pipeline::bodies;
use crate::pipeline::types::{PageBody, SiteOutput, Stats};
use crate::render::{self, Site};

/// `expand_urls: site.url` (expand_urls.rb): make root-relative `href`/`src`
/// absolute. Protocol-relative `//host` is left alone (the `[^/>]` guard).
fn expand_urls(html: &str, url: &str) -> String {
    let re = regex::Regex::new(r#"(\s+(?:href|src)\s*=\s*["'])(/[^/>][^"'>]*)"#).unwrap();
    re.replace_all(html, |c: &regex::Captures| {
        format!("{}{}{}", &c[1], url, &c[2])
    })
    .into_owned()
}

/// `feed_images` (feed_images.rb): float images get `align`/`width` so feed
/// readers, which ignore our CSS, still flow text around them. The plugin
/// selects `a.floatright > img`; `{% image right %}` is the only thing that
/// emits that shape (see tags::image), so a targeted rewrite matches it.
fn feed_images(html: &str) -> String {
    // q26 puts width/height on every body image. The feed wants its own fixed
    // width for floats, and two `width` attributes is invalid markup in which
    // the FIRST one wins — so the page's 640 would silently beat the feed's
    // 200. Strip ours from the matched tag before injecting the feed's.
    let dims = regex::Regex::new(r#"\s*(?:width|height)='[^']*'"#).unwrap();
    let inject = |html: String, class: &str, align: &str| -> String {
        let re = regex::Regex::new(&format!(
            r#"(<a class='image {class}'[^>]*><img )([^>]*?)>"#
        ))
        .unwrap();
        re.replace_all(&html, |c: &regex::Captures| {
            format!(
                r#"{}{} align="{align}" width="200">"#,
                &c[1],
                dims.replace_all(&c[2], "").trim_end()
            )
        })
        .into_owned()
    };
    let s = inject(html.to_string(), "floatright", "right");
    inject(s, "floatleft", "left")
}

/// Escape a `]]>` that would otherwise close the surrounding CDATA section.
fn cdata_escape(s: &str) -> String {
    s.replace("]]>", "]]]]><![CDATA[>")
}

/// The Atom feed. `updated` is the build timestamp already in xmlschema form;
/// entries are `(post, rendered_body)`, newest first. `self_path` is the feed
/// route's own URL (§6f: locale-parallel feeds — /atom.xml and /fr/atom.xml —
/// each claim their own self link).
pub fn xml(site: &Site, self_path: &str, updated: &str, entries: &[(&Row, &str)]) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    s.push_str("\t<feed xmlns=\"http://www.w3.org/2005/Atom\">\n");
    let _ = writeln!(
        s,
        "\t<title><![CDATA[{}]]></title>",
        cdata_escape(site.title)
    );
    let _ = writeln!(
        s,
        "\t<link href=\"{u}{self_path}\" rel=\"self\"/>",
        u = site.url
    );
    let _ = writeln!(s, "\t<link href=\"{u}/\"/>", u = site.url);
    // §4d: this used to name grack.com's own favicon path, in every site's
    // feed — `examples/minimal` shipped a link to a file it does not have.
    // Both elements take the site icon absolutely, and a site without one
    // emits neither: the same "empty means absent" the head runs on.
    if !site.icon.is_empty() {
        let _ = writeln!(s, "\t<icon>{u}{i}</icon>", u = site.url, i = site.icon);
        let _ = writeln!(s, "\t<logo>{u}{i}</logo>", u = site.url, i = site.icon);
    }
    let _ = writeln!(s, "\t<updated>{updated}</updated>");
    let _ = writeln!(s, "\t<id>{u}/</id>", u = site.url);
    s.push_str("\t<author>\n");
    let _ = writeln!(s, "\t\t<name><![CDATA[{}]]></name>", site.author);
    if let Some(email) = site.email {
        let _ = writeln!(s, "\t\t<email><![CDATA[{email}]]></email>");
    }
    s.push_str("\t</author>\n");
    s.push_str("\t<generator uri=\"http://jekyllrb.com/\">Jekyll</generator>\n");
    for (p, body) in entries {
        let content = cdata_escape(&feed_images(&expand_urls(body, site.url)));
        let updated = p.as_date("date").map(render::xmlschema).unwrap_or_default();
        s.push_str("\t<entry>\n");
        let _ = writeln!(
            s,
            "\t\t<title type=\"html\"><![CDATA[{}]]></title>",
            cdata_escape(p.title.as_deref().unwrap_or_default())
        );
        let _ = writeln!(s, "\t\t<link href=\"{}{}\"/>", site.url, p.url);
        let _ = writeln!(s, "\t\t<updated>{updated}</updated>");
        let _ = writeln!(
            s,
            "\t\t<id>{}{}</id>",
            site.url,
            p.url.trim_end_matches('/')
        );
        let _ = writeln!(
            s,
            "\t\t<content type=\"html\"><![CDATA[{content}]]></content>"
        );
        s.push_str("\t</entry>\n");
    }
    s.push_str("</feed>\n");
    s
}

/// Emit every `shell = "atom"` fold into `out_map`.
///
/// A serialization, not a themed page — it bypasses themes entirely (§5e:
/// "feed bypasses themes; serializations have no look"). The route already
/// carries its members (newest-first); we render each body and apply the
/// feed's content transforms.
pub(crate) fn emit(
    cfg: &Config,
    db: &SiteDb,
    site: &Site<'_>,
    bodies: &HashMap<&grackle_db::Key, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) {
    let updated = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S+00:00")
        .to_string();
    for r in &db.routes {
        let Some(view) = &r.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        // Declared, not inferred from a template filename (q44).
        if v.shell.as_deref() != Some("atom") {
            continue;
        }
        // `members` indexes one row store whatever the view's base table, so
        // the kind does not enter into it. Finding the row's HTML does: posts
        // hold their body, tree rows are re-read, and the two maps below
        // answer that. A dated tree collection can have a feed.
        let entries: Vec<(&Row, &str)> = r
            .members
            .iter()
            .filter_map(|k| db.rows.get(k))
            .map(|p| {
                (
                    p,
                    bodies::row_body_html(p, bodies, page_bodies).unwrap_or(""),
                )
            })
            .collect();
        let body = xml(site, &r.url, &updated, &entries);
        out_map.insert(r.url.clone(), body.into_bytes());
        stats.serialized += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_urls_makes_root_relative_absolute() {
        let out = expand_urls(
            r#"<a href="/blog/x/"><img src="/a.png"></a>"#,
            "https://grack.com",
        );
        assert_eq!(
            out,
            r#"<a href="https://grack.com/blog/x/"><img src="https://grack.com/a.png"></a>"#
        );
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
        assert!(
            out.contains(r#"alt='' align="right" width="200">"#),
            "{out}"
        );
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
}
