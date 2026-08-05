//! Join outputs/arrangement and graph check.

use super::*;
use anyhow::Result;

/// Join output half: `output` and `alternates` from routes.
/// At planning, before `build_views`; recomputed when q45 retracts routes.
pub(crate) fn join_outputs(db: &mut SiteDb) {
    let mut by_row: HashMap<grackle_db::Key, (Option<grackle_db::Key>, Vec<grackle_db::Key>)> =
        HashMap::new();
    for r in db.routes.iter() {
        let Some(k) = &r.row else { continue };
        let e = by_row.entry(k.clone()).or_default();
        if r.axis.iter().all(|m| m.canonical) {
            e.0 = Some(r.id.clone());
        } else {
            e.1.push(r.id.clone());
        }
    }
    for row in db.rows.iter_mut() {
        let (canonical, mut alternates) = by_row.remove(&row.key).unwrap_or_default();
        // Mint order is axis-product; sort so the field does not depend on when the pass ran.
        alternates.sort();
        row.output = canonical;
        row.alternates = alternates;
    }
}

/// Arrangement half: `viewed_by` and `inputs` after materialization planning.
pub(crate) fn join_arrangement(cfg: &Config, db: &mut SiteDb) {
    let mut viewed_by: HashMap<grackle_db::Key, Vec<grackle_db::Key>> = HashMap::new();
    for r in db.routes.iter() {
        for m in &r.members {
            viewed_by.entry(m.clone()).or_default().push(r.id.clone());
        }
    }
    for row in db.rows.iter_mut() {
        let mut seen = viewed_by.remove(&row.key).unwrap_or_default();
        seen.sort();
        seen.dedup();
        row.viewed_by = seen;
    }

    // Resolve against the row store before the mut borrow that writes `inputs`.
    let mut inputs: Vec<(grackle_db::Key, Vec<grackle_db::Key>)> = Vec::new();
    for r in db.routes.iter() {
        let mut ins: Vec<grackle_db::Key> = Vec::new();
        if let Some(k) = &r.row {
            ins.push(k.clone());
        }
        // q45: literal claim on the view, templated claim on the route.
        let claimed = r.content.clone().or_else(|| {
            let v = cfg.views.get(r.view.as_deref()?)?;
            let c = v.content.as_deref()?;
            (!crate::config::is_templated(c)).then(|| c.to_string())
        });
        if let Some(logical) = claimed {
            // §6f: landing route is per pairing-axis member; so is the claimed row.
            for k in db.by_logical.get(&logical).into_iter().flatten() {
                let matches = db.rows.get(k).is_some_and(|row| match cfg.pairing_axis() {
                    Some((n, _)) => cfg.same_on(row, r, n),
                    None => true,
                });
                if matches {
                    ins.push(k.clone());
                }
            }
        }
        ins.extend(r.members.iter().cloned());
        for rk in &r.route_members {
            if let Some(row) = db.routes.get(rk).and_then(|o| o.row.clone()) {
                ins.push(row);
            }
        }
        ins.sort();
        ins.dedup();
        inputs.push((r.id.clone(), ins));
    }
    for (id, ins) in inputs {
        if let Some(r) = db.routes.get_mut(&id) {
            r.inputs = ins;
        }
    }
}

/// The graph tripwire: no output's content may depend on its own.
/// Content edges are input->output today (no cycle); facts half can loop legally.
/// Label `route_members` as `Demand::Content` in `graph::Graph::of` to arm it.
pub(crate) fn check_graph(db: &SiteDb) -> Result<()> {
    if let Err(cycle) = grackle_model::graph::Graph::of(db).check_acyclic() {
        bail!(
            "dependency cycle: {} — an output's content may not depend on its own",
            grackle_model::graph::describe_cycle(&cycle)
        );
    }
    Ok(())
}
