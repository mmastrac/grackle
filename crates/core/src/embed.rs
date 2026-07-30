//! Embeddings → related posts (DESIGN §6b: "this retires LSI").
//!
//! Each post embeds as the text `[schema.embeddings].string` renders over the
//! row (plus `{body}`) — so which fields carry signal is config, and a change
//! to that string re-embeds (it is part of the cache key). Vectors cache by
//! content hash; an `index.json` maps post name → current hash so a post
//! whose text changed can keep serving its **stale** vector until the
//! background pass re-embeds it — stale-while-revalidate, the resident
//! database's move (§7). Similarity is brute-force cosine over ~327 vectors;
//! a vector index at this scale would be the classic mistake.
//!
//! Ranking policy for the `grackle query similar` diagnostic lives in
//! [`RankPolicy`]. Site rendering no longer ranks here — §6g's relation engine
//! does, via the `embedding_similarity` score function — so `[related]`
//! retired from config; this is the raw-order inspector only.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::model::SiteDb;

const DIM: usize = 384;

pub type Vector = Vec<f32>;

/// How `grackle query similar` ranks: a candidate limit and the same optional
/// recency shaping the old `[related]` block carried. Not config any more —
/// the one caller (the CLI) builds it — but kept as a struct so the diagnostic
/// can still show a year-penalised order if asked.
#[derive(Debug, Clone, Copy)]
pub struct RankPolicy {
    pub limit: usize,
    pub min_score: Option<f32>,
    pub year_penalty: Option<f32>,
    pub max_years: Option<i32>,
}

impl Default for RankPolicy {
    fn default() -> Self {
        RankPolicy {
            limit: 4,
            min_score: None,
            year_penalty: None,
            max_years: None,
        }
    }
}

/// The cache identity of a row: its source path, root-relative. `rel` is
/// unique across collections, which is what makes it usable as a key.
pub fn cache_key(p: &crate::model::Row) -> String {
    p.rel.to_string_lossy().to_string()
}

/// The text a post embeds as — `[schema.embeddings].string` over the row.
pub fn text_of(cfg: &Config, p: &crate::model::Row, body: &str) -> Result<String> {
    cfg.embedding_text(p, body)
}

/// A post whose embedding is out of date (or absent): what the background
/// pass needs to bring the cache current.
pub struct Pending {
    pub name: String,
    pub hash: String,
    pub text: String,
}

/// Cache-only load: every post's best-available vector. `vectors[i]` is the
/// fresh embedding when the cache has one for the current text, else the
/// stale one the index remembers, else None (new post, first sight). Never
/// touches the model; `pending` lists what a background pass should embed.
pub struct Loaded {
    pub vectors: Vec<Option<Vector>>,
    pub pending: Vec<Pending>,
}

pub fn load(db: &SiteDb, cfg: &Config, cache_dir: &Path) -> Result<Loaded> {
    std::fs::create_dir_all(cache_dir)?;
    let index = read_index(cache_dir);
    let mut vectors = Vec::with_capacity(db.post_ix.len());
    let mut pending = Vec::new();

    for p in db.posts() {
        let text = text_of(cfg, p, &crate::store::read_body(&p.path)?)?;
        if text.trim().is_empty() {
            vectors.push(None);
            continue;
        }
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let fresh = read_vec(&vec_path(cache_dir, &hash));
        if fresh.is_none() {
            // Stale fallback: the vector this post's *previous* text produced.
            let stale = index
                .get(&cache_key(p))
                .and_then(|old| read_vec(&vec_path(cache_dir, old)));
            vectors.push(stale);
            pending.push(Pending {
                name: cache_key(p),
                hash,
                text,
            });
        } else {
            vectors.push(fresh);
        }
    }
    Ok(Loaded { vectors, pending })
}

