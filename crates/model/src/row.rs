//! Input-side row: one content file in the site database.

use crate::{Key, RowAxis};
use chrono::NaiveDate;
use grackle_db::{filter, Keyed};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize)]
pub struct Row {
    /// What this row IS, as opposed to where it currently sits. Assigned by
    /// `insert_rows` from `rel`, because a row's source file is the one thing
    /// about it that survives a rebuild.
    pub key: Key,
    /// The collection whose source claimed this file. Relations anchor to
    /// it, not to the table: two dated collections in one table interleave,
    /// making a blog post's "later post" a note.
    pub collection: String,
    /// The `match` glob of the rule that CLAIMED this row — the first rule of
    /// the first eligible scope that wanted it (IO.md I7d).
    ///
    /// Membership is an answer the ordered rule sequence gives per file, so
    /// something has to be able to say which rule gave it: `collection` names
    /// the scope and this names the rule inside it, and `grackle explain`
    /// prints the pair. An ordering law nobody can observe is an ordering law
    /// nobody can debug.
    ///
    /// Distinct from the rule that ROUTED the row, which may be a later one:
    /// a defaults-only rule claims files it does not land.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    pub path: PathBuf,
    pub rel: PathBuf,
    #[serde(serialize_with = "hex")]
    pub version: u64,
    pub date: Option<NaiveDate>,
    pub slug: String,
    /// Filename without extension — unique, because it carries the date.
    pub stem: String,
    /// `Option` because a PAGE may genuinely have none — a titleless page
    /// is searchable by body and wears its URL as the only honest label.
    /// A post's loader always fills it (front matter, else the slug read
    /// as words), so this is `Some` for every post; the option exists to
    /// let one row type serve both (q51).
    pub title: Option<String>,
    pub description: Option<String>,
    /// Which theme renders this row, and how much wrapper it wears (§5a,
    /// §5g).
    pub theme: Option<String>,
    pub shell: Option<String>,
    /// Typed extra fields, validated against the governing `.schema.toml`
    /// (§5b). Declarations come from a root-wide walk, so they are visible
    /// to every row whatever loader filled it. List fields such as `tags`
    /// live here — there is no parallel column.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, filter::Value>,
    /// The image-typed subset of `fields`: field name -> the value, kept
    /// apart because only the loader knows which fields `.schema.toml`
    /// declared as images and the renderer still has to find them.
    ///
    /// Each value is checked at load to name a row of this site, or to be an
    /// absolute url naming something outside it (`resolve_image_fields`) — so
    /// a relative one here is a reference that resolves, not a hopeful string.
    #[serde(skip)]
    pub images: BTreeMap<String, String>,
    /// Declared position (§6e). A post's *table* order is chronological;
    /// this is what a view sorts on when it says so — see `order_by` in
    /// `build_views`.
    pub order: Option<i64>,
    /// Logical identity shared by a row and its file-axis twins
    /// (collection-relative, no extension). Pairing key for `by_logical`.
    #[serde(skip)]
    pub logical: String,
    pub url: String,
    /// IO.md §4a's second address slot: the **hash address** the embed policy
    /// published for this row, `/static/{hash}.{ext}`.
    ///
    /// `url` is the canonical address — where a rule said the row lands, what
    /// an authored link resolves to, what `rel=canonical` names. This is the
    /// content store made public: what an EMBEDDED citation resolves to when
    /// no rule routed the row. `Some` exactly when a rule said `embed = true`
    /// and the policy admitted the row, and then `url` is empty — the two
    /// slots are not alternatives on one row today, because a routed output
    /// wins and I12 owns the twin that carries both.
    ///
    /// Computed at LOAD, from the input bytes and the transform parameters and
    /// from nothing else (`strong::address`) — the hashing law, which is what
    /// keeps §1's "facts at planning" true of an output whose whole address is
    /// a hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strong_url: Option<String>,
    /// Measured at load, when the body is briefly in hand. The body itself
    /// is not kept — every consumer re-reads it (§2).
    pub body_bytes: usize,
    /// Tree heritage: a rendered row (front matter) vs a static file copied
    /// verbatim. Always true for a row that came from a posts collection —
    /// a post with no front matter is still parsed.
    pub rendered: bool,
    /// IO.md §3: this row's file carried a front-matter block.
    ///
    /// Not the same bit as `rendered`, and the difference is the whole reason
    /// it is a separate one: `rendered` says the pipeline parsed this row,
    /// which a posts scope grants unconditionally, while this says the author
    /// wrote identity into the file. They agree everywhere except a `.md` in
    /// a posts scope with no block — grack.com has exactly one — where
    /// `rendered` is true and this is false.
    ///
    /// Sidecars (IO.md I8) widen the fact rather than change it: identity
    /// comes from a block **or** a sidecar file, and this is the column that
    /// answers "either". Which of the two is `sidecar`, below.
    pub front_mattered: bool,
    /// IO.md I8: this row's identity arrived from a **sidecar** rather than
    /// from a block in the file. `front_mattered` is then true while the
    /// row's own bytes were never parsed — which is the split sidecars exist
    /// for, and the one shape where `rendered` does not follow from
    /// `front_mattered`.
    ///
    /// A bool rather than the sidecar's path, because the path is the row's
    /// own `rel` plus `.toml` by construction — there is exactly one name a
    /// sidecar can have. `grackle explain` prints it as the provenance of the
    /// identity fact.
    pub sidecar: bool,
    /// §4: this row's route rule was `on_demand`, so it publishes only when
    /// something references it. The URL is computed either way — what is
    /// deferred is whether a `Route` exists — which is what lets a link
    /// resolve to a row nothing has materialized yet.
    pub on_demand: bool,
    /// IO.md §2, the join's input side: this row's **canonical** output, keyed
    /// by its URL, or `None` when the row lands nowhere.
    ///
    /// The route table's shadow, not a second opinion: `load` fills it from
    /// the routes it minted, so `output` is `Some` exactly when a `Route`
    /// names this row. That is what makes the three shapes with no output
    /// sayable rather than structural — a **claimed** row (its landing owns
    /// the URL, q45), an **on-demand** row nobody has referenced yet (§4 — the
    /// pull model, so the answer becomes `Some` at `materialize_referenced`
    /// and not before), and a row a rule declined to route at all.
    ///
    /// A key rather than a `String` because a route's key IS its URL: the
    /// join's three fields are all key lists, which is what lets I10's graph
    /// read them as edges without a lookup table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Key>,
    /// IO.md §2: this row's NON-canonical outputs — the `rel="alternate"` set
    /// (q53's axis), keyed by URL, sorted.
    ///
    /// "A form is an output" made literal: the axis design's sentence — this
    /// route points at other forms of THIS row — is a field rather than a
    /// scan of the route table. Empty for a row published once, which is
    /// almost every row.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternates: Vec<Key>,
    /// IO.md §2: every output that carries this row as a **member** — the
    /// listings, archives and feeds that arrange it, keyed by URL, sorted.
    ///
    /// **Arrangement, not citation.** `linked_from` is the citation half (a
    /// human wrote a link); this is the arrangement half (a query put the row
    /// in a list). The backlink scanner learned that distinction the hard way
    /// — a listing that carries a row is not a page that cites it — and this
    /// is the second of its two clients, now with a name of its own.
    ///
    /// Not a filter column, deliberately: see `row_schema`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub viewed_by: Vec<Key>,
    pub size: u64,
    /// An object's pixel shape, header-read at load beside `size` (§6b's
    /// dimension facts, q26). A file property like any other, so a view can
    /// ask for it: `where = "width > height"` selects the landscape ones.
    /// `None` for a row that is not an image, or one whose header would not
    /// parse.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// q45: this row is a landing view's content — no standalone route,
    /// excluded from every query structurally.
    #[serde(skip)]
    pub claimed: bool,
    /// q53: the axes this row's route rule spends. Empty for a row published
    /// once; more than one when the route template names more than one axis
    /// placeholder, which is what makes two axes over one row a cartesian
    /// product of routes rather than a collision. The names drive the product.
    #[serde(skip)]
    pub axis: Vec<RowAxis>,
    /// The rule's route template(s), rendered (path/group tokens filled, axis
    /// placeholders preserved). One in the ordinary case; a list for the
    /// default-axis case (§6f), where a member at its canonical value drops its
    /// segment by falling to a shorter template. The materializer selects among
    /// these per member-tuple.
    #[serde(skip)]
    pub route_templates: Vec<String>,
}

