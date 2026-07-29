//! Join outputs/arrangement and graph check.

use super::*;
use anyhow::Result;

/// The join's OUTPUT half, from the input side (IO.md §2): every row's
/// `output` and `alternates`, read off the routes that name it.
///
/// **Facts at planning.** This is called the moment the route list exists,
/// before `build_views` — which is what makes `output` a column a view's
/// `where` may read and get a true answer from. `viewed_by` and `inputs`
/// cannot be built here (a view's membership is what produces them), and that
/// asymmetry is the whole reason they are not filter columns.
///
/// A recomputation rather than an increment, because it is called twice: once
/// at minting, and again where q45's TEMPLATED landing retracts a claimed
/// row's routes. Cheap (one pass over routes, one over rows) and, more to the
/// point, it cannot drift from the route table by construction — the field is
/// the table's shadow, and a shadow is not a second opinion.
///
/// **The canonical one is the all-canonical member-tuple**, which is the
/// settled axis design: a row published once has an empty tuple and so is
/// trivially canonical; a row on N axes has exactly one tuple where every
/// coordinate is the first-declared value, and that is what `rel="canonical"`
/// names and the only one a fold over the output pool sees.
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
        // Routes are minted in axis-product order and only sorted later, so
        // the field sorts itself: an order that depends on when the pass ran
        // is an order a test cannot pin.
        alternates.sort();
        row.output = canonical;
        row.alternates = alternates;
    }
}

/// The join's ARRANGEMENT half (IO.md §2): each row's `viewed_by`, and each
/// output's `inputs`.
///
/// **Membership at materialization planning.** Both halves read the same
/// finished fact — which rows a view materialized — so they are built
/// together, at the one point where that fact is complete: after
/// `resolve_pool_folds`, with the route list final and q45's claims settled.
/// That is later than any filter the engine runs, which is why neither is a
/// filter column (see `route_schema`'s note).
///
/// `inputs` gets the ROW-LEVEL closure the invalidation edge set needs, minus
/// the half that cannot exist yet: the citation edges are facts about
/// *content*, so `build::join_citations` adds them after the write pass. What
/// lands here is everything planning knows —
///
/// - a row-backed output: the row it renders;
/// - a landing: the row it claims as content (literal or templated), which is
///   the one input a landing has that its member list does not name;
/// - a view route: its members, in materialization order;
/// - a fold over the output pool: the rows behind the routes it selected —
///   `route_members` is the output→output edge, and this is the same edge
///   followed one step further into the inputs database.
pub(crate) fn join_arrangement(cfg: &Config, db: &mut SiteDb) {
    // Arrangement, from the routes that did the arranging.
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

    // The inputs half. Resolved against the row store, so it is computed
    // before the borrow that writes it.
    let mut inputs: Vec<(grackle_db::Key, Vec<grackle_db::Key>)> = Vec::new();
    for r in db.routes.iter() {
        let mut ins: Vec<grackle_db::Key> = Vec::new();
        if let Some(k) = &r.row {
            ins.push(k.clone());
        }
        // q45: a landing's body is a row nothing else names — a literal claim
        // lives on the view, a templated one on the route.
        let claimed = r.content.clone().or_else(|| {
            let v = cfg.views.get(r.view.as_deref()?)?;
            let c = v.content.as_deref()?;
            (!crate::config::is_templated(c)).then(|| c.to_string())
        });
        if let Some(logical) = claimed {
            // §6f: a landing route is per locale, and so is the row it claims.
            // The route's `locale` is None for the default one, which is the
            // same spelling `route_locale` writes.
            for k in db.by_logical.get(&logical).into_iter().flatten() {
                let matches = db.rows.get(k).is_some_and(|row| {
                    let want = (row.locale() != cfg.i18n.default).then_some(row.locale());
                    r.locale() == want
                });
                if matches {
                    ins.push(k.clone());
                }
            }
        }
        ins.extend(r.members.iter().cloned());
        // A fold over the output pool arranges OUTPUTS; the rows behind them
        // are what a change to any of them would move.
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

/// IO.md §5's tripwire: no output's CONTENT may depend on its own.
///
/// **This cannot fire today, and it is not decoration.** The graph's content
/// edges run input → output (a row's bytes feed an output; nothing derives an
/// output from another output's bytes yet), so the content subgraph is
/// bipartite with every source on the inputs side and has no cycle to find.
/// What CAN loop is the facts half — a pool fold with no `where` selects its
/// own route, so `/all.xml` is its own `route_members` member on any site that
/// writes one — and that is legal by §4's column rule: a facts edge demands
/// only what planning already finished. The two are told apart by the edge
/// label and by nothing else, which is what this call is really guarding.
///
/// The mutation that proves it: label `route_members` as `Demand::Content` in
/// `graph::Graph::of` and every site with a from-less fold stops loading, this
/// error naming the fold's own URL.
///
/// **The check is armed for nothing named** (corrected at I13; `graph.rs` and
/// `io_graph.rs` were corrected at I12 and this third copy was missed). I10
/// expected renditions to bring the first output→output content edge; I12
/// measured that they do not, because the transform reads the INPUT's bytes
/// and the citing page reads the rendition's ADDRESS, which the hashing law
/// makes a planning fact. Nothing in the engine derives an output from another
/// output's content, so no live fixture is owed by any item — the day
/// something does, this is the tripwire it trips.
pub(crate) fn check_graph(db: &SiteDb) -> Result<()> {
    if let Err(cycle) = grackle_model::graph::Graph::of(db).check_acyclic() {
        bail!(
            "dependency cycle: {} — an output's content may not depend on its own (IO.md §5)",
            grackle_model::graph::describe_cycle(&cycle)
        );
    }
    Ok(())
}
