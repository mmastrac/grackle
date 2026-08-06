//! Filesystem walk: claim files, build rows.

use super::*;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// One walk, scopes in order, first rule wins.
pub(crate) fn walk_site(
    cfg: &Config,
    scopes: &[Scope],
    markers: &Markers,
    sidecars: &Sidecars,
    schemas: &Schemas,
    not_content: &store::NotContent,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Row>, Vec<PathBuf>)> {
    let root = cfg.root();
    // `None` = no embed subset declared (not the same as empty).
    let embed_subset = cfg.embeds.compiled()?;
    let sources: Vec<PathBuf> = scopes
        .iter()
        .filter_map(|s| s.owned())
        .map(|p| p.to_path_buf())
        .collect();
    // `exclude` says not content; `source` says content: refuse the contradiction.
    for s in scopes {
        let Some(src) = s.owned() else { continue };
        if !not_content.keeps_dir(src) {
            bail!(
                "collection {}: `source = {:?}` declares that directory to be \
                 content, and the tree's `exclude` takes it back out of the \
                 walk — so this scope would load nothing. There is one walk \
                 now: a declared source is content, and the dot/underscore \
                 skip already keeps it out of the tree. Delete the `exclude` \
                 entry that names it.",
                s.name,
                src.display()
            );
        }
    }
    let files = store::walk_tree(&root, not_content, cfg.gitignore, &sources)?;

    // View templates are not independently routable; the view owns their routes.
    let templates: Vec<PathBuf> = cfg
        .views
        .values()
        .filter_map(|v| v.template.as_ref())
        .map(PathBuf::from)
        .collect();
    let files: Vec<_> = files
        .into_iter()
        .filter(|f| !templates.contains(&f.rel))
        .filter(|f| !markers.is_marker(&f.path))
        // Sidecar declares identity of the file beside it, not by `.toml` name alone.
        .filter(|f| !sidecars.is_sidecar(&f.rel))
        .filter(|f| {
            std::fs::canonicalize(&f.path)
                .map(|p| p != cfg.config_file)
                .unwrap_or(true)
        })
        // `themes/` at site root is engine vocabulary by position, like `.slots/`.
        .filter(|f| not_content.included(&f.rel) || !under_themes(&f.rel))
        .collect();

    let claims = cfg.content_claims();

    // Extension fact: objects globs, parallel to the rule sequence.
    // Drives peek skip, i18n file axis, image dimensions, and `by_name`.
    let obj_globs: Vec<&GlobMatcher> = scopes
        .iter()
        .filter(|s| s.source.is_none())
        .flat_map(|s| s.rules.iter().map(|r| &r.matcher))
        .collect();
    let is_obj = |rel: &Path| obj_globs.iter().any(|m| m.is_match(rel));

    let mut files = files;
    files.par_iter_mut().for_each(|f| {
        if !is_obj(&f.rel) {
            f.has_front_matter = store::peek_front_matter(&f.path);
        }
    });

    let mut rows: Vec<Row> = Vec::new();
    let mut pictures: Vec<PathBuf> = Vec::new();

    for f in files {
        let object_shaped = is_obj(&f.rel);
        if object_shaped {
            pictures.push(f.rel.clone());
        }

        // Sidecar and front-matter block both supply identity: refuse both.
        let sidecar = sidecars.get(&f.rel);
        if sidecar.is_some() && f.has_front_matter {
            bail!(
                "{}: identity twice — the file carries a front-matter block and \
                 {}.toml is a sidecar for it. A sidecar exists for files that \
                 CANNOT carry a block; nothing ranks the two, so pick one: \
                 delete the sidecar, or delete the block.",
                f.rel.display(),
                f.rel.display()
            );
        }
        let has_identity = f.has_front_matter || sidecar.is_some();

        let mut claim: Option<(&Scope, Routing, PathBuf)> = None;
        // Owning scope asked and passed: not content; only co-owners below may continue.
        let mut owner_passed: Option<&Path> = None;
        for s in scopes {
            if let Some(o) = owner_passed {
                if s.owned() != Some(o) {
                    break;
                }
            }
            let Some(scope_rel) = s.relative(&f.rel) else {
                continue;
            };
            s.offered.set(s.offered.get() + 1);
            let r = apply_rules(&s.rules, &s.formats, &scope_rel, has_identity);
            if r.claimed.is_some() {
                claim = Some((s, r, scope_rel));
                break;
            }
            if let Some(o) = s.owned() {
                owner_passed = Some(o);
            }
        }
        let Some((scope, routing, scope_rel)) = claim else {
            if owner_passed.is_some() {
                continue;
            }
            bail!("no rule supplies a route for {}", f.path.display());
        };
        let extracted = filename::extract(routing.formats, &path_key(&scope_rel));
        let pairing = cfg.pairing_axis();
        let pairing_default = pairing
            .and_then(|(_, a)| a.canonical().map(str::to_owned))
            .unwrap_or_default();
        let (logical_rel, pairing_value) = match &extracted {
            Some(m) => {
                let value = pairing
                    .and_then(|(n, _)| m.axes.get(n).cloned())
                    .unwrap_or_else(|| pairing_default.clone());
                (with_logical(&scope_rel, &m.logical_stem), value)
            }
            None => (scope_rel.clone(), pairing_default.clone()),
        };
        let logical_root: PathBuf = match scope.owned() {
            Some(src) => src.join(&logical_rel),
            None => logical_rel.clone(),
        };
        scope.found.set(scope.found.get() + 1);
        check_on_demand_cover(&logical_rel, &routing)?;
        let on_demand = routing.on_demand;
        let marker_defaults = markers.defaults_for(&f.rel);
        let defaults = merged_defaults(&marker_defaults, routing.defaults);

        // `file_stem()` on `logical` returns `v1` for `v1.2-release.md`.
        let stem = logical_rel
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let captures = extracted.as_ref().map(|m| &m.captures);
        let slug = captures
            .and_then(|c| c.get("slug").cloned())
            .unwrap_or_else(|| stem.clone());

        let (fm, mut body_bytes) = match (f.has_front_matter, sidecar) {
            (true, _) => read_front_matter(&f.path)?,
            (false, Some(sc)) => (sc.front.clone(), 0),
            (false, None) => Default::default(),
        };
        let parent = logical_root.parent().unwrap_or(Path::new("")).to_path_buf();
        let schema = schemas.resolve(scope.name, &parent);
        let identity_path = sidecar.map_or(f.path.as_path(), |sc| sc.path.as_path());
        let mut checked = match has_identity {
            true => schema::validate(&schema, &fm.extra, identity_path)?,
            false => Default::default(),
        };
        schema::cascade_front(&schema, &fm, &mut checked, identity_path)?;
        if let Some(m) = &extracted {
            for (field, iso) in filename::dates_from_captures(&m.captures)? {
                if checked.values.contains_key(&field) {
                    continue;
                }
                let Some(ty) = schema.get(field.as_str()) else {
                    bail!(
                        "{}: file pattern names {{{field}.*}} but {field:?} is \
                         not declared\n  declared fields: {}",
                        f.path.display(),
                        schema::knowns(&schema)
                    );
                };
                if *ty != schema::FieldType::Date {
                    bail!(
                        "{}: file pattern names {{{field}.*}} but {field:?} is \
                         declared {}, not date",
                        f.path.display(),
                        ty.name()
                    );
                }
                schema::write_typed(
                    ty.clone(),
                    &field,
                    &toml::Value::String(iso),
                    &mut checked,
                    &format!("{}: file pattern", f.path.display()),
                )?;
            }
        }
        schema::apply_defaults(&schema, &defaults, &mut checked, &f.path)?;
        let schema_defaults = schemas.schema_defaults(scope.name, &parent);
        schema::apply_schema_defaults(&schema, &schema_defaults, &mut checked, &f.path)?;
        schema::force(&cfg.forced, &schema, &mut checked, &f.path)?;
        let worn = cascade(&checked, &f.rel)?;

        // Reads the block, not identity: sidecar says nothing about the file's bytes.
        let rendered = crate::shell::renders(f.has_front_matter, worn.shell.as_deref());
        // Description page (html output from row fields, not bytes) is not built yet.
        if rendered && object_shaped {
            bail!(
                "{}: shell = {:?} would render this file as a document, and its \
                 bytes are a picture. An image's own outputs are its bytes; \
                 a description PAGE for one is a second output and is not \
                 built yet. Route it `raw`{}.",
                f.rel.display(),
                worn.shell.as_deref().unwrap_or("-"),
                match sidecar.is_some() {
                    true =>
                        " — the sidecar still gives it a title, fields and a \
                             place in the link graph",
                    false => "",
                }
            );
        }
        if rendered && !f.has_front_matter {
            body_bytes = read_front_matter(&f.path)?.1;
        }
        let date = schema
            .iter()
            .filter(|(_, ty)| **ty == schema::FieldType::Date)
            .find_map(|(name, _)| match checked.values.get(*name) {
                Some(grackle_db::Value::Str(s)) => grackle_model::parse_date_str(s),
                _ => None,
            });
        let title = match (fm.title, rendered) {
            (Some(t), _) => Some(t),
            (None, true) => Some(implied_title(&slug)),
            (None, false) => None,
        };
        if let Some(sh) = crate::shell::degenerate(has_identity, worn.shell.as_deref()) {
            warnings.push(degenerate_warning(&f.rel, sh, &implied_title(&slug)));
        }

        let mut strong_url: Option<String> = None;
        if routing.embed && fm.permalink.is_none() {
            strong_url = Some(embed_address(cfg, &embed_subset, &f, routing.pattern)?);
        }
        let route_templates: Vec<String> = if let Some(p) = &fm.permalink {
            vec![p.clone()]
        } else if strong_url.is_some() {
            Vec::new()
        } else {
            if routing.templates.is_empty() {
                bail!("no rule supplies a route for {}", f.path.display());
            }
            RouteTokens {
                cfg,
                rel: &logical_rel,
                path: &f.path,
                hash: Default::default(),
                date,
                extracted: extracted.as_ref(),
                slug: &slug,
            }
            .render_all(routing.templates, routing.pattern, &f.path)?
        };
        let url = match route_templates.is_empty() {
            true => String::new(),
            false => canonical_url(cfg, &route_templates, &pairing_value, routing.formats)?,
        };

        let logical = logical_root.to_string_lossy().to_string();
        let claimed = claims.contains_key(logical.as_str());
        if claimed && !f.has_front_matter {
            bail!(
                "view {}: content {logical:?} has no front-matter block, so it \
                 is a static file, not a claimable row",
                claims[logical.as_str()]
            );
        }
        let row = Row {
            axis: row_axes(cfg, &route_templates, routing.formats),
            route_templates,
            width: None,
            height: None,
            key: Default::default(),
            on_demand,
            collection: scope.name.to_string(),
            rule: routing.claimed.map(str::to_string),
            slug,
            stem,
            body_bytes,
            path: f.path,
            rel: f.rel,
            // Sidecar edits must bump `version` for incremental rebuilds.
            version: match sidecar {
                Some(sc) => f.version ^ sc.version,
                None => f.version,
            },
            url,
            strong_url,
            rendered,
            front_mattered: has_identity,
            sidecar: sidecar.is_some(),
            size: f.size,
            title,
            order: fm.order,
            theme: worn.theme,
            shell: worn.shell,
            fields: {
                let mut values = checked.values;
                if let Some(m) = &extracted {
                    for (axis_name, value) in &m.axes {
                        if let Some(axis) = cfg.axes.get(axis_name) {
                            values
                                .insert(axis.field.clone(), grackle_db::Value::Str(value.clone()));
                        }
                    }
                }
                if let Some((_, axis)) = pairing {
                    values
                        .entry(axis.field.clone())
                        .or_insert_with(|| grackle_db::Value::Str(pairing_value.clone()));
                }
                values
            },
            metadata: Default::default(),
            images: checked.images,
            logical,
            claimed,
            output: None,
            alternates: Vec::new(),
            viewed_by: Vec::new(),
        };
        rows.push(row);
    }

    for (path, view) in &claims {
        if !rows.iter().any(|p| p.claimed && p.logical == *path) {
            bail!("view {view}: content {path:?} names no row in the tree");
        }
    }
    rows.par_iter_mut().for_each(|r| {
        if !is_obj(&r.rel) {
            return;
        }
        if let Ok((w, h)) = image::image_dimensions(&r.path) {
            r.width = Some(w);
            r.height = Some(h);
        }
    });

    for s in scopes {
        warnings.extend(dead_rules(s.name, &s.rules, s.found.get()));
        warnings.extend(empty_source(s));
    }
    Ok((rows, pictures))
}

