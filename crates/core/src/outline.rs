//! Section trees: a `.section` marker roots a subtree
//! unit, and every rendered row beneath it carries the section's page tree
//! with itself marked current, the downward walk that complements the
//! breadcrumbs' upward one.
//!
//! Labels come from page titles; an index-less directory is an unlinked
//! label. Ordering is declared (`order:`
//! front matter), else lexical by label, the -audit rule that
//! lexical-by-luck is not a contract, made explicit per node.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::model::SiteDb;
use crate::parts::{Part, PartMap};
use grackle_model::Heading;

pub use grackle_model::OutlineNode as Node;

/// The nearest section root governing `rel`, if any, the same
/// nearest-wins ascent as markers, buckets and slot fills.
pub fn nearest<'a>(sections: &'a [PathBuf], rel: &Path) -> Option<&'a Path> {
    sections
        .iter()
        .filter(|s| rel.starts_with(s))
        .max_by_key(|s| s.components().count())
        .map(PathBuf::as_path)
}

/// A building directory node: named children plus its own identity, which
/// arrives when (if) its index page is seen.
#[derive(Default)]
struct Dir {
    url: Option<String>,
    title: Option<String>,
    order: Option<i64>,
    dirs: BTreeMap<String, Dir>,
    pages: Vec<Node>,
}

impl Dir {
    fn at(&mut self, comps: &[String]) -> &mut Dir {
        let mut cur = self;
        for c in comps {
            cur = cur.dirs.entry(c.clone()).or_default();
        }
        cur
    }

    fn into_nodes(self) -> Vec<Node> {
        let mut out: Vec<Node> = self
            .dirs
            .into_iter()
            .map(|(name, d)| {
                let label = d.title.clone().unwrap_or_else(|| name.clone());
                let (url, order) = (d.url.clone(), d.order);
                Node {
                    label,
                    url,
                    order,
                    children: d.into_nodes(),
                }
            })
            .collect();
        out.extend(self.pages);
        out.sort_by(|a, b| {
            (a.order.unwrap_or(i64::MAX), a.label.to_lowercase())
                .cmp(&(b.order.unwrap_or(i64::MAX), b.label.to_lowercase()))
        });
        out
    }
}

/// The section tree rooted at `root` (a root-relative directory): the
/// root's own index first, then the nested pages and directories beneath.
/// Derived once per section per build; `current` moves per page in
/// `to_parts`.
pub fn section_tree(db: &SiteDb, root: &Path, pairing_keep: Option<(&str, &str)>) -> Vec<Node> {
    let mut top = Dir::default();
    let mut root_index: Option<Node> = None;

    // A section tree keeps the pairing-axis canonical like every other
    // derived set, a twin shares its original's logical rel and would
    // otherwise collide with it node-for-node.
    for p in db.rows.iter().filter(|p| {
        p.rendered
            && match pairing_keep {
                Some((field, canon)) => p.string(field) == Some(canon),
                None => true,
            }
    }) {
        let Ok(rest) = p.rel.strip_prefix(root) else {
            continue;
        };
        let comps: Vec<String> = rest
            .iter()
            .map(|c| c.to_string_lossy().to_string())
            .collect();
        let Some((file, dirs)) = comps.split_last() else {
            continue;
        };
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        let label = p.title.clone().unwrap_or_else(|| stem.to_string());

        if stem == "index" {
            if dirs.is_empty() {
                root_index = Some(Node {
                    label,
                    url: Some(p.url.clone()),
                    order: p.order,
                    children: Vec::new(),
                });
            } else {
                // The index page IS its directory's identity.
                let d = top.at(dirs);
                d.url = Some(p.url.clone());
                d.title = Some(label);
                d.order = p.order;
            }
        } else {
            top.at(dirs).pages.push(Node {
                label,
                url: Some(p.url.clone()),
                order: p.order,
                children: Vec::new(),
            });
        }
    }

    let mut out = Vec::new();
    out.extend(root_index); // the section home leads, outside the sort
    out.extend(top.into_nodes());
    out
}

