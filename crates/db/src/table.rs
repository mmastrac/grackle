//! A table: rows of one type, and everything you can do to them without
//! knowing what that type is.
//!
//! A row here is whatever answers `filter::Row` — a name goes in, a typed
//! value comes out. That is the whole contract, and it is what lets one
//! `matching` serve every row type a caller defines.
//!
//! Positions, not references, are the currency. Every index in this engine is
//! a position into its table, so a query result composes with an index result
//! without either knowing what the other selected on.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::ops::Deref;

use serde::Serialize;

use crate::filter::{Filter, Row};
use crate::index::{self, Collision};

/// Rows of one type, in load order.
///
/// Derefs to a slice, so reading a table reads like reading the `Vec` it
/// wraps. Writing does not: rows go in through `push`/`extend` and come out
/// through queries, which is what keeps an index from silently outliving the
/// rows it points at.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct Table<R>(Vec<R>);

impl<R> Table<R> {
    pub fn new(rows: Vec<R>) -> Table<R> {
        Table(rows)
    }

    pub fn push(&mut self, row: R) {
        self.0.push(row);
    }

    pub fn extend(&mut self, rows: impl IntoIterator<Item = R>) {
        self.0.extend(rows);
    }

    /// Mutable access by position, for the passes that revise a row after the
    /// table is built (§q45's claimed-row URL fixup).
    pub fn get_mut(&mut self, i: usize) -> Option<&mut R> {
        self.0.get_mut(i)
    }

    pub fn sort_by(&mut self, cmp: impl FnMut(&R, &R) -> std::cmp::Ordering) {
        self.0.sort_by(cmp);
    }

    /// A unique index over this table: at most one row per key, a second
    /// claim is a `Collision` naming both positions.
    pub fn unique_index<K: Eq + Hash + Clone>(
        &self,
        key: impl Fn(usize, &R) -> Option<K>,
    ) -> Result<HashMap<K, usize>, Collision<K>> {
        index::unique(&self.0, key)
    }

    /// A non-unique index: a row joins the list of every key it yields.
    pub fn multi_index<K: Ord, I: IntoIterator<Item = K>>(
        &self,
        keys: impl Fn(usize, &R) -> I,
    ) -> BTreeMap<K, Vec<usize>> {
        index::multi(&self.0, keys)
    }
}

impl<R: Row> Table<R> {
    /// The rows a filter admits, in table order.
    pub fn matching<'a>(&'a self, f: &'a Filter) -> impl Iterator<Item = &'a R> {
        self.0.iter().filter(move |r| f.eval(*r))
    }

    /// Positions a filter admits within `within` — a set narrowed by
    /// something the filter language cannot say (a glob scope, a locale), or
    /// one of the table's own index lists.
    ///
    /// Order follows `within`, not the table, so a caller that sorted its
    /// subset keeps that sort. Positions off the end are dropped rather than
    /// panicking: `within` may outlive the rows it was built from.
    pub fn select_within(&self, within: &[usize], f: &Filter) -> Vec<usize> {
        within
            .iter()
            .copied()
            .filter(|&i| self.0.get(i).is_some_and(|r| f.eval(r)))
            .collect()
    }
}

impl<R> Deref for Table<R> {
    type Target = [R];
    fn deref(&self) -> &[R] {
        &self.0
    }
}

impl<'a, R> IntoIterator for &'a Table<R> {
    type Item = &'a R;
    type IntoIter = std::slice::Iter<'a, R>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<R> Default for Table<R> {
    fn default() -> Table<R> {
        Table(Vec::new())
    }
}

impl<R> FromIterator<R> for Table<R> {
    fn from_iter<I: IntoIterator<Item = R>>(it: I) -> Table<R> {
        Table(it.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{Schema, Type, Value};

    struct Fixture(&'static str, i64);

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
        Table::new(vec![Fixture("a", 1), Fixture("b", 2), Fixture("c", 3)])
    }

    fn schema() -> Schema {
        let mut s = Schema::new();
        s.insert("name", Type::Str);
        s.insert("n", Type::Int);
        s
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
        assert_eq!(table().select_within(&[2, 1, 0], &f), vec![2, 1]);
    }

    #[test]
    fn select_within_ignores_positions_off_the_end() {
        let f = Filter::always();
        assert_eq!(table().select_within(&[0, 99], &f), vec![0]);
    }
}
