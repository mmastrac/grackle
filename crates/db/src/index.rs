//! The two shapes an index over a row store comes in.
//!
//! Callers supply the keys; these supply the collision rule and the grouping.
//! Nothing here knows what a row is, so a key function that yields nothing for
//! a row simply leaves it out of that index — which is how a posts-only or
//! default-locale-only index says so.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

use crate::key::{Key, Keyed};

/// Two rows claimed one key in a unique index. The claimants are named by
/// their own keys, so the caller can render them however its domain does.
#[derive(Debug)]
pub struct Collision<K> {
    pub key: K,
    pub first: Key,
    pub second: Key,
}

/// A unique index: at most one row per key, and a second claim is an error.
pub fn unique<R: Keyed, K: Eq + Hash + Clone>(
    rows: &[R],
    key: impl Fn(&R) -> Option<K>,
) -> Result<HashMap<K, Key>, Collision<K>> {
    let mut out = HashMap::new();
    for r in rows {
        let Some(k) = key(r) else { continue };
        if let Some(first) = out.insert(k.clone(), r.key().clone()) {
            return Err(Collision {
                key: k,
                first,
                second: r.key().clone(),
            });
        }
    }
    Ok(out)
}

/// A non-unique index: a row joins the list of every key it yields.
///
/// One function covers both arities because `Option<K>` and `Vec<K>` are both
/// `IntoIterator` — a single-keyed index returns `Some(k)`, a list-valued one
/// (tags) returns the list, and a row that belongs nowhere returns `None`.
/// Ordered, because several of these are walked in key order.
pub fn multi<R: Keyed, K: Ord, I: IntoIterator<Item = K>>(
    rows: &[R],
    keys: impl Fn(&R) -> I,
) -> BTreeMap<K, Vec<Key>> {
    let mut out: BTreeMap<K, Vec<Key>> = BTreeMap::new();
    for r in rows {
        for k in keys(r) {
            out.entry(k).or_default().push(r.key().clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row is a name and an optional group; its key is the name.
    struct Fixture(&'static str, Option<&'static str>, Key);

    impl Fixture {
        fn new(name: &'static str, group: Option<&'static str>) -> Fixture {
            Fixture(name, group, Key::new(name))
        }
    }

    impl Keyed for Fixture {
        fn key(&self) -> &Key {
            &self.2
        }
    }

    fn rows() -> Vec<Fixture> {
        vec![
            Fixture::new("a", Some("x")),
            Fixture::new("b", None),
            Fixture::new("c", Some("x")),
        ]
    }

    #[test]
    fn unique_reports_both_claimants_by_key() {
        let dupes = vec![Fixture::new("a", Some("g")), Fixture::new("b", Some("g"))];
        let e = unique(&dupes, |r| r.1).unwrap_err();
        assert_eq!(e.key, "g");
        assert_eq!(e.first, Key::new("a"));
        assert_eq!(e.second, Key::new("b"));
    }

    #[test]
    fn a_keyless_row_joins_no_index() {
        let rows = rows();
        let u = unique(&rows, |r| r.1.filter(|_| r.0 == "a")).unwrap();
        assert_eq!(u.get("x"), Some(&Key::new("a")));

        let m = multi(&rows, |r| r.1);
        assert_eq!(
            m[&"x"],
            vec![Key::new("a"), Key::new("c")],
            "the groupless row is absent, not grouped"
        );
    }

    /// Membership is by key, so an index outlives the order it was built in.
    #[test]
    fn multi_names_rows_not_positions() {
        let m = multi(&rows(), |r| r.1);
        assert_eq!(m.len(), 1);
        assert!(m[&"x"]
            .iter()
            .all(|k| k.as_str() == "a" || k.as_str() == "c"));
    }
}
