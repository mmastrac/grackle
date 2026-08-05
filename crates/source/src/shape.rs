//! Law 2: merge depth read off config types via [`Shape`].

use std::collections::BTreeMap;
use std::path::PathBuf;

/// How one config key merges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Law {
    Atom,
    Descend(usize),
    /// `[[collections]]` pair by source, then merge under `Collection`'s shape.
    Collections,
    /// Site list first; §4 first-writer-wins per key.
    Prepend,
}

/// Config value structure in TOML name space (serde renames applied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shape {
    Atom,
    /// Table-spelled atom (`LocalizedStr`). Same law as [`Shape::Atom`]; variant exists for descent checks.
    TableAtom,
    Struct(Vec<(&'static str, Shape)>),
    Map(Box<Shape>),
    /// §1 annotation on the field it governs.
    Annotated(Law, Box<Shape>),
}

impl Shape {
    /// Definition atom under a user-chosen name; fields intentionally undescribed.
    pub(crate) fn definition() -> Shape {
        Shape::Struct(Vec::new())
    }

    /// Level at which the first atom sits; 0 means this value is the atom.
    pub(crate) fn depth(&self) -> usize {
        match self {
            Shape::Atom | Shape::TableAtom => 0,
            Shape::Struct(fields) => 1 + fields.iter().map(|(_, s)| s.depth()).max().unwrap_or(0),
            // Map value descends further only if it is itself a map; struct/enum under user name is a definition (atom).
            Shape::Map(value) => match &**value {
                Shape::Map(_) => 1 + value.depth(),
                _ => 1,
            },
            Shape::Annotated(_, inner) => inner.depth(),
        }
    }

    pub(crate) fn law(&self) -> Law {
        match self {
            Shape::Annotated(law, _) => *law,
            other => match other.depth() {
                0 => Law::Atom,
                n => Law::Descend(n),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn is_table_atom(&self) -> bool {
        match self {
            Shape::TableAtom => true,
            Shape::Annotated(_, inner) => inner.is_table_atom(),
            _ => false,
        }
    }

    pub(crate) fn fields(&self) -> &[(&'static str, Shape)] {
        match self {
            Shape::Struct(fields) => fields,
            _ => &[],
        }
    }
}

pub(crate) trait Shaped {
    fn shape() -> Shape;
}

/// Field shape read off the field's own type; name is the TOML spelling.
pub(crate) fn field<S, T: Shaped>(
    name: &'static str,
    _select: fn(&S) -> &T,
) -> (&'static str, Shape) {
    (name, T::shape())
}

pub(crate) fn annotated<S, T: Shaped>(
    name: &'static str,
    _select: fn(&S) -> &T,
    law: Law,
) -> (&'static str, Shape) {
    (name, Shape::Annotated(law, Box::new(T::shape())))
}

macro_rules! atoms {
    ($($t:ty),* $(,)?) => { $(impl Shaped for $t {
        fn shape() -> Shape { Shape::Atom }
    })* };
}

atoms![String, bool, PathBuf, toml::Value];

impl<T> Shaped for Vec<T> {
    fn shape() -> Shape {
        Shape::Atom
    }
}

impl<T: Shaped> Shaped for Option<T> {
    fn shape() -> Shape {
        T::shape()
    }
}

impl<V: Shaped> Shaped for BTreeMap<String, V> {
    fn shape() -> Shape {
        Shape::Map(Box::new(V::shape()))
    }
}

impl Shaped for toml::Table {
    fn shape() -> Shape {
        Shape::Map(Box::new(Shape::Atom))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four depths the config surface uses, stated as pure shapes so the
    /// law is legible without a config in the way.
    #[test]
    fn depth_is_where_the_first_atom_sits() {
        let map = |v: Shape| Shape::Map(Box::new(v));
        // A scalar is the atom.
        assert_eq!(Shape::Atom.depth(), 0);
        // A bag of scalars, and a registry of definitions: one law, read at
        // one depth. `[site]` and `[sets]`.
        assert_eq!(
            Shape::Struct(vec![("title", Shape::Atom)]).depth(),
            1,
            "a struct under an engine name descends per field"
        );
        assert_eq!(
            map(Shape::definition()).depth(),
            1,
            "a definition under a user-chosen name is the atom"
        );
        // A map of maps descends twice: `[records.<field>.<id>]`.
        assert_eq!(map(map(Shape::definition())).depth(), 2);
        // And a struct of a struct of a map three times: `[html.head.*]`.
        assert_eq!(
            Shape::Struct(vec![(
                "head",
                Shape::Struct(vec![("meta", map(Shape::Atom))])
            )])
            .depth(),
            3
        );
    }

    /// The law is the shape, read at the depth the shape has — and the two
    /// annotated keys are the only place that sentence needs a footnote.
    #[test]
    fn the_law_falls_out_of_the_shape_unless_it_is_annotated() {
        assert_eq!(Shape::Atom.law(), Law::Atom);
        // A table-spelled atom is an atom: same depth, same law. What it
        // knows that `Atom` does not is what a descent PAST it would cost,
        // and no law is asked that question.
        assert_eq!(Shape::TableAtom.law(), Law::Atom);
        assert_eq!(Shape::TableAtom.depth(), 0);
        assert!(Shape::TableAtom.is_table_atom());
        assert!(!Shape::Atom.is_table_atom());
        assert!(
            !Shape::definition().is_table_atom(),
            "a definition is table-spelled but never sits where a descent reaches it"
        );
        assert_eq!(
            Shape::Map(Box::new(Shape::definition())).law(),
            Law::Descend(1)
        );
        // `[[collections]]` is a `Vec` — an atom by structure, like
        // `[[parts]]` beside it. Only the annotation tells the two apart, and
        // it leaves the structure it overrides in place.
        let collections = Shape::Annotated(Law::Collections, Box::new(Shape::Atom));
        assert_eq!(collections.law(), Law::Collections);
        assert_eq!(collections.depth(), 0, "the type is still an array");
    }

    /// The deepest field governs, and a shallower one is unharmed: `[i18n]`'s
    /// `default` is a string sitting a level above `strings`' entries.
    #[test]
    fn a_struct_takes_the_deepest_of_its_fields() {
        let s = Shape::Struct(vec![
            ("default", Shape::Atom),
            ("strings", Shape::Map(Box::new(Shape::Atom))),
        ]);
        assert_eq!(s.depth(), 2);
    }
}
