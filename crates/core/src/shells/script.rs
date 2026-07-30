//! Registered script shells (`[shells.*] command = "…"`): the experimental
//! fold bench (§5g — yes, the pun).

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;
use crate::markdown::Doc;
use crate::model::SiteDb;
use crate::pipeline::bodies;
use crate::pipeline::types::{PageBody, SiteOutput, Stats};
use crate::render::{self, Site};

/// Emit every view whose `shell` names a `[shells.*]` registration.
///
/// A `[shells.name] command = "…"` entry plus `shell = "name"` on a view pipes
/// the view's member rows as JSON into the command's stdin, and whatever bytes
/// it prints land at the view's route verbatim — PDF, PostScript, whatever.
/// The JSON schema is TEMP (stamped "grackle-shell/0"); it gets versioned the
/// day anything beyond an experiment depends on it. A shell that earns keeping
/// gets promoted to a built-in.
pub(crate) fn emit(
    cfg: &Config,
    db: &SiteDb,
    site: &Site<'_>,
    bodies: &HashMap<&grackle_db::Key, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    root: &Path,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    for r in &db.routes {
        let Some(view) = &r.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        let Some(shell) = v.shell.as_deref() else {
            continue;
        };
        let Some(def) = cfg.shells.get(shell) else {
            continue;
        };
        let rows: Vec<serde_json::Value> = r
            .members
            .iter()
            .filter_map(|k| db.rows.get(k))
            .map(|p| {
                let fields: serde_json::Map<String, serde_json::Value> = p
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), field_json(v)))
                    .collect();
                serde_json::json!({
                    "url": p.url,
                    "title": p.title,
                    "date": p.as_date("date").map(render::xmlschema),
                    "fields": fields,
                    "html": bodies::row_body_html(p, bodies, page_bodies).unwrap_or(""),
                })
            })
            .collect();
        let payload = serde_json::json!({
            "schema": "grackle-shell/0",
            "shell": shell,
            "view": view,
            "route": r.url,
            "site": { "url": site.url, "title": site.title, "author": site.author },
            "rows": rows,
        });
        let bytes = run(root, &def.command, &payload)
            .with_context(|| format!("view {view}: script shell {shell:?} ({})", def.command))?;
        out_map.insert(r.url.clone(), bytes);
        stats.serialized += 1;
    }
    Ok(())
}

fn field_json(v: &crate::filter::Value) -> serde_json::Value {
    use crate::filter::Value as V;
    match v {
        V::Str(s) => serde_json::json!(s),
        V::Int(i) => serde_json::json!(i),
        V::Double(d) => serde_json::json!(d),
        V::Bool(b) => serde_json::json!(b),
        V::List(items) => serde_json::json!(items),
        V::Null => serde_json::Value::Null,
    }
}

/// `sh -c command` from the site root, JSON on stdin, bytes on stdout.
/// Non-zero exit is a build error carrying stderr. Stdin is fed from a thread
/// so a script that streams output before draining its input can't deadlock
/// against the pipe buffer.
fn run(root: &Path, command: &str, payload: &serde_json::Value) -> Result<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command as Proc, Stdio};
    let mut child = Proc::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn failed")?;
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let data = serde_json::to_vec(payload)?;
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let out = child.wait_with_output()?;
    let _ = writer.join();
    if !out.status.success() {
        anyhow::bail!(
            "exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}
