//! The two shapes an index over a row store comes in.
//!
//! Callers supply the keys; these supply the collision rule and the grouping.
//! Nothing here knows what a row is, so a key function that yields nothing for
//! a row simply leaves it out of that index — which is how a posts-only or
//! default-locale-only index says so.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

/// Two rows claimed one key in a unique index. Positions rather than rows, so
/// the caller can name them however its domain names things.
#[derive(Debug)]
pub struct Collision<K> {
    pub key: K,
    pub first: usize,
    pub second: usize,
}

/// A unique index: at most one row per key, and a second claim is an error.
pub fn unique<R, K: Eq + Hash + Clone>(
    rows: &[R],
    key: impl Fn(usize, &R) -> Option<K>,
) -> Result<HashMap<K, usize>, Collision<K>> {
    let mut out = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        let Some(k) = key(i, r) else { continue };
        if let Some(first) = out.insert(k.clone(), i) {
            return Err(Collision {
                key: k,
                first,
                second: i,
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
pub fn multi<R, K: Ord, I: IntoIterator<Item = K>>(
    rows: &[R],
    keys: impl Fn(usize, &R) -> I,
) -> BTreeMap<K, Vec<usize>> {
    let mut out: BTreeMap<K, Vec<usize>> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        for k in keys(i, r) {
            out.entry(k).or_default().push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_reports_both_claimants() {
        let rows = ["a", "b", "a"];
        let e = unique(&rows, |_, r| Some(*r)).unwrap_err();
        assert_eq!((e.key, e.first, e.second), ("a", 0, 2));
    }

    #[test]
    fn a_keyless_row_joins_no_index() {
        let rows = [Some(1), None, Some(1)];
        assert!(unique(&rows, |_, r| *r).is_err());
        let m = multi(&rows, |_, r| *r);
        assert_eq!(m[&1], vec![0, 2], "the None row is absent, not grouped");
    }

    /// Positions are into the whole store, so an index over a subset still
    /// indexes by the row's real position.
    #[test]
    fn multi_keys_by_position_not_by_arrival() {
        let rows = ["skip", "x y", "skip", "y"];
        let m = multi(&rows, |i, r| {
            if i % 2 == 1 {
                r.split(' ').collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        });
        assert_eq!(m["y"], vec![1, 3]);
        assert_eq!(m["x"], vec![1]);
    }
}
