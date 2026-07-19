//! The search core (§6b: "this retires Swiftype") — ONE implementation of
//! tokenizing, stemming, index building and ranking, used by both ends:
//!
//! - `grackle build` calls `build_index` and ships the result as
//!   `/search.bin` (postcard);
//! - the same code compiled to WebAssembly (`search-wasm`) loads those bytes
//!   in the browser and answers queries as the user types.
//!
//! Symmetry by construction: the query goes through the exact `stem()` the
//! index was built with, because it is not a port — it is the same compiled
//! function. (This is also why the stemmer is free to stay simple: swapping
//! in Snowball later is a one-crate change that cannot desynchronize.)

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Very common words that would dominate postings without informing them.
const STOPWORDS: &[&str] = &[
    "a", "about", "after", "all", "also", "an", "and", "any", "are", "as",
    "at", "be", "because", "been", "before", "but", "by", "can", "could",
    "did", "do", "does", "for", "from", "get", "had", "has", "have", "here",
    "how", "i", "if", "in", "into", "is", "it", "its", "just", "like", "ll",
    "me", "more", "most", "my", "no", "not", "now", "of", "on", "one",
    "only", "or", "other", "our", "out", "over", "re", "s", "same", "so",
    "some", "such", "t", "than", "that", "the", "their", "them", "then",
    "there", "these", "they", "this", "those", "through", "to", "too",
    "under", "up", "ve", "very", "was", "we", "were", "what", "when",
    "where", "which", "while", "who", "why", "will", "with", "would", "you",
    "your",
];

/// Light, guarded suffix stripping. Deliberately simple; see the module doc
/// for why simplicity costs nothing here.
pub fn stem(word: &str) -> Option<String> {
    let w = word.to_lowercase();
    if w.len() < 2 || w.len() > 24 || STOPWORDS.contains(&w.as_str()) {
        return None;
    }
    let mut s = w;
    if let Some(p) = s.strip_suffix("'s") {
        s = p.to_string();
    }
    for (suffix, min_stem) in
        [("ies", 3), ("ing", 4), ("edly", 4), ("ed", 4), ("ly", 4), ("es", 3), ("s", 3)]
    {
        if let Some(p) = s.strip_suffix(suffix) {
            if p.len() >= min_stem {
                s = if suffix == "ies" { format!("{p}y") } else { p.to_string() };
                break;
            }
        }
    }
    (s.len() >= 2).then_some(s)
}

/// Tokens of visible text: alphanumeric runs, stemmed, stopwords dropped.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter_map(stem)
        .collect()
}

/// Visible text of an HTML fragment: characters outside tags, and outside
/// the raw-text elements whose content is code rather than prose.
///
/// Dropping only the tags is not enough: `<style>` and `<script>` hold text
/// by the parser's reckoning, so a stylesheet's identifiers would land in
/// the index as terms. The main corpus was searchable for `rgba`, `fafafa`
/// and `ffffff` — from §6c's three styled posts, and nowhere else. Prose
/// mentions survive, which is the point: two posts discuss `margin` in
/// their text and stay findable by it.
pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        out.push(' ');
        let tail = &rest[i..];
        // `get` rather than a slice: the byte after a `<` may be the middle
        // of a multi-byte char (the corpus is full of `’`), and slicing
        // there panics.
        let raw = ["<script", "<style"].into_iter().find(|open| {
            tail.get(..open.len()).is_some_and(|h| h.eq_ignore_ascii_case(open))
                && tail[open.len()..].starts_with(|c: char| c == '>' || c.is_whitespace())
        });
        rest = match raw {
            // Resume at the close tag, which the next pass skips as an
            // ordinary tag.
            Some(open) => {
                let close = format!("</{}", &open[1..]);
                match tail.to_ascii_lowercase().find(&close) {
                    Some(j) => &tail[j..],
                    None => "",
                }
            }
            None => match tail.find('>') {
                Some(j) => &tail[j + 1..],
                None => "",
            },
        };
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------------- index

/// The shipped index. `docs` are `(url, title, date)`; `terms` maps a stem
/// to postings `(doc index, quantised TF·IDF)`, strongest first.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Index {
    pub docs: Vec<(String, String, String)>,
    pub terms: BTreeMap<String, Vec<(u32, u16)>>,
}

/// One searchable document, as the build side sees it.
pub struct SearchDoc {
    pub url: String,
    pub title: String,
    pub date: String,
    /// Rendered body HTML (tags are stripped here).
    pub html: String,
    pub tags: Vec<String>,
}

