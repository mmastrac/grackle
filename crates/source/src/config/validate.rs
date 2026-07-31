//! Load-time config checks.

use anyhow::{Context, Result};
use std::collections::BTreeMap;

use super::{
    archive_route_fill, describe_collection, fence, split_profile, Config, Field, LocalizedStr,
    BASE, FORCE,
};

impl Config {
    /// An objects rule may not declare `front_matter` — either value
    /// (IO.md IR9). The dead-key family one table over, and the reason is one
    /// sentence: **an objects rule selects by shape; the identity gate belongs
    /// to the scopes that parse.**
    ///
    /// Two questions run over an image and they are not the same question
    /// (IO.md I7e). *Is this row a picture* is the extension fact — `load`'s
    /// `is_obj`, the objects globs asked of the path alone, before anything is
    /// peeked — and it is what keys `object_ix`, `by_name` and the header read
    /// that fills `width`/`height`, whichever scope ends up claiming the row.
    /// *Which scope claims this row* is the ordered rule sequence. A
    /// `front_matter` gate is the one spelling that makes the two disagree,
    /// because the gate reads identity and the fact reads the path:
    ///
    /// - `front_matter = true` — an image with a sidecar passes the gate, so
    ///   this scope claims it and spends its route; the blockless image beside
    ///   it fails and falls to whatever scope comes next, keeping its place in
    ///   the objects index all the same. One directory of pictures, split
    ///   across two scopes by whether someone wrote a `.toml`.
    /// - `front_matter = false` — the same split with the sides swapped.
    ///
    /// The corner was recorded three items running rather than guarded (I7a,
    /// I7d's flag 5, I7e's "the one corner where the two questions could
    /// disagree") on the strength of a premise I8 retired: that an object is
    /// never peeked, so `has_front_matter` was always `false` and such a rule
    /// claimed nothing. Since I8 the gate reads IDENTITY, and a sidecar is
    /// identity a `.png` can have — so the corner is live, and this is where
    /// it stops. Refused at config time, in the I7b family, because it is a
    /// question about the config's shape alone.
    pub(crate) fn check_objects_rule_gate(&self) -> Result<()> {
        for (name, c) in &self.collections {
            // Objects scopes only: a parsing scope's rule (posts or the tree)
            // MAY gate on `front_matter` — that is how a `.md` with no front
            // matter becomes a static copy. The gate is a contradiction only
            // where the row is never parsed, which is the sourceless scope.
            if !c.is_objects() {
                continue;
            }
            // Inherited rules are checked too. The base declares no such rule,
            // so this can only fire on something a site wrote — but the reason
            // is the rule's own text, not who wrote it, and a base that grew
            // one would be exactly as wrong.
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

    /// A rule decides an address ONCE (IO.md §4a, I11).
    ///
    /// `route` and `embed` are the two answers to "where does a row this rule
    /// claims land", and they are not layers: a routed output wins, so a rule
    /// declaring both has written a fallback that can never be reached and a
    /// reader cannot tell which half is the mistake. The routed+strong twin —
    /// one output at a canonical URL that ALSO publishes its hash address, for
    /// an affordance to expand into — is a real shape and is I12's; it is not
    /// this line, which would give it no way to say which address a citation
    /// takes.
    ///
    /// `on_demand` beside `embed` is the I7b dead-key family: it defers a
    /// ROUTE, and an embed rule mints none. Every embed-addressed row is
    /// already published on demand — that is what the policy is — so the key
    /// configures nothing here.
    ///
    /// Config time, like `check_objects_rule_gate`: it is a question about the
    /// rule's own text, so no walk and no file can change the answer, and every
    /// declared profile is projected through the same deserializer.
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
                         these rows (IO.md §4a).",
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
                         which is the whole of the policy (IO.md §4a). Delete the \
                         line.",
                        describe_collection(name, c),
                        r.pattern,
                    );
                }
            }
        }
        Ok(())
    }

    /// The fence and rung 0, for EVERY declared profile, at load (§4a,
    /// MERGE.md E2 and E1).
    ///
    /// Both are facts about this config alone — which top-level keys a profile
    /// writes, and whether a forced name is declared and typed — so both are
    /// answerable for every `[profiles.*]` entry rather than only for the one
    /// being applied (MERGE.md R5): `--profile` is a flag that picks a
    /// projection, not the moment its declaration becomes checkable.
    ///
    /// It is deliberately the CHEAP half. The expensive half — merge the
    /// overlay, deserialize it, validate the result — is the dry run in
    /// [`Config::from_toml_profile`], which needs the config's own TOML and so
    /// cannot live on a `&self`. The two do not overlap: nothing below reaches
    /// past a profile's top-level keys.
    ///
    /// Placement (`sets` vs `routes`) and view names were checked here until
    /// E2 and are not any more, because the overlay subsumes both: a profile
    /// naming an unknown view now ADDS a definition, which is what a registry
    /// does, and the addition is held to the same rules as any other — a set
    /// with no `from` is `missing field \`from\``, and a name declared under
    /// both sections collides in the one namespace `merge_queries` folds them
    /// into. (A set with no `from` and no fold shell is
    /// `crate::shell::check_absent_from`'s error, by the same argument: the
    /// overlay is held to the rules every other entry is.)
    pub(crate) fn check_profiles(&self) -> Result<()> {
        // The vocabulary rung 0 may name: the site's own `[schema]`, parsed by
        // the parser `Schemas::set_site` uses, so the two cannot come to
        // different verdicts about what a declaration says. A positional
        // `.schema.toml` is deliberately NOT in it — see `schema::site_fields`.
        let declared = crate::schema::site_fields(&self.schema.fields, "grackle.toml [schema]")?;
        let field_knowns = || {
            let mut names: Vec<&str> = declared.keys().map(String::as_str).collect();
            names.sort_unstable();
            match names.is_empty() {
                true => "none".to_string(),
                false => names.join(", "),
            }
        };
        for (pname, p) in &self.profiles {
            // The fence: §4a's iron law, and the two retired spellings, which
            // are checked before anything else this profile says because
            // everything else it says is beside the point until the key moves.
            for key in p.body().keys() {
                fence(pname, key)?;
            }
            // Rung 0: every forced name is declared, and every forced value
            // fits its declaration. Both are checked for a profile nobody is
            // building, which is R5's whole sentence one table over.
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
                    *ty,
                    field,
                    v,
                    &format!("profile {pname}: [profiles.{pname}.{FORCE}]"),
                )?;
            }
        }
        Ok(())
    }

    /// Type-check every `where` a profile wrote (§4a, MERGE.md C6a/C6b).
    ///
    /// Run from [`Config::validate`], which the projection goes through like
    /// any other config (MERGE.md C6b, E2) — so a profile's `where` is checked
    /// by the same pass as everything else the config says, rather than at the
    /// moment it happened to be written. It is keyed off `View::filter_profile`
    /// and is therefore vacuous on a config no profile wrote to.
    ///
    /// **Unknown names are deferred, not accepted.** The vocabulary reachable
    /// from a `Config` is short of the positional `.schema.toml` declarations
    /// by exactly one tree walk (see [`Config::view_filter_schema`]), so an
    /// unknown field here may be a typo or may be a perfectly good name this
    /// early. Rejecting it would make a profile's `where` STRICTER than the
    /// `where` it replaces, which is the one thing §4a says a profile is not
    /// allowed to be; and it cannot escape, because `build_views` and
    /// `resolve_pool_folds` parse the filter they find with the full schema
    /// and error naming the view. What is caught here is everything that is
    /// wrong however the tree walk turns out: syntax, arity, and types.
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

    /// Every load-time config check (split from `load` so tests can run
    /// them on in-memory configs).
    pub(crate) fn validate(&self) -> Result<()> {
        let cfg = self;
        // Zero collections builds an empty site and reports success.
        if cfg.collections.is_empty() {
            anyhow::bail!(
                "no collections declared — nothing would be built. A site \
                 needs at least one `[[collections]]` saying where its \
                 content lives, e.g.\n\n  \
                 [[collections]]\n    source = \"_posts\"\n\n  \
                   [[collections.rules]]\n  match = \"**\"\n  \
                 route = \"/blog/{{year}}/{{month:02}}/{{slug}}/\""
            );
        }
        let declared_route =
            crate::schema::site_fields(&cfg.schema.fields, "grackle.toml [schema]")?;
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
            // §5a / THEME.md §3: `layout` (or `variant`) names the member face
            // the theme must ship as `row--{face}`. No closed vocabulary —
            // themes invent faces by shipping fragments; missing faces bail
            // at render / embed time.
            // §4e: route field values (`noindex = true`, …) must be declared
            // in `[schema]` and fit their types — the same gate as
            // `[profiles.*.force]`. Keys that are neither engine view
            // vocabulary nor schema fields (including retired spellings like
            // `match` / `over`) are unknown fields.
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
                crate::schema::typed(*ty, field, val, &format!("view {vname}"))?;
            }
            // §7 q5 / MERGE.md F3: a set's `theme` can never apply, so declaring
            // one is declared-and-ignored. A set does not materialize, so there
            // is no document for a theme to dress; embedded, it is content
            // inside the HOST's document, and a document wears one stylesheet.
            // `layout` and `variant` on a set are LIVE by contrast — `tags.rs`'s
            // `{% view %}` dispatches on the layout and renders through the
            // variant — so this is about `theme` alone.
            if v.declared_set && v.theme.is_some() {
                anyhow::bail!(
                    "[sets.{vname}] declares a theme, and nothing could ever \
                     wear it. A set never lands, so there is no page for a \
                     theme to dress; embedded with {{% view {vname} %}} it \
                     wears the embedding page's theme. Theme belongs on a \
                     route — move it to the [routes.*] entry that lands this \
                     query, or drop it."
                );
            }
            // The same family, one field over (IO.md §4, IR1(c)): a set may
            // not wear a FOLD shell, because a fold lands at a route. A fold
            // serializes its query into one artifact, and an artifact is a
            // file at a path — every fold pass in `build.rs` (atom, sitemap,
            // search, the script shells) ranges over `db.routes` and finds a
            // view by the route that carries it, so a routeless one is
            // unreachable by construction. Today it fails LATE and only half
            // the time: a `from`-less set reaches `build_pool_folds` and dies
            // with "view x needs a route" mid-build, while a set WITH a `from`
            // goes through `build_views` into `insert_routeless` and publishes
            // nothing at all, silently. Config-time, both say why.
            //
            // Only a fold, deliberately: a MAP shell here is an arity mistake,
            // and `check_view` below owns that sentence.
            if v.declared_set {
                if let Some(s) = v
                    .shell
                    .as_deref()
                    .filter(|s| crate::shell::is_fold(s) || self.shells.contains_key(*s))
                {
                    anyhow::bail!(
                        "[sets.{vname}] wears shell = {s:?}, and a set never \
                         lands. A fold shell serializes its query into ONE \
                         artifact, and an artifact needs an address to be \
                         written at — so a fold belongs on a route: move it to \
                         `[routes.{vname}]` with a `path`, or drop the shell \
                         and let the set stay a query."
                    );
                }
            }
            for (fname, f) in &v.fields {
                match f {
                    Field::Expr(src) => {
                        let want = grackle_db::field_return_type(fname).ok_or_else(|| {
                            anyhow::anyhow!(
                                "view {vname}: field {fname:?} is not a known computed \
                                 field (have: summary, toc, hero)"
                            )
                        })?;
                        grackle_db::FieldExpr::parse(
                            src,
                            &grackle_db::field_schema(),
                            want,
                        )
                        .map_err(|e| anyhow::anyhow!("view {vname}: field {fname:?}: {e}"))?;
                    }
                    Field::Value(_) if fname == "summary" || fname == "toc" || fname == "hero" => {
                        anyhow::bail!(
                            "view {vname}: field {fname:?} must be an expression, \
                             not a literal value"
                        );
                    }
                    Field::Value(_) => {}
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
        // q32: each archives entry must name a view grouped by that field,
        // with a route that fills from the group key. Absent a declaration,
        // a unique view grouped by the field is enough; more than one is a
        // load error naming how to disambiguate.
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
            // Ambiguous auto-discover / route probe for list fields that can
            // own pill chrome. Declared `archives` entries are always probed;
            // a unique view grouped by a list field is enough without one.
            // Date specs (`date.year`) are not list fields and stay out
            // unless named in `archives`.
            let list_fields: Vec<&str> = declared_route
                .iter()
                .filter(|(_, ty)| **ty == crate::schema::FieldType::List)
                .map(|(n, _)| n.as_str())
                .collect();
            for field in &list_fields {
                let views: Vec<&str> = cfg
                    .views
                    .iter()
                    .filter(|(_, v)| {
                        v.group_by.as_deref() == Some(field)
                    })
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
                    // Same token law as `archive_url`: `{key}` / the field
                    // name are the group; axes stay placeholders for
                    // `select_path`. A bare `{name}` for the i18n axis stays a placeholder for `select_path`.
                    archive_route_fill(
                        tmpl,
                        field,
                        "probe",
                        cfg.pairing_axis().map(|(n, _)| n).unwrap_or(""),
                    )
                    .with_context(
                        || {
                            format!(
                                "view {name}: archive route for {field} needs more \
                                 than {{key}}"
                            )
                        },
                    )?;
                }
            }
        }
        // `trail` is the same shape of reference as `archives` — a collection
        // naming a view — and until MERGE.md C3 it was the only one nothing
        // checked: `chain` stops at an unknown name and `post_trail` walks an
        // empty chain, so `trail = "montly_archive"` produced no trail and
        // said nothing. What the machinery needs is not "a view" but a
        // SUBDIVISION CHAIN it can render a crumb from at every level
        // (`trails.rs::post_trail`), so that is what is checked.
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
            // The trail renders each GROUPED view along the `from` chain, so
            // the named view need not itself be grouped — but something in
            // its chain must be, or the trail is a chain of nothing.
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
                // A level with no route, or nothing to label it with, is
                // SKIPPED by `post_trail` — the crumb between its neighbours
                // silently goes missing, which is the failure this whole item
                // is about, one rung in.
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
        // §6f: every LocalizedStr in the config obeys ONE rule — a
        // per-member map may only name declared pairing-axis members, and must
        // include the canonical member so resolution is total. Without a
        // pairing axis, only bare strings are legal.
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
            // The global map: same member rule; values are literal (a
            // reference chain would make resolution non-total).
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
            // References must resolve, and every non-engine global string
            // must be referenced — an unused key is a load error, which is
            // what catches a typo'd engine-vocabulary override ("hom") now
            // that user keys are legal.
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
                // Relation labels (§6g) are `@refs` too, so a custom label
                // (`same_course`) can name a `[i18n.strings]` entry — and a
                // dangling one is caught here, like every other reference.
                for (cname, c) in &cfg.collections {
                    for (rname, r) in &c.relations {
                        if let Some(l) = &r.label {
                            refs.push((format!("collection {cname}: relation {rname} label"), l));
                        }
                    }
                }
                for (what, s) in refs {
                    // `@table[…]` looks up `[i18n.tables]`, not strings.
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
            // Base vocabulary (`home`, `blog`, …) is looked up by name from
            // Rust, so it may sit unreferenced. Everything else must be an
            // `"@key"` somewhere — which is what catches a typo'd override.
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
        // q45: a landing's prose is a slot text OR a claimed row, never
        // both (the engine would have to guess the arrangement); either
        // form belongs to a view that materializes routes; and a row may
        // serve exactly one landing.
        {
            let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
            for (vname, v) in &cfg.views {
                if v.intro.is_some() && v.content.is_some() {
                    anyhow::bail!(
                        "view {vname}: declares both intro and content — the \
                         slot text and a claimed row are exclusive (q45): the \
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
                         out of pairing-parallel materialization, §6f)"
                    );
                }
                if v.reads_all_outputs() {
                    anyhow::bail!(
                        "view {vname}: a fold over every output serializes the \
                         whole route set and never materializes per pairing-axis \
                         member — filter on the axis field instead (§6f)"
                    );
                }
            }
            // One vocabulary, one validator (IO.md §4, I2). A view is a query,
            // so its declared shell is a FOLD — and a map shell here is an
            // arity error rather than an unknown word, because `html` is a
            // perfectly good shell that simply wraps one output.
            let registered: Vec<&str> = cfg.shells.keys().map(|k| k.as_str()).collect();
            if let Some(s) = v.shell.as_deref() {
                crate::shell::check_view(s, vname, &registered)?;
            }
            // And the other half of the same contract (IO.md §4, I3): a fold
            // with no `from` reads every output — the successor to `from =
            // "*"` — while every other view is a listing and has to say what
            // it lists. Runs after the shell check so a map shell here is
            // still diagnosed as the arity mistake it is.
            if v.reads_all_outputs() {
                crate::shell::check_absent_from(v.shell.as_deref(), vname, &registered)?;
            }
        }
        // A per-member route is one output, so an axis spending `shell`
        // declares MAP shells. Checked here because the values never pass
        // through a row's cascade: `build.rs` reads the member's value
        // directly, and an unchecked one renders the wrong tier in silence.
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

/// Keys `[i18n.strings]` ships in the built-in base — vocabulary the engine
/// looks up by name. Read from [`BASE`] so the allowlist cannot drift from
/// the file.
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
