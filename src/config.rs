use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_root")]
    pub root: PathBuf,
    /// Honour .gitignore when walking (default true). It is the site's existing
    /// declaration of what is not content; see store::walker.
    #[serde(default = "default_true")]
    pub gitignore: bool,
    pub site: Site,
    #[serde(default)]
    pub collections: BTreeMap<String, Collection>,
    #[serde(default)]
    pub views: BTreeMap<String, View>,
    /// Marker filename -> defaults it applies to its directory and below.
    /// The config says what a marker means; the tree says where (DESIGN.md §4b).
    #[serde(default)]
    pub markers: BTreeMap<String, BTreeMap<String, toml::Value>>,
    #[serde(skip)]
    pub dir: PathBuf,
}

fn default_root() -> PathBuf {
    PathBuf::from(".")
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Site {
    pub url: String,
    #[serde(default)]
    pub baseurl: String,
    pub title: String,
    pub author: String,
    /// The feed's `<author><email>`; omitted from the feed when absent.
    pub email: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Posts,
    Tree,
    Objects,
}

#[derive(Debug, Deserialize)]
pub struct Collection {
    pub kind: Kind,
    pub source: Option<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    pub bucket: Option<String>,
    #[serde(default)]
    pub filename_formats: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    #[serde(rename = "match")]
    pub pattern: String,
    pub route: Option<String>,
    /// Gate the rule on front-matter presence. This is what separates a Jekyll
    /// *page* (rendered, pretty URL) from a static file (copied verbatim).
    pub front_matter: Option<bool>,
    #[serde(default)]
    pub defaults: BTreeMap<String, toml::Value>,
}

/// A view is a *query* plus, optionally, a *materialization*.
///
/// The two were welded together until the home page needed the top 3 posts
/// with no route of its own (DESIGN.md §5c). Splitting them gives three shapes:
///
///   * query only (no route, no layout) — a named set, e.g. `published`
///   * query + layout, no route         — embeddable, e.g. `latest`
///   * query + layout + route(s)        — materialized, e.g. `blog_index`
#[derive(Debug, Deserialize)]
pub struct View {
    /// A collection name, another view's name, or `*` for the route set.
    pub over: String,
    pub filter: Option<String>,
    pub group_by: Option<String>,
    pub paginate: Option<usize>,
    pub route: Option<String>,
    #[serde(default)]
    pub routes: Vec<String>,
    pub layout: Option<String>,
    pub limit: Option<usize>,
    pub template: Option<String>,
}

impl View {
    /// A view with nothing but a query — nothing to inherit ambiguously, which
    /// is why it is the only thing `over` may name.
    pub fn is_query_only(&self) -> bool {
        self.route.is_none()
            && self.routes.is_empty()
            && self.layout.is_none()
            && self.template.is_none()
            && self.paginate.is_none()
            && self.limit.is_none()
            && self.group_by.is_none()
    }

    /// Whether this view materializes routes of its own.
    pub fn is_materialized(&self) -> bool {
        self.route.is_some() || !self.routes.is_empty()
    }
}

/// A view's query, with the `over` chain flattened.
#[derive(Debug)]
pub struct Query {
    /// Collection name, or `*`.
    pub base: String,
    /// Every filter along the chain, outermost view last. All must hold.
    pub filters: Vec<String>,
}

impl Query {
    /// The conjunction of the chain, or None when nothing filters.
    pub fn predicate(&self) -> Option<String> {
        match self.filters.len() {
            0 => None,
            1 => Some(self.filters[0].clone()),
            _ => Some(
                self.filters
                    .iter()
                    .map(|f| format!("({f})"))
                    .collect::<Vec<_>>()
                    .join(" && "),
            ),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text)
            .with_context(|| format!("parsing config {}", path.display()))?;
        cfg.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        Ok(cfg)
    }

    /// Flatten a view's `over` chain into a base collection plus every filter
    /// along the way.
    ///
    /// `over` may only name a **query-only** view. That restriction is the
    /// whole reason this is simple: composing over `blog_index` would raise
    /// "is `paginate = 5` inherited?", and every answer surprises someone.
    pub fn query(&self, name: &str) -> Result<Query> {
        let mut filters = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        let mut cur = name;
        loop {
            let v = self
                .views
                .get(cur)
                .with_context(|| format!("view {name}: `over` names unknown view {cur:?}"))?;
            if seen.contains(&cur) {
                anyhow::bail!("view {name}: `over` chain is cyclic at {cur:?}");
            }
            seen.push(cur);
            if let Some(f) = &v.filter {
                filters.push(f.clone());
            }
            // A collection or `*` terminates the chain.
            let Some(next) = self.views.get(v.over.as_str()) else {
                if v.over != "*" && !self.collections.contains_key(&v.over) {
                    anyhow::bail!(
                        "view {name}: `over = {:?}` is neither a collection nor a view",
                        v.over
                    );
                }
                filters.reverse();
                return Ok(Query {
                    base: v.over.clone(),
                    filters,
                });
            };
            if !next.is_query_only() {
                anyhow::bail!(
                    "view {name}: `over = {:?}` names a view that is not query-only. \
                     Only views with no route/layout/paginate/limit/group_by may be composed over.",
                    v.over
                );
            }
            cur = &v.over;
        }
    }

    /// Site root, resolved relative to the config file's directory.
    pub fn root(&self) -> PathBuf {
        let joined = self.dir.join(&self.root);
        std::fs::canonicalize(&joined).unwrap_or(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(views: &str) -> Config {
        let src = format!(
            "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [collections.blog]\nkind = \"posts\"\nsource = \"_posts\"\n{views}"
        );
        toml::from_str(&src).expect("test config should parse")
    }

    #[test]
    fn chain_flattens_and_conjoins_filters() {
        let c = cfg(r#"
            [views.published]
            over = "blog"
            filter = "!draft && !hidden"

            [views.latest]
            over = "published"
            filter = "!noindex"
            limit = 3
        "#);
        let q = c.query("latest").unwrap();
        assert_eq!(q.base, "blog");
        // Outermost last, and every link in the chain must hold.
        assert_eq!(
            q.predicate().unwrap(),
            "(!draft && !hidden) && (!noindex)"
        );
    }

    #[test]
    fn single_filter_is_not_parenthesised() {
        let c = cfg("[views.published]\nover = \"blog\"\nfilter = \"!draft\"\n");
        assert_eq!(c.query("published").unwrap().predicate().unwrap(), "!draft");
    }

    #[test]
    fn unfiltered_chain_has_no_predicate() {
        let c = cfg("[views.all]\nover = \"blog\"\n");
        assert!(c.query("all").unwrap().predicate().is_none());
    }

    /// The rule that keeps composition from needing inheritance semantics.
    #[test]
    fn composing_over_a_materialized_view_is_an_error() {
        let c = cfg(r#"
            [views.blog_index]
            over = "blog"
            filter = "!draft"
            paginate = 5
            routes = ["/blog/"]

            [views.latest]
            over = "blog_index"
            limit = 3
        "#);
        let e = c.query("latest").unwrap_err().to_string();
        assert!(e.contains("query-only"), "unexpected error: {e}");
    }

    #[test]
    fn unknown_over_is_an_error() {
        let c = cfg("[views.latest]\nover = \"pubished\"\nlimit = 3\n");
        let e = c.query("latest").unwrap_err().to_string();
        assert!(
            e.contains("neither a collection nor a view"),
            "unexpected error: {e}"
        );
    }

    #[test]
    fn cyclic_chain_terminates() {
        let c = cfg("[views.a]\nover = \"b\"\n\n[views.b]\nover = \"a\"\n");
        let e = c.query("a").unwrap_err().to_string();
        assert!(e.contains("cyclic"), "unexpected error: {e}");
    }
}
