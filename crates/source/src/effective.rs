//! `grackle config --effective`: the merged config, printed with the
//! provenance the merge itself recorded (MERGE.md B3, DESIGN.md §4d).
//!
//! The point of §4d is that a site inherits a config it never sees, and the
//! only honest way to show it is to have the MERGE say where each value came
//! from. So nothing here diffs two configs after the fact: [`Trace`] is
//! written by `config::merge_table` and the functions below it, as they make
//! each decision, and this module only prints what it was handed. A recorder
//! that lies would have to lie about the merge it is part of.
//!
//! The unit of provenance is the ATOM (Law 2): a scalar, an array, or a
//! definition under a user-chosen name. So `[sets.published]` carries one
//! comment on its header and none on its three keys — you never inherit half
//! a definition — while `[site]`, which the merge descends, carries one per
//! key. Where the comment SITS is the law made visible.

use std::collections::BTreeMap;

/// Which writer supplied a value.
///
/// Four rungs of §2's spine as a site meets them: its own file, the base
/// config underneath it, and — for a key neither file wrote — the default
/// compiled into the deserializer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prov {
    /// The site wrote it and the base had nothing at that key.
    Site,
    /// The site wrote it over a value the base had: Law 1, visible.
    SiteOverBase,
    /// The base wrote it and the site never mentioned the key. This is the
    /// row that makes the command worth having.
    Base,
    /// Neither file wrote it; serde's own default stands.
    Default,
}

impl Prov {
    fn label(self) -> &'static str {
        match self {
            Prov::Site => "site",
            Prov::SiteOverBase => "site over base",
            Prov::Base => "base",
            Prov::Default => "default",
        }
    }

    fn gloss(self) -> &'static str {
        match self {
            Prov::Site => "the site's file; the base had nothing there",
            Prov::SiteOverBase => "the site's file, shadowing a value the base had",
            Prov::Base => "inherited untouched — the site never wrote it",
            Prov::Default => "neither file wrote it; the engine's default stands",
        }
    }
}

/// In the order a reader meets them, nearest writer first (§2's spine).
const PROVENANCES: [Prov; 4] = [Prov::Site, Prov::SiteOverBase, Prov::Base, Prov::Default];

/// A comment: who wrote the value, and whether it was taken WHOLE (a table or
/// an array — the shapes where "half of it was inherited" would be a lie worth
/// forestalling).
type Note = (Prov, bool);

/// Where each atom of the merged config came from, keyed by the path the merge
/// walked to reach it.
///
/// A path segment is a TOML key, except inside an array: `[[collections]]`
/// entries are keyed by their identity (`source:_posts` — the annotation in
/// MERGE.md §1, which is also why a renamed collection still pairs), and every
/// other array element by its index. [`collection_seg`] and [`index_seg`] are
/// the two spellings, used by the recorder and by the printer alike so the
/// two cannot disagree about what a path is.
///
/// `on` is what keeps this free on the load path: `Trace::off()` is what
/// `Config::from_toml` merges with, and every record is one bool test.
pub(crate) struct Trace {
    on: bool,
    notes: BTreeMap<Vec<String>, Prov>,
}

impl Trace {
    /// The load path's trace: records nothing.
    pub(crate) fn off() -> Trace {
        Trace {
            on: false,
            notes: BTreeMap::new(),
        }
    }

    pub(crate) fn recording() -> Trace {
        Trace {
            on: true,
            notes: BTreeMap::new(),
        }
    }

    pub(crate) fn on(&self) -> bool {
        self.on
    }

    pub(crate) fn record(&mut self, path: &[String], prov: Prov) {
        if !self.on {
            return;
        }
        self.notes.insert(path.to_vec(), prov);
    }

    pub(crate) fn at(&self, path: &[String]) -> Option<Prov> {
        self.notes.get(path).copied()
    }

    fn uses(&self, prov: Prov) -> bool {
        self.notes.values().any(|p| *p == prov)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.notes.len()
    }
}

/// One array element's path segment: its index.
pub(crate) fn index_seg(i: usize) -> String {
    format!("[{i}]")
}

/// One `[[collections]]` entry's path segment: its identity, not its position,
/// because the merge pairs them by source and a paired entry may sit at a
/// different index on each side.
pub(crate) fn collection_seg(entry: &toml::Value) -> String {
    crate::config::collection_key(entry).unwrap_or_else(|| "?".to_string())
}