pub(crate) fn read_front_matter(path: &Path) -> Result<(store::FrontMatter, usize)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (fmt, block, body) = store::split_front_matter(&text);
    let fm = if block.trim().is_empty() {
        store::FrontMatter::default()
    } else {
        match fmt {
            // A `+++` block is TOML, the same deserialization a sidecar uses.
            Some(store::FmFmt::Toml) => toml::from_str(block)
                .with_context(|| format!("front matter of {}", path.display()))?,
            _ => serde_yaml_ng::from_str(block)
                .with_context(|| format!("front matter of {}", path.display()))?,
        }
    };
    Ok((fm, body.len()))
}

/// Image fields name rows; runs after `insert_rows`.
pub(crate) fn resolve_image_fields(db: &SiteDb, schemas: &Schemas) -> Result<()> {
    for row in db.rows.iter() {
        let dir = row.rel.parent().unwrap_or(Path::new("")).to_path_buf();
        let declared = schemas.resolve(&row.collection, &dir);
        for (name, ty) in &declared {
            if *ty != crate::schema::FieldType::Image {
                continue;
            }
            let Some(grackle_db::Value::Str(target)) = row.fields.get(*name) else {
                continue;
            };
            if target.contains("://") || target.starts_with("//") {
                continue; // outside the site; no row to check it against
            }
            if db
                .rows
                .get(&grackle_db::Key::new(target.as_str()))
                .is_none()
            {
                anyhow::bail!(
                    "{}: field `{name}` names {target:?}, which is not a file this site \
                     loads. An image field is a reference to a row — check the path, and \
                     that an objects collection claims that extension.",
                    row.rel.display()
                );
            }
        }
    }
    Ok(())
}
