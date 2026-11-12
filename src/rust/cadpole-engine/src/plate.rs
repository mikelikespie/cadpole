//! The `Plate` operation: a document's bodies, plated and written as one
//! Bambu Studio project. The plan is `cadpole_script::plating`'s, the same
//! one `print_plan(...)` makes in a script; the file is
//! [`crate::print::export_bambu_project`]'s, the same writer every Bambu
//! export uses.

use std::collections::HashMap;

use cadpole_core::context::Diagnostic;
use cadpole_core::ids::BodyId;
use cadpole_print::plate::{Pin, Variant};
use cadpole_proto::v1 as pb;
use cadpole_script::plating::{PlateItem, PlateRequest, piece_rows, plate_bodies};

/// The path the plan's exports are recorded under while it is written;
/// only the bytes leave this function.
const PLAN_PATH: &str = "cadpole-plate.3mf";

pub fn plate(req: &pb::PlateRequest) -> anyhow::Result<pb::PlateResponse> {
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("plate: no document"))?;
    let (mut ctx, _) = crate::evaluate_document_with(doc, &HashMap::new(), req.tolerance, req.boolean_tolerance);
    let mut diagnostics: Vec<pb::Diagnostic> = ctx.diagnostics.iter().map(Diagnostic::to_proto).collect();
    if diagnostics.iter().any(|d| d.severity() == pb::Severity::Error) {
        return Ok(pb::PlateResponse { diagnostics, ..Default::default() });
    }
    let before = ctx.diagnostics.len();

    let picked = ctx
        .select_bodies(&req.body_ids, &req.layer_names, req.include_hidden_layers)
        .map_err(|e| anyhow::anyhow!("plate: {e}"))?;
    let mut coupon_layers = Vec::new();
    for l in &req.coupon_layers {
        coupon_layers.push(cadpole_core::layers::resolve_layer(l, &ctx.layers).map_err(|e| anyhow::anyhow!("plate: {e}"))?);
    }
    let on_coupon_layer = |ctx: &cadpole_core::Context, id: BodyId| {
        ctx.body(id).map(|b| coupon_layers.contains(&b.layer)).unwrap_or(false)
    };
    let mut coupons: Vec<BodyId> = Vec::new();
    if !coupon_layers.is_empty() {
        for b in &ctx.bodies {
            if b.visible && coupon_layers.contains(&b.layer) {
                coupons.push(b.id);
            }
        }
    }
    let main: Vec<BodyId> = picked.ids.iter().copied().filter(|id| !on_coupon_layer(&ctx, *id)).collect();
    if main.is_empty() && coupons.is_empty() {
        if picked.on_hidden_layers > 0 {
            anyhow::bail!(
                "plate: no visible bodies: all {} are on hidden layers (include_hidden_layers, `--include-hidden`, \
                 plates them; `--layer NAME` plates one)",
                picked.on_hidden_layers
            );
        }
        anyhow::bail!("plate: no visible bodies to plate");
    }

    // The pieces: the script's own named export objects when asked, else
    // every body loose for the analysis to join.
    let mut items: Vec<PlateItem> = Vec::new();
    let mut taken: Vec<BodyId> = Vec::new();
    if req.script_objects {
        for spec in &ctx.exports {
            let Some(name) = &spec.name else { continue };
            let ids: Vec<BodyId> = spec.bodies.iter().copied().filter(|id| main.contains(id) && !taken.contains(id)).collect();
            if ids.is_empty() {
                continue;
            }
            taken.extend(&ids);
            items.push(PlateItem { bodies: ids, declared: true, pin: Pin { name: Some(name.clone()), ..Pin::default() } });
        }
    }
    for id in &main {
        if !taken.contains(id) {
            items.push(PlateItem { bodies: vec![*id], ..PlateItem::default() });
        }
    }
    for id in &coupons {
        items.push(PlateItem { bodies: vec![*id], pin: Pin { coupon: true, ..Pin::default() }, ..PlateItem::default() });
    }

    let mut plan = PlateRequest { items, printer: req.printer.trim().to_owned(), ..PlateRequest::default() };
    if !plan.printer.is_empty() {
        plan.printer = cadpole_print::printer(&plan.printer)
            .ok_or_else(|| anyhow::anyhow!("plate: unknown printer {:?}; Cadpole knows {}", req.printer, cadpole_print::printer_list()))?
            .id
            .to_owned();
    }
    if req.ams > 0 {
        plan.ams = req.ams;
    }
    if !req.variants.is_empty() {
        plan.variants = req
            .variants
            .iter()
            .map(|v| Variant::parse(v).ok_or_else(|| anyhow::anyhow!("plate: variant {v:?} is not \"colour\" or \"mono\"")))
            .collect::<anyhow::Result<_>>()?;
    }
    if req.spacing > 0.0 {
        plan.spacing = req.spacing;
    }
    if req.joint_gap > 0.0 {
        plan.joint_gap = req.joint_gap;
    }
    plan.max_bridge = if req.max_bridge > 0.0 {
        req.max_bridge
    } else if req.max_bridge < 0.0 {
        0.0
    } else {
        cadpole_print::overhang::DEFAULT_MAX_BRIDGE
    };
    if !req.template.trim().is_empty() {
        plan.template = Some(crate::document_template(doc, req.template.trim(), "plate")?);
    }
    plan.path = Some(PLAN_PATH.to_owned());
    // The layer the copies go on must not be one the script uses already.
    plan.layer = (0..)
        .map(|k| if k == 0 { "plate".to_owned() } else { format!("plate_{k}") })
        .find(|n| !ctx.layers.iter().any(|l| &l.name == n))
        .expect("an unused name");

    // The script's own hand-placed print copies -- its Bambu exports and its
    // `print` layer -- judged against the best orientation found, so a
    // standing wheel in a hand-made layout is called out beside the plan.
    let hand = hand_placed(&mut ctx, &main, &plan)?;

    let first_export = ctx.exports.len();
    let mut plated = plate_bodies(&mut ctx, &plan)?;
    for line in &hand {
        plated.report.push_str(&format!("hand-placed: {line}\n"));
        diagnostics.push(Diagnostic::warning(format!("plate: hand-placed {line}")).to_proto());
    }
    let specs: Vec<_> = ctx.exports[first_export..].to_vec();
    let title = if req.title.trim().is_empty() { "plates".to_owned() } else { req.title.trim().to_owned() };
    let images = req.plate_images.then(crate::print::PlateImageOptions::default);
    let project = crate::print::export_bambu_project(&mut ctx, &specs, &title, true, images.as_ref())?;
    for d in &ctx.diagnostics[before..] {
        diagnostics.push(d.to_proto());
    }
    for w in &project.warnings {
        diagnostics.push(Diagnostic::warning(format!("plate: {w}")).to_proto());
    }

    let plates = plated
        .plan
        .plates
        .iter()
        .map(|p| pb::PlannedPlate {
            name: p.name.clone(),
            variant: p.variant.map(|v| v.as_str()).unwrap_or("coupons").to_owned(),
            pieces: p.placements.iter().map(|&k| plated.plan.pieces[plated.plan.placements[k].piece].name.clone()).collect(),
            filaments: p.colours.clone(),
            colour_changes: p.colour_changes as u32,
            layers: p.layers as u32,
        })
        .collect();
    let pieces = piece_rows(&plated)
        .into_iter()
        .zip(&plated.plan.placements)
        .map(|(row, pl)| {
            let num = |k: &str| row.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            pb::PlannedPiece {
                name: row["name"].clone(),
                plate: row["plate"].clone(),
                kind: row["kind"].clone(),
                orientation: row["orientation"].clone(),
                support_area: num("support_area"),
                modelled_support_area: num("modelled_support_area"),
                spin: num("spin"),
                reason: row["reason"].clone(),
                bodies: plated.plan.pieces[pl.piece]
                    .bodies
                    .iter()
                    .filter_map(|&b| ctx.body(plated.sources[b]).ok().map(|r| r.name.clone()))
                    .collect(),
            }
        })
        .collect();
    Ok(pb::PlateResponse {
        data: project.bytes,
        report: plated.report,
        plates,
        pieces,
        diagnostics,
        plate_images: project.plate_images.iter().map(|i| i.to_proto()).collect(),
        plate_sheet: project.plate_sheet.as_ref().map(|s| s.image.to_png()).unwrap_or_default(),
    })
}

