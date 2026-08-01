//! `grackle config --effective`: merged config with merge-recorded provenance
//! (MERGE.md B3, DESIGN.md §4d). Unit of provenance is the atom (Law 2).

use std::collections::BTreeMap;

/// Writer that supplied a value (§2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prov {
    /// Selected profile overlay (MERGE.md E2).
    Profile,
    Site,
    /// Site shadowed base (Law 1).
    SiteOverBase,
    Base,
    /// Serde / engine default.
    Default,
}

impl Prov {
    fn label(self, profile: &str) -> String {
        match self {
            Prov::Profile => format!("profile {profile}"),
            Prov::Site => "site".to_string(),
            Prov::SiteOverBase => "site over base".to_string(),
            Prov::Base => "base".to_string(),
            Prov::Default => "default".to_string(),
        }
    }

    fn gloss(self) -> &'static str {
        match self {
            Prov::Profile => "the profile's overlay, merged over everything below (§4a)",
            Prov::Site => "the site's file; the base had nothing there",
            Prov::SiteOverBase => "the site's file, shadowing a value the base had",
            Prov::Base => "inherited untouched — the site never wrote it",
            Prov::Default => "neither file wrote it; the engine's default stands",
        }
    }
}

const PROVENANCES: [Prov; 5] = [
    Prov::Profile,
    Prov::Site,
    Prov::SiteOverBase,
    Prov::Base,
    Prov::Default,
];

type Note = (Prov, bool);

/// Merge provenance by path. Collections keyed by identity (MERGE.md §1);
/// `near`/`far` let one merge serve base then profile layers (MERGE.md E2).
pub(crate) struct Trace {
    on: bool,
    near: Prov,
    far: Option<Prov>,
    notes: BTreeMap<Vec<String>, Prov>,
}

impl Trace {
    pub(crate) fn off() -> Trace {
        Trace {
            on: false,
            near: Prov::Site,
            far: Some(Prov::Base),
            notes: BTreeMap::new(),
        }
    }

    pub(crate) fn recording() -> Trace {
        Trace {
            on: true,
            near: Prov::Site,
            far: Some(Prov::Base),
            notes: BTreeMap::new(),
        }
    }

    /// Next overlay layer: `near = prov`, no farther writer.
    pub(crate) fn layer(&mut self, prov: Prov) {
        self.near = prov;
        self.far = None;
    }

    pub(crate) fn near(&self) -> Prov {
        self.near
    }

    /// Both writers: SiteOverBase for site layer; else `near`.
    pub(crate) fn near_over(&self) -> Prov {
        match self.near {
            Prov::Site => Prov::SiteOverBase,
            other => other,
        }
    }

    pub(crate) fn far(&self) -> Option<Prov> {
        self.far
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

pub(crate) fn index_seg(i: usize) -> String {
    format!("[{i}]")
}

/// Collection path segment by identity (not array index).
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

/// Top-level print order matches base.toml.
const ORDER: &[&str] = &[
    "extends",
    "root",
    "gitignore",
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

pub(crate) fn render(merged: &toml::Value, trace: &Trace, preamble: &str, profile: &str) -> String {
    let mut e = Emit {
        out: String::new(),
        trace,
        profile,
        hdr: Vec::new(),
        path: Vec::new(),
    };
    e.out.push_str(preamble);
    e.out.push_str("#\n# Every value says who wrote it:\n#\n");
    for p in PROVENANCES.iter().filter(|p| trace.uses(**p)) {
        e.out
            .push_str(&format!("#   # {:<15} {}\n", p.label(profile), p.gloss()));
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
    profile: &'a str,
    hdr: Vec<String>,
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
            format!("# {}, whole", prov.label(self.profile))
        } else {
            format!("# {}", prov.label(self.profile))
        };
        let pad = COMMENT_COL.saturating_sub(text.chars().count()).max(2);
        self.out
            .push_str(&format!("{text}{:pad$}{comment}\n", "", pad = pad));
    }

    fn is_block(&self, v: &toml::Value) -> bool {
        match v {
            // Settled atom: block unless it fits inline.
            toml::Value::Table(_) if self.trace.at(&self.path).is_some() => {
                let k = self.path.last().map(String::as_str).unwrap_or("");
                key_name(k).chars().count() + 3 + v.to_string().chars().count() > INLINE_MAX
            }
            toml::Value::Table(_) => true,
            toml::Value::Array(a) => !a.is_empty() && a.iter().all(|e| e.is_table()),
            _ => false,
        }
    }

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
            // Nested: sub-tables before arrays of tables.
            blocks.sort_by_key(|(_, v)| v.is_array());
        }

        // Skip empty namespace headers (`[html]` when only `[html.head.*]`).
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
                _ => unreachable!(),
            }
            self.hdr.pop();
            self.path.pop();
        }
    }

    fn array_of_tables(&mut self, entries: &[toml::Value], settled: bool) {
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

    #[test]
    fn a_trace_that_is_off_records_nothing() {
        let mut t = Trace::off();
        t.record(&["site".to_string()], Prov::Site);
        assert_eq!(t.len(), 0);
        assert_eq!(t.at(&["site".to_string()]), None);
    }
}
