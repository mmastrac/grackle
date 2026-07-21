//! A stable identity for a row.
//!
//! Positions are what the engine indexes with today, and positions are only
//! meaningful inside one load: sort the table and every one of them is wrong.
//! That has already cost two bugs — the star views counted against a route
//! list that was still growing, and `insert_rows` decides `RouteKind` with
//! `i < n_posts`, which is correct only while posts happen to be laid down
//! first.
//!
//! A key is what a row IS rather than where it sits. It is cheap to clone
//! (one `Arc` bump), cheap to compare, and — the point — the same value after
//! a rebuild, so a set resolved against one load still names the same rows in
//! the next one.

use std::fmt;
use std::sync::Arc;

/// What a row is identified by: a short, stable, caller-chosen string.
///
/// The caller picks the vocabulary — a row's source path, a route's URL —
/// and only has to guarantee it is unique within its table and stable across
/// loads. `Arc<str>` rather than `String` because keys are cloned into every
/// index that mentions the row, and rather than `Arc<String>` because that
/// would be two pointer hops to reach the bytes.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Key(Arc<str>);

impl Key {
    pub fn new(s: impl AsRef<str>) -> Key {
        Key(Arc::from(s.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Compared and hashed BY VALUE, not by pointer. Interning would make both
/// O(1), and is the obvious optimisation if these ever get hot — but it would
/// also mean a key from a previous load only matched if the intern table
/// outlived it, which is the one property this type exists to have.
impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &*self.0)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Key {
    fn from(s: &str) -> Key {
        Key::new(s)
    }
}

impl From<String> for Key {
    fn from(s: String) -> Key {
        Key::new(s)
    }
}

impl serde::Serialize for Key {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

/// A row that knows its own identity.
pub trait Keyed {
    fn key(&self) -> &Key;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The property the type exists for: a key built in one load equals one
    /// built in the next, so an index survives a rebuild.
    #[test]
    fn equal_by_value_across_separate_constructions() {
        let first = Key::new("_posts/2026-07-17-espresso.md");
        let rebuilt = Key::new("_posts/2026-07-17-espresso.md");
        assert_eq!(first, rebuilt);

        let mut ix = HashMap::new();
        ix.insert(first, 12usize);
        assert_eq!(ix.get(&rebuilt), Some(&12), "a rebuilt key finds its row");
    }

    #[test]
    fn cloning_shares_rather_than_copies() {
        let k = Key::new("recipes/carbonara.md");
        let c = k.clone();
        assert_eq!(k, c);
        assert!(
            Arc::ptr_eq(&k.0, &c.0),
            "a clone should be a refcount bump, not an allocation"
        );
    }

    #[test]
    fn distinct_paths_are_distinct_keys() {
        assert_ne!(Key::new("a/index.md"), Key::new("b/index.md"));
        // §6f: a translation is its own file and its own row.
        assert_ne!(Key::new("index.md"), Key::new("index.fr.md"));
    }

    #[test]
    fn ordering_is_the_strings() {
        let mut ks = [Key::new("b"), Key::new("a"), Key::new("c")];
        ks.sort();
        assert_eq!(ks.map(|k| k.as_str().to_string()), ["a", "b", "c"]);
    }
}
