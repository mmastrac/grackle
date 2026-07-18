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
    /// Custom block widgets (§5d): `{% name %}…{% endname %}` expands to the
    /// wrapper template with the markdown body spliced at `{body}`. Adding a
    /// widget is one config entry, no code.
    #[serde(default)]
    pub widgets: BTreeMap<String, String>,
    /// Related-posts ranking policy (§6b). Cosine similarity supplies the
    /// candidates; this shapes them per site.
    #[serde(default)]
    pub related: RelatedCfg,
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
    /// §6a bubble+bucket asset resolution names this directory; declared
    /// ahead of the code that consumes it (deferred with the q26 pass).
    #[allow(dead_code)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub filename_formats: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// The collection's crumb identity: what it contributes to breadcrumb
    /// trails (§5c provenance — the chain roots at the collection), and
    /// where that crumb links.
    pub crumb: Option<String>,
    pub index: Option<String>,
    /// The view whose subdivision chain forms this collection's row trails
    /// (e.g. `monthly_archive` → Home > Blog > 2022 > December > 16).
    pub trail: Option<String>,
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
    /// Path-glob scoping (§5 audit): globs already exist in rules (§4), so
    /// view scoping reuses them rather than growing the filter language a
    /// path operator. Matched against the row's root-relative path.
    #[serde(rename = "match")]
    pub scope: Option<String>,
    /// Explicit ordering for rows that have no natural one (§5 audit:
    /// posts sort reverse-chronologically by construction; objects don't).
    /// `"name"` is the only value so far. Declared, not defaulted — the
    /// corpus's zero-padding making lexical order correct is luck.
    pub order_by: Option<String>,
    pub group_by: Option<String>,
    pub paginate: Option<usize>,
    pub route: Option<String>,
    #[serde(default)]
    pub routes: Vec<String>,
    pub layout: Option<String>,
    /// Fragment variant (q24): the theme renders this view through
    /// `{kind}--{variant}.html` when it ships one, falling back to the
    /// kind's base fragment. How `/books/` gets cards while `/blog/`
    /// stays textual, both being listings.
    pub variant: Option<String>,
    /// Fill the listing's `featured` slot with the first row (q36) — the
    /// book-of-the-month shape. Most listings leave it off.
    #[serde(default)]
    pub featured: bool,
    /// The view's outermost serialization (Matt, 2026-07): `"atom"` and
    /// `"sitemap"` are built-in XML shells — the feed is not a special
    /// pass, it is the same rows in a different wrapper. Absent = the
    /// HTML shell (theme). The full generalization is q44.
    pub shell: Option<String>,
    pub limit: Option<usize>,
    pub template: Option<String>,
    /// Listing title, as a template over the route's group params
    /// (`"{year} {month_name}"`, `"Posts Tagged “{key}”"`). Same placeholder
    /// language as routes, same load-time discipline.
    pub title: Option<String>,
    /// What this view contributes to descendants' breadcrumb trails.
    /// Defaults to `title`.
    pub crumb: Option<String>,
    /// Computed fields (§6d): columns this view adds to its rows, each
    /// defined by a deriver. Views composed `over` this one inherit them —
    /// fields flow with rows through query composition the way filters do —
    /// and redeclaring a name overrides (nearest wins). The field named
    /// `summary` is what listing previews consume.
    #[serde(default)]
    pub fields: BTreeMap<String, Field>,
}

/// One computed field: exactly one deriver names how the value is computed
/// from the row. `deny_unknown_fields` makes an unknown deriver a parse
/// error naming the known ones.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    /// The row's content blocks, kept until a budget runs out (block
    /// granularity, at least one block; `max_chars` counts visible text).
    /// Carries a `truncated` fact for the theme's ★.
    pub truncate: Option<Truncate>,
}

/// `[related]`: how embedding similarity becomes a related-posts list.
/// `year_penalty` subtracts per year of date distance (a soft prior toward
/// contemporaries); `max_years` is a hard cap; `min_score` drops weak
/// matches after adjustment — a 2004 post is probably not relevant on this
/// blog, but might be on another, so all of it is per-site policy.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RelatedCfg {
    pub limit: usize,
    pub min_score: Option<f32>,
    pub year_penalty: Option<f32>,
    pub max_years: Option<i32>,
}