impl Keyed for Row {
    fn key(&self) -> &Key {
        &self.key
    }
}

impl Row {
    pub fn year_month(&self) -> Option<(i32, u32)> {
        use chrono::Datelike;
        self.date.map(|d| (d.year(), d.month()))
    }

    /// A declared `bool` field, false when absent or unset (§4e).
    ///
    /// Site vocabulary, not engine columns: prefer a head expression or a
    /// generic field dump. The outline pass still spells `toc` here because
    /// building the heading tree is work, not a string the head can emit.
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.fields.get(name), Some(filter::Value::Bool(true)))
    }

    /// A declared string field, or None when absent / not a string.
    pub fn string(&self, name: &str) -> Option<&str> {
        match self.fields.get(name) {
            Some(filter::Value::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl Row {
    /// The hero image source (q23): the explicit `cover:` field beats
    /// `image:`; both must be image-typed schema fields (§5b). The
    /// first-image-block fallback remains open.
    pub fn hero_source(&self) -> Option<&str> {
        self.images
            .get("cover")
            .or_else(|| self.images.get("image"))
            .map(String::as_str)
    }
}

fn hex<S: serde::Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{v:016x}"))
}

impl filter::Row for Row {
    fn field(&self, name: &str) -> filter::Value {
        use chrono::Datelike;
        use filter::Value as V;
        let opt_str = |o: &Option<String>| match o {
            Some(s) => V::Str(s.clone()),
            None => V::Null,
        };
        match name {
            "title" => opt_str(&self.title),
            "slug" => V::Str(self.slug.clone()),
            "stem" => V::Str(self.stem.clone()),
            "url" => V::Str(self.url.clone()),
            "description" => opt_str(&self.description),
            "date" => match self.date {
                Some(d) => V::Str(d.format("%Y-%m-%d").to_string()),
                None => V::Null,
            },
            "year" => self.date.map_or(V::Null, |d| V::Int(d.year() as i64)),
            "month" => self.date.map_or(V::Null, |d| V::Int(d.month() as i64)),
            "day" => self.date.map_or(V::Null, |d| V::Int(d.day() as i64)),
            "body_bytes" => V::Int(self.body_bytes as i64),
            "order" => self.order.map_or(V::Null, V::Int),
            "rendered" => V::Bool(self.rendered),
            "front_mattered" => V::Bool(self.front_mattered),
            // The join (IO.md §2). Bare `output` is the record's existence;
            // `output.url` is its one projected field, Null when there is no
            // record to project — never the empty string, because "lands at
            // nowhere" and "lands at ''" are different claims.
            "output" => V::Bool(self.output.is_some()),
            "output.url" => self
                .output
                .as_ref()
                .map_or(V::Null, |k| V::Str(k.to_string())),
            "alternates" => V::List(self.alternates.iter().map(|k| k.to_string()).collect()),
            // `rel` is root-relative for every row, whatever its origin.
            "path" => V::Str(self.rel.to_string_lossy().to_string()),
            "dir" => V::Str(
                self.rel
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            "collection" => V::Str(self.collection.clone()),
            "name" => self
                .rel
                .file_name()
                .map_or(V::Null, |s| V::Str(s.to_string_lossy().to_string())),
            "ext" => self.rel.extension().map_or(V::Str(String::new()), |s| {
                V::Str(s.to_string_lossy().to_lowercase())
            }),
            "size" => V::Int(self.size as i64),
            "width" => self.width.map_or(V::Null, |w| V::Int(w as i64)),
            "height" => self.height.map_or(V::Null, |h| V::Int(h as i64)),
            // Schema fields (§5b) resolve after the base names — the same
            // fallthrough a page has had.
            other => self.fields.get(other).cloned().unwrap_or(V::Null),
        }
    }
}

impl Row {
    /// Values of a declared list field, or empty when absent / not a list.
    pub fn list(&self, name: &str) -> Vec<String> {
        match filter::Row::field(self, name) {
            filter::Value::List(v) => v,
            _ => Vec::new(),
        }
    }
}
