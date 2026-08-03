//! Site config: authored types, merge laws, load, and validation.

mod effective;
mod merge;
mod types;
mod validate;

#[allow(unused_imports)] // re-export home for `crate::config::merge_*`
pub(crate) use merge::*;
pub use types::*;

use crate::config::effective::{Prov, Trace};
use crate::shape::Shaped;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Expand `@name[index]`; `@@` -> literal `@`.
fn expand_table_refs(cfg: &Config, text: &str, member: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        if after.starts_with('@') {
            out.push('@');
            rest = &after[1..];
            continue;
        }
        if let Some((name, index)) = parse_table_ref(&rest[at..]) {
            let norm = normalize_table_index(index);
            out.push_str(cfg.i18n_table(name, &norm, member));
            rest = &rest[at + 1 + name.len() + 1 + index.len() + 1..];
            continue;
        }
        out.push('@');
        rest = after;
    }
    out.push_str(rest);
    out
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        Config::load_profile(path, None)
    }

    /// Load, then project through a profile (§4a). `dev` is implicit.
    pub fn load_profile(path: &Path, profile: Option<&str>) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        // Projection is inside from_toml on merged TOML (MERGE.md E2).
        let mut cfg = Config::from_toml_profile(&text, profile)
            .with_context(|| format!("parsing config {}", path.display()))?;
        cfg.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        cfg.config_file = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        cfg.resolve_default_content();
        cfg.validate()?;
        Ok(cfg)
    }

    /// Inherit base? Shared by load and `--effective` (§4d).
    fn extends_of(value: &toml::Value) -> Result<bool> {
        match value
            .get("extends")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
        {
            "default" => Ok(true),
            "none" => Ok(false),
            other => anyhow::bail!(
                "extends = {other:?} — the only values are \"default\" (inherit \
                 the engine's base config, §4d) and \"none\" (declare \
                 everything yourself)."
            ),
        }
    }

    /// Merged TOML with provenance (`--effective`, MERGE.md B3). Pre-deserialize
    /// so rejected configs still show what the engine saw.
    pub fn effective(path: &Path, profile: Option<&str>) -> Result<String> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        Config::effective_toml(&text, &path.display().to_string(), profile)
            .with_context(|| format!("reading config {}", path.display()))
    }

    pub fn effective_toml(text: &str, label: &str, profile: Option<&str>) -> Result<String> {
        let value: toml::Value = toml::from_str(text)?;
        let mut trace = Trace::recording();
        let inherits_base = Config::extends_of(&value)?;
        let mut merged = if inherits_base {
            merge_base_traced(value, &mut trace)?
        } else {
            // extends=none: attribute every atom as Site at merge paths.
            note_table(
                &mut trace,
                &mut Vec::new(),
                &Config::shape(),
                &value,
                Prov::Site,
            );
            value
        };
        // Engine defaults for keys neither file wrote.
        if let Some(t) = merged.as_table_mut() {
            for (k, v) in engine_defaults() {
                if !t.contains_key(k) {
                    trace.record(&[k.to_string()], Prov::Default);
                    t.insert(k.to_string(), v);
                }
            }
        }
        let mut preamble = format!("# The effective config for {label}.\n#\n");
        preamble.push_str(if inherits_base {
            "# This site's grackle.toml merged over the base config compiled into\n\
             # the engine (DESIGN.md §4d, MERGE.md §3A). It is the table the\n\
             # deserializer is handed — not a diff of the two files: the merge\n\
             # itself recorded where every line below came from.\n"
        } else {
            "# `extends = \"none\"`, so no base was merged: this site declares its\n\
             # whole config, and every key below is its own (DESIGN.md §4d).\n"
        });
        if let Some(name) = profile {
            // MERGE.md C6e: preamble must match a real projection.
            let declared = merged.get("profiles").and_then(|p| p.as_table());
            let mut known: Vec<&str> = declared
                .map(|t| t.keys().map(String::as_str).collect())
                .unwrap_or_default();
            known.push("dev");
            known.sort_unstable();
            let real = declared.is_some_and(|t| t.contains_key(name));
            preamble.push_str(&match (known.contains(&name), real) {
                (_, true) => format!(
                    "#\n# Projected through profile {name:?} (§4a): the profile's own body,\n\
                     # minus `force`, merged over the table above as the NEAREST writer.\n\
                     # Every `# profile {name}` line below is a key it wrote.\n\
                     #\n\
                     # [profiles.{name}.force] is NOT part of that overlay: it is rung 0\n\
                     # (§2), applied per row and per route at load rather than to the\n\
                     # config, and it is printed below under [profiles] like any other\n\
                     # config value.\n"
                ),
                (true, false) => format!(
                    "#\n# NOTE: profile {name:?} is implicit (§4a) — this config declares no\n\
                     # [profiles.{name}], and an undeclared profile projects nothing. The\n\
                     # table below is what it would build.\n"
                ),
                (false, _) => format!(
                    "#\n# NOTE: {name:?} names no profile (knowns: {}), so nothing\n\
                     # would be projected — `build --profile {name}` is a load error.\n\
                     # The merged config below is unaffected and is printed anyway.\n",
                    known.join(", ")
                ),
            });
            if real {
                (merged, _, _) = project(merged, name, &mut trace)?;
            }
        }
        Ok(crate::config::effective::render(
            &merged,
            &trace,
            &preamble,
            profile.unwrap_or_default(),
        ))
    }

    /// One parse path (tests use `extends = "none"` for isolation).
    pub fn from_toml(text: &str) -> Result<Config> {
        Config::from_toml_profile(text, None)
    }

    /// Project through `profile` between base merge and deserialize (§4a,
    /// MERGE.md E2). Dry-runs every declared profile when none selected (R5).
    pub fn from_toml_profile(text: &str, profile: Option<&str>) -> Result<Config> {
        let value: toml::Value = toml::from_str(text)?;
        let inherits_base = Config::extends_of(&value)?;
        // Site-declared view names, before merge blurs base vs site.
        let mut declared: Vec<String> = ["sets", "routes"]
            .iter()
            .filter_map(|k| value.get(k)?.as_table())
            .flat_map(|t| t.keys().cloned())
            .collect();
        // Site rule counts per collection (prepend: first n are site's).
        let site_rules: BTreeMap<String, usize> = value
            .get("collections")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let n = e
                            .get("rules")
                            .and_then(|r| r.as_array())
                            .map_or(0, |r| r.len());
                        Some((collection_key(e)?, n))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let value = match inherits_base {
            false => value,
            true => merge_base(value)?,
        };
        let (value, forced, patched) = match profile {
            None => (value, toml::Table::new(), Vec::new()),
            Some(name) => project(value, name, &mut Trace::off())?,
        };
        declared.extend(patched.iter().cloned());
        let mut cfg: Config = match value.try_into() {
            Ok(c) => c,
            Err(e) => {
                // Prefer site-text error when message() matches (spans); else
                // merged error (MERGE.md R7: bare site may miss base-supplied fields).
                return match toml::from_str::<Config>(text) {
                    Err(spanned) if spanned.message() == e.message() => {
                        Err(anyhow::Error::new(spanned))
                    }
                    _ => Err(anyhow::Error::new(e)),
                };
            }
        };
        cfg.merge_collections()?;
        cfg.merge_queries()?;
        for (name, v) in cfg.views.iter_mut() {
            v.inherited = !declared.contains(name);
        }
        for c in cfg.collections.values_mut() {
            let mine = identity(c.source.as_deref(), c.name.as_deref())
                .and_then(|k| site_rules.get(&k).copied());
            c.inherited = mine.is_none();
            let mine = mine.unwrap_or(0);
            for (i, r) in c.rules.iter_mut().enumerate() {
                r.inherited = i >= mine;
            }
        }
        cfg.check_objects_rule_gate()?;
        cfg.check_rule_address()?;
        if let Some(name) = profile {
            cfg.forced = forced.into_iter().collect();
            // Attribute profile-written filters (MERGE.md C6a).
            for vname in &patched {
                if let Some(v) = cfg.views.get_mut(vname) {
                    v.filter_profile = Some(name.to_string());
                }
            }
            cfg.profile = Some(name.to_string());
        } else {
            // Dry-run every declared profile (MERGE.md R5). No resolve_default_content:
            // no dir here; it only adds errors (MERGE.md §6, E2).
            for name in cfg.profiles.keys() {
                Config::from_toml_profile(text, Some(name))
                    .and_then(|p| p.validate())
                    .with_context(|| {
                        format!(
                            "profile {name} (checked at every load — a projection \
                             is part of this config, §4a)"
                        )
                    })?;
            }
        }
        cfg.check_pairing_axis()?;
        Ok(cfg)
    }

    /// Pairing axis, if declared, must list a canonical member.
    pub fn check_pairing_axis(&self) -> Result<()> {
        let Some(name) = self.i18n.axis.as_deref() else {
            return Ok(());
        };
        let Some(a) = self.axes.get(name) else {
            return Ok(());
        };
        anyhow::ensure!(
            !a.values.is_empty(),
            "[axes.{name}]: values must list at least the canonical member"
        );
        Ok(())
    }

    pub fn pairing_axis(&self) -> Option<(&str, &Axis)> {
        let name = self.i18n.axis.as_deref()?;
        self.axes.get(name).map(|a| (name, a))
    }

    pub fn pairing_canonical(&self) -> Option<&str> {
        self.pairing_axis().and_then(|(_, a)| a.canonical())
    }

    /// True if any collection `file` pattern spends this axis.
    pub fn is_file_axis(&self, name: &str) -> bool {
        let bare = format!("{{{name}}}");
        let namespaced = format!("{{axis:{name}}}");
        self.collections
            .values()
            .any(|c| c.file.iter().any(|f| f.contains(&bare) || f.contains(&namespaced)))
    }

    /// Axis field value, or canonical when unstamped/empty.
    pub fn axis_on(&self, row: &(impl grackle_db::filter::Row + ?Sized), axis_name: &str) -> Option<String> {
        let axis = self.axes.get(axis_name)?;
        Some(match row.field(&axis.field) {
            grackle_db::Value::Str(s) if !s.is_empty() => s,
            _ => axis.canonical()?.to_owned(),
        })
    }

    pub fn on_canonical(&self, row: &impl grackle_db::filter::Row, axis_name: &str) -> bool {
        let Some(axis) = self.axes.get(axis_name) else {
            return true;
        };
        let Some(canon) = axis.canonical() else {
            return true;
        };
        match row.field(&axis.field) {
            grackle_db::Value::Str(s) if !s.is_empty() => s == canon,
            _ => true,
        }
    }

    pub fn stamp_axis_field(
        &self,
        fields: &mut std::collections::BTreeMap<String, grackle_db::Value>,
        axis_name: &str,
        value: &str,
    ) {
        let Some(axis) = self.axes.get(axis_name) else {
            return;
        };
        if axis.canonical() == Some(value) {
            fields.remove(&axis.field);
        } else {
            fields.insert(
                axis.field.clone(),
                grackle_db::Value::Str(value.to_string()),
            );
        }
    }

    pub fn same_on(
        &self,
        a: &impl grackle_db::filter::Row,
        b: &impl grackle_db::filter::Row,
        axis: &str,
    ) -> bool {
        self.axis_on(a, axis) == self.axis_on(b, axis)
    }

    /// Pairing-axis member on `row`; empty if no pairing axis.
    pub fn pairing_member(&self, row: &(impl grackle_db::filter::Row + ?Sized)) -> String {
        match self.pairing_axis() {
            Some((n, _)) => self.axis_on(row, n).unwrap_or_default(),
            None => String::new(),
        }
    }

    pub fn i18n_string<'a>(&'a self, key: &str, member: &str) -> &'a str {
        let canon = self.pairing_canonical().unwrap_or(member);
        self.i18n.string(key, member, canon)
    }

    pub fn i18n_table<'a>(&'a self, name: &str, index: &str, member: &str) -> &'a str {
        let canon = self.pairing_canonical().unwrap_or(member);
        self.i18n.table(name, index, member, canon)
    }

    pub fn i18n_text<'a>(&'a self, s: &'a LocalizedStr, member: &str) -> &'a str {
        let canon = self.pairing_canonical().unwrap_or(member);
        self.i18n.text(s, member, canon)
    }

    pub fn site_title<'a>(&'a self, member: &str) -> &'a str {
        self.i18n_text(&self.site.title, member)
    }

    /// Render LocalizedStr: `@key` / `@table[index]` / inline + embedded tables (§6f).
    pub fn render_localized<F>(
        &self,
        s: &LocalizedStr,
        member: &str,
        get: &F,
    ) -> Result<String>
    where
        F: Fn(&str) -> Option<String>,
    {
        match s {
            LocalizedStr::One(raw) => {
                if let Some(rest) = raw.strip_prefix("@@") {
                    let rendered = grackle_db::template::render(rest, get)?;
                    return Ok(format!("@{rendered}"));
                }
                if let Some((table, index_tmpl)) = parse_table_ref(raw) {
                    let index = grackle_db::template::render(index_tmpl, get)?;
                    let index = normalize_table_index(&index);
                    return Ok(self.i18n_table(table, &index, member).to_string());
                }
                if let Some(key) = raw
                    .strip_prefix('@')
                    .filter(|r| !r.starts_with('@') && !r.contains('['))
                {
                    let tmpl = self.i18n_string(key, member);
                    if tmpl.is_empty() {
                        return Ok(String::new());
                    }
                    let rendered = grackle_db::template::render(tmpl, get)?;
                    return Ok(expand_table_refs(self, &rendered, member));
                }
                let rendered = grackle_db::template::render(raw, get)?;
                Ok(expand_table_refs(self, &rendered, member))
            }
            LocalizedStr::PerMember(_) => {
                let text = self.i18n_text(s, member);
                let rendered = grackle_db::template::render(text, get)?;
                Ok(expand_table_refs(self, &rendered, member))
            }
        }
    }

    /// Format date via `[i18n.strings]` style key; missing => `""`.
    pub fn format_date(
        &self,
        d: chrono::NaiveDate,
        style: &str,
        member: &str,
    ) -> String {
        use chrono::Datelike;
        let get = |tok: &str| -> Option<String> {
            match tok {
                "year" => Some(d.year().to_string()),
                "month" => Some(d.month().to_string()),
                "day" => Some(d.day().to_string()),
                _ => None,
            }
        };
        let s = LocalizedStr::One(format!("@{style}"));
        self.render_localized(&s, member, &get)
            .unwrap_or_default()
    }

    pub fn axis_values_for_file(&self) -> crate::filename::AxisValues<'_> {
        self.axes
            .iter()
            .map(|(n, a)| (n.as_str(), a.values.as_slice()))
            .collect()
    }

    /// Settle `default_content` against the tree (§4d): missing = plain landing;
    /// accepts `{% view %}` = claim; else stand down the offered route.
    fn resolve_default_content(&mut self) {
        let root = self.root();
        for (name, v) in self.views.iter_mut() {
            let Some(pat) = v.default_content.as_deref() else {
                continue;
            };
            if is_templated(pat) {
                continue;
            }
            let Some(found) = brace_alternatives(pat)
                .into_iter()
                .find(|c| root.join(c).exists())
            else {
                continue;
            };
            let tag = format!("{{% view {name} %}}");
            let accepted = std::fs::read_to_string(root.join(&found))
                .map(|t| t.contains(&tag))
                .unwrap_or(false);
            if accepted {
                v.content = Some(found);
            } else {
                v.route = None;
                v.routes.clear();
            }
        }
    }

    /// Table name: `name`, else source basename (strip `_`); `.` => `entries`.
    fn table_name(c: &Collection) -> Result<String> {
        if let Some(n) = &c.name {
            return Ok(n.clone());
        }
        let Some(src) = c.source.as_deref() else {
            anyhow::bail!(
                "a collection with no `source` (objects are matched by \
                 extension, not by directory) has no directory to name it — \
                 give it a `name`."
            );
        };
        let base = Path::new(src)
            .file_name()
            .map(|s| s.to_string_lossy().trim_start_matches('_').to_string())
            .unwrap_or_default();
        Ok(if base.is_empty() {
            "entries".to_string()
        } else {
            base
        })
    }

    /// Key collections by resolved table name (`from` namespace).
    fn merge_collections(&mut self) -> Result<()> {
        for c in std::mem::take(&mut self.declared_collections) {
            let name = Config::table_name(&c)?;
            let src = c.source.clone().unwrap_or_default();
            if let Some(prev) = self.collections.insert(name.clone(), c) {
                anyhow::bail!(
                    "two collections resolve to the name {name:?} (sources \
                     {:?} and {src:?}) — `from` needs one name per thing, so \
                     give one of them an explicit `name`.",
                    prev.source.unwrap_or_default(),
                );
            }
        }
        Ok(())
    }

    /// Fold sets/routes into `views`; one namespace with collections.
    fn merge_queries(&mut self) -> Result<()> {
        let sets = std::mem::take(&mut self.sets);
        let routes = std::mem::take(&mut self.routes);
        for (name, v) in &sets {
            if v.route.is_some() || !v.routes.is_empty() {
                anyhow::bail!(
                    "[sets.{name}] declares a path. A set is a query that \
                     never lands — move it to [routes.{name}]."
                );
            }
        }
        for (name, v) in sets.iter().chain(&routes) {
            if v.content.is_some() && v.default_content.is_some() {
                anyhow::bail!(
                    "view {name}: declares both content and default_content — \
                     one claims a row unconditionally, the other only if it \
                     exists. Pick which."
                );
            }
        }
        for (name, v) in &routes {
            if v.route.is_none() && v.routes.is_empty() {
                anyhow::bail!(
                    "[routes.{name}] declares no `path`. A route is a query \
                     that lands — give it one, or move it to [sets.{name}]."
                );
            }
        }
        let owned = sets
            .into_iter()
            .map(|(n, v)| (n, v, true))
            .chain(routes.into_iter().map(|(n, v)| (n, v, false)));
        for (name, mut v, declared_set) in owned {
            v.declared_set = declared_set;
            if self.collections.contains_key(&name) {
                anyhow::bail!(
                    "{name:?} names both a collection and a set/route. `from` \
                     resolves against one namespace, so the name must be unique."
                );
            }
            if self.views.insert(name.clone(), v).is_some() {
                anyhow::bail!("{name:?} is declared as both a set and a route.");
            }
        }
        Ok(())
    }

    /// Schema from config only (no `.schema.toml` yet).
    fn config_declared_schema(&self) -> grackle_db::filter::Schema {
        let mut s = grackle_db::filter::Schema::new();
        let tables = std::iter::once(&self.schema.fields)
            .chain(self.collections.values().map(|c| &c.schema));
        for t in tables {
            for (name, v) in t {
                let ty = v
                    .as_table()
                    .and_then(|t| t.get("type"))
                    .and_then(|t| t.as_str())
                    .and_then(crate::schema::FieldType::parse);
                if let Some(ty) = ty {
                    s.insert(grackle_model::intern(name.clone()), ty.filter_type());
                }
            }
        }
        s
    }

    pub fn field_expr_schema(&self) -> grackle_db::filter::Schema {
        let mut s = grackle_db::field_schema();
        for (k, t) in self.config_declared_schema() {
            s.insert(k, t);
        }
        s
    }

    /// The full row vocabulary a widget's expression argument may name: the
    /// built-in columns plus the `[schema]`-declared fields. (Per-subtree
    /// `.schema.toml` fields are not in it — those live in the loader's
    /// schemas, not the config.)
    pub fn row_expr_schema(&self) -> grackle_db::filter::Schema {
        let mut s = grackle_model::row_schema();
        for (k, t) in self.config_declared_schema() {
            s.insert(k, t);
        }
        s
    }

    /// Filter schema for a view's `where` (pre-walk; see `check_profile_filters`).
    fn view_filter_schema(&self, name: &str) -> grackle_db::filter::Schema {
        let declared = self.config_declared_schema();
        let Some(v) = self.views.get(name) else {
            return grackle_model::row_schema();
        };
        if v.reads_all_outputs() {
            return grackle_model::route_schema(&declared);
        }
        let mut s = grackle_model::row_schema();
        for (k, t) in &declared {
            s.insert(k, *t);
        }
        s
    }

    /// Flatten `from` chain: query-only or grouped unpaginated (§5c; q30).
    pub fn query(&self, name: &str) -> Result<Query> {
        let mut filters = Vec::new();
        let mut patched = Vec::new();
        let mut order_by: Option<String> = None;
        let mut seen: Vec<&str> = Vec::new();
        let mut cur = name;
        loop {
            let v = self
                .views
                .get(cur)
                .with_context(|| format!("view {name}: `from` names unknown view {cur:?}"))?;
            if seen.contains(&cur) {
                anyhow::bail!("view {name}: `from` chain is cyclic at {cur:?}");
            }
            seen.push(cur);
            if let Some(f) = &v.filter {
                if let Some(p) = &v.filter_profile {
                    patched.push(format!("profile {p} replaced view {cur}'s `where`"));
                }
                filters.push(f.clone());
            }
            if order_by.is_none() {
                order_by.clone_from(&v.order_by);
            }
            let Some(from) = &v.from else {
                filters.reverse();
                patched.reverse();
                return Ok(Query {
                    base: Vec::new(),
                    filters,
                    order_by,
                    patched,
                });
            };
            let next = from.single().and_then(|s| self.views.get(s));
            let Some(next) = next else {
                self.check_base(cur, name, from)?;
                filters.reverse();
                patched.reverse();
                return Ok(Query {
                    base: from.names().to_vec(),
                    filters,
                    order_by,
                    patched,
                });
            };
            if !next.is_query_only() {
                let subdividable = next.group_by.is_some()
                    && next.paginate.is_none()
                    && next.limit.is_none()
                    && next.template.is_none();
                if !subdividable {
                    anyhow::bail!(
                        "{cur}: `from = {}` names something that is neither a set nor a \
                         grouped route. Only sets and grouped, unpaginated routes may be \
                         composed over (subdivision, §5c); pagination × subdivision is \
                         punted (open question 30).{}",
                        from.display(),
                        self.whose_from(cur, name)
                    );
                }
                if v.group_by.is_none() {
                    anyhow::bail!(
                        "{cur}: `from = {}` names a grouped route, but {cur} has no \
                         `group_by`. Composing over a grouped route means subdividing its \
                         partition (§5c), so the composer must be grouped too.{}",
                        from.display(),
                        self.whose_from(cur, name)
                    );
                }
            }
            cur = from.single().expect("a union terminates the chain above");
        }
    }

    /// Terminated `from`: collection or same-kind union (§5c). `*` is invalid
    /// (IO.md I3); see [`whose_from`] for carrier vs asked.
    fn check_base(&self, carrier: &str, asked: &str, from: &From) -> Result<()> {
        if matches!(from, From::One(s) if s == "*") {
            anyhow::bail!(
                "{carrier}: `from = \"*\"` names nothing — the star spelling is \
                 gone (IO.md §4). A fold shell reads every output by having no \
                 `from` at all, so delete the line: the `shell` ({}) is what \
                 says this folds the pool.{}",
                crate::shell::FOLD.join(", "),
                self.whose_from(carrier, asked)
            );
        }
        for member in from.names() {
            if self.collections.contains_key(member) {
                continue;
            }
            if matches!(from, From::Union(_)) {
                anyhow::bail!(
                    "{carrier}: `from` unions {member:?}, which is not a collection. A union \
                     ranges over collections; to narrow a set, compose over it with `from = \
                     {member:?}` and a `where`.{}",
                    self.whose_from(carrier, asked)
                );
            }
            anyhow::bail!(
                "{carrier}: `from = {}` is neither a collection, a set nor a route \
                 (collections: {}; sets and routes: {}){}",
                from.display(),
                self.collections
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                self.views.keys().cloned().collect::<Vec<_>>().join(", "),
                self.whose_from(carrier, asked)
            );
        }
        if from.names().is_empty() {
            anyhow::bail!(
                "{carrier}: `from = []` names nothing to range over.{}",
                self.whose_from(carrier, asked)
            );
        }
        Ok(())
    }

    /// Note when bad `from` is on a different / inherited view (MERGE.md C7b).
    fn whose_from(&self, carrier: &str, asked: &str) -> String {
        let mut note = String::new();
        if carrier != asked {
            note.push_str(&format!(
                "\n  (reached from {asked:?}, which composes over it.)"
            ));
        }
        let Some(v) = self.views.get(carrier) else {
            return note;
        };
        if v.inherited {
            let table = if v.declared_set { "sets" } else { "routes" };
            note.push_str(&format!(
                "\n  {carrier:?} is inherited from the base config (§4d) — it is not in your \
                 grackle.toml, and its `from` names a collection the BASE declares. A site \
                 that renames or drops that collection has to say what {carrier:?} means to \
                 it: declare your own [{table}.{carrier}] over the inherited one, or keep a \
                 collection under the name it asks for."
            ));
        }
        note
    }

    /// `from` chain, nearest first (acyclic; validated by `query`).
    pub fn chain<'a: 'b, 'b>(&'a self, name: &'b str) -> Vec<(&'b str, &'a View)> {
        let mut out = Vec::new();
        let mut cur = name;
        while let Some(v) = self.views.get(cur) {
            out.push((cur, v));
            let Some(n) = v.from.as_ref().and_then(From::single) else {
                break;
            };
            cur = n;
        }
        out
    }

    /// `group_by` specs, outermost first (§5c subdivision).
    pub fn group_specs(&self, name: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .chain(name)
            .iter()
            .filter_map(|(_, v)| v.group_by.clone())
            .collect();
        v.reverse();
        v
    }

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

    /// Computed fields along `from`; nearest wins (§5c).
    pub fn fields_for(&self, view: &str) -> BTreeMap<&str, &Field> {
        let mut out: BTreeMap<&str, &Field> = BTreeMap::new();
        for (_, v) in self.chain(view) {
            for (name, f) in &v.fields {
                out.entry(name.as_str()).or_insert(f);
            }
        }
        out
    }

    pub fn field_expr(&self, name: &str) -> Option<&str> {
        for vname in self.views.keys() {
            if let Some(src) = self.fields_for(vname).get(name).and_then(|f| f.as_expr()) {
                return Some(src);
            }
        }
        None
    }

    pub fn root(&self) -> PathBuf {
        let joined = self.dir.join(&self.root);
        std::fs::canonicalize(&joined).unwrap_or(joined)
    }

    pub fn record(&self, field: &str, id: &str) -> Option<&RecordCfg> {
        self.records.get(field)?.get(id)
    }

    pub fn record_name<'a>(&'a self, field: &str, id: &'a str, locale: &str) -> &'a str {
        match self.record(field, id).and_then(|r| r.name.as_ref()) {
            Some(n) => self.i18n_text(n, locale),
            None => id,
        }
    }

    pub fn record_slug<'a>(&'a self, field: &str, id: &'a str) -> &'a str {
        self.record(field, id)
            .and_then(|t| t.slug.as_deref())
            .unwrap_or(id)
    }

    /// Source path -> owning view; templated claims settled post-materialize.
    pub fn content_claims(&self) -> BTreeMap<&str, &str> {
        self.views
            .iter()
            .filter_map(|(n, v)| v.content.as_deref().map(|c| (c, n.as_str())))
            .filter(|(c, _)| !is_templated(c))
            .collect()
    }

    /// Archive view for a list field; None = none / ambiguous without declare.
    pub fn archive_view(&self, field: &str) -> Option<(&str, &View)> {
        if let Some(name) = self
            .collections
            .values()
            .find_map(|c| c.archives.get(field))
            .map(|s| s.as_str())
        {
            return self.views.get(name).map(|v| (name, v));
        }
        let mut found = None;
        for (name, v) in &self.views {
            if v.group_by.as_deref() == Some(field) {
                if found.is_some() {
                    return None;
                }
                found = Some((name.as_str(), v));
            }
        }
        found
    }

    pub fn archive_url(&self, field: &str, id: &str, axis_value: &str) -> Option<String> {
        let (_, v) = self.archive_view(field)?;
        let tmpls: Vec<&str> = if !v.routes.is_empty() {
            v.routes.iter().map(String::as_str).collect()
        } else {
            v.route.iter().map(String::as_str).collect()
        };
        if tmpls.is_empty() {
            return None;
        }
        let slug = self.record_slug(field, id).to_string();
        let pairing = self.pairing_axis().map(|(n, _)| n).unwrap_or("");
        let rendered: Result<Vec<String>, _> = tmpls
            .iter()
            .map(|tmpl| archive_route_fill(tmpl, field, &slug, pairing))
            .collect();
        let rendered = rendered.ok()?;
        let mut coords = Vec::new();
        if let Some((axis, a)) = self.pairing_axis() {
            if rendered.iter().any(|t| crate::load::spends(t, axis)) {
                coords.push(crate::load::Coord {
                    axis,
                    value: axis_value,
                    canonical: a.canonical() == Some(axis_value),
                });
            }
        }
        crate::load::select_path(&rendered, &coords).ok()
    }

    pub fn embedding_text(&self, p: &grackle_model::Row, body: &str) -> Result<String> {
        let tmpl = &self.schema.embeddings.string;
        if tmpl.is_empty() {
            return Ok(String::new());
        }
        grackle_db::template::render(tmpl, |tok| {
            Some(match tok {
                "body" => body.trim().to_string(),
                "title" => p.title.clone().unwrap_or_default(),
                other => match grackle_db::filter::Row::field(p, other) {
                    grackle_db::Value::List(v) => v
                        .into_iter()
                        .filter_map(|x| match x {
                            grackle_db::Value::Str(s) => Some(s),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                    grackle_db::Value::Str(s) => s,
                    grackle_db::Value::Int(n) => n.to_string(),
                    grackle_db::Value::Bool(b) => b.to_string(),
                    grackle_db::Value::Double(d) => d.to_string(),
                    grackle_db::Value::Content(_)
                    | grackle_db::Value::Outline(_)
                    | grackle_db::Value::Map(_)
                    | grackle_db::Value::Null => String::new(),
                },
            })
        })
    }

    pub fn search_streams(&self, p: &grackle_model::Row, body: &str) -> Vec<(u32, String)> {
        const BOOST: u32 = 5;
        const BODY: u32 = 1;
        let mut out = Vec::new();
        for f in &self.schema.search.index {
            match f.as_str() {
                "body" => {
                    if !body.is_empty() {
                        out.push((BODY, body.to_string()));
                    }
                }
                "title" => {
                    if let Some(t) = p.title.as_deref().filter(|s| !s.is_empty()) {
                        out.push((BOOST, t.to_string()));
                    }
                }
                other => match grackle_db::filter::Row::field(p, other) {
                    grackle_db::Value::List(v) => {
                        for x in v {
                            if let grackle_db::Value::Str(s) = x {
                                if !s.is_empty() {
                                    out.push((BOOST, s));
                                }
                            }
                        }
                    }
                    grackle_db::Value::Str(s) if !s.is_empty() => out.push((BOOST, s)),
                    grackle_db::Value::Int(n) => out.push((BOOST, n.to_string())),
                    grackle_db::Value::Bool(b) => out.push((BOOST, b.to_string())),
                    _ => {}
                },
            }
        }
        out
    }

    pub fn search_store(
        &self,
        p: &grackle_model::Row,
        body: &str,
    ) -> std::collections::BTreeMap<String, String> {
        let mut out = std::collections::BTreeMap::new();
        for f in &self.schema.search.store {
            let value = match f.as_str() {
                "body" => {
                    if body.is_empty() {
                        continue;
                    }
                    body.to_string()
                }
                "title" => p
                    .title
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| p.url.clone()),
                other => {
                    if let Some(d) = p.as_date(other) {
                        format!("{d}T00:00:00+00:00")
                    } else {
                        match grackle_db::filter::Row::field(p, other) {
                            grackle_db::Value::List(v) => {
                                let joined = v
                                    .into_iter()
                                    .filter_map(|x| match x {
                                        grackle_db::Value::Str(s) if !s.is_empty() => Some(s),
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                if joined.is_empty() {
                                    continue;
                                }
                                joined
                            }
                            grackle_db::Value::Str(s) if !s.is_empty() => s,
                            grackle_db::Value::Int(n) => n.to_string(),
                            grackle_db::Value::Bool(b) => b.to_string(),
                            _ => continue,
                        }
                    }
                }
            };
            out.insert(f.clone(), value);
        }
        out
    }
}

/// Fill archive route for probe/pills; axis tokens stay for `select_path`.
pub(crate) fn archive_route_fill(
    tmpl: &str,
    field: &str,
    key: &str,
    pairing_axis: &str,
) -> anyhow::Result<String> {
    grackle_db::template::render(tmpl, |tok| {
        let (ns, k) = grackle_db::template::classify(tok);
        match ns {
            Some("axis") => Some(format!("{{{tok}}}")),
            None if !pairing_axis.is_empty() && k == pairing_axis => {
                Some(format!("{{{pairing_axis}}}"))
            }
            None | Some("group") if k == "key" || k == field => Some(key.to_string()),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests;
