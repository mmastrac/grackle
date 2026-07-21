//! A table: rows of one type, and everything you can do to them without
//! knowing what that type is.
//!
//! A row here is whatever answers `filter::Row` — a name goes in, a typed
//! value comes out. That is the whole contract, and it is what lets one
//! `matching` serve every row type a caller defines.
//!
//! Keys, not positions, are the currency. Every index in this engine names
//! rows by `Key`, so a query result composes with an index result without
//! either knowing what the other selected on — and a set outlives the load it
//! was built against, which a position cannot.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::ops::Deref;

use serde::Serialize;

use crate::filter::{Filter, Row};
use crate::index::{self, Collision};
use crate::key::{Key, Keyed};
use crate::view::View;

/// Rows of one type, in load order.
///
/// Derefs to a slice, so reading a table reads like reading the `Vec` it
/// wraps. Writing does not: rows go in through `push`/`extend` and come out
/// through queries, which is what keeps an index from silently outliving the
/// rows it points at.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct Table<R> {
    rows: Vec<R>,
    /// Where each key currently sits. Rebuilt whenever the order changes, so
    /// a `Key` a caller held across a sort still resolves.
    #[serde(skip)]
    at: HashMap<Key, usize>,
}

impl<R: Keyed> Table<R> {
    pub fn new(rows: Vec<R>) -> Table<R> {
        let mut t = Table {
            rows,
            at: HashMap::new(),
        };
        t.reindex();
        t
    }

    pub fn push(&mut self, row: R) {
        self.at.insert(row.key().clone(), self.rows.len());
        self.rows.push(row);
    }

    pub fn extend(&mut self, rows: impl IntoIterator<Item = R>) {
        for r in rows {
            self.push(r);
        }
    }

    /// Reordering moves every row, so the position map is rebuilt. This is
    /// what makes a held `Key` survive a sort — the bug that bit the star
    /// views, which cached positions across `routes.sort_by`.
    pub fn sort_by(&mut self, cmp: impl FnMut(&R, &R) -> std::cmp::Ordering) {
        self.rows.sort_by(cmp);
        self.reindex();
    }

    fn reindex(&mut self) {
        self.at.clear();
        self.at.reserve(self.rows.len());
        for (i, r) in self.rows.iter().enumerate() {
            self.at.insert(r.key().clone(), i);
        }
    }

    /// The row a key names.
    pub fn get(&self, key: &Key) -> Option<&R> {
        self.at.get(key).map(|&i| &self.rows[i])
    }

    /// Mutable access, for the passes that revise a row after the table is
    /// built (§q45's claimed-row URL fixup).
    pub fn get_mut(&mut self, key: &Key) -> Option<&mut R> {
        let i = *self.at.get(key)?;
        self.rows.get_mut(i)
    }

    /// A unique index over this table: at most one row per key, a second
    /// claim is a `Collision` naming both.
    pub fn unique_index<K: Eq + Hash + Clone>(
        &self,
        key: impl Fn(&R) -> Option<K>,
    ) -> Result<HashMap<K, Key>, Collision<K>> {
        index::unique(&self.rows, key)
    }

    /// A non-unique index: a row joins the list of every key it yields.
    pub fn multi_index<K: Ord, I: IntoIterator<Item = K>>(
        &self,
        keys: impl Fn(&R) -> I,
    ) -> BTreeMap<K, Vec<Key>> {
        index::multi(&self.rows, keys)
    }
}

impl<R: Row + Keyed> Table<R> {
    /// The rows a filter admits, in table order.
    pub fn matching<'a>(&'a self, f: &'a Filter) -> impl Iterator<Item = &'a R> {
        self.rows.iter().filter(move |r| f.eval(*r))
    }

    /// The keys a filter admits, in table order — `matching` for callers that
    /// need to name the rows rather than read them.
    pub fn select(&self, f: &Filter) -> Vec<Key> {
        self.rows
            .iter()
            .filter(|r| f.eval(*r))
            .map(|r| r.key().clone())
            .collect()
    }

    /// The keys a filter admits within `within` — a set the caller narrowed
    /// by something the query language cannot say (a locale, a collection).
    ///
    /// Order follows `within`, not the table, so a caller that sorted its
    /// subset keeps that sort. A key naming no row is dropped rather than
    /// panicking: `within` may have been built against an earlier load, which
    /// is the whole reason these are keys.
    pub fn select_within(&self, within: &[Key], f: &Filter) -> Vec<Key> {
        within
            .iter()
            .filter(|k| self.get(k).is_some_and(|r| f.eval(r)))
            .cloned()
            .collect()
    }

