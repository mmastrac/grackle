//! A view: which rows, in what order, how many.
//!
//! The three things every selection in this engine wanted and each caller
//! spelled out for itself — narrow, sort, truncate. Naming them together is
//! most of the point; a view is a value you can build, pass around and
//! resolve, rather than a shape three functions happen to share.
//!
//! What is NOT here: where the rows came from, what the resolved positions
//! then become, and how a view partitions into groups. The first two are the
//! caller's, and grouping is still entangled with route templates on the
//! caller's side.

use crate::filter::Filter;

/// One level of a sort, in priority order.
#[derive(Debug, Clone)]
pub struct Order {
    pub column: String,
    /// Reverse the values. Nulls stay last either way — see `Value::order`.
    pub desc: bool,
}

impl Order {
    pub fn asc(column: impl Into<String>) -> Order {
        Order {
            column: column.into(),
            desc: false,
        }
    }

    pub fn desc(column: impl Into<String>) -> Order {
        Order {
            column: column.into(),
            desc: true,
        }
    }
}

/// A selection over a table.
///
/// `order_by` is a list because a single column rarely settles it: a tie on
/// the sort column has to break somewhere, and leaving that to the table's
/// load order makes output depend on directory iteration.
#[derive(Debug, Clone)]
pub struct View {
    pub filter: Filter,
    pub order_by: Vec<Order>,
    pub limit: Option<usize>,
}

impl View {
    /// Everything, in table order.
    pub fn all() -> View {
        View {
            filter: Filter::always(),
            order_by: Vec::new(),
            limit: None,
        }
    }

    pub fn filter(mut self, f: Filter) -> View {
        self.filter = f;
        self
    }

    pub fn order(mut self, order: Vec<Order>) -> View {
        self.order_by = order;
        self
    }

    pub fn limit(mut self, n: Option<usize>) -> View {
        self.limit = n;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{Row, Schema, Type, Value};
    use crate::table::Table;

    /// `(name, rank)` where a `None` rank is a null column.
    struct Fixture(&'static str, Option<i64>);

    impl Row for Fixture {
        fn field(&self, name: &str) -> Value {
            match name {
                "name" => Value::Str(self.0.to_string()),
                "rank" => self.1.map_or(Value::Null, Value::Int),
                _ => Value::Null,
            }
        }
    }

    fn table() -> Table<Fixture> {
        Table::new(vec![
            Fixture("c", Some(2)),
            Fixture("a", None),
            Fixture("b", Some(1)),
            Fixture("d", Some(2)),
        ])
    }

    fn names(t: &Table<Fixture>, v: &View) -> Vec<&'static str> {
        t.view(v).into_iter().map(|i| t[i].0).collect()
    }

    #[test]
    fn no_order_keeps_table_order() {
        assert_eq!(names(&table(), &View::all()), vec!["c", "a", "b", "d"]);
    }

    #[test]
    fn a_null_column_sorts_last_ascending() {
        let v = View::all().order(vec![Order::asc("rank")]);
        assert_eq!(names(&table(), &v), vec!["b", "c", "d", "a"]);
    }

    /// The rule that is not `Ordering::reverse`: descending reverses the
    /// values and leaves nulls at the end. Newest first still means the
    /// undated last, not the undated first.
    #[test]
    fn a_null_column_sorts_last_descending_too() {
        let v = View::all().order(vec![Order::desc("rank")]);
        assert_eq!(names(&table(), &v), vec!["c", "d", "b", "a"]);
    }

    /// Ties fall through to the next key, which is why `order_by` is a list.
    #[test]
    fn later_keys_break_ties() {
        let v = View::all().order(vec![Order::desc("rank"), Order::asc("name")]);
        assert_eq!(names(&table(), &v), vec!["c", "d", "b", "a"]);
        let v = View::all().order(vec![Order::desc("rank"), Order::desc("name")]);
        assert_eq!(names(&table(), &v), vec!["d", "c", "b", "a"]);
    }

    #[test]
    fn limit_applies_after_the_sort_not_before() {
        let v = View::all().order(vec![Order::asc("name")]).limit(Some(2));
        assert_eq!(names(&table(), &v), vec!["a", "b"]);
    }

    #[test]
    fn filter_narrows_before_the_sort() {
        let mut schema = Schema::new();
        schema.insert("rank", Type::Int);
        schema.insert("name", Type::Str);
        let v = View::all()
            .filter(crate::Filter::parse("rank >= 2", &schema).unwrap())
            .order(vec![Order::asc("name")]);
        assert_eq!(names(&table(), &v), vec!["c", "d"]);
    }

    /// A sort with no keys must not disturb a set the caller already ordered.
    #[test]
    fn view_within_keeps_the_callers_order_when_unsorted() {
        let t = table();
        assert_eq!(
            t.view_within(&[3, 1, 0], &View::all())
                .into_iter()
                .map(|i| t[i].0)
                .collect::<Vec<_>>(),
            vec!["d", "a", "c"]
        );
    }
}