impl Default for RelatedCfg {
    fn default() -> Self {
        RelatedCfg { limit: 4, min_score: None, year_penalty: None, max_years: None }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Truncate {
    pub max_blocks: Option<usize>,
    pub max_chars: Option<usize>,
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
        for (vname, v) in &cfg.views {
            for (fname, f) in &v.fields {
                if f.truncate.is_none() {
                    anyhow::bail!(
                        "view {vname}: field {fname:?} declares no deriver \
                         (have: truncate)"
                    );
                }
            }
        }
        for (name, tmpl) in &cfg.widgets {
            if !tmpl.contains("{body}") {
                anyhow::bail!(
                    "widget {name:?}: wrapper template has no {{body}} hole, \
                     so the author's markdown would be dropped"
                );
            }
        }
        for (vname, v) in &cfg.views {
            if let Some(s) = v.shell.as_deref() {
                if !matches!(s, "atom" | "sitemap") {
                    anyhow::bail!(
                        "view {vname}: unknown shell {s:?} (built-in shells: atom, sitemap)"
                    );
                }
            }
        }
        Ok(cfg)
    }

    /// Flatten a view's `over` chain into a base collection plus every filter
    /// along the way.
    ///
    /// `over` may name a **query-only** view (nothing to inherit ambiguously)
    /// or a **grouped, unpaginated** view — subdivision (§5c): the composer
    /// refines the parent's partition, so it must itself be grouped, and the
    /// parent's route/layout are *not* inherited (the child declares its own).
    /// Composing over a paginated view is punted (open question 30): a
    /// pageable year with months on its root raises a URL-namespace question
    /// we haven't answered.
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
                let subdividable = next.group_by.is_some()
                    && next.paginate.is_none()
                    && next.limit.is_none()
                    && next.template.is_none();
                if !subdividable {
                    anyhow::bail!(
                        "view {name}: `over = {:?}` names a view that is neither query-only \
                         nor grouped. Only query-only views and grouped unpaginated views \
                         (subdivision, §5c) may be composed over; pagination × subdivision \
                         is punted (open question 30).",
                        v.over
                    );
                }
                if v.group_by.is_none() {
                    anyhow::bail!(
                        "view {name}: `over = {:?}` names a grouped view, but {name} has no \
                         `group_by`. Composing over a grouped view means subdividing its \
                         partition (§5c), so the composer must be grouped too.",
                        v.over
                    );
                }
            }
            cur = &v.over;
        }
    }

    /// The `over` chain from `name` down to its base, nearest view first.
    /// The one chain walker — everything derived from composition
    /// (`fields_for`, `group_specs`, `grouped_chain`) reads this. Assumes the
    /// chain is acyclic, which `query()` validated at load.
    pub fn chain<'a: 'b, 'b>(&'a self, name: &'b str) -> Vec<(&'b str, &'a View)> {
        let mut out = Vec::new();
        let mut cur = name;
        while let Some(v) = self.views.get(cur) {
            out.push((cur, v));
            cur = &v.over;
        }
        out
    }

    /// The `group_by` specs governing a view, outermost ancestor first. This
    /// is subdivision (§5c): a grouped view `over` a grouped view refines the
    /// parent's partition, so the parent's spec applies before the child's.
    pub fn group_specs(&self, name: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .chain(name)
            .iter()
            .filter_map(|(_, v)| v.group_by.clone())
            .collect();
        v.reverse();
        v
    }

    /// The grouped views forming a view's subdivision chain, outermost first
    /// — the provenance axis breadcrumb trails walk (§5c).
    pub fn grouped_chain(&self, name: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .chain(name)
            .iter()
            .filter(|(_, v)| v.group_by.is_some())
            .map(|(n, _)| n.to_string())
            .collect();
        v.reverse();
        v
    }

    /// The computed-field set a view's rows carry: the union along the
    /// `over` chain, nearest declaration winning per name — fields compose
    /// exactly as filters do (§5c). Declaring `fields.summary` once on a
    /// shared query view (`published`) covers every listing composed over
    /// it; a view wanting different budgets redeclares the field.
    pub fn fields_for(&self, view: &str) -> BTreeMap<&str, &Field> {
        let mut out: BTreeMap<&str, &Field> = BTreeMap::new();
        for (_, v) in self.chain(view) {
            for (name, f) in &v.fields {
                out.entry(name.as_str()).or_insert(f);
            }
        }
        out
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

    /// Subdivision (§5c): a grouped view may compose over a grouped view —
    /// the filters flatten straight through it.
    #[test]
    fn grouped_over_grouped_is_subdivision() {
        let c = cfg(r#"
            [views.yearly]
            over = "blog"
            filter = "!draft"
            group_by = "date.year"
            route = "/blog/{year}/"

            [views.monthly]
            over = "yearly"
            group_by = "date.month"
            route = "/blog/{year}/{month:02}/"
        "#);
        let q = c.query("monthly").unwrap();
        assert_eq!(q.base, "blog");
        assert_eq!(q.predicate().unwrap(), "!draft");
    }

    /// Only subdivision is defined: a non-grouped view over a grouped one
    /// has no meaning (yet), and pagination × subdivision is punted (q30).
    #[test]
    fn non_grouped_over_grouped_is_an_error() {
        let c = cfg(r#"
            [views.yearly]
            over = "blog"
            group_by = "date.year"
            route = "/blog/{year}/"

            [views.latest]
            over = "yearly"
            limit = 3
        "#);
        let e = c.query("latest").unwrap_err().to_string();
        assert!(e.contains("subdividing"), "unexpected error: {e}");
    }

    #[test]
    fn subdividing_a_paginated_view_is_punted() {
        let c = cfg(r#"
            [views.yearly]
            over = "blog"
            group_by = "date.year"
            paginate = 10
            route = "/blog/{year}/"

            [views.monthly]
            over = "yearly"
            group_by = "date.month"
            route = "/blog/{year}/{month:02}/"
        "#);
        let e = c.query("monthly").unwrap_err().to_string();
        assert!(e.contains("punted"), "unexpected error: {e}");
    }

    /// Computed fields flow with rows through composition (§6d): declared
    /// once on a shared query view, visible to everything over it; nearest
    /// redeclaration wins.
    #[test]
    fn fields_inherit_along_over_nearest_wins() {
        let c = cfg(r#"
            [views.published]
            over = "blog"
            [views.published.fields.summary]
            truncate = { max_blocks = 4 }

            [views.blog_index]
            over = "published"
            paginate = 5
            routes = ["/blog/"]

            [views.tag_index]
            over = "published"
            group_by = "tags"
            route = "/blog/tags/{key}/"
            [views.tag_index.fields.summary]
            truncate = { max_blocks = 1 }
        "#);
        let inherited = c.fields_for("blog_index");
        assert_eq!(inherited["summary"].truncate.unwrap().max_blocks, Some(4));
        let overridden = c.fields_for("tag_index");
        assert_eq!(overridden["summary"].truncate.unwrap().max_blocks, Some(1));
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
