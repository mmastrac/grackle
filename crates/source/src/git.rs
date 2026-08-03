//! git as a metadata provider: each tracked file's last non-merge commit.
//!
//! Cached by HEAD. Git data changes only when a commit lands, so HEAD is the
//! whole key: a file's last commit cannot move while HEAD stands, whatever its
//! mtime does. Repeated builds between commits reuse the cache and never walk
//! history. (A content-derived overlay like outbound links would cache by the
//! row's `version`, the mtime/size fingerprint, instead: the other keying
//! layer, for when such an overlay exists.)

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The last commit that touched a file, overlaid as `.git.*` when
/// `metadata = ["git"]` is set.
#[derive(Clone, Debug)]
pub struct GitMeta {
    pub commit: String,
    /// Committer date, `YYYY-MM-DD`, the shape a `date` field takes.
    pub date: String,
    pub author: String,
    pub subject: String,
}

/// [`last_commits`], but served from `cache_dir` while HEAD is unchanged and
/// the history walk skipped. A tree with no HEAD (no repo) falls through
/// uncached.
pub fn last_commits_cached(root: &Path, cache_dir: &Path) -> HashMap<PathBuf, GitMeta> {
    let Some(head) = head(root) else {
        return last_commits(root);
    };
    if let Some((cached_head, map)) = read_cache(cache_dir) {
        if cached_head == head {
            return map;
        }
    }
    let map = last_commits(root);
    write_cache(cache_dir, &head, &map);
    map
}

fn head(root: &Path) -> Option<String> {
    run(root, &["rev-parse", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `<head>\n` then one `rel\x1fcommit\x1fdate\x1fauthor\x1fsubject` line per
/// file. The unit separator cannot appear in a path or a one-line subject.
fn cache_file(dir: &Path) -> PathBuf {
    dir.join("git.txt")
}

fn read_cache(dir: &Path) -> Option<(String, HashMap<PathBuf, GitMeta>)> {
    let text = std::fs::read_to_string(cache_file(dir)).ok()?;
    let mut lines = text.lines();
    let head = lines.next()?.to_string();
    let mut map = HashMap::new();
    for line in lines {
        let mut f = line.split('\u{1f}');
        let (Some(rel), Some(commit), Some(date), Some(author), Some(subject)) =
            (f.next(), f.next(), f.next(), f.next(), f.next())
        else {
            continue;
        };
        map.insert(
            PathBuf::from(rel),
            GitMeta {
                commit: commit.into(),
                date: date.into(),
                author: author.into(),
                subject: subject.into(),
            },
        );
    }
    Some((head, map))
}

fn write_cache(dir: &Path, head: &str, map: &HashMap<PathBuf, GitMeta>) {
    let mut out = format!("{head}\n");
    for (rel, m) in map {
        let _ = writeln!(
            out,
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            rel.display(),
            m.commit,
            m.date,
            m.author,
            m.subject
        );
    }
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(cache_file(dir), out);
}

/// Every tracked file's last non-merge commit, root-relative. Empty when the
/// tree is no usable git repo (no git, no repo, or shallow). A shallow clone
/// warns: its truncated history would report one commit for every file, and
/// the site asked for the metadata, so silence would ship wrong dates.
pub fn last_commits(root: &Path) -> HashMap<PathBuf, GitMeta> {
    if is_shallow(root) {
        eprintln!(
            "grackle: metadata = [\"git\"], but {} is a shallow clone; git \
             metadata omitted. Fetch full history to enable it.",
            root.display()
        );
        return HashMap::new();
    }
    // One walk, newest first: the first commit seen for a path is the last that
    // touched it. `--relative` scopes paths to the site root (which may sit
    // below the repo root) and reports them relative to it, matching `row.rel`.
    // RS (0x1e) opens each record, US (0x1f) splits its fields.
    let args = [
        "log",
        "--no-merges",
        "--name-only",
        "--relative",
        "--format=%x1e%H%x1f%cs%x1f%an%x1f%s",
    ];
    run(root, &args).map(|out| parse(&out)).unwrap_or_default()
}

/// Fold `git log` output into a path -> last-commit map. Records open with RS
/// (0x1e); a record is its `%H\x1f%cs\x1f%an\x1f%s` header, then one path per
/// line. Newest first, so the first record naming a path is the last commit to
/// touch it: `or_insert` keeps it.
fn parse(out: &str) -> HashMap<PathBuf, GitMeta> {
    let mut map = HashMap::new();
    for rec in out.split('\u{1e}') {
        let mut lines = rec.split('\n');
        let mut head = lines.next().unwrap_or_default().split('\u{1f}');
        let (Some(commit), Some(date), Some(author), Some(subject)) =
            (head.next(), head.next(), head.next(), head.next())
        else {
            continue;
        };
        let meta = GitMeta {
            commit: commit.to_string(),
            date: date.to_string(),
            author: author.to_string(),
            subject: subject.to_string(),
        };
        for f in lines {
            let f = f.trim();
            if !f.is_empty() {
                map.entry(PathBuf::from(f)).or_insert_with(|| meta.clone());
            }
        }
    }
    map
}

fn is_shallow(root: &Path) -> bool {
    run(root, &["rev-parse", "--is-shallow-repository"]).is_some_and(|s| s.trim() == "true")
}

fn run(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_commit_per_path_wins() {
        // Two records, newest first. `a.md` appears in both, so the newer wins.
        let out = "\u{1e}h1\u{1f}2026-07-20\u{1f}Ada\u{1f}fix a\n\
                   a.md\nb.md\n\n\
                   \u{1e}h0\u{1f}2026-01-01\u{1f}Ada\u{1f}add a\n\
                   a.md\n";
        let m = parse(out);
        assert_eq!(m.len(), 2);
        assert_eq!(m[&PathBuf::from("a.md")].date, "2026-07-20");
        assert_eq!(m[&PathBuf::from("a.md")].commit, "h1");
        assert_eq!(m[&PathBuf::from("a.md")].subject, "fix a");
        assert_eq!(m[&PathBuf::from("b.md")].date, "2026-07-20");
    }

    #[test]
    fn empty_output_is_empty_map() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn cache_round_trips() {
        let dir = std::env::temp_dir().join("grackle-git-cache-round-trip");
        let _ = std::fs::remove_dir_all(&dir);
        let mut map = HashMap::new();
        map.insert(
            PathBuf::from("a.md"),
            GitMeta {
                commit: "h1".into(),
                date: "2026-07-20".into(),
                author: "Ada".into(),
                subject: "fix a".into(),
            },
        );
        write_cache(&dir, "HEADSHA", &map);
        let (head, back) = read_cache(&dir).expect("the cache reads back");
        assert_eq!(head, "HEADSHA");
        assert_eq!(back[&PathBuf::from("a.md")].date, "2026-07-20");
        assert_eq!(back[&PathBuf::from("a.md")].subject, "fix a");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
