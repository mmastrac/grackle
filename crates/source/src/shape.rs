//! Law 2 (MERGE.md §1) in code: the TYPE STRUCTURE of a config value, and the
//! depth a merge descends into it.
//!
//! > Merging descends namespaces and stops at atoms; an atom is taken whole
//! > from the nearest writer. Atoms: scalars, arrays, enums, and *definitions*
//! > (a struct under a user-chosen name). Namespaces: maps, and structs under
//! > engine-chosen names.
//!
//! That law is a statement about the Rust types, so the depth a table merges
//! to should be READ OFF the types rather than assigned by hand — which is
//! what this module exists to do. [`Shape`] is the type tree in TOML-name
//! space; [`Shape::depth`] is the law; the per-key laws fall out of the two
//! (`config::derived_laws`).
//!
//! The one thing a shape is NOT is a second spelling of the merge table. It
//! describes types, not decisions: whether `[axes]` is a registry or a bag is
//! not a question this file can be asked, because both are `Descend(1)` and
//! the difference is what the values mean, not how they merge.

// B1 builds this beside the hand tables rather than under them: the merge
// still dispatches through `CONFIG_LAWS`, and only the agreement test reads
// what is derived here. B2 turns that around, at which point this attribute
// goes.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The structure of one config value, in TOML's name space (serde renames
/// applied, skipped fields absent) rather than Rust's.
///
/// Deliberately three variants: the law reads exactly three things off a
/// type — has it keys, are they the engine's or the user's, and is there
/// anything under them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shape {
    /// A scalar, an array or an enum. No keys to merge by; the nearest
    /// writer's value is taken whole.
    Atom,
    /// A struct: its fields, under their TOML names.
    ///
    /// Whether one is a namespace or an atom is a question of POSITION, not
    /// of the type. Under an engine-chosen name (`[site]`, `[html]`) a struct
    /// descends per field; under a user-chosen one (`[sets.<name>]`) it is a
    /// *definition*, and definitions are atoms. [`Shape::depth`] applies that
    /// rule, so a type is described once and merges correctly wherever it is
    /// used.
    Struct(Vec<(&'static str, Shape)>),
    /// `BTreeMap<String, V>` (or `toml::Table`): user-chosen keys over one
    /// value shape. Descends per key.
    Map(Box<Shape>),
}

impl Shape {
    /// A struct that only ever appears under a user-chosen name, with its
    /// fields left undescribed.
    ///
    /// This is not a claim that it has none: it is the claim that nothing
    /// reads them. A definition is an atom, so the merge stops above its
    /// fields and transcribing them here would be a list with no reader and
    /// no test. `a_definition_never_sits_under_an_engine_name` holds the
    /// claim — the moment such a type appears as a field of an engine-named
    /// struct, the empty list becomes a lie and that test says so.
    pub(crate) fn definition() -> Shape {
        Shape::Struct(Vec::new())
    }

    /// Law 2's depth: the level at which the first atom sits, counting this
    /// value's own keys as level 1. Zero is "this value IS the atom".
    ///
    /// A struct takes the DEEPEST of its fields, because one depth governs
    /// the whole table (`merge_to_depth` carries a single `n`). Descending
    /// past a shallower field's atom is harmless while that atom is a scalar
    /// or an array, since a merge that reaches a non-table hands back the
    /// nearer value whole. The shape that would break the rule is an atom
    /// that IS a table — a definition, a `LocalizedStr` — sitting shallower
    /// than a map beside it; `a_nested_struct_ends_at_one_depth` asserts the
    /// config has none.
    pub(crate) fn depth(&self) -> usize {
        match self {
            Shape::Atom => 0,
            Shape::Struct(fields) => 1 + fields.iter().map(|(_, s)| s.depth()).max().unwrap_or(0),
            // A map descends per key. Its value descends FURTHER only if it
            // is itself a map: a struct or an enum under a user-chosen name
            // is a definition, and a definition is an atom.
            Shape::Map(value) => match &**value {
                Shape::Map(_) => 1 + value.depth(),
                _ => 1,
            },
        }
    }

    /// This shape's fields, empty for anything that is not a struct.
    pub(crate) fn fields(&self) -> &[(&'static str, Shape)] {
        match self {
            Shape::Struct(fields) => fields,
            _ => &[],
        }
    }
}

/// A type that can say what shape it is. Implemented for the container types
/// here — where the law is entirely mechanical — and per struct in
/// `config.rs`, where a field list is the only thing Rust cannot tell us.
pub(crate) trait Shaped {
    fn shape() -> Shape;
}

/// One field: its TOML name, and its shape read off the FIELD'S OWN TYPE.
///
/// The selector is never called. It exists so that the description tracks the
/// declaration — retype a field and its law changes with it, with nothing
/// here to edit — which is the difference between deriving a law and
/// restating one. The name stays a literal because it is the TOML spelling,
/// which serde owns; the tests hold the two together against serde's own
/// `deny_unknown_fields` list.
pub(crate) fn field<S, T: Shaped>(
    name: &'static str,
    _select: fn(&S) -> &T,
) -> (&'static str, Shape) {
    (name, T::shape())
}

macro_rules! atoms {
    ($($t:ty),* $(,)?) => { $(impl Shaped for $t {
        fn shape() -> Shape { Shape::Atom }
    })* };
}

atoms![String, bool, PathBuf, toml::Value];

/// An array is an atom whatever it holds — "half-inheriting a list is never
/// what was meant" — so its element type is never described.
impl<T> Shaped for Vec<T> {
    fn shape() -> Shape {
        Shape::Atom
    }
}

/// `Option` is transparent: absence is not structure.
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

/// `toml::Table` is a map by another name: user-chosen keys over values the
/// engine does not type. `[schema]` and `[collections.*.schema]` are this —
/// a field declaration is opaque here and typed by `schema.rs`.
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