/// Column the provenance comments align at. Past it a line gets two spaces and
/// runs long, which is better than a comment that wraps.
const COMMENT_COL: usize = 46;

/// A table-valued atom wider than this prints as a `[block]` instead of an
/// inline table. Purely presentational: `draft = { type = "bool" }` reads as
/// base.toml writes it, and a six-key view definition does not.
const INLINE_MAX: usize = 62;

/// Top-level key order: base.toml's, so that a diff against `examples/raw`
/// reads as a diff and not as a shuffle. Keys not named here follow, in the
/// order the merged table already has (alphabetical), which is also the order
/// every nested table is printed in.
const ORDER: &[&str] = &[
    "extends",
    "root",
    "gitignore",
    "parts",
    "site",
    "schema",
    "html",
    "markers",
    "collections",
    "sets",
    "routes",
    "axes",
    "widgets",
    "shells",
    "i18n",
    "records",
    "profiles",
    "links",
];

fn rank(k: &str) -> usize {
    ORDER.iter().position(|x| *x == k).unwrap_or(ORDER.len())
}

/// A TOML key, bare where TOML allows it and quoted where it does not
/// (`"og:title"`, `".draft"`, `"shortcut icon"`).
fn key_name(k: &str) -> String {
    let bare = !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        k.to_string()
    } else {
        toml::Value::String(k.to_string()).to_string()
    }
}

/// The merged config as commented TOML. `preamble` is the caller's first
/// lines — which file this is, and which profile is in force — and the legend
/// below it is this module's.
pub(crate) fn render(merged: &toml::Value, trace: &Trace, preamble: &str) -> String {
    let mut e = Emit {
        out: String::new(),
        trace,
        hdr: Vec::new(),
        path: Vec::new(),
    };
    e.out.push_str(preamble);
    e.out.push_str("#\n# Every value says who wrote it:\n#\n");
    // Only the rungs this config actually has. A site with no base merged
    // should not be handed a glossary of the merge it did not do.
    for p in PROVENANCES.iter().filter(|p| trace.uses(**p)) {
        e.out
            .push_str(&format!("#   # {:<15} {}\n", p.label(), p.gloss()));
    }
    e.out.push_str(
        "#\n\
         # A comment marked `whole` is one ATOM (MERGE.md §1, Law 2): a table or\n\
         # a list taken entire from one writer. Its own keys carry no provenance,\n\
         # because half of a definition is never what was inherited.\n\n",
    );
    let Some(t) = merged.as_table() else {
        return e.out;
    };
    e.table(t, false, false, None);
    e.out
}

struct Emit<'a> {
    out: String,
    trace: &'a Trace,
    /// The TOML header path — what goes between the brackets.
    hdr: Vec<String>,
    /// The provenance path — the same walk the merge took, which differs
    /// inside arrays (see [`Trace`]).
    path: Vec<String>,
}

