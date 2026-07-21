//! §6d stage B: the raw-HTML seam.
//!
//! Most of what §6d originally wanted from a rewrite stage now happens at the
//! comrak node, before HTML exists — code-block shapes, link resolution and
//! (since q26) image dimensions all mutate the AST, so they cost no re-parse
//! and no selector matching. §9a records that shrinkage.
//!
//! What is left is the thing an AST pass structurally cannot reach: rows whose
//! source *is* HTML. A `.html` page body and a `.html` slot fill never meet
//! comrak, so `markdown::render_doc_with`'s resolver never sees their links —
//! `build.rs` and `slots.rs` both carried a comment naming this seam. That is
//! what this module closes, and only that.
//!
//! This is deliberately **not** the `.rewrite.toml` rule table §6d sketches.
//! No selector language, no wrap/template actions, no config surface: neither
//! site wants an authored rule today, and §6d's own risk note calls a
//! template-injecting selector table "unbounded rope" that would need the
//! filter language's load-time validation to be safe. The mechanism arrives
//! with its second consumer, per §5b's incremental path. Today there is one.

use anyhow::Result;
use lol_html::{element, rewrite_str, RewriteStrSettings};

/// Resolve `<a href>` in raw HTML, exactly as the markdown path resolves
/// `NodeValue::Link` (§6a).
///
/// `resolve` is the same closure the comrak pass takes: `Ok(Some(url))`
/// rewrites, `Ok(None)` leaves the href alone, `Err` fails the build naming
/// the file. Matching that contract is the point — a link in a `.html` row
/// should resolve, and fail, identically to the same link in a `.md` row.
///
/// **`a[href]` only, on purpose.** The AST pass resolves link nodes and not
/// image nodes, so widening this to `img[src]` would give raw-HTML rows a
/// capability markdown rows do not have. Bare-name asset resolution (§6a) is
/// a separate question and wants to land on both paths at once.
pub fn resolve_links(
    html: &str,
    resolve: &dyn Fn(&str) -> Result<Option<String>>,
) -> Result<String> {
    // lol_html handlers return a boxed error that loses its type crossing the
    // rewriter, so the real one is parked here and re-raised after.
    let failure: std::cell::RefCell<Option<anyhow::Error>> = std::cell::RefCell::new(None);

    let settings = RewriteStrSettings::new().append_element_content_handler(element!(
        "a[href]",
        |el| {
            let Some(href) = el.get_attribute("href") else {
                return Ok(());
            };
            match resolve(&href) {
                Ok(Some(url)) => el.set_attribute("href", &url)?,
                Ok(None) => {}
                Err(e) => {
                    // First failure wins; the rewriter runs to completion but
                    // the caller gets this one.
                    failure.borrow_mut().get_or_insert(e);
                }
            }
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
        let out = resolve_links(html, &|h| {
            Ok((h == "foo.md").then(|| "/blog/foo/".to_string()))
        })
        .unwrap();
        assert!(out.contains(r#"href="/blog/foo/""#), "{out}");
        assert!(out.contains(r#"href="https://e.com/""#), "{out}");
    }

    #[test]
    fn a_resolver_error_fails_the_build() {
        let html = r#"<a href="gone.md">x</a>"#;
        let e = resolve_links(html, &|_| anyhow::bail!("dangling")).unwrap_err();
        assert!(e.to_string().contains("dangling"), "{e}");
    }

    /// The seam exists for rows comrak never sees, so a fragment that is not a
    /// whole document must survive intact.
    #[test]
    fn leaves_a_bare_fragment_otherwise_untouched() {
        let html = r#"<div class="x"><img src='/static/a.png' alt=''><br></div>"#;
        let out = resolve_links(html, &|_| Ok(None)).unwrap();
        assert_eq!(out, html);
    }
}
