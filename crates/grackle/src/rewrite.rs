//! §6d stage B: link resolution for rows whose source *is* HTML.
//!
//! A `.html` page body, a `.html` slot fill and a raw-HTML landing never meet
//! comrak, so the AST resolver never sees their links. This is the seam
//! `build.rs` and `slots.rs` point at.
//!
//! Narrow on purpose: no selector language and no config surface, because the
//! rule table §6d sketches has no consumer yet.

use anyhow::Result;
use lol_html::{element, rewrite_str, RewriteStrSettings};

/// Resolve `<a href>` and embed `src`s in raw HTML, exactly as the markdown
/// path resolves `NodeValue::Link` and `NodeValue::Image` (§6a, IO.md §4a).
///
/// `resolve` is the same closure the comrak pass takes, tagged with which
/// citation form is asking: `Ok(Some(url))` rewrites, `Ok(None)` leaves the
/// attribute alone, `Err` fails the build naming the file. Matching that
/// contract is the point — a citation in a `.html` row should resolve, and
/// fail, identically to the same citation in a `.md` row.
///
/// **`img`/`iframe`/`video`/`audio`/`source` are the embed set**, and it landed
/// with the markdown side rather than before it, which is what this comment
/// used to say was the condition: widening one path alone would give raw-HTML
/// rows a capability markdown rows do not have. Bare-name asset resolution
/// (§6a) remains parked and is not this.
pub fn resolve_links(
    html: &str,
    resolve: &dyn Fn(crate::links::Cite, &str) -> Result<Option<String>>,
) -> Result<String> {
    // lol_html handlers return a boxed error that loses its type crossing the
    // rewriter, so the real one is parked here and re-raised after.
    let failure: std::cell::RefCell<Option<anyhow::Error>> = std::cell::RefCell::new(None);
    let one = |el: &mut lol_html::html_content::Element, attr: &str, form: crate::links::Cite| {
        let Some(v) = el.get_attribute(attr) else {
            return;
        };
        match resolve(form, &v) {
            Ok(Some(url)) => {
                let _ = el.set_attribute(attr, &url);
            }
            Ok(None) => {}
            Err(e) => {
                // First failure wins; the rewriter runs to completion but
                // the caller gets this one.
                failure.borrow_mut().get_or_insert(e);
            }
        }
    };

    let settings = RewriteStrSettings::new()
        .append_element_content_handler(element!("a[href]", |el| {
            one(el, "href", crate::links::Cite::Link);
            Ok(())
        }))
        .append_element_content_handler(element!(
            "img[src], iframe[src], video[src], audio[src], source[src]",
            |el| {
                one(el, "src", crate::links::Cite::Embed);
                Ok(())
            }
        ));

    let out = rewrite_str(html, settings)?;

    if let Some(e) = failure.borrow_mut().take() {
        return Err(e);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_a_resolved_href_and_leaves_the_rest() {
        let html = r#"<p><a href="foo.md">x</a> <a href="https://e.com/">y</a></p>"#;
        let out = resolve_links(html, &|_, h| {
            Ok((h == "foo.md").then(|| "/blog/foo/".to_string()))
        })
        .unwrap();
        assert!(out.contains(r#"href="/blog/foo/""#), "{out}");
        assert!(out.contains(r#"href="https://e.com/""#), "{out}");
    }

    #[test]
    fn a_resolver_error_fails_the_build() {
        let html = r#"<a href="gone.md">x</a>"#;
        let e = resolve_links(html, &|_, _| anyhow::bail!("dangling")).unwrap_err();
        assert!(e.to_string().contains("dangling"), "{e}");
    }

    /// The seam exists for rows comrak never sees, so a fragment that is not a
    /// whole document must survive intact.
    #[test]
    fn leaves_a_bare_fragment_otherwise_untouched() {
        let html = r#"<div class="x"><img src='/static/a.png' alt=''><br></div>"#;
        let out = resolve_links(html, &|_, _| Ok(None)).unwrap();
        assert_eq!(out, html);
    }
}