pub struct IndexStats {
    pub docs: usize,
    pub terms: usize,
    pub postings: usize,
}

/// Title and tag hits matter more than body mentions.
const TITLE_BOOST: u32 = 5;
const TAG_BOOST: u32 = 5;
/// Postings per term are capped: for a common term only the strongest docs
/// matter, and the tail is index weight with no ranking power.
const MAX_POSTINGS: usize = 40;

pub fn build_index(docs: &[SearchDoc]) -> (Index, IndexStats) {
    let n = docs.len().max(1);
    let tfs: Vec<HashMap<String, u32>> = docs
        .iter()
        .map(|d| {
            let mut tf: HashMap<String, u32> = HashMap::new();
            for t in tokenize(&d.title) {
                *tf.entry(t).or_default() += TITLE_BOOST;
            }
            for tag in &d.tags {
                for t in tokenize(tag) {
                    *tf.entry(t).or_default() += TAG_BOOST;
                }
            }
            for t in tokenize(&strip_tags(&d.html)) {
                *tf.entry(t).or_default() += 1;
            }
            tf
        })
        .collect();

    let mut df: HashMap<&str, u32> = HashMap::new();
    for tf in &tfs {
        for term in tf.keys() {
            *df.entry(term).or_default() += 1;
        }
    }

    let mut terms: BTreeMap<String, Vec<(u32, u16)>> = BTreeMap::new();
    for (i, tf) in tfs.iter().enumerate() {
        for (term, &count) in tf {
            let idf = (n as f32 / df[term.as_str()] as f32).ln().max(0.05);
            let score = ((count as f32 * idf) * 100.0).min(65535.0) as u16;
            terms.entry(term.clone()).or_default().push((i as u32, score));
        }
    }
    let mut postings = 0;
    for list in terms.values_mut() {
        list.sort_by(|a, b| b.1.cmp(&a.1));
        list.truncate(MAX_POSTINGS);
        postings += list.len();
    }

    let stats = IndexStats { docs: docs.len(), terms: terms.len(), postings };
    (
        Index { docs: docs.iter().map(|d| (d.url.clone(), d.title.clone(), d.date.clone())).collect(), terms },
        stats,
    )
}

// ------------------------------------------------------------------ search

/// One ranked hit: `(url, title, date)`.
pub type Hit = (String, String, String);

impl Index {
    pub fn from_bytes(bytes: &[u8]) -> Result<Index, postcard::Error> {
        postcard::from_bytes(bytes)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("index serializes")
    }

