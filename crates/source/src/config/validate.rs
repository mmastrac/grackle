//! Load-time config checks.

use anyhow::{Context, Result};
use std::collections::BTreeMap;

use super::{
    archive_route_fill, describe_collection, fence, split_profile, Config, Field, LocalizedStr,
    BASE, FORCE,
};

impl Config {
    /// Objects rules may not gate on `front_matter`: shape vs
    /// identity would disagree on sidecars.
    pub(crate) fn check_objects_rule_gate(&self) -> Result<()> {
        for (name, c) in &self.collections {
            if !c.is_objects() {
                continue;
            }
            for r in &c.rules {
                let Some(want) = r.front_matter else { continue };
                anyhow::bail!(
                    "collection {}: rule `match = {:?}` declares \
                     `front_matter = {want}`. An objects rule selects by SHAPE — \
                     what makes a row a picture is its extension, read off the \
                     path alone, and the objects index answers that way whichever \
                     scope claims the row — while the identity gate belongs to \
                     the scopes that PARSE. Gating here splits one directory of \
                     images between two scopes by whether someone wrote a sidecar \
                     beside them, and calls all of them pictures either way. \
                     Delete the line.",
                    describe_collection(name, c),
                    r.pattern,
                );
            }
        }
        Ok(())
    }

    /// `route` and `embed` are exclusive; `on_demand` beside `embed` is dead.
    pub(crate) fn check_rule_address(&self) -> Result<()> {
        for (name, c) in &self.collections {
            for r in &c.rules {
                if r.embed != Some(true) {
                    continue;
                }
                if !r.route.is_empty() {
                    anyhow::bail!(
                        "collection {}: rule `match = {:?}` declares both `route` \
                         and `embed = true`. A rule decides an address once: \
                         `route` mints a canonical URL and a routed output WINS, \
                         so the embed policy beneath it could never be reached. \
                         Keep the route, or delete it and let `/static/` address \
                         these rows.",
                        describe_collection(name, c),
                        r.pattern,
                    );
                }
                if r.on_demand == Some(true) {
                    anyhow::bail!(
                        "collection {}: rule `match = {:?}` declares `on_demand` \
                         beside `embed = true`, and `on_demand` defers a ROUTE \
                         this rule does not mint — so it configures nothing. An \
                         embed-addressed row publishes when something embeds it, \
                         which is the whole of the policy. Delete the \
                         line.",
                        describe_collection(name, c),
                        r.pattern,
                    );
                }
            }
        }
        Ok(())
    }

    /// Fence + rung-0 typing for every declared profile.
    pub(crate) fn check_profiles(&self) -> Result<()> {
        // Site [schema] only (not .schema.toml); same parser as Schemas::set_site.
        let declared = crate::schema::site_fields(&self.schema.decls, "grackle.toml [schema]")?;
        let field_knowns = || {
            let mut names: Vec<&str> = declared.keys().map(String::as_str).collect();
            names.sort_unstable();
            match names.is_empty() {
                true => "none".to_string(),
                false => names.join(", "),
            }
        };
        for (pname, p) in &self.profiles {
            for key in p.body().keys() {
                fence(pname, key)?;
            }
            let (_, force) = split_profile(pname, p.body())?;
            for (field, v) in &force {
                let Some(ty) = declared.get(field) else {
                    anyhow::bail!(
                        "profile {pname}: [profiles.{pname}.{FORCE}] {field} — a \
                         forced field is written onto every row and every route, \
                         so it must be declared in the site's own [schema]\n  \
                         declared fields: {}",
                        field_knowns()
                    );
                };
                crate::schema::typed(
                    ty,
                    field,
                    v,
                    &format!("profile {pname}: [profiles.{pname}.{FORCE}]"),
                )?;
            }
        }
        Ok(())
    }

    /// Type-check profile-written `where`s; defer unknown fields until walk.
    pub(crate) fn check_profile_filters(&self) -> Result<()> {
        for (vname, v) in &self.views {
            let (Some(p), Some(f)) = (v.filter_profile.as_deref(), v.filter.as_deref()) else {
                continue;
            };
            let schema = self.view_filter_schema(vname);
            if let Err(e) = grackle_db::filter::Filter::parse(f, &schema) {
                let msg = format!("{e:#}");
                if msg.contains("unknown field") {
                    continue;
                }
                return Err(e).with_context(|| format!("profile {p}: view {vname}: filter {f:?}"));
            }
        }
        Ok(())
    }

