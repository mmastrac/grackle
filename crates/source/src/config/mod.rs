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

/// Replace each `@name[index]` in `text` with the table entry (empty if
/// missing). `@@` yields a literal `@`.
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
        // `parse_table_ref` wants the leading `@`.
        if let Some((name, index)) = parse_table_ref(&rest[at..]) {
            let norm = normalize_table_index(index);
            out.push_str(cfg.i18n_table(name, &norm, member));
            // Advance past `@name[index]` using the original index span.
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

    /// Load, then project through a profile (§4a).
    ///
    /// `dev` is implicit: it needs no declaration, and undeclared it changes
    /// nothing — which is what makes `serve` safe to default to it. Any
    /// other name must be declared, so a typo is a load error naming what
    /// exists rather than a build that silently ships the wrong projection.
    pub fn load_profile(path: &Path, profile: Option<&str>) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        // The projection happens inside `from_toml`, on the merged TOML, and
        // not to the `Config` afterwards (MERGE.md E2): a profile is an
        // OVERLAY, so what it produces is an ordinary config that has been
        // through the same merge, the same deserializer and — below — the same
        // `validate` as the default projection. There is nothing left here that
        // knows a profile from a site.
        let mut cfg = Config::from_toml_profile(&text, profile)
            .with_context(|| format!("parsing config {}", path.display()))?;
        cfg.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        cfg.config_file = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        cfg.resolve_default_content();
        cfg.validate()?;
        Ok(cfg)
    }

    /// Whether this config inherits the base (§4d). Written once so that the
    /// error naming the two legal values is one sentence with one author, and
    /// `--effective` cannot come to a different verdict than the load does.
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

    /// The config the engine actually runs, as TOML with per-key provenance —
    /// `grackle config --effective` (MERGE.md B3). DESIGN.md §4d calls this
    /// the thing that makes §4d "inheritance rather than magic".
    ///
    /// Stops before deserialization on purpose, for two reasons. The merged
    /// `toml::Value` is the honest artifact — it is exactly what the
    /// deserializer is handed, where a re-serialization of `Config` would be a
    /// second rendering of the truth with its own bugs. And it means the
    /// command answers on a config the engine has REJECTED, which is when a
    /// person most needs to see what the engine thinks they wrote.
    pub fn effective(path: &Path, profile: Option<&str>) -> Result<String> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        Config::effective_toml(&text, &path.display().to_string(), profile)
            .with_context(|| format!("reading config {}", path.display()))
    }

    /// [`Config::effective`] on text already in hand; `label` names the file
    /// in the preamble.
    pub fn effective_toml(text: &str, label: &str, profile: Option<&str>) -> Result<String> {
        let value: toml::Value = toml::from_str(text)?;
        let mut trace = Trace::recording();
        let inherits_base = Config::extends_of(&value)?;
        let mut merged = if inherits_base {
            merge_base_traced(value, &mut trace)?
        } else {
            // `extends = "none"`: no merge happened, so there is nothing for
            // the merge to have recorded — every key is the site's own. Walked
            // through the recorder's own descent so the atoms land at exactly
            // the paths a merged config's would.
            note_table(
                &mut trace,
                &mut Vec::new(),
                &Config::shape(),
                &value,
                Prov::Site,
            );
            value
        };
        // The keys neither file wrote still have values, and those are the ones
        // a reader has least chance of finding. Not a copy of serde's defaults:
        // the same functions `#[serde(default = "…")]` names.
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
            // MERGE.md C6e: the note asserts a projection, so it has to check
            // that there is one. `dev` needs no declaration and changes
            // nothing, which is what makes it the safe default for `serve`.
            let declared = merged.get("profiles").and_then(|p| p.as_table());
            let mut known: Vec<&str> = declared
                .map(|t| t.keys().map(String::as_str).collect())
                .unwrap_or_default();
            known.push("dev");
            known.sort_unstable();
            let real = declared.is_some_and(|t| t.contains_key(name));
            preamble.push_str(&match (known.contains(&name), real) {
                // The projection is IN the table below (MERGE.md E2), which is
                // what retired this note's old caveat: the overlay went through
                // the same `merge_table` as the base merge, one layer nearer,
                // so a `# profile` line is the profile writing a key exactly
                // the way a `# site` line is the site writing one.
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
                // `dev`, undeclared: a real projection that writes nothing.
                (true, false) => format!(
                    "#\n# NOTE: profile {name:?} is implicit (§4a) — this config declares no\n\
                     # [profiles.{name}], and an undeclared profile projects nothing. The\n\
                     # table below is what it would build.\n"
                ),
                // Keep printing: the merge below is what the reader asked for
                // and is unaffected — the profile is the part that would not
                // have happened, and the build would have refused outright.
                (false, _) => format!(
                    "#\n# NOTE: {name:?} names no profile (knowns: {}), so nothing\n\
                     # would be projected — `build --profile {name}` is a load error.\n\
                     # The merged config below is unaffected and is printed anyway.\n",
                    known.join(", ")
                ),
            });
            if real {
                // The same `project` the load path runs, with the recorder
                // turned on — so what is printed is what the build did, which
                // is B3's whole design carried to one more writer.
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

    /// Parse and fold the query sections. The one parse path, so a config
    /// built in a test is the same shape as one read from disk — including
    /// the §4d base merge, which is why a test wanting isolation says
    /// `extends = "none"` rather than reaching for a second entry point.
    pub fn from_toml(text: &str) -> Result<Config> {
        Config::from_toml_profile(text, None)
    }

    /// [`Config::from_toml`], as projected through `profile` (§4a, MERGE.md
    /// E2).
    ///
    /// The projection sits between the base merge and the deserializer, which
    /// is the whole of the design: the profile is one more writer over the
    /// merged table, and everything below this line — deserialization,
    /// `merge_collections`, `merge_queries`, `validate` — runs on the result
    /// without knowing a projection happened.
    ///
    /// **Every declared profile is dry-run here** when none is selected
    /// (MERGE.md R5's principle, E2's shape): the same merge, the same
    /// deserializer and the same `validate`, for each `[profiles.*]` entry, so
    /// a broken overlay is a load error with no `--profile` anywhere.
    pub fn from_toml_profile(text: &str, profile: Option<&str>) -> Result<Config> {
        let value: toml::Value = toml::from_str(text)?;
        let inherits_base = Config::extends_of(&value)?;
        // Whose view is whose, recorded before the merge blurs the two. A view
        // the PROFILE declared is the author's too — it is in the file they are
        // reading — so the overlay's names join the list below.
        let mut declared: Vec<String> = ["sets", "routes"]
            .iter()
            .filter_map(|k| value.get(k)?.as_table())
            .flat_map(|t| t.keys().cloned())
            .collect();
        // And whose RULE is whose. The site's rules prepend (§1's annotation),
        // so how many it wrote per collection is all the provenance a list
        // needs: the first n are the site's, the tail is the base's.
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
        // The projection (MERGE.md E2). `Trace::off()` here for the same reason
        // the base merge passes it: `--effective` is the only recorder, and it
        // runs this same function through `project` with one turned on.
        let (value, forced, patched) = match profile {
            None => (value, toml::Table::new(), Vec::new()),
            Some(name) => project(value, name, &mut Trace::off())?,
        };
        declared.extend(patched.iter().cloned());
        let mut cfg: Config = match value.try_into() {
            Ok(c) => c,
            Err(e) => {
                // Deserializing from a merged Value loses TOML spans, and a
                // typo in the site's own file is the common failure. So
                // re-parse the site's text alone: if THAT is what's wrong, its
                // error carries the line number and is the actionable one.
                //
                // But the site's text alone is not a valid config on a site
                // that leans on the base for required `[site]` keys — it fails
                // with a `missing field` the merged value never had, and
                // returning THAT would report a fiction and swallow the real
                // error (MERGE.md R7). So the re-parse is only allowed to
                // speak when it says the same thing: `message()` is serde's
                // sentence with the span and the key path stripped off, which
                // is what makes the two comparable — the merged error carries
                // a key path and no span, the re-parse a span and (in its
                // Display) no key path, and only the sentence is common to
                // both. Same sentence = the same failure, now with a line
                // number; anything else = the site's text has a *different*
                // problem (or none), and the merged error is the true one.
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
            // `site_rules` is keyed by the same identity the merge pairs on,
            // so its KEY SET is "the collections the site declared" and its
            // values are how many rules each of them wrote. One read of the
            // pre-merge TOML answers both questions.
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
            // Rung 0, lifted out of the profile so the loader can reach it
            // without knowing about profiles (§2, MERGE.md E1).
            cfg.forced = forced.into_iter().collect();
            // Who wrote the `where` a view carries. The overlay replaced the
            // whole definition, so the config no longer remembers on its own —
            // and an error about a filter must not send a reader to a `[sets]`
            // entry whose text is not the text in the message (MERGE.md C6a).
            for vname in &patched {
                if let Some(v) = cfg.views.get_mut(vname) {
                    v.filter_profile = Some(name.to_string());
                }
            }
            cfg.profile = Some(name.to_string());
        } else {
            // Every declared profile, projected and validated, at every load —
            // so a typo in a projection nobody is building today is a load
            // error today (MERGE.md R5). It is the same three passes the
            // selected profile gets: fence, merge + deserialize, validate.
            //
            // `resolve_default_content` is deliberately NOT re-run per profile:
            // it reads the filesystem, `from_toml` has no directory, and every
            // difference it makes is one that ADDS an error (a claimed row, a
            // route stood down) — so the dry run is strictly the more lenient
            // of the two and cannot invent a failure the real load would not
            // have. See MERGE.md §6, E2.
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

    /// When `[i18n] axis` names a declared axis, that axis must list at least
    /// the canonical member. Identity lives on the axis — not a cached copy.
    pub fn check_pairing_axis(&self) -> Result<()> {
        let Some(name) = self.i18n.axis.as_deref() else {
            return Ok(());
        };
        let Some(a) = self.axes.get(name) else {
            // Axis name is aspirational until `[axes.*]` declares it.
            return Ok(());
        };
        anyhow::ensure!(
            !a.values.is_empty(),
            "[axes.{name}]: values must list at least the canonical member"
        );
        Ok(())
    }

    /// The axis `[i18n] axis` names, when that axis is declared. Sites pick
    /// which file axis drives pairing, view partition, and display-string
    /// keys; the engine only follows the pointer.
    pub fn pairing_axis(&self) -> Option<(&str, &Axis)> {
        let name = self.i18n.axis.as_deref()?;
        self.axes.get(name).map(|a| (name, a))
    }

    /// The pairing axis's canonical member (`values[0]`), when declared.
    pub fn pairing_canonical(&self) -> Option<&str> {
        self.pairing_axis().and_then(|(_, a)| a.canonical())
    }

    /// Whether any collection `file` pattern spends this axis (a file axis
    /// pairs siblings via `by_logical`; a route axis multiplies one row).
    pub fn is_file_axis(&self, name: &str) -> bool {
        let bare = format!("{{{name}}}");
        let namespaced = format!("{{axis:{name}}}");
        self.collections
            .values()
            .any(|c| c.file.iter().any(|f| f.contains(&bare) || f.contains(&namespaced)))
    }

    /// A row's value for an axis's declared field, or that axis's canonical
    /// when the field is unstamped / empty.
    pub fn axis_on(&self, row: &(impl grackle_db::filter::Row + ?Sized), axis_name: &str) -> Option<String> {
        let axis = self.axes.get(axis_name)?;
        Some(match row.field(&axis.field) {
            grackle_db::Value::Str(s) if !s.is_empty() => s,
            _ => axis.canonical()?.to_owned(),
        })
    }

    /// True when the row sits at an axis's canonical member (or the axis is
    /// absent / the field unstamped).
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

    /// Stamp a non-canonical axis field into route fields; clear for canonical.
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

    /// Whether two rows share a value on `axis` (both resolve via [`axis_on`]).
    pub fn same_on(
        &self,
        a: &impl grackle_db::filter::Row,
        b: &impl grackle_db::filter::Row,
        axis: &str,
    ) -> bool {
        self.axis_on(a, axis) == self.axis_on(b, axis)
    }

    /// The configured pairing axis's member on `row` — LocalizedStr keys,
    /// HTML `lang`, twin pivots. Composed from [`pairing_axis`] + [`axis_on`];
    /// without a pairing axis, empty.
    pub fn pairing_member(&self, row: &(impl grackle_db::filter::Row + ?Sized)) -> String {
        match self.pairing_axis() {
            Some((n, _)) => self.axis_on(row, n).unwrap_or_default(),
            None => String::new(),
        }
    }

    /// [`I18nCfg::string`] with the pairing axis's canonical as fallback.
    pub fn i18n_string<'a>(&'a self, key: &str, member: &str) -> &'a str {
        let canon = self.pairing_canonical().unwrap_or(member);
        self.i18n.string(key, member, canon)
    }

    /// [`I18nCfg::table`] with the pairing axis's canonical as fallback.
    pub fn i18n_table<'a>(&'a self, name: &str, index: &str, member: &str) -> &'a str {
        let canon = self.pairing_canonical().unwrap_or(member);
        self.i18n.table(name, index, member, canon)
    }

    /// [`I18nCfg::text`] with the pairing axis's canonical as fallback.
    pub fn i18n_text<'a>(&'a self, s: &'a LocalizedStr, member: &str) -> &'a str {
        let canon = self.pairing_canonical().unwrap_or(member);
        self.i18n.text(s, member, canon)
    }

    /// Render a display string: `"@key"` / `"@table[index]"` / inline template
    /// with embedded `@table[…]` after `{token}` substitution (§6f).
    ///
    /// A whole-value `"@key"` whose string contains `{tokens}` or `@table[…]`
    /// is expanded again (date templates: `@medium_date` →
    /// `"{day} @months[{month}] {year}"`). String values may not themselves
    /// be `"@other"` references (validate forbids chains).
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
                // `@@…` → literal leading `@` (no table/string resolution).
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
                    // Expand the looked-up string as an inline template (not
                    // another `@key`), so `@medium_date` fills date tokens.
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

    /// Format a calendar date through an `[i18n.strings]` template (`short_date`,
    /// `medium_date`, `long_date`, …) at `member`. Missing template → `""`.
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

    /// Axis values a `{axis:NAME}` file token may capture (skip canonical).
    pub fn axis_values_for_file(&self) -> crate::filename::AxisValues<'_> {
        self.axes
            .iter()
            .map(|(n, a)| (n.as_str(), a.values.as_slice()))
            .collect()
    }

    /// Settle every `default_content` offer against the tree (§4d). A
    /// filesystem question, so it happens here rather than in `from_toml`,
    /// which has no directory to resolve against.
    ///
    /// Three outcomes, and each leaves exactly one thing at the URL:
    ///
    /// * **No such row** — the route lands on its own, as an ordinary landing.
    /// * **The row exists and places `{% view <name> %}`** — it accepts the
    ///   offer, and the claim is an ordinary q45 mode B claim from there on.
    /// * **The row exists and declines** — it wants the URL to itself, and it
    ///   already has its own route there, so the offered route stands down.
    ///
    /// The third case is what keeps this safe to inherit. A site whose
    /// homepage is a hand-built page has said nothing about `[routes.home]`
    /// and must not have its rendering changed by a route it never wrote.
    fn resolve_default_content(&mut self) {
        let root = self.root();
        for (name, v) in self.views.iter_mut() {
            let Some(pat) = v.default_content.as_deref() else {
                continue;
            };
            // A templated offer resolves per route once the group keys exist, so
            // the filesystem question this settles is answered post-materialize.
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

    /// The table a collection contributes to: its `name`, else its source
    /// directory with any leading underscore stripped. `_posts` is the
    /// `posts` table; `recipes/` is the `recipes` table; a source of `.`
    /// has no directory to name it and is `entries`.
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

    /// Key every collection by its resolved name. This names the thing
    /// `from` refers to — it does NOT decide which table rows land in, which
    /// is still `kind` (`_posts` and `_drafts` are two `posts` collections
    /// feeding one corpus, §4, and stay two entries here).
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

    /// Fold `[sets]` and `[routes]` into the one `views` map: a set never
    /// lands, a route always does. The namespace is shared with collections,
    /// so a name may live in exactly one of the three.
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
            // Checked here rather than in validate() because it is a question
            // about the config's shape alone — `resolve_default_content` has
            // folded one into the other by the time validate runs.
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
            // Which section declared it, kept because the fold below is what
            // loses it and a profile is held to the same split (§4a).
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

    /// The fields CONFIG declares, as a filter schema. Not the whole declared
    /// set — `.schema.toml` is read during the tree walk, which has not
    /// happened yet wherever this is used.
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

    /// The vocabulary a view's own `where` type-checks against — one function,
    /// so a profile patching that `where` is held to exactly the same words.
    ///
    /// It is the same three-way dispatch the build makes: a fold over every
    /// output ranges over ROUTES (`resolve_pool_folds`), an objects view over
    /// objects, and
    /// everything else over rows with every declared field beside the
    /// built-ins (`Base::resolve` → `Schemas::row_filter_schema`). Which
    /// matters because the three vocabularies genuinely differ — `kind` is a
    /// route column, `title` is a row column, and `dir` is a `Str` on a row
    /// and a `Bool` on a route — so "the union of all three" is not a schema
    /// anything could type-check against.
    ///
    /// **One narrowing, and it is why this is a pre-check rather than the
    /// check.** `.schema.toml` declarations are read during the tree walk,
    /// which has not run wherever a `Config` method can be called, so
    /// `config_declared_schema()` stands in for `Schemas::declared()`. A name
    /// only a positional file declares is invisible here — and is deferred,
    /// not rejected: see [`Config::check_profile_filters`].
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

    /// Flatten a view's `from` chain into a base collection plus every filter
    /// along the way.
    ///
    /// `from` may name a **query-only** view (nothing to inherit ambiguously)
    /// or a **grouped, unpaginated** view — subdivision (§5c): the composer
    /// refines the parent's partition, so it must itself be grouped, and the
    /// parent's route/layout are *not* inherited (the child declares its own).
    /// Composing over a paginated view is punted (open question 30): a
    /// pageable year with months on its root raises a URL-namespace question
    /// we haven't answered.
    pub fn query(&self, name: &str) -> Result<Query> {
        let mut filters = Vec::new();
        let mut patched = Vec::new();
        // Nearest wins, and we walk outermost-first, so the first one seen.
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
            // No `from` at all terminates it hardest: a fold over every output
            // ranges over no collection, so there is nothing to name and
            // nothing to check (IO.md §4). Its filters still travel — a chain
            // cannot compose over it (nothing may name a route), but its own
            // `where` is the query.
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
            // A collection or a union terminates the chain.
            let next = from.single().and_then(|s| self.views.get(s));
            let Some(next) = next else {
                // `cur`, not `name`: the entry that CARRIES the `from` is the
                // one an author has to edit, and on a composed chain it is not
                // the one whose query was asked for.
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

    /// What a terminated chain is allowed to name (§5c).
    ///
    /// One name may be a collection. A union may name only collections,
    /// and they must share a kind: the members decide the vocabulary a `where`
    /// type-checks against and whether the rows are parsed, so two kinds in one
    /// union is a query with two answers to both questions.
    ///
    /// `"*"` is a name like any other now (IO.md I3) and names nothing, so it
    /// lands in the generic arm below — except that the generic arm would send
    /// its reader off to look for a collection called `*`, and the fix is to
    /// delete a line rather than to write one. It gets a sentence of its own:
    /// the value is invalid, not deprecated.
    ///
    /// `carrier` is the view whose `from` this is; `asked` is the view whose
    /// query was requested, which on a composed chain is a different entry
    /// (`blog_index` composes over `published`, and it is `published`'s
    /// `from` that terminates). Both, because a message naming only one of
    /// them sends the reader to the wrong table — see [`Config::whose_from`].
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

    /// Who owns a bad `from`, when it is not the entry the reader is looking
    /// at (MERGE.md C7b).
    ///
    /// Two things blur that. A view composes over another, so the entry
    /// carrying the broken reference need not be the one whose query was
    /// asked for. And **an inherited `from` is the one reference in this
    /// config a site can break without touching the entry that carries it**:
    /// views are a registry keyed by NAME, so an inherited set survives every
    /// rename a site can perform, while collections key on `source` (§1's
    /// annotation), so renaming the collection at `_posts` retires the name
    /// `posts` — and the base's `[sets.published] from = "posts"` then names
    /// nothing, on a site whose grackle.toml contains no `published` at all.
    /// The old message quoted that line back at its reader as if they had
    /// written it.
    ///
    /// Empty for a view the site wrote and asked about directly, which is
    /// every message this does not need to explain.
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

    /// The `from` chain from `name` down to its base, nearest view first.
    /// The one chain walker — everything derived from composition
    /// (`fields_for`, `group_specs`, `grouped_chain`) reads this. Assumes the
    /// chain is acyclic, which `query()` validated at load.
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

    /// The `group_by` specs governing a view, outermost ancestor first. This
    /// is subdivision (§5c): a grouped view composed `from` a grouped view
    /// refines the parent's partition, so the parent's spec applies before
    /// the child's.
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
    /// `from` chain, nearest declaration winning per name — fields compose
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

    /// The enum record for one value of a grouped field, if declared.
    pub fn record(&self, field: &str, id: &str) -> Option<&RecordCfg> {
        self.records.get(field)?.get(id)
    }

    /// The display name a field value wears for a locale (§6f): the
    /// record's name through the standard hierarchy (inline / "@ref" /
    /// global), else the id itself.
    pub fn record_name<'a>(&'a self, field: &str, id: &'a str, locale: &str) -> &'a str {
        match self.record(field, id).and_then(|r| r.name.as_ref()) {
            Some(n) => self.i18n_text(n, locale),
            None => id,
        }
    }

    /// The slug a field value wears in routes (§6f). Defaults to the id —
    /// URLs are the only surface slugs touch; keys, params and titles
    /// keep the id.
    pub fn record_slug<'a>(&'a self, field: &str, id: &'a str) -> &'a str {
        self.record(field, id)
            .and_then(|t| t.slug.as_deref())
            .unwrap_or(id)
    }

    /// Content-claimed rows: logical source path → the owning view.
    /// Uniqueness is a validate() invariant, so a map is honest.
    pub fn content_claims(&self) -> BTreeMap<&str, &str> {
        self.views
            .iter()
            .filter_map(|(n, v)| v.content.as_deref().map(|c| (c, n.as_str())))
            // A templated `content` resolves to a different row per route, so its
            // claims are settled post-materialization (see `load`), not here.
            .filter(|(c, _)| !is_templated(c))
            .collect()
    }

    /// The view that owns archive routes for a list field: a collection's
    /// `archives` entry, else the unique view grouped by that field. Ambiguity
    /// without a declaration is a load error, so None means "no archive".
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

    /// An archive URL for a list-field value under a pairing-axis member
    /// (q32 + §6f): the owning view's route template(s) via
    /// [`crate::load::select_path`]. None = no archive, and the pill renders
    /// unlinked.
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

    /// The text a row embeds as (`[schema.embeddings].string`). `{body}` is
    /// the markdown body; every other hole is a row field (lists join with
    /// `", "`). Empty template → empty text (nothing to embed).
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
                    grackle_db::Value::List(v) => v.join(", "),
                    grackle_db::Value::Str(s) => s,
                    grackle_db::Value::Int(n) => n.to_string(),
                    grackle_db::Value::Bool(b) => b.to_string(),
                    grackle_db::Value::Double(d) => d.to_string(),
                    grackle_db::Value::Null => String::new(),
                },
            })
        })
    }

    /// Weighted plain-text streams for the search index (`[schema.search].index`).
    /// `title` and list/scalar fields boost; `body` is the caller's body text
    /// at weight 1 (HTML already stripped, or markdown for the CLI).
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
                        for s in v {
                            if !s.is_empty() {
                                out.push((BOOST, s));
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

    /// Named fields for search-hit display (`[schema.search].store`). A
    /// missing `title` falls back to the URL so a hit still has a label;
    /// date-typed fields use the xmlschema form.
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
                                    .filter(|s| !s.is_empty())
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

/// Fill an archive route template for probing or pill URLs. The group key
/// (or field name) takes `key`; `{axis:…}` tokens stay placeholders for
/// [`crate::load::select_path`]. A bare `{name}` for the pairing axis is kept
/// when that axis is set.
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