/// The blocking model pass: embed everything pending, write the vectors, and
/// point the index at them. Callers choose *when* — `build` runs it before
/// rendering (published output is always fresh); `serve` runs it on a
/// background thread and re-renders on completion.
pub fn embed_pending(cache_dir: &Path, pending: &[Pending]) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let mut model = load_model(cache_dir)?;
    let texts: Vec<&str> = pending.iter().map(|p| p.text.as_str()).collect();
    let embedded = model
        .embed(texts, None)
        .map_err(|e| anyhow::anyhow!("embedding {} posts: {e}", pending.len()))?;
    let mut index = read_index(cache_dir);
    for (p, mut v) in pending.iter().zip(embedded) {
        normalize(&mut v);
        write_vec(&vec_path(cache_dir, &p.hash), &v)
            .with_context(|| format!("writing embedding for {}", p.name))?;
        index.insert(p.name.clone(), p.hash.clone());
    }
    write_index(cache_dir, &index)?;
    Ok(())
}

/// Load, embed whatever is pending (blocking), and reload — the "fresh now"
/// path: AOT builds and CLI queries.
pub fn fresh(db: &SiteDb, cfg: &Config, cache_dir: &Path) -> Result<Vec<Option<Vector>>> {
    let l = load(db, cfg, cache_dir)?;
    if l.pending.is_empty() {
        return Ok(l.vectors);
    }
    embed_pending(cache_dir, &l.pending)?;
    Ok(load(db, cfg, cache_dir)?.vectors)
}

/// Related-posts table: post index → top-N (index, adjusted score), best
/// first, under the config's policy.
pub struct Related {
    pub by_post: HashMap<crate::model::Key, Vec<(crate::model::Key, f32)>>,
}