    /// Resolve a view over the whole table.
    pub fn view(&self, v: &View) -> Vec<Key> {
        let all: Vec<Key> = self.rows.iter().map(|r| r.key().clone()).collect();
        self.view_within(&all, v)
    }

    /// Resolve a view within `within`.
    ///
    /// Sorting is stable, so a view with no `order_by` keeps `within`'s order
    /// rather than inventing one, and a partial sort leaves ties as they came.
    pub fn view_within(&self, within: &[Key], v: &View) -> Vec<Key> {
        let mut out = self.select_within(within, &v.filter);
        if !v.order_by.is_empty() {
            out.sort_by(|a, b| match (self.get(a), self.get(b)) {
                (Some(x), Some(y)) => v
                    .order_by
                    .iter()
                    .map(|o| x.field(&o.column).order(&y.field(&o.column), o.desc))
                    .find(|ord| ord.is_ne())
                    .unwrap_or(std::cmp::Ordering::Equal),
                _ => std::cmp::Ordering::Equal,
            });
        }
        if let Some(n) = v.limit {
            out.truncate(n);
        }
        out
    }
}

impl<R> Deref for Table<R> {
    type Target = [R];
    fn deref(&self) -> &[R] {
        &self.rows
    }
}

impl<'a, R> IntoIterator for &'a Table<R> {
    type Item = &'a R;
    type IntoIter = std::slice::Iter<'a, R>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}

impl<R> Default for Table<R> {
    fn default() -> Table<R> {
        Table {
            rows: Vec::new(),
            at: HashMap::new(),
        }
    }
}

impl<R: Keyed> FromIterator<R> for Table<R> {
    fn from_iter<I: IntoIterator<Item = R>>(it: I) -> Table<R> {
        Table::new(it.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{Schema, Type, Value};

    struct Fixture(&'static str, i64, Key);

    impl Fixture {
        fn new(name: &'static str, n: i64) -> Fixture {
            Fixture(name, n, Key::new(name))
        }
    }

    impl Keyed for Fixture {
        fn key(&self) -> &Key {
            &self.2
        }
    }

    impl Row for Fixture {
        fn field(&self, name: &str) -> Value {
            match name {
                "name" => Value::Str(self.0.to_string()),
                "n" => Value::Int(self.1),
                _ => Value::Null,
            }
        }
    }

    fn table() -> Table<Fixture> {
        Table::new(vec![Fixture::new("a", 1), Fixture::new("b", 2), Fixture::new("c", 3)])
    }

    fn schema() -> Schema {
        let mut s = Schema::new();
        s.insert("name", Type::Str);
        s.insert("n", Type::Int);
        s
    }

    fn names(ks: &[Key]) -> Vec<&str> {
        ks.iter().map(|k| k.as_str()).collect()
    }

    #[test]
    fn select_yields_keys_in_table_order() {
        let f = Filter::parse("n >= 2", &schema()).unwrap();
        assert_eq!(names(&table().select(&f)), ["b", "c"]);
    }

    /// The property positions did not have: reordering the table leaves every
    /// previously-selected key still naming its row.
    #[test]
    fn keys_survive_a_reorder() {
        let mut t = table();
        let picked = t.select(&Filter::always());
        t.sort_by(|a, b| b.1.cmp(&a.1));
        assert_eq!(names(&picked), ["a", "b", "c"], "the set is unchanged");
        for k in &picked {
            assert_eq!(t.get(k).map(|r| r.0), Some(k.as_str()));
        }
    }

    #[test]
    fn matching_yields_rows_in_table_order() {
        let f = Filter::parse("n >= 2", &schema()).unwrap();
        let got: Vec<i64> = table().matching(&f).map(|r| r.1).collect();
        assert_eq!(got, vec![2, 3]);
    }

    /// `select_within` keeps the caller's ordering, which is what lets a
    /// sorted set stay sorted through a second narrowing.
    #[test]
    fn select_within_follows_the_subset_order() {
        let f = Filter::parse("n >= 2", &schema()).unwrap();
        let within = [Key::new("c"), Key::new("b"), Key::new("a")];
        assert_eq!(names(&table().select_within(&within, &f)), ["c", "b"]);
    }

    /// A key naming no row is dropped, which is how a set built against an
    /// earlier load degrades rather than panicking.
    #[test]
    fn select_within_ignores_keys_that_name_nothing() {
        let within = [Key::new("a"), Key::new("gone")];
        assert_eq!(
            names(&table().select_within(&within, &Filter::always())),
            ["a"]
        );
    }
}