/// The pieces of the script's own print layout -- what its Bambu exports
/// name, by object, and what lies on a layer called `print` -- that stand
/// worse than the best orientation found, as report lines. Bodies being
/// plated are not judged here.
fn hand_placed(ctx: &mut cadpole_core::Context, plating: &[BodyId], plan: &PlateRequest) -> anyhow::Result<Vec<String>> {
    let mut sets: Vec<Vec<BodyId>> = Vec::new();
    let mut seen: Vec<BodyId> = Vec::new();
    for e in &ctx.exports {
        if e.flavor != cadpole_core::export::Flavor::Bambu {
            continue;
        }
        let ids: Vec<BodyId> = e.bodies.iter().copied().filter(|id| !plating.contains(id) && !seen.contains(id)).collect();
        seen.extend(&ids);
        match e.name {
            Some(_) if !ids.is_empty() => sets.push(ids),
            _ => sets.extend(ids.into_iter().map(|id| vec![id])),
        }
    }
    let on_print: Vec<BodyId> = ctx
        .bodies
        .iter()
        .filter(|b| b.visible && b.layer == "print" && !plating.contains(&b.id) && !seen.contains(&b.id))
        .map(|b| b.id)
        .collect();
    sets.extend(on_print.into_iter().map(|id| vec![id]));
    let tol = cadpole_print::plate::score::DEFAULT_ORIENTATION_TOLERANCE;
    let mut out = Vec::new();
    for set in sets {
        for j in cadpole_script::plating::judge_orientation(ctx, &set, plan, None)? {
            if j.worse(tol) {
                out.push(format!("{}: {}", j.name, j.verdict.words()));
            }
        }
    }
    Ok(out)
}