pub fn rank(db: &SiteDb, vectors: &[Option<Vector>], cfg: &RankPolicy) -> Related {
    use chrono::Datelike;
    // `vectors` is parallel to `post_ix`, so a position here names a POST,
    // not a row — indexing `db.rows` with it is wrong.
    let keys = &db.post_ix;
    let row = |i: usize| keys.get(i).and_then(|k| db.rows.get(k));
    let year = |i: usize| row(i).and_then(|r| r.as_date("date")).map(|d| d.year());
    let mut by_post = HashMap::new();
    for (i, vi) in vectors.iter().enumerate() {
        let Some(vi) = vi else { continue };
        // A post never matches itself — its own vector is cosine 1.0 and
        // would top every list if this filter regressed (pinned by test).
        let mut scored: Vec<(usize, f32)> = vectors
            .iter()
            .enumerate()
            .filter(|(j, vj)| *j != i && vj.is_some())
            // q53: similarity does not cross an axis. Twins of this row
            // (translation, other file-axis form) share a non-empty `logical`
            // and nearly identical text — they would otherwise top the list.
            // Route-only axes (theme, …) are the same row, already excluded by
            // `j != i`. Empty logical means no pairing (fixture / monolingual).
            .filter(|(j, _)| match (row(i), row(*j)) {
                (Some(a), Some(b)) => {
                    a.logical.is_empty() || b.logical.is_empty() || a.logical != b.logical
                }
                _ => false,
            })
            .filter_map(|(j, vj)| {
                let gap = match (year(i), year(j)) {
                    (Some(a), Some(b)) => (a - b).abs(),
                    _ => 0,
                };
                if cfg.max_years.is_some_and(|m| gap > m) {
                    return None;
                }
                let score =
                    dot(vi, vj.as_ref().unwrap()) - cfg.year_penalty.unwrap_or(0.0) * gap as f32;
                if cfg.min_score.is_some_and(|m| score < m) {
                    return None;
                }
                Some((j, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(cfg.limit);
        let Some(mine) = keys.get(i) else { continue };
        by_post.insert(
            mine.clone(),
            scored
                .into_iter()
                .filter_map(|(j, s)| keys.get(j).map(|k| (k.clone(), s)))
                .collect(),
        );
    }
    Related { by_post }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn load_model(cache_dir: &Path) -> Result<fastembed::TextEmbedding> {
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
    let models = cache_dir.parent().unwrap_or(cache_dir).join("models");
    TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_cache_dir(models)
            .with_show_download_progress(true),
    )
    .map_err(|e| anyhow::anyhow!("loading embedding model: {e}"))
}

fn normalize(v: &mut [f32]) {
    let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}

fn vec_path(cache_dir: &Path, hash: &str) -> PathBuf {
    cache_dir.join(format!("{hash}.vec"))
}

fn index_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("index.json")
}

fn read_index(cache_dir: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(index_path(cache_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_index(cache_dir: &Path, index: &HashMap<String, String>) -> Result<()> {
    std::fs::write(index_path(cache_dir), serde_json::to_string_pretty(index)?)?;
    Ok(())
}

fn read_vec(path: &Path) -> Option<Vector> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() != DIM * 4 {
        return None; // stale/foreign cache entry: re-embed
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

fn write_vec(path: &Path, v: &[f32]) -> Result<()> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for x in v {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Row;

    fn post(title: &str, year: i32, v: Option<Vec<f32>>) -> (Row, Option<Vector>) {
        let mut p = Row {
            title: Some(title.into()),
            // Distinct, because a row's key is its path and these rows have
            // to be tellable apart once ranking names them.
            rel: std::path::PathBuf::from(format!("{title}.md")),
            ..Row::default()
        };
        p.fields.insert(
            "date".into(),
            grackle_db::Value::Str(format!("{year}-01-01")),
        );
        (p, v)
    }

    fn mkdb(posts: Vec<Row>) -> SiteDb {
        SiteDb::seed(posts, true)
    }

    /// A seeded post's key, which `seed` derives from its `rel`.
    fn key(title: &str) -> crate::model::Key {
        crate::model::Key::new(format!("{title}.md"))
    }

    fn embed_cfg() -> crate::config::Config {
        crate::config::Config::from_toml(
            "extends = \"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource=\"_posts\"\n\
             [[collections.rules]]\nmatch=\"**\"\nroute=\"/{slug}/\"\n\
             [schema.embeddings]\n\
             string = \"title: {title} tags: {tags} body: {body}\"\n",
        )
        .unwrap()
    }

    #[test]
    fn embed_text_carries_title_and_tags() {
        let p = Row {
            title: Some("T".into()),
            fields: [(
                "tags".into(),
                grackle_db::Value::List(vec!["a".into(), "b".into()]),
            )]
            .into(),
            ..Row::default()
        };
        assert_eq!(
            text_of(&embed_cfg(), &p, "hello").unwrap(),
            "title: T tags: a, b body: hello"
        );
    }

    #[test]
    fn year_penalty_reorders_and_min_score_drops() {
        // Anchor (2020) vs near-identical 2004 post and slightly-less-similar
        // 2019 post: raw cosine prefers 2004; the penalty flips it, and
        // min_score can drop the old one entirely.
        let a = vec![1.0, 0.0];
        let near = vec![0.999, 0.045]; // ~0.999 cosine, year 2004
        let close = vec![0.97, 0.24]; // ~0.97 cosine, year 2019
        let (p0, v0) = post("anchor", 2020, Some(a));
        let (p1, v1) = post("old", 2004, Some(near));
        let (p2, v2) = post("recent", 2019, Some(close));
        let db = mkdb(vec![p0, p1, p2]);
        let vecs = vec![v0, v1, v2];

        let raw = rank(
            &db,
            &vecs,
            &RankPolicy {
                limit: 2,
                ..Default::default()
            },
        );
        assert_eq!(
            raw.by_post[&key("anchor")][0].0,
            key("old"),
            "raw cosine prefers the old post"
        );

        let pen = RankPolicy {
            limit: 2,
            year_penalty: Some(0.01),
            ..Default::default()
        };
        let penalized = rank(&db, &vecs, &pen);
        assert_eq!(
            penalized.by_post[&key("anchor")][0].0,
            key("recent"),
            "penalty prefers the recent post"
        );

        let strict = RankPolicy {
            limit: 2,
            year_penalty: Some(0.01),
            min_score: Some(0.9),
            ..Default::default()
        };
        let dropped = rank(&db, &vecs, &strict);
        assert_eq!(
            dropped.by_post[&key("anchor")].len(),
            1,
            "the penalized old post falls below min"
        );
    }

    #[test]
    fn a_post_never_matches_itself() {
        // Identical vectors: self would be the perfect match (cosine 1.0)
        // if the exclusion ever regressed.
        let v = vec![1.0, 0.0];
        let (p0, v0) = post("a", 2020, Some(v.clone()));
        let (p1, v1) = post("b", 2020, Some(v));
        let db = mkdb(vec![p0, p1]);
        let r = rank(&db, &[v0, v1], &RankPolicy::default());
        for (i, list) in &r.by_post {
            assert!(list.iter().all(|(j, _)| j != i), "post {i} matched itself");
        }
        assert_eq!(
            r.by_post[&key("a")][0].0,
            key("b"),
            "the twin ranks; the self does not"
        );
    }

    #[test]
    fn axis_twins_do_not_rank_each_other() {
        // Same logical identity, identical vectors: the translation would be
        // cosine 1.0 if the axis gate ever regressed.
        let v = vec![1.0, 0.0];
        let (mut en, v0) = post("a", 2020, Some(v.clone()));
        en.logical = "a".into();
        let (mut fr, v1) = post("a.fr", 2020, Some(v.clone()));
        fr.logical = "a".into();
        let (other, v2) = post("b", 2020, Some(v));
        let db = mkdb(vec![en, fr, other]);
        let r = rank(&db, &[v0, v1, v2], &RankPolicy::default());
        let en_hits = &r.by_post[&key("a")];
        assert!(
            en_hits.iter().all(|(k, _)| k != &key("a.fr")),
            "a translation must not rank: {en_hits:?}"
        );
        assert_eq!(en_hits[0].0, key("b"));
    }

    #[test]
    fn max_years_is_a_hard_cap() {
        let (p0, v0) = post("a", 2020, Some(vec![1.0, 0.0]));
        let (p1, v1) = post("b", 2004, Some(vec![1.0, 0.0]));
        let db = mkdb(vec![p0, p1]);
        let cfg = RankPolicy {
            limit: 4,
            max_years: Some(10),
            ..Default::default()
        };
        let r = rank(&db, &[v0, v1], &cfg);
        assert!(r.by_post[&key("a")].is_empty());
    }

    #[test]
    fn stale_vectors_serve_until_reprocessed() {
        let dir = std::env::temp_dir().join(format!("grackle-embed-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // A post whose OLD text was embedded and indexed. The body lives on
        // disk — no row holds one — so the fixture writes a real file.
        let src = dir.join("a.md");
        std::fs::write(&src, "---\ntitle: x\n---\nbody").unwrap();
        let mut p = Row {
            path: src.clone(),
            rel: std::path::PathBuf::from("_posts/2020/a.md"),
            title: Some("old title".into()),
            ..Row::default()
        };
        let old_hash = blake3::hash(text_of(&embed_cfg(), &p, "body").unwrap().as_bytes())
            .to_hex()
            .to_string();
        let v: Vec<f32> = (0..DIM).map(|i| i as f32).collect();
        write_vec(&vec_path(&dir, &old_hash), &v).unwrap();
        write_index(&dir, &HashMap::from([(cache_key(&p), old_hash)])).unwrap();

        // …then edited: load serves the stale vector and queues a re-embed.
        p.title = Some("new title".into());
        let db = mkdb(vec![p]);
        let loaded = load(&db, &embed_cfg(), &dir).unwrap();
        assert_eq!(
            loaded.vectors[0].as_ref().unwrap(),
            &v,
            "stale vector served"
        );
        assert_eq!(loaded.pending.len(), 1, "re-embed queued");
        assert!(loaded.pending[0].text.contains("new title"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_length_cache_entries_are_rejected() {
        let dir = std::env::temp_dir().join("grackle-embed-test");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("bad.vec");
        std::fs::write(&p, [0u8; 12]).unwrap();
        assert!(read_vec(&p).is_none());
        let _ = std::fs::remove_file(&p);
    }
}