impl Emit<'_> {
    fn note(&self, whole: bool) -> Option<Note> {
        self.trace.at(&self.path).map(|p| (p, whole))
    }

    fn line(&mut self, text: &str, note: Option<Note>) {
        let Some((prov, whole)) = note else {
            self.out.push_str(text);
            self.out.push('\n');
            return;
        };
        let comment = if whole {
            format!("# {}, whole", prov.label())
        } else {
            format!("# {}", prov.label())
        };
        let pad = COMMENT_COL.saturating_sub(text.chars().count()).max(2);
        self.out
            .push_str(&format!("{text}{:pad$}{comment}\n", "", pad = pad));
    }

    /// Does this value print as `[header]` blocks rather than as one line?
    /// Called with `self.path` already pointing at the value.
    fn is_block(&self, v: &toml::Value) -> bool {
        match v {
            // Settled here: the merge stopped, so this is a definition taken
            // whole. A block unless it fits on a line.
            toml::Value::Table(_) if self.trace.at(&self.path).is_some() => {
                let k = self.path.last().map(String::as_str).unwrap_or("");
                key_name(k).chars().count() + 3 + v.to_string().chars().count() > INLINE_MAX
            }
            // Not settled: the merge went further in, so the printer does too.
            toml::Value::Table(_) => true,
            toml::Value::Array(a) => !a.is_empty() && a.iter().all(|e| e.is_table()),
            _ => false,
        }
    }

    /// The bracketed path, each segment quoted where TOML needs it —
    /// `[markers.".draft"]` and not `[markers..draft]`.
    fn header_path(&self) -> String {
        self.hdr
            .iter()
            .map(|s| key_name(s))
            .collect::<Vec<_>>()
            .join(".")
    }

    fn is_collections(&self) -> bool {
        self.path.len() == 1 && self.path[0] == "collections"
    }

    /// One table. `settled` says provenance was decided above this point, so
    /// nothing inside carries a comment; `own_header` and `head` are the
    /// caller's, because an array element's `[[header]]` is printed by the
    /// caller that knows the element exists.
    fn table(&mut self, tbl: &toml::Table, settled: bool, own_header: bool, head: Option<Note>) {
        let top = self.path.is_empty();
        let (mut inline, mut blocks) = (Vec::new(), Vec::new());
        for (k, v) in tbl {
            self.path.push(k.clone());
            let block = !settled && self.is_block(v);
            self.path.pop();
            if block {
                blocks.push((k, v));
            } else {
                inline.push((k, v));
            }
        }
        if top {
            inline.sort_by_key(|(k, _)| rank(k));
            blocks.sort_by_key(|(k, _)| rank(k));
        } else {
            // Sub-tables before arrays of them: `[collections.schema]` written
            // after `[[collections.rules]]` still binds to the right
            // collection, but only because a header path is absolute, and a
            // printer that leans on that is one edit from being wrong. At the
            // top level there is no such trap, and base.toml's order wins.
            blocks.sort_by_key(|(_, v)| v.is_array());
        }

        // A table that holds nothing but sub-tables needs no header of its
        // own: `[html]` and `[html.head]` are namespaces on the way to
        // `[html.head.meta]`, and printing them would be three headers for
        // one table of values.
        if own_header && !self.hdr.is_empty() && (!inline.is_empty() || blocks.is_empty()) {
            self.out.push('\n');
            let h = format!("[{}]", self.header_path());
            self.line(&h, head);
        }

        for (k, v) in inline {
            self.path.push(k.clone());
            let note = if settled {
                None
            } else {
                self.note(v.is_table() || v.is_array())
            };
            let text = format!("{} = {}", key_name(k), v);
            self.line(&text, note);
            self.path.pop();
        }

        for (k, v) in blocks {
            self.hdr.push(k.clone());
            self.path.push(k.clone());
            match v {
                toml::Value::Array(a) => self.array_of_tables(a, settled),
                toml::Value::Table(t) => {
                    let note = if settled { None } else { self.note(true) };
                    self.table(t, settled || note.is_some(), true, note);
                }
                // `is_block` says no.
                _ => unreachable!(),
            }
            self.hdr.pop();
            self.path.pop();
        }
    }

    fn array_of_tables(&mut self, entries: &[toml::Value], settled: bool) {
        // An array taken whole (`[[parts]]`) is noted on the LIST; one whose
        // entries were paired or interleaved (`[[collections]]`, its rules) is
        // noted per entry. Ask the entry first, then the list.
        let whole_list = if settled { None } else { self.note(true) };
        let collections = self.is_collections();
        for (i, e) in entries.iter().enumerate() {
            let Some(t) = e.as_table() else { continue };
            self.path.push(if collections {
                collection_seg(e)
            } else {
                index_seg(i)
            });
            let note = if settled {
                None
            } else {
                self.note(true).or(whole_list)
            };
            self.out.push('\n');
            let h = format!("[[{}]]", self.header_path());
            self.line(&h, note);
            self.table(t, settled || note.is_some(), false, None);
            self.path.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_quoted_only_when_toml_needs_it() {
        assert_eq!(key_name("draft"), "draft");
        assert_eq!(key_name("og-title_2"), "og-title_2");
        assert_eq!(key_name("og:title"), "\"og:title\"");
        assert_eq!(key_name(".draft"), "\".draft\"");
        assert_eq!(key_name("shortcut icon"), "\"shortcut icon\"");
    }

    /// `off()` is what the load path merges with, and it is the whole of the
    /// cost argument: a recorder that records nothing holds nothing.
    #[test]
    fn a_trace_that_is_off_records_nothing() {
        let mut t = Trace::off();
        t.record(&["site".to_string()], Prov::Site);
        assert_eq!(t.len(), 0);
        assert_eq!(t.at(&["site".to_string()]), None);
    }
}