/// heading axis: the same recursive shape, from the document's own
/// headings. Nesting is by level, and a jump (h2 -> h4) nests under the
/// nearest shallower entry. The depth window is production policy,
/// lesson stands: never ship levels a stylesheet hides.
pub fn heading_tree(hs: &[Heading], min: u8, max: u8) -> Vec<Node> {
    let db_hs: Vec<grackle_db::Heading> = hs
        .iter()
        .map(|h| grackle_db::Heading {
            level: h.level,
            id: h.id.clone(),
            text: h.text.clone(),
        })
        .collect();
    grackle_db::filter::heading_tree(&db_hs, min, max)
        .into_iter()
        .map(db_to_node)
        .collect()
}

fn db_to_node(n: grackle_db::OutlineNode) -> Node {
    Node {
        label: n.label,
        url: n.url,
        order: None,
        children: n.children.into_iter().map(db_to_node).collect(),
    }
}

/// The tree as `outline_entry` part maps, with the row's own entry marked
/// `current`, the literal `aria-current` value, the pagination trick.
pub fn to_parts(nodes: &[Node], current_url: &str) -> Vec<PartMap> {
    nodes
        .iter()
        .map(|n| {
            let mut m = PartMap::new("outline_entry");
            m.set("label", Part::Text(n.label.clone()));
            if let Some(u) = &n.url {
                m.set("url", Part::Text(u.clone()));
            }
            if n.url.as_deref() == Some(current_url) {
                m.set("current", Part::Text("page".into()));
            }
            let kids = to_parts(&n.children, current_url);
            if !kids.is_empty() {
                m.set("children", Part::Stream(kids));
            }
            m
        })
        .collect()
}

#[cfg(test)]
// Section-tree nesting moved to a fixture test: seven `Row`
// literals became seven files, which is what the feature is about. See
// `crates/core/tests/fixtures/section-tree`.
mod tests {
    use super::*;

    #[test]
    fn heading_tree_nests_by_level_and_tolerates_jumps() {
        let hs: Vec<Heading> = [
            (2, "a", "A"),
            (3, "a1", "A1"),
            (3, "a2", "A2"),
            (2, "b", "B"),
        ]
        .iter()
        .map(|(l, id, t)| Heading {
            level: *l,
            id: id.to_string(),
            text: t.to_string(),
        })
        .collect();
        let t = heading_tree(&hs, 2, 3);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].children.len(), 2);
        assert_eq!(t[0].children[1].url.as_deref(), Some("#a2"));
        assert!(t[1].children.is_empty());

        // A jump (h2 -> h4) nests under the nearest shallower entry, and the
        // depth window drops what production policy says to drop.
        let jump: Vec<Heading> = [(2, "x", "X"), (4, "deep", "Deep"), (3, "y", "Y")]
            .iter()
            .map(|(l, id, t)| Heading {
                level: *l,
                id: id.to_string(),
                text: t.to_string(),
            })
            .collect();
        let t = heading_tree(&jump, 2, 4);
        assert_eq!(t[0].children.len(), 2, "Deep and Y both under X");
        let t = heading_tree(&jump, 2, 3);
        assert_eq!(t[0].children.len(), 1, "depth window drops h4");
    }

    #[test]
    fn nearest_wins_on_nested_sections() {
        let sections = vec![PathBuf::from("manual"), PathBuf::from("manual/advanced")];
        assert_eq!(
            nearest(&sections, Path::new("manual/advanced/x.md")).unwrap(),
            Path::new("manual/advanced")
        );
        assert_eq!(
            nearest(&sections, Path::new("manual/install.md")).unwrap(),
            Path::new("manual")
        );
        assert!(nearest(&sections, Path::new("recipes/x.md")).is_none());
    }
}