    /// Rank documents for a query. All tokens but the last match their stem
    /// exactly; the **last token is a prefix** over the sorted term map, so
    /// results appear as the user types ("bluet" finds bluetooth). Docs
    /// matching more distinct tokens rank first, then by summed score; a
    /// prefix contributes each doc's best expansion, not the sum of all.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let raw: Vec<&str> = query
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|t| !t.is_empty())
            .collect();
        if raw.is_empty() {
            return Vec::new();
        }
        // (distinct hits, summed score) per doc.
        let mut matched: HashMap<u32, (u32, u32)> = HashMap::new();

        let exact: Vec<String> = raw[..raw.len() - 1].iter().filter_map(|t| stem(t)).collect();
        for s in &exact {
            if let Some(postings) = self.terms.get(s) {
                for &(doc, score) in postings {
                    let e = matched.entry(doc).or_default();
                    e.0 += 1;
                    e.1 += score as u32;
                }
            }
        }

        // The trailing token as a live prefix. Stem it (so "parsing|" and
        // "pars|" land in the same range), then take each doc's best match
        // among the expansions.
        let last = raw[raw.len() - 1].to_lowercase();
        let prefix = stem(&last).unwrap_or(last);
        let mut best: HashMap<u32, u32> = HashMap::new();
        for (term, postings) in self.terms.range(prefix.clone()..) {
            if !term.starts_with(&prefix) {
                break;
            }
            for &(doc, score) in postings {
                let b = best.entry(doc).or_default();
                *b = (*b).max(score as u32);
            }
        }
        for (doc, score) in best {
            let e = matched.entry(doc).or_default();
            e.0 += 1;
            e.1 += score;
        }

        let mut ranked: Vec<(u32, (u32, u32))> = matched.into_iter().collect();
        ranked.sort_by(|a, b| (b.1 .0, b.1 .1, a.0).cmp(&(a.1 .0, a.1 .1, b.0)));
        ranked
            .into_iter()
            .take(limit)
            .filter_map(|(doc, _)| self.docs.get(doc as usize).cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stemming_is_light_and_guarded() {
        assert_eq!(stem("Parsing").as_deref(), Some("pars"));
        assert_eq!(stem("parsed").as_deref(), Some("pars"));
        assert_eq!(stem("parses").as_deref(), Some("pars"));
        assert_eq!(stem("libraries").as_deref(), Some("library"));
        assert_eq!(stem("rust's").as_deref(), Some("rust"));
        assert_eq!(stem("using").as_deref(), Some("using"));
        assert_eq!(stem("the"), None);
        // Years are real queries on a 27-year blog.
        assert_eq!(stem("2010").as_deref(), Some("2010"));
    }

    fn corpus() -> Vec<SearchDoc> {
        vec![
            SearchDoc {
                url: "/a/".into(),
                title: "Hacking Bluetooth to Brew Coffee".into(),
                date: "1 January 2012".into(),
                html: "<p>an espresso machine speaks bluetooth now</p>".into(),
                tags: vec!["hardware".into()],
            },
            SearchDoc {
                url: "/b/".into(),
                title: "Rust linkers".into(),
                date: "2 June 2026".into(),
                html: "<p>bluetooth mentioned once, in passing</p>".into(),
                tags: vec!["rust".into()],
            },
            SearchDoc {
                url: "/c/".into(),
                title: "Unrelated".into(),
                date: "3 March 2020".into(),
                html: "<p>nothing relevant here</p>".into(),
                tags: vec![],
            },
        ]
    }

    #[test]
    fn title_hits_outrank_body_mentions() {
        let (idx, _) = build_index(&corpus());
        let hits = idx.search("bluetooth", 10);
        assert_eq!(hits[0].0, "/a/");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn last_token_is_a_live_prefix() {
        let (idx, _) = build_index(&corpus());
        // As the user types: "blu" already finds bluetooth posts.
        let hits = idx.search("blu", 10);
        assert_eq!(hits[0].0, "/a/");
        // And a multi-word query keeps earlier words exact.
        let hits = idx.search("espresso blu", 10);
        assert_eq!(hits.len(), 2, "espresso-only doc still ranks first");
        assert_eq!(hits[0].0, "/a/");
    }

    #[test]
    fn round_trips_through_postcard() {
        let (idx, _) = build_index(&corpus());
        let bytes = idx.to_bytes();
        let back = Index::from_bytes(&bytes).unwrap();
        assert_eq!(back.docs.len(), 3);
        assert_eq!(back.search("bluetooth", 10)[0].0, "/a/");
    }

    #[test]
    fn postings_are_capped() {
        let docs: Vec<SearchDoc> = (0..60)
            .map(|i| SearchDoc {
                url: format!("/{i}/"),
                title: "x".into(),
                date: String::new(),
                html: "<p>common word here</p>".into(),
                tags: vec![],
            })
            .collect();
        let (idx, stats) = build_index(&docs);
        assert!(idx.terms["common"].len() <= MAX_POSTINGS);
        assert_eq!(stats.docs, 60);
    }

    #[test]
    fn empty_and_stopword_queries_return_nothing_sane() {
        let (idx, _) = build_index(&corpus());
        assert!(idx.search("", 10).is_empty());
        assert!(idx.search("   ", 10).is_empty());
    }

    /// `<style>`/`<script>` content is code, not prose: it must not become
    /// searchable terms. Regression for the CSS that was in the shipped
    /// index (rgba, px, margin, webkit).
    #[test]
    fn raw_text_elements_are_not_indexable_prose() {
        let html = "<p>real prose</p><style>.a { color: rgba(0,0,0,.5); }</style>\
                    <p>more</p><script>var margin = 4;</script><p>end</p>";
        let text = strip_tags(html);
        for code in ["rgba", "color", "margin", "var"] {
            assert!(!text.contains(code), "{code:?} leaked into {text:?}");
        }
        for prose in ["real prose", "more", "end"] {
            assert!(text.contains(prose), "{prose:?} lost from {text:?}");
        }
        // A `<` followed by a multi-byte char must not split it. (An
        // unterminated `<` still drops the rest, as it always has.)
        assert_eq!(strip_tags("a < \u{2019}b\u{2019} c").trim(), "a");
        assert!(strip_tags("<p>\u{2019}quoted\u{2019}</p>").contains('\u{2019}'));
        // Uppercase and attribute-bearing forms close too.
        let t2 = strip_tags("<p>a</p><STYLE type=\"text/css\">.x{color:red}</STYLE><p>b</p>");
        assert!(!t2.contains("color"), "{t2:?}");
        assert!(t2.contains('a') && t2.contains('b'), "{t2:?}");
    }
}