    /// One computed field: a CEL expression over the row, whose return
    /// type the checker infers, the name is free, so a site computes whatever
    /// column it likes. `whose` names the table for errors.
    fn check_computed_field(&self, fname: &str, f: &Field, whose: &str) -> Result<()> {
        match f {
            Field::Expr(src) => {
                grackle_db::FieldExpr::infer(src, &self.field_expr_schema())
                    .map_err(|e| anyhow::anyhow!("{whose}: field {fname:?}: {e}"))?;
            }
            // A computed column is derived from the row; a bare literal renders
            // nothing, so it is a config mistake rather than a silent no-op.
            Field::Value(_) => {
                anyhow::bail!(
                    "{whose}: field {fname:?} must be an expression, not a literal value"
                );
            }
        }
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let cfg = self;
        for m in &cfg.metadata {
            if m != "git" {
                anyhow::bail!("metadata: unknown provider {m:?}, the providers are: git");
            }
        }
        if cfg.collections.is_empty() {
            anyhow::bail!(
                "no collections declared — nothing would be built. A site \
                 needs at least one `[[collections]]` saying where its \
                 content lives, e.g.\n\n  \
                 [[collections]]\n    name = \"posts\"\n    source = \"_posts\"\n\n  \
                   [[collections.rules]]\n  match = \"**\"\n  \
                 route = \"/blog/{{year}}/{{month:02}}/{{slug}}/\""
            );
        }
        let declared_route =
            crate::schema::site_fields(&cfg.schema.decls, "grackle.toml [schema]")?;
        {
            let row = grackle_model::row_schema();
            for (slot, fields) in [
                ("index", &cfg.schema.search.index),
                ("store", &cfg.schema.search.store),
            ] {
                for f in fields {
                    if f == "body" || f == "title" {
                        continue;
                    }
                    if declared_route.contains_key(f) || row.contains_key(f.as_str()) {
                        continue;
                    }
                    anyhow::bail!(
                        "[schema.search].{slot}: field {f:?} is not a row field — \
                         declare it in [schema] or use \"title\" / \"body\""
                    );
                }
            }
        }
        for (vname, v) in &cfg.views {
            for (field, val) in &v.route_fields {
                let Some(ty) = declared_route.get(field.as_str()) else {
                    let mut names: Vec<&str> = declared_route.keys().map(String::as_str).collect();
                    names.sort_unstable();
                    anyhow::bail!(
                        "unknown field `{field}`, expected a view key or a \
                         [schema] field\n  schema fields: {}",
                        if names.is_empty() {
                            "none".into()
                        } else {
                            names.join(", ")
                        }
                    );
                };
                crate::schema::typed(ty, field, val, &format!("view {vname}"))?;
            }
            // A view with no path never lands; theme on it is dead.
            if v.declared_query_only && v.theme.is_some() {
                anyhow::bail!(
                    "[views.{vname}] declares a theme but no `path`, so \
                     nothing could ever wear it. A view without a path never \
                     lands; embedded with {{% view {vname} %}} it wears the \
                     embedding page's theme. Give the view a `path`, or drop \
                     the theme."
                );
            }
            // Fold shells need a landing.
            if v.declared_query_only {
                if let Some(s) = v
                    .shell
                    .as_deref()
                    .filter(|s| crate::shell::is_fold(s) || self.shells.contains_key(*s))
                {
                    anyhow::bail!(
                        "[views.{vname}] wears shell = {s:?} but declares no \
                         `path`. A fold shell serializes its query into ONE \
                         artifact, and an artifact needs an address to be \
                         written at — give the view a `path`, or drop the \
                         shell and let it stay a query."
                    );
                }
            }
            for (fname, f) in &v.fields {
                self.check_computed_field(fname, f, &format!("view {vname}"))?;
            }
        }
        for (fname, f) in &cfg.schema.fields {
            // `queryable` is a bool column, spelled as a predicate.
            if fname == "queryable" {
                let src = f.as_expr().with_context(|| {
                    "[schema.fields]: queryable must be a predicate expression".to_string()
                })?;
                grackle_db::filter::Filter::parse(src, &self.field_expr_schema())
                    .map_err(|e| anyhow::anyhow!("[schema.fields]: queryable: {e}"))?;
                continue;
            }
            self.check_computed_field(fname, f, "[schema.fields]")?;
        }
        // archives
        {
            let mut declared: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
            for (cname, c) in &cfg.collections {
                for (field, view) in &c.archives {
                    declared
                        .entry(field.as_str())
                        .or_default()
                        .push((cname.as_str(), view.as_str()));
                }
            }
            for (field, owners) in &declared {
                for (cname, vname) in owners {
                    let Some(v) = cfg.views.get(*vname) else {
                        anyhow::bail!(
                            "collection {cname}: archives.{field} = {vname:?} \
                             is not a declared view"
                        );
                    };
                    if v.group_by.as_deref() != Some(field) {
                        anyhow::bail!(
                            "collection {cname}: archives.{field} view {vname:?} \
                             is not grouped by {field}"
                        );
                    }
                    if v.route.is_none() && v.routes.is_empty() {
                        anyhow::bail!(
                            "collection {cname}: archives.{field} view {vname:?} \
                             has no route"
                        );
                    }
                }
            }
            let list_fields: Vec<&str> = declared_route
                .iter()
                .filter(|(_, ty)| **ty == crate::schema::FieldType::List)
                .map(|(n, _)| n.as_str())
                .collect();
            for field in &list_fields {
                let views: Vec<&str> = cfg
                    .views
                    .iter()
                    .filter(|(_, v)| v.group_by.as_deref() == Some(field))
                    .map(|(n, _)| n.as_str())
                    .collect();
                if !declared.contains_key(field) && views.len() > 1 {
                    anyhow::bail!(
                        "multiple views group by {field} ({}) — declare which \
                         owns archive routes: [collections.<name>] archives = \
                         {{ {field} = \"<view>\" }}",
                        views.join(", ")
                    );
                }
            }
            let mut probe: BTreeMap<&str, &str> = BTreeMap::new();
            for (field, owners) in &declared {
                if let Some((_, vname)) = owners.first() {
                    probe.insert(field, vname);
                }
            }
            for field in &list_fields {
                if probe.contains_key(field) {
                    continue;
                }
                if let Some((name, _)) = cfg.archive_view(field) {
                    probe.insert(field, name);
                }
            }
            for (field, name) in probe {
                let Some(v) = cfg.views.get(name) else {
                    continue;
                };
                let tmpls: Vec<&str> = if !v.routes.is_empty() {
                    v.routes.iter().map(String::as_str).collect()
                } else {
                    v.route.iter().map(String::as_str).collect()
                };
                for tmpl in tmpls {
                    archive_route_fill(
                        tmpl,
                        field,
                        "probe",
                        cfg.pairing_axis().map(|(n, _)| n).unwrap_or(""),
                    )
                    .with_context(|| {
                        format!(
                            "view {name}: archive route for {field} needs more \
                                 than {{key}}"
                        )
                    })?;
                }
            }
        }
        // trail must name a subdivision chain.
        for (cname, c) in &cfg.collections {
            let Some(name) = c.trail.as_deref() else {
                continue;
            };
            let knowns = || cfg.views.keys().cloned().collect::<Vec<_>>().join(", ");
            if !cfg.views.contains_key(name) {
                anyhow::bail!(
                    "collection {cname}: trail {name:?} is not a declared view \
                     — views: {}",
                    knowns()
                );
            }
            let chain = cfg.grouped_chain(name);
            if chain.is_empty() {
                anyhow::bail!(
                    "collection {cname}: trail {name:?} declares no `group_by`, \
                     and neither does anything it composes `from` — a trail is a \
                     subdivision chain (a year archive, then a month archive), \
                     rendered from a row's own group keys. Grouped views: {}",
                    cfg.views
                        .iter()
                        .filter(|(_, v)| v.group_by.is_some())
                        .map(|(n, _)| n.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            for level in &chain {
                let v = &cfg.views[level];
                if v.route.is_none() {
                    anyhow::bail!(
                        "collection {cname}: trail {name:?} — its subdivision \
                         chain ({}) passes through view {level:?}, which lands \
                         at no single `path`, so that crumb has no URL and the \
                         level would be dropped from every trail.",
                        chain.join(" > ")
                    );
                }
                if v.crumb.is_none() && v.title.is_none() {
                    anyhow::bail!(
                        "collection {cname}: trail {name:?} — its subdivision \
                         chain ({}) passes through view {level:?}, which declares \
                         neither `crumb` nor `title`, so that crumb has no label \
                         and the level would be dropped from every trail.",
                        chain.join(" > ")
                    );
                }
            }
        }
        // LocalizedStr member maps
        {
            let (axis_label, known, canon): (String, Vec<&str>, Option<&str>) =
                match cfg.pairing_axis() {
                    Some((n, a)) => (
                        format!("[axes.{n}]"),
                        a.values.iter().map(String::as_str).collect(),
                        a.canonical(),
                    ),
                    None => ("[i18n]".into(), Vec::new(), None),
                };
            let known_set = &known;
            let check = |what: &str, s: &LocalizedStr| -> Result<()> {
                let LocalizedStr::PerMember(m) = s else {
                    return Ok(());
                };
                let Some(canon) = canon else {
                    anyhow::bail!(
                        "{what}: a per-member map needs a declared pairing axis \
                         ([i18n] axis → [axes.*]); use a bare string instead"
                    );
                };
                for loc in m.keys() {
                    if !known_set.contains(&loc.as_str()) {
                        anyhow::bail!(
                            "{what}: declares member {loc:?}, which is not in \
                             {axis_label} values {known_set:?}"
                        );
                    }
                }
                if !m.contains_key(canon) {
                    anyhow::bail!(
                        "{what}: a per-member name must include the canonical \
                         member ({canon:?})"
                    );
                }
                Ok(())
            };
            for loc in cfg.i18n.names.keys() {
                if !known_set.contains(&loc.as_str()) {
                    anyhow::bail!(
                        "i18n.names: names member {loc:?}, which is not in \
                         {axis_label} values {known_set:?} — nothing would ever read it"
                    );
                }
            }
            for (field, recs) in &cfg.records {
                for (id, t) in recs {
                    if let Some(n) = &t.name {
                        check(&format!("record {field}.{id}: name"), n)?;
                    }
                    if let Some(i) = &t.intro {
                        check(&format!("record {field}.{id}: intro"), i)?;
                    }
                }
            }
            for (name, v) in &cfg.views {
                if let Some(t) = &v.title {
                    check(&format!("view {name}: title"), t)?;
                }
                if let Some(c) = &v.crumb {
                    check(&format!("view {name}: crumb"), c)?;
                }
                if let Some(i) = &v.intro {
                    check(&format!("view {name}: intro"), i)?;
                }
            }
            check("site.title", &cfg.site.title)?;
            for (key, s) in &cfg.i18n.strings {
                check(&format!("i18n.strings.{key}"), s)?;
                if s.reference().is_some() {
                    anyhow::bail!(
                        "i18n.strings.{key}: a global string may not itself be a \
                         reference (no chains)"
                    );
                }
            }
            for (tname, table) in &cfg.i18n.tables {
                for (index, s) in table.iter() {
                    check(&format!("i18n.tables.{tname}.{index}"), s)?;
                    if s.reference().is_some() {
                        anyhow::bail!(
                            "i18n.tables.{tname}.{index}: a table entry may not \
                             itself be a reference (no chains)"
                        );
                    }
                }
            }
            let mut referenced: Vec<&str> = Vec::new();
            {
                let mut refs: Vec<(String, &LocalizedStr)> = Vec::new();
                for (field, recs) in &cfg.records {
                    for (id, t) in recs {
                        if let Some(n) = &t.name {
                            refs.push((format!("record {field}.{id}: name"), n));
                        }
                        if let Some(i) = &t.intro {
                            refs.push((format!("record {field}.{id}: intro"), i));
                        }
                    }
                }
                for (name, v) in &cfg.views {
                    if let Some(t) = &v.title {
                        refs.push((format!("view {name}: title"), t));
                    }
                    if let Some(c) = &v.crumb {
                        refs.push((format!("view {name}: crumb"), c));
                    }
                    if let Some(i) = &v.intro {
                        refs.push((format!("view {name}: intro"), i));
                    }
                }
                refs.push(("site.title".into(), &cfg.site.title));
                for (cname, c) in &cfg.collections {
                    for (rname, r) in &c.relations {
                        if let Some(l) = &r.label {
                            refs.push((format!("collection {cname}: relation {rname} label"), l));
                        }
                    }
                }
                for (what, s) in refs {
                    for name in s.table_names() {
                        if !cfg.i18n.tables.contains_key(name) {
                            let mut knowns: Vec<&str> =
                                cfg.i18n.tables.keys().map(String::as_str).collect();
                            knowns.sort_unstable();
                            anyhow::bail!(
                                "{what}: table @{name}[…] names no table (knowns: {})",
                                if knowns.is_empty() {
                                    "(none)".into()
                                } else {
                                    knowns.join(", ")
                                }
                            );
                        }
                    }
                    let Some(key) = s.reference() else { continue };
                    if !cfg.i18n.strings.contains_key(key) {
                        let mut knowns: Vec<&str> =
                            cfg.i18n.strings.keys().map(String::as_str).collect();
                        knowns.sort_unstable();
                        anyhow::bail!(
                            "{what}: reference @{key} names no string (knowns: {})",
                            if knowns.is_empty() {
                                "(none)".into()
                            } else {
                                knowns.join(", ")
                            }
                        );
                    }
                    referenced.push(key);
                }
            }
            // Base i18n keys may be unreferenced; site keys must be used.
            let base_keys = base_i18n_string_keys();
            for key in cfg.i18n.strings.keys() {
                if !base_keys.contains(key) && !referenced.iter().any(|r| r == key) {
                    anyhow::bail!(
                        "i18n.strings.{key}: unused string — nothing references \
                         @{key}, and it is not in the base vocabulary (a typo'd \
                         base key would look exactly like this)"
                    );
                }
            }
        }
        // intro/content exclusivity
        {
            let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
            for (vname, v) in &cfg.views {
                if v.intro.is_some() && v.content.is_some() {
                    anyhow::bail!(
                        "view {vname}: declares both intro and content — the \
                         slot text and a claimed row are exclusive: the \
                         theme owns the arrangement, or the row does"
                    );
                }
                if (v.intro.is_some() || v.content.is_some()) && !v.is_materialized() {
                    anyhow::bail!(
                        "view {vname}: intro/content on a view with no route — \
                         a landing materializes somewhere"
                    );
                }
                if (v.intro.is_some() || v.content.is_some()) && v.reads_all_outputs() {
                    anyhow::bail!(
                        "view {vname}: a fold over every output serializes the \
                         route set and has no landing to give prose to"
                    );
                }
                if let Some(c) = v.content.as_deref() {
                    if let Some(other) = claimed.insert(c, vname) {
                        anyhow::bail!(
                            "row {c:?} is claimed as content by two views \
                             ({other} and {vname}) — a row serves one landing"
                        );
                    }
                }
            }
        }
        for (vname, v) in &cfg.views {
            if let Some(l) = v.partition.as_deref() {
                if !matches!(l, "*" | "default") {
                    anyhow::bail!(
                        "view {vname}: partition must be \"*\" (every declared \
                         pairing-axis member — the default) or \"default\" (opt \
                         out of pairing-parallel materialization)"
                    );
                }
                if v.reads_all_outputs() {
                    anyhow::bail!(
                        "view {vname}: a fold over every output serializes the \
                         whole route set and never materializes per pairing-axis \
                         member — filter on the axis field instead"
                    );
                }
            }
            let registered: Vec<&str> = cfg.shells.keys().map(|k| k.as_str()).collect();
            if let Some(s) = v.shell.as_deref() {
                crate::shell::check_view(s, vname, &registered)?;
            }
            if v.reads_all_outputs() {
                crate::shell::check_absent_from(v.shell.as_deref(), vname, &registered)?;
            }
        }
        for (aname, a) in &cfg.axes {
            if a.field == "shell" {
                for value in &a.values {
                    crate::shell::check_axis_value(value, aname)?;
                }
            }
        }
        for name in cfg.shells.keys() {
            crate::shell::check_registered_name(name)?;
        }
        cfg.check_profiles()?;
        cfg.check_profile_filters()?;
        Ok(())
    }
}

/// Base `[i18n.strings]` keys; read from [`BASE`] so the allowlist cannot drift.
fn base_i18n_string_keys() -> &'static std::collections::HashSet<String> {
    use std::collections::HashSet;
    use std::sync::OnceLock;
    static KEYS: OnceLock<HashSet<String>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let base: toml::Table = toml::from_str(BASE).expect("base.toml parses");
        base.get("i18n")
            .and_then(|i| i.get("strings"))
            .and_then(|s| s.as_table())
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default()
    })
}
