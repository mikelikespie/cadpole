//! Printing: the `PrintCheck` operation, the analysis views and the colour
//! side of 3MF export.
//!
//! The geometry lives in `cadpole-print` and the file format in
//! `cadpole-export-3mf`; this module is the part that knows about protobuf and
//! about the feature tree — which bodies to check, what colour each one is,
//! and which faces a `paint(...)` call named.
//!
//! Per-face colour is recorded as a `paint` feature; the key names live in
//! [`cadpole_core::paint`], which is the one place `cadpole-script` (which writes
//! them) and this module (which reads them) have to agree.

use std::collections::BTreeMap;

use cadpole_core::Context;
use cadpole_core::paint::{self, hex, unhex};
use cadpole_core::context::Diagnostic;
use cadpole_core::geom::Vec3;
use cadpole_core::ids::BodyId;
use cadpole_proto::v1 as pb;

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

/// A [`cadpole_print::Profile`] from a request.
///
/// `printer` names a machine from [`cadpole_print::PRINTERS`] and supplies the
/// starting numbers; every other argument that is set overrides it, so a
/// request may say "a Prusa MK4 with a 0.6 mm nozzle" without spelling out the
/// bed. An empty `printer` is the built-in default (a 256 mm cube, 0.4 mm,
/// 0.2 mm layers, PLA), which is what every request meant before profiles
/// existed — so nothing that worked before changes.
#[allow(clippy::too_many_arguments)]
pub fn profile(
    printer: &str,
    min_wall: f64,
    nozzle: f64,
    perimeters: f64,
    layer_height: f64,
    material: &str,
    max_overhang: f64,
    bed: &[f64],
    build_direction: &[f64],
) -> anyhow::Result<cadpole_print::Profile> {
    let mut p = if printer.trim().is_empty() {
        cadpole_print::Profile::default()
    } else {
        cadpole_print::Profile::named(printer).map_err(|e| anyhow::anyhow!("{e}"))?
    };
    if nozzle > 0.0 {
        p.nozzle = nozzle;
        p.min_wall = nozzle * p.perimeters;
    }
    if perimeters > 0.0 {
        p.perimeters = perimeters;
        p.min_wall = p.nozzle * perimeters;
    }
    if min_wall > 0.0 {
        p.min_wall = min_wall;
    }
    if layer_height > 0.0 {
        p.layer_height = layer_height;
        // A tip within one layer is not an island, whatever the layer.
        p.support.min_island = layer_height;
    }
    if !material.trim().is_empty() {
        p.material = cadpole_print::Material::parse(material).ok_or_else(|| {
            anyhow::anyhow!("unknown material {material:?}; Cadpole knows {}", cadpole_print::Material::list())
        })?;
    }
    if max_overhang > 0.0 {
        p.max_overhang_degrees = max_overhang;
    }
    p.bed = match bed {
        [] => p.bed,
        [a] => [*a, *a, *a],
        [a, b] => [*a, *b, p.bed[2]],
        [a, b, c, ..] => [*a, *b, *c],
    };
    if let [x, y, z, ..] = build_direction {
        p.build_direction = Vec3::new(*x, *y, *z);
    }
    Ok(p)
}

// ---------------------------------------------------------------------------
// PrintCheck
// ---------------------------------------------------------------------------

pub fn print_check(req: &pb::PrintCheckRequest) -> anyhow::Result<pb::PrintCheckResponse> {
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("print check: no document"))?;
    let (mut ctx, _) =
        crate::evaluate_document_with(doc, &std::collections::HashMap::new(), req.tolerance, req.boolean_tolerance);
    let diagnostics: Vec<pb::Diagnostic> = ctx.diagnostics.iter().map(Diagnostic::to_proto).collect();
    if diagnostics.iter().any(|d| d.severity() == pb::Severity::Error) {
        return Ok(pb::PrintCheckResponse { diagnostics, ok: false, ..Default::default() });
    }
    let p = profile(
        &req.printer,
        req.min_wall,
        req.nozzle,
        req.perimeters,
        req.layer_height,
        &req.material,
        req.max_overhang_degrees,
        &req.bed,
        &req.build_direction,
    )?;

    // The longest bridge: the request's, else Cadpole's default; a
    // negative number turns bridging off.
    let max_bridge = if req.max_bridge > 0.0 {
        req.max_bridge
    } else if req.max_bridge < 0.0 {
        0.0
    } else {
        cadpole_print::overhang::DEFAULT_MAX_BRIDGE
    };
    let mut p = p;
    support_rules(req, &mut p.support);
    p.validate().map_err(|e| anyhow::anyhow!("print check: {e}"))?;
    // What a whole-document export would write, narrowed by
    // `body_ids` and `layer_names`.
    let picked = ctx
        .select_bodies(&req.body_ids, &req.layer_names, req.include_hidden_layers)
        .map_err(|e| anyhow::anyhow!("print check: {e}"))?;
    let ids: Vec<(BodyId, String, String)> = picked
        .ids
        .iter()
        .filter_map(|id| ctx.body(*id).ok().map(|b| (b.id, b.id_string(), b.name.clone())))
        .collect();
    if ids.is_empty() {
        if picked.on_hidden_layers > 0 {
            anyhow::bail!(
                "print check: no visible bodies: all {} are on hidden layers (include_hidden_layers, `cadpole check \
                 --include-hidden`, checks them; `--layer NAME` checks one)",
                picked.on_hidden_layers
            );
        }
        anyhow::bail!("print check: no visible bodies");
    }

    // The chord-error-vs-gap rule is judged over every visible body
    // at once, whichever were asked for: a gap is between two of them.
    let chord = cadpole_print::chord::chord_issues(&mut ctx, p.chord_gap_fraction)?;

    // One object of a Bambu project (`export(..., name=)`) is one piece on
    // the printer: a slicer unions its parts' slices, so a wall resting on a
    // slab is not an overhang and two thin parts side by side are one wall.
    // Such an object is checked fused, as the slicer will print it,
    // and its parts are not checked one by one; a fuse that fails says why
    // and falls back to the parts.
    let chosen: std::collections::BTreeSet<BodyId> = ids.iter().map(|(id, _, _)| *id).collect();
    let mut objects: Vec<(String, String, Vec<BodyId>)> = Vec::new();
    for e in &ctx.exports {
        let Some(object) = &e.name else { continue };
        let parts: Vec<BodyId> = e.bodies.iter().copied().filter(|id| chosen.contains(id)).collect();
        if parts.len() < 2 || objects.iter().any(|(_, _, p)| *p == parts) {
            continue;
        }
        objects.push((e.path.clone(), object.clone(), parts));
    }
    let mut fused_reports: BTreeMap<BodyId, pb::BodyPrintReport> = BTreeMap::new();
    let mut in_object: std::collections::BTreeSet<BodyId> = std::collections::BTreeSet::new();
    let mut fallback_notes: BTreeMap<BodyId, pb::PrintIssue> = BTreeMap::new();
    for (path, object, parts) in &objects {
        match fused_mesh(&mut ctx, parts) {
            Ok(mesh) => {
                let report = cadpole_print::check_with(&mesh, &p, max_bridge);
                let orientation = req.orient.then(|| cadpole_print::orient::orient(&mesh, &p));
                let mut r = report_to_proto(&format!("{path}:{object}"), object, &report, orientation.as_ref());
                r.fused_body_ids = parts.iter().filter_map(|id| ctx.body(*id).ok().map(|b| b.id_string())).collect();
                r.issues.push(pb::PrintIssue {
                    check: "fused".to_owned(),
                    severity: cadpole_print::Severity::Info.as_str().to_owned(),
                    message: format!(
                        "{path}: the {} parts of object {object:?} are checked as the one piece a slicer prints them \
                         as, fused",
                        parts.len()
                    ),
                    at: None,
                    value: parts.len() as f64,
                });
                in_object.extend(parts.iter().copied());
                fused_reports.insert(parts[0], r);
            }
            Err(e) => {
                fallback_notes.insert(
                    parts[0],
                    pb::PrintIssue {
                        check: "fused".to_owned(),
                        severity: cadpole_print::Severity::Info.as_str().to_owned(),
                        message: format!(
                            "{path}: the {} parts of object {object:?} could not be fused into one piece ({e}); they \
                             are checked one by one",
                            parts.len()
                        ),
                        at: None,
                        value: parts.len() as f64,
                    },
                );
            }
        }
    }

    let mut bodies = Vec::with_capacity(ids.len());
    let mut ok = true;
    let mut meshed = Vec::with_capacity(ids.len());
    for (id, id_string, name) in ids {
        if let Some(r) = fused_reports.remove(&id) {
            ok &= r.ok;
            bodies.push(r);
        }
        if in_object.contains(&id) {
            continue;
        }
        let (mesh, topo) = ctx.body_mesh(id)?.clone();
        let mut report = cadpole_print::check_named(&mesh, Some(&topo), &p, max_bridge);
        for (_, issue) in chord.iter().filter(|(b, _)| *b == id) {
            report.push_issue(issue.clone());
        }
        let orientation = req.orient.then(|| cadpole_print::orient::orient(&mesh, &p));
        ok &= report.ok();
        let mut r = report_to_proto(&id_string, &name, &report, orientation.as_ref());
        if let Some(note) = fallback_notes.remove(&id) {
            r.issues.push(note);
        }
        if req.optimal_orientation {
            // As placed against the best orientation `print_plan` finds:
            // a hand-placed print layer's wheel left standing on its tread.
            let tol = if req.orientation_tolerance > 0.0 {
                req.orientation_tolerance
            } else {
                cadpole_print::plate::score::DEFAULT_ORIENTATION_TOLERANCE
            };
            let plan = cadpole_script::plating::PlateRequest {
                printer: cadpole_print::printer(&req.printer).map(|p| p.id.to_owned()).unwrap_or_default(),
                max_bridge,
                ..Default::default()
            };
            for j in cadpole_script::plating::judge_orientation(&mut ctx, &[id], &plan, None)? {
                if j.worse(tol) {
                    let i = j.verdict.issue(&name);
                    r.issues.push(pb::PrintIssue {
                        check: i.check.as_str().to_owned(),
                        severity: i.severity.as_str().to_owned(),
                        message: i.message,
                        at: None,
                        value: i.value,
                    });
                }
            }
        }
        bodies.push(r);
        meshed.push((id, name, mesh, topo));
    }
    // Between the bodies: every mesh export of two or more bodies —
    // a 3MF, STL or OBJ, which is what a slicer opens — is one print, and
    // the print-in-place rules run over its bodies: gaps that fuse,
    // horizontal gaps that are not whole layers, bodies in the air. A
    // finding lands on the first body of its pair, in that body's `issues`
    // like every other, so nothing in the response's shape changes.
    //
    // Every one of them is a *warning* here, whatever the rule's own
    // severity, because `cadpole check` cannot know whether such an export is
    // one multi-material part, whose bodies are meant to fuse (a die with
    // its pips), or a plate of separate parts that must not (a planetary):
    // the script has no `group=` yet to say so. The author states the set
    // with `print_check([...])` or `assert_printable([...])`, and there the
    // rules keep their severities. A STEP export is not a print and is not
    // judged; neither is a document with no multi-body mesh export.
    let checked: BTreeMap<BodyId, usize> = meshed.iter().enumerate().map(|(k, (id, _, _, _))| (*id, k)).collect();
    let mut seen: Vec<Vec<BodyId>> = Vec::new();
    for e in &ctx.exports {
        if !matches!(e.format.as_str(), "stl" | "3mf" | "obj") {
            continue;
        }
        let set: Vec<BodyId> = e.bodies.iter().copied().filter(|id| checked.contains_key(id)).collect();
        if set.len() < 2 || seen.contains(&set) {
            continue;
        }
        seen.push(set.clone());
        let refs: Vec<cadpole_print::assembly::BodyRef<'_>> = set
            .iter()
            .map(|id| {
                let (_, name, mesh, topo) = &meshed[checked[id]];
                cadpole_print::assembly::BodyRef { name, mesh, topo: Some(topo) }
            })
            .collect();
        for f in cadpole_print::assembly::check(&refs, &p, ctx.tolerance) {
            let i = &f.issue;
            let b = &mut bodies[checked[&set[f.body]]];
            if i.check.is_support() {
                b.support_free = false;
            }
            b.issues.push(pb::PrintIssue {
                check: i.check.as_str().to_owned(),
                severity: cadpole_print::Severity::Warning.as_str().to_owned(),
                message: format!("{}: {}", e.path, i.message),
                at: i.at.map(|p| p.to_proto()),
                value: i.value,
            });
        }
    }
    if req.support_free {
        // Only what says it needs support, and the verdict on that alone.
        let support: std::collections::BTreeSet<&str> = [
            cadpole_print::Check::Overhang,
            cadpole_print::Check::Island,
            cadpole_print::Check::Cantilever,
            cadpole_print::Check::HorizontalHole,
            cadpole_print::Check::Bridge,
        ]
        .iter()
        .map(|c| c.as_str())
        .collect();
        ok = true;
        for b in &mut bodies {
            b.issues.retain(|i| support.contains(i.check.as_str()) && i.severity != "info");
            b.support_free = b.issues.is_empty();
            b.ok = b.support_free;
            ok &= b.ok;
        }
    }
    Ok(pb::PrintCheckResponse { bodies, diagnostics, ok })
}

/// The request's support rules over the profile's: 0 keeps the profile's
/// (Cadpole's default, or the printer's), a negative number is the rule's
/// strictest setting -- every patch, ledge, hole and tip judged -- or, for
/// the two ratios, the check off.
fn support_rules(req: &pb::PrintCheckRequest, r: &mut cadpole_print::SupportRules) {
    let set = |v: f64, slot: &mut f64| {
        if v > 0.0 {
            *slot = v;
        } else if v < 0.0 {
            *slot = 0.0;
        }
    };
    set(req.max_overhang_area, &mut r.max_overhang_area);
    set(req.max_cantilever, &mut r.max_cantilever);
    set(req.max_horizontal_hole, &mut r.max_horizontal_hole);
    set(req.min_island, &mut r.min_island);
    set(req.max_slenderness, &mut r.max_slenderness);
    set(req.max_height_to_base, &mut r.max_height_to_base);
    if req.slender_width > 0.0 {
        r.slender_width = req.slender_width;
    }
}

/// The weld the fused check joins parts at, at most: as close as two parts
/// can be and still be two to a slicer, and below the 0.05 mm grid a box
/// model is usually drawn on, so a chain of lines one grid step apart is
/// never welded into one.
pub const FUSE_WELD: f64 = 0.01;

/// The parts of one `name=` object as the one piece a slicer prints them as.
/// Every part that is an orthogonal solid -- an
/// axis-aligned box, or a body every face of which is perpendicular to an
/// axis, which is what `union(weld=)` makes of boxes -- is taken apart into
/// the boxes it fills (`gridjoin::axis_box`, `gridjoin::orthogonal_boxes`),
/// and the boxes of every such part are joined exactly on one grid, welded
/// at the document's tessellation tolerance or [`FUSE_WELD`], the smaller:
/// two parts that share a face -- a wall of one filament standing on the
/// slab of another -- are one solid there, where a mesh boolean of the
/// two had to guess at the tie and left doubled faces and pinched edges
/// that the check then reported as the piece's own. Every other part is
/// moved onto that grid's planes within the weld (`gridjoin::snap_body`)
/// and unioned on by the mesh kernel. A result that is not a solid is an
/// error, which the check answers by checking the parts one by one: the
/// fuse's own failure is never reported as the piece's. Only checked, never
/// stored or written.
pub fn fused_mesh(ctx: &mut Context, parts: &[BodyId]) -> anyhow::Result<cadpole_core::tessellate::Mesh> {
    use cadpole_core::gridjoin;
    use cadpole_core::kernel::BoolOp;
    use cadpole_core::mesh_kernel::{self, SourceFace};
    let tolerance = ctx.tolerance;
    let weld = tolerance.min(FUSE_WELD);
    let mut boxes = Vec::new();
    let mut others = Vec::new();
    for id in parts {
        let mb = ctx.body_mesh(*id)?;
        if let Some((lo, hi, _)) = gridjoin::axis_box(&mb.0, &mb.1) {
            boxes.push(gridjoin::GridBox { lo, hi, sides: [0, 1, 2, 3, 4, 5] });
            continue;
        }
        // Coordinates of one plane a float rounding apart are one line. A
        // part that is orthogonal only in part -- a corbel's slope, a
        // slanted sliver a cleanup left -- puts its exact core on the grid
        // too, so its contacts with the others are joined (and bridged)
        // there, and is unioned whole after.
        let extent = mb.0.bbox().map(|(lo, hi)| (hi.x - lo.x).max(hi.y - lo.y).max(hi.z - lo.z)).unwrap_or(0.0);
        let core = gridjoin::orthogonal_core(&mb.0, 1e-6 * (1.0 + extent));
        match core {
            Some(core) => {
                boxes.extend(core.boxes.into_iter().map(|(lo, hi)| gridjoin::GridBox { lo, hi, sides: [0, 1, 2, 3, 4, 5] }));
                if !core.complete {
                    others.push(mb.clone());
                }
            }
            None => others.push(mb.clone()),
        }
    }
    let mut acc: Option<mesh_kernel::MeshBody> = None;
    let mut grid: Option<([Vec<f64>; 3], Vec<([f64; 3], [f64; 3])>)> = None;
    if !boxes.is_empty() {
        let g = gridjoin::union_boxes(&boxes, weld, 6)?;
        let groups: Vec<SourceFace> = (0..7)
            .map(|k| SourceFace {
                name: format!("fused/{}", if k < 6 { gridjoin::SIDES[k] } else { "weld" }),
                aliases: Vec::new(),
                surface_kind: "group",
                surface: cadpole_core::surface_class::SurfaceClass::Other,
                geometry: None,
            })
            .collect();
        grid = Some((g.lines.clone(), g.welded.clone()));
        acc = Some(mesh_kernel::from_indexed(&g.positions, g.triangles, g.provenance, &groups, None, tolerance)?);
    }
    let opts = cadpole_core::solid::SolidOptions { pair_limit: mesh_kernel::UNION_PAIR_LIMIT, ..Default::default() };
    let solid = |m: &mesh_kernel::MeshBody| cadpole_core::solid::validate_with(&m.0, &opts).is_solid();
    for mb in others {
        // Onto the grid's planes within the weld, and no further: the part's
        // own core is on the grid already, coincident with its faces, and
        // pushing a face against the others would tilt its neighbours off
        // that core into slivers. When that union is not a solid, the part
        // as it is.
        let moved = grid.as_ref().and_then(|(l, _)| {
            let mut m = mb.clone();
            (gridjoin::snap_body(&mut m, l, weld).snap.moved > 0).then_some(m)
        });
        acc = Some(match acc {
            None => moved.unwrap_or(mb),
            Some(a) => match moved.and_then(|m| mesh_kernel::boolean(BoolOp::Union, &a, &m, tolerance).ok()).filter(solid) {
                Some(joined) => joined,
                None => mesh_kernel::boolean(BoolOp::Union, &a, &mb, tolerance)?,
            },
        });
    }
    let (mesh, _) = acc.ok_or_else(|| anyhow::anyhow!("no parts"))?;
    let report = cadpole_core::solid::validate_with(&mesh, &opts);
    if !report.is_solid() {
        anyhow::bail!("fused, they are not one solid ({})", report.problems().join("; "));
    }
    Ok(mesh)
}

fn v3(v: Vec3) -> Option<pb::Vec3> {
    Some(v.to_proto())
}

pub fn report_to_proto(
    body_id: &str,
    name: &str,
    r: &cadpole_print::Report,
    orientation: Option<&cadpole_print::Orientation>,
) -> pb::BodyPrintReport {
    pb::BodyPrintReport {
        body_id: body_id.to_owned(),
        name: name.to_owned(),
        triangles: r.triangles as u32,
        volume_mm3: r.volume_mm3,
        area_mm2: r.area_mm2,
        bbox: r.bbox.map(|(lo, hi)| pb::BBox { min: v3(lo), max: v3(hi) }),
        watertight: r.mesh.watertight,
        boundary_edges: r.mesh.boundary_edges as u32,
        holes: r
            .mesh
            .holes
            .iter()
            .map(|h| pb::PrintHole {
                center: v3(h.center),
                perimeter_mm: h.perimeter,
                edges: h.edges as u32,
                bbox: Some(pb::BBox { min: v3(h.bbox.0), max: v3(h.bbox.1) }),
            })
            .collect(),
        // Exact counts from the validator, not the clustered spots.
        non_manifold_edges: r.mesh.validator.non_manifold.len() as u32,
        flipped_edges: r.mesh.validator.flipped.len() as u32,
        inverted_normals: r.mesh.inverted,
        self_intersections: r.mesh.validator.crossings.len() as u32,
        degenerate_triangles: (r.mesh.validator.degenerate.len() + r.mesh.collapsed) as u32,
        solid: r.mesh.solid,
        internal_faces: r.mesh.validator.overlaps.len() as u32,
        components: r.mesh.components as u32,
        genus: r.mesh.genus.map(|g| g as i32).unwrap_or(-1),
        fused_body_ids: Vec::new(),
        min_wall_mm: if r.thickness.min.is_finite() { r.thickness.min } else { 0.0 },
        thin_area_mm2: r.thickness.thin_area,
        unsupported_area_mm2: r.overhang.unsupported_area,
        max_overhang_degrees: r.overhang.max_degrees,
        bed_contact_area_mm2: r.overhang.bed_contact_area,
        bridge_area_mm2: r.overhang.bridge_area,
        longest_bridge_mm: r.overhang.longest_bridge,
        bridges: r.overhang.bridges as u32,
        max_bridge_mm: r.overhang.max_bridge.max(0.0),
        support_area_mm2: r.overhang.support_area,
        largest_overhang_mm2: r.overhang.largest_patch,
        longest_cantilever_mm: r.overhang.longest_cantilever,
        islands: r.support.islands as u32,
        deepest_island_mm: r.support.deepest_island,
        horizontal_holes: r.support.horizontal_holes as u32,
        wide_holes: r.support.wide_holes as u32,
        slender: r.support.slender as u32,
        slenderness: r.support.slenderness,
        first_layer_area_mm2: r.support.first_layer_area,
        // A part with no first layer reports a large number rather than inf,
        // which JSON cannot carry.
        height_to_base: if r.support.height_to_base.is_finite() { r.support.height_to_base } else { 1e9 },
        support_free: r.support_findings().next().is_none(),
        size_mm: v3(r.bed.size),
        fits_bed: r.bed.fits,
        fits_bed_rotated: r.bed.fits_rotated,
        bed_excess_mm: v3(r.bed.excess),
        issues: r
            .issues
            .iter()
            .map(|i| pb::PrintIssue {
                check: i.check.as_str().to_owned(),
                severity: i.severity.as_str().to_owned(),
                message: i.message.clone(),
                at: i.at.map(|p| p.to_proto()),
                value: i.value,
            })
            .collect(),
        orientation: orientation.map(|o| pb::PrintOrientation {
            down: v3(o.chosen.down),
            label: o.chosen.label.clone(),
            changed: o.changed,
            unsupported_area_mm2: o.chosen.unsupported_area,
            bed_contact_area_mm2: o.chosen.bed_contact_area,
            height_mm: o.chosen.height,
            reasoning: o.reasoning.clone(),
        }),
        ok: r.ok(),
        summary: r.summary(),
        profile: r.profile_name.clone(),
        mass_g: r.mass_g,
        layers: r.layers as u32,
        split: r.bed.split.as_ref().map(|s| pb::PrintSplit {
            axis: s.axis.to_owned(),
            cuts: s.cuts.clone(),
            pieces: s.pieces as u32,
            call: s.call.clone(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Colour: bodies, paint features, filament slots
// ---------------------------------------------------------------------------

/// One body's colour assignment, ready for the 3MF writer.
pub struct Painted {
    pub body: BodyId,
    pub id_string: String,
    pub name: String,
    /// 1-based filament slot for the whole body.
    pub slot: u32,
    /// Per-triangle override, in `mesh.indices` order; `0` = use `slot`.
    pub tri_slots: Vec<u32>,
}

/// Filament slots in slot order, and the bodies that use them.
pub struct Palette {
    pub filaments: Vec<cadpole_export_3mf::Filament>,
    pub bodies: Vec<Painted>,
    pub warnings: Vec<String>,
}

/// Work out the filament slots for a set of bodies.
///
/// Slots are assigned **in order of first use** walking the bodies in feature
/// order and then their paint features in feature order, so the same script
/// always produces the same slot numbering. `paint(..., slot=n)` pins a slot
/// instead; a pinned slot wins, and a colour pinned to two different slots is
/// a warning, not an error (the file is still written).
pub fn palette(ctx: &mut Context, ids: &[BodyId]) -> anyhow::Result<Palette> {
    palette_with(ctx, ids, &BTreeMap::new())
}

/// [`palette`] with `export(extruder=)` pins: a body in `pins`
/// prints in that slot and claims no slot for its colour; the pinned slot
/// takes the body's colour unless a colour of its own already sits there.
pub fn palette_with(ctx: &mut Context, ids: &[BodyId], pins: &BTreeMap<BodyId, u32>) -> anyhow::Result<Palette> {
    let mut filaments: Vec<cadpole_export_3mf::Filament> = Vec::new();
    let mut warnings = Vec::new();
    // colour hex -> slot
    let mut slots: BTreeMap<String, u32> = BTreeMap::new();
    let mut pinned: BTreeMap<u32, String> = BTreeMap::new();

    let paints = paint::paints(ctx);
    // Pinned slots first, so an explicit `slot=` really does own that number.
    for p in &paints {
        let Some(slot) = p.slot else { continue };
        match pinned.get(&slot) {
            Some(other) if other != &p.hex => warnings
                .push(format!("filament slot {slot} is pinned to both {other} and {}; keeping {other}", p.hex)),
            Some(_) => {}
            None => {
                pinned.insert(slot, p.hex.clone());
                slots.insert(p.hex.clone(), slot);
            }
        }
    }

    let mut next = 1u32;
    let assign = |color: String, slots: &mut BTreeMap<String, u32>, next: &mut u32| -> u32 {
        if let Some(s) = slots.get(&color) {
            return *s;
        }
        while pinned.contains_key(next) {
            *next += 1;
        }
        let s = *next;
        *next += 1;
        slots.insert(color, s);
        s
    };

    let mut painted = Vec::new();
    for id in ids {
        let (id_string, name, base, label) = {
            let b = ctx.body(*id)?;
            (b.id_string(), b.name.clone(), hex(b.material.base_color), b.material.name.clone())
        };
        let _ = &label;
        let slot = match pins.get(id) {
            Some(s) => *s,
            None => assign(base.clone(), &mut slots, &mut next),
        };
        painted.push(Painted { body: *id, id_string, name, slot, tri_slots: Vec::new() });
    }

    // The paint features may introduce more colours; they are assigned in
    // feature order, after the bodies' own.
    for pt in &paints {
        if !pt.hex.is_empty() {
            assign(pt.hex.clone(), &mut slots, &mut next);
        }
    }

    for pt in &paints {
        let Some(p) = painted.iter_mut().find(|p| p.id_string == pt.body_id) else { continue };
        let Some(&slot) = slots.get(&pt.hex) else { continue };
        let (mesh, topo) = ctx.body_mesh(p.body)?;
        if p.tri_slots.is_empty() {
            p.tri_slots = vec![0; mesh.triangle_count()];
        }
        if pt.faces.is_empty() {
            // The whole body: this is how a filament slot gets pinned.
            p.slot = slot;
            p.tri_slots.iter_mut().for_each(|s| *s = 0);
            continue;
        }
        let wanted: Vec<u32> =
            pt.faces.iter().filter_map(|n| topo.faces.iter().position(|f| &f.name == n).map(|i| i as u32)).collect();
        let missing: Vec<String> = pt
            .faces
            .iter()
            .filter(|n| !topo.faces.iter().any(|f| &f.name == *n))
            .map(|n| format!("{n:?}"))
            .collect();
        if !missing.is_empty() {
            warnings.push(format!("paint: {} has no face named {}", p.name, missing.join(", ")));
        }
        for (t, f) in mesh.face_ids.iter().enumerate() {
            if wanted.contains(f) {
                p.tri_slots[t] = slot;
            }
        }
    }

    // Materialise the filament list in slot order.
    let max = slots.values().copied().chain(pins.values().copied()).max().unwrap_or(0);
    filaments.resize(max as usize, cadpole_export_3mf::Filament { name: String::new(), color: [0.75, 0.75, 0.75] });
    for (color, slot) in &slots {
        filaments[*slot as usize - 1] = cadpole_export_3mf::Filament { name: String::new(), color: unhex(color) };
    }
    // A pinned slot no colour claimed takes the first body pinned to it.
    let mut claimed: std::collections::BTreeSet<u32> = slots.values().copied().collect();
    for id in ids {
        if let Some(slot) = pins.get(id) {
            if claimed.insert(*slot) {
                filaments[*slot as usize - 1].color = ctx.body(*id)?.material.base_color;
            }
        }
    }
    // Give a filament the label of the first body's material that uses it.
    for p in &painted {
        let b = ctx.body(p.body)?;
        if !b.material.name.is_empty() {
            if let Some(f) = filaments.get_mut(p.slot as usize - 1) {
                if f.name.is_empty() {
                    f.name = b.material.name.clone();
                }
            }
        }
    }
    Ok(Palette { filaments, bodies: painted, warnings })
}

/// Write the visible bodies as a 3MF package. `name` is `export(name=)`:
/// one object of that name holding every body, which is
/// [`cadpole_export_3mf::Grouping::Part`] under that title.
pub fn export_3mf(
    ctx: &mut Context,
    ids: &[BodyId],
    title: &str,
    slicer: &str,
    group: &str,
    name: Option<&str>,
) -> anyhow::Result<(Vec<u8>, Vec<String>)> {
    let slicer = cadpole_export_3mf::Slicer::parse(slicer).map_err(|e| anyhow::anyhow!(e))?;
    let group = match name {
        Some(_) => cadpole_export_3mf::Grouping::Part,
        None => cadpole_export_3mf::Grouping::parse(group).map_err(|e| anyhow::anyhow!(e))?,
    };
    let title = name.unwrap_or(title);
    let pal = palette(ctx, ids)?;
    // An instance body is written as its *part body's* mesh, once
    // per part, with one build item per instance carrying the pose: the
    // 3MF `<object>` is the part and the `<item>`s are the instances.
    // Instances of one part that differ in colour, or that carry paint of
    // their own, are separate objects, because an item cannot carry a
    // material. The exploded view is not a pose and never reaches a file.
    struct Group {
        name: String,
        mesh_of: cadpole_core::BodyId,
        slot: u32,
        tri_slots: Vec<u32>,
        items: Vec<[f64; 12]>,
        layer: String,
    }
    let mut groups: Vec<Group> = Vec::new();
    for p in &pal.bodies {
        let rec = ctx.body(p.body)?;
        let shared = match (rec.instance, rec.source()) {
            (Some(_), Some((src, pose))) if p.tri_slots.iter().all(|s| *s == 0) => Some((src, pose)),
            _ => None,
        };
        match shared {
            Some((src, pose)) => {
                let item = pose.to_3mf();
                if let Some(g) = groups.iter_mut().find(|g| g.mesh_of == src && g.slot == p.slot && g.tri_slots.is_empty()) {
                    g.items.push(item);
                } else {
                    let name = ctx.body(src)?.name.clone();
                    let layer = rec.layer.clone();
                    groups.push(Group { name, mesh_of: src, slot: p.slot, tri_slots: Vec::new(), items: vec![item], layer });
                }
            }
            None => groups.push(Group {
                name: p.name.clone(),
                mesh_of: p.body,
                slot: p.slot,
                tri_slots: p.tri_slots.clone(),
                items: Vec::new(),
                layer: rec.layer.clone(),
            }),
        }
    }
    // Tessellate first so the meshes can be borrowed for the whole call.
    let mut meshes = Vec::with_capacity(groups.len());
    for g in &groups {
        meshes.push(ctx.body_mesh(g.mesh_of)?.0.clone());
    }
    let parts: Vec<cadpole_export_3mf::Part<'_>> = groups
        .iter()
        .zip(&meshes)
        .map(|(g, m)| cadpole_export_3mf::Part {
            name: g.name.clone(),
            mesh: m,
            slot: g.slot,
            tri_slots: g.tri_slots.clone(),
            items: g.items.clone(),
            layer: g.layer.clone(),
        })
        .collect();
    let scene = cadpole_export_3mf::Scene {
        title: title.to_owned(),
        parts,
        palette: pal.filaments,
        slicer,
        group,
        ..cadpole_export_3mf::Scene::default()
    };
    let written = cadpole_export_3mf::write(&scene);
    let mut warnings = pal.warnings;
    warnings.extend(written.warnings);
    Ok((written.bytes, warnings))
}

// ---------------------------------------------------------------------------
// Bambu Studio projects
// ---------------------------------------------------------------------------

/// Which plate each `export()` call of a project lands on, resolved from
/// `plate=` numbers and names: a number is that plate; a name is the plate
/// first given that name, or the next number after every plate so far; no
/// `plate=` is plate 1. Returns the plate number per call and the plate
/// names (empty for a plate only ever numbered).
pub fn resolve_plates(specs: &[cadpole_core::context::ExportSpec]) -> (Vec<u32>, Vec<String>) {
    use cadpole_core::context::PlateRef;
    let mut names: Vec<String> = Vec::new();
    let mut per_call = Vec::with_capacity(specs.len());
    for spec in specs {
        let n = match &spec.plate {
            None => 1,
            Some(PlateRef::Number(n)) => *n,
            Some(PlateRef::Name(name)) => match names.iter().position(|x| x.as_str() == name.as_str()) {
                Some(i) => i as u32 + 1,
                None => {
                    names.push(name.clone());
                    names.len() as u32
                }
            },
        };
        if names.len() < n as usize {
            names.resize(n as usize, String::new());
        }
        per_call.push(n);
    }
    (per_call, names)
}

// ---------------------------------------------------------------------------
// Plate scenes: the thumbnails and the standalone plate images are two
// renders of ONE scene per plate, built by
// `plate_scene`, so the picture in the project and the picture beside it
// cannot show different things.
// ---------------------------------------------------------------------------

/// The background a plate thumbnail is rendered on, then keyed to alpha 0:
/// uncovered pixels come back bit-exactly this colour, and light, so the
/// anti-aliased rim of a part blends towards Bambu Studio's pale panels.
const THUMBNAIL_BG: [f32; 3] = [0.97, 0.97, 0.97];

/// The key colour behind a transparent plate image. Light, so the
/// anti-aliased rim blends towards a white page, and *off-neutral* on
/// purpose: the bed is a neutral grey under a neutral rig, so no pixel of it
/// can land on this colour and be keyed out by mistake, where the
/// thumbnail's neutral key could be.
const PLATE_KEY: [f32; 3] = [0.965, 0.972, 0.984];

/// The plate image's camera: a three-quarter view from a little above
/// `iso`, on the product presets' 24° lens, so a square bed reads as a
/// bed (not a sliver, as it does from `product`'s 18° elevation) and a
/// part's faces do not converge.
pub const PLATE_VIEW: &str = "az=33,el=32";
pub const PLATE_FOV_DEGREES: f64 = 24.0;
/// Air around the fitted plate, as a fraction of its size.
pub const PLATE_MARGIN: f64 = 0.08;
/// The bed's drawn thickness as a fraction of its longer side (2 mm on a
/// 256 mm bed), and its colour: a light neutral grey, linear.
const BED_THICKNESS: f64 = 0.008;
const BED_COLOR: [f64; 3] = [0.58, 0.58, 0.57];
/// The bed's top sits this far under z = 0, so a part's underside and the
/// bed never share a plane.
const BED_TOP: f64 = -0.02;

/// A plate's scene: the given bodies where they stand on the plate -- each
/// moved by its offset, the plate's centring and its object's set-down,
/// so the scene is in the plate's own coordinates with the bed's
/// corner at the origin -- each drawn in the colour given for it -- the
/// filament slot's, so a body pinned to another slot shows the colour it
/// will come off the printer in, as Bambu's own thumbnails do -- or its own
/// material's for `None`. The bodies' materials are restored before this
/// returns.
pub fn plate_scene(ctx: &mut Context, bodies: &[(BodyId, Option<[f64; 3]>, Vec3)]) -> anyhow::Result<crate::RenderScene> {
    let mut items = Vec::with_capacity(bodies.len());
    let (mut lo, mut hi) = (Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN));
    for (id, colour, offset) in bodies {
        let (saved, id_string) = {
            let b = ctx.body_mut(*id)?;
            let saved = b.material.base_color;
            if let Some(c) = colour {
                b.material.base_color = *c;
            }
            (saved, b.id_string())
        };
        let scene = crate::scene_of_bodies(ctx, &[id_string]);
        ctx.body_mut(*id)?.material.base_color = saved;
        let moved = cadpole_core::Transform::translation(*offset);
        for mut item in scene.items {
            if offset.length() > 0.0 {
                item.mesh = item.mesh.transformed(&moved);
                for line in item.edges.iter_mut().chain(item.highlight_edges.iter_mut()) {
                    for p in line.iter_mut() {
                        *p = *p + *offset;
                    }
                }
            }
            if let Some((a, b)) = item.mesh.bbox() {
                lo = lo.min(a);
                hi = hi.max(b);
            }
            items.push(item);
        }
    }
    let bbox = (lo.x <= hi.x).then_some((lo, hi));
    Ok(crate::RenderScene { items, bbox })
}

/// Make every pixel of exactly `key` transparent: the rasterizer writes its
/// background bit-exactly to uncovered pixels, so this is the whole matte.
fn key_out(img: &mut cadpole_core::render::Image, key: [f32; 3]) {
    let key = cadpole_core::render::Image::new(1, 1, key).rgba;
    for px in img.rgba.chunks_exact_mut(4) {
        if px[..3] == key[..3] {
            px[3] = 0;
        }
    }
}

/// A plate's thumbnail as an image: the plate's scene from Bambu's own
/// three-quarter angle, on a transparent background, at the 512 px square
/// every sample uses. Edges, grid and the contact shadow are off: the
/// shadow would darken uncovered pixels, which then would not key.
pub fn plate_thumbnail_of(scene: &crate::RenderScene) -> cadpole_core::render::Image {
    use cadpole_core::render::RenderOptions;
    let opts = RenderOptions {
        width: cadpole_export_3mf::bambu::THUMBNAIL_PX,
        height: cadpole_export_3mf::bambu::THUMBNAIL_PX,
        background: THUMBNAIL_BG,
        draw_grid: false,
        draw_edges: false,
        draw_silhouettes: false,
        contact_shadow: false,
        ..RenderOptions::default()
    };
    let cam = Some(pb::Camera { preset: "iso".into(), ..Default::default() });
    let camera = crate::camera_for_scene(scene, &cam, None, &opts);
    let mut img = crate::render_scene(scene, &camera, &opts);
    key_out(&mut img, THUMBNAIL_BG);
    img
}

/// [`plate_thumbnail_of`] for the bodies `body_ids`, in their own colours,
/// opaque, whatever layer they are on ([`crate::scene_of_bodies`]).
pub fn plate_thumbnail_image(ctx: &mut Context, body_ids: &[String]) -> cadpole_core::render::Image {
    plate_thumbnail_of(&crate::scene_of_bodies(ctx, body_ids))
}

/// The two PNGs a plate carries: [`plate_thumbnail_of`] at 512 px and a
/// box-filtered 128 px copy.
fn plate_thumbnails(scene: &crate::RenderScene) -> (Vec<u8>, Vec<u8>) {
    let img = plate_thumbnail_of(scene);
    let small = downscale(&img, cadpole_export_3mf::bambu::THUMBNAIL_SMALL_PX);
    (img.to_png(), small.to_png())
}

/// A box-filtered downscale of the square `img` to `side` px, averaging
/// every channel, alpha included.
pub fn downscale(img: &cadpole_core::render::Image, side: u32) -> cadpole_core::render::Image {
    downscale_by(img, (img.width / side).max(1))
}

/// A box-filtered downscale by the whole factor `f`: each output pixel is
/// the mean of an `f × f` block, alpha included. Any trailing rows and
/// columns that do not make a whole block are dropped.
pub fn downscale_by(img: &cadpole_core::render::Image, f: u32) -> cadpole_core::render::Image {
    let f = f.max(1);
    let (w, h) = ((img.width / f).max(1), (img.height / f).max(1));
    let mut out = cadpole_core::render::Image::new(w, h, [0.0, 0.0, 0.0]);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for dy in 0..f {
                for dx in 0..f {
                    let i = (((y * f + dy) * img.width + (x * f + dx)) * 4) as usize;
                    for k in 0..4 {
                        acc[k] += img.rgba[i + k] as u32;
                    }
                }
            }
            let n = f * f;
            out.put(x, y, [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, (acc[3] / n) as u8]);
        }
    }
    out
}

/// How the standalone plate images are rendered: the request's or the
/// script's `plate_images(...)` declaration's size, background and backend.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateImageOptions {
    pub width: u32,
    pub height: u32,
    /// `None` is transparent; otherwise the backdrop the plate stands in.
    pub backdrop: Option<cadpole_core::render::Backdrop>,
    pub backend: cadpole_core::render::Backend,
}

impl Default for PlateImageOptions {
    fn default() -> Self {
        let (width, height) = cadpole_core::context::PlateImageSpec::DEFAULT_SIZE;
        PlateImageOptions { width, height, backdrop: None, backend: cadpole_core::render::Backend::Raster }
    }
}

impl PlateImageOptions {
    /// From the strings a request or a declaration carries. `background` is
    /// `"transparent"` (or empty) or a backdrop name; `backend` is
    /// `"raster"` (or empty) or `"path"`.
    pub fn parse(width: u32, height: u32, background: &str, backend: &str) -> anyhow::Result<PlateImageOptions> {
        let d = PlateImageOptions::default();
        let backdrop = match background.trim() {
            "" | "transparent" => None,
            b => Some(cadpole_core::render::Backdrop::parse(b).ok_or_else(|| {
                anyhow::anyhow!(
                    "plate images: background {b:?} is not one of \"transparent\", \"studio\", \"white\", \
                     \"black\" or \"#rrggbb\""
                )
            })?),
        };
        let backend = match backend.trim() {
            "" | "raster" => cadpole_core::render::Backend::Raster,
            "path" => cadpole_core::render::Backend::Path,
            b => anyhow::bail!("plate images: backend {b:?} is not \"raster\" or \"path\""),
        };
        Ok(PlateImageOptions {
            width: if width == 0 { d.width } else { width.min(8192) },
            height: if height == 0 { d.height } else { height.min(8192) },
            backdrop,
            backend,
        })
    }

    pub fn from_spec(spec: &cadpole_core::context::PlateImageSpec) -> anyhow::Result<PlateImageOptions> {
        PlateImageOptions::parse(spec.width, spec.height, &spec.background, &spec.backend)
    }

    pub fn from_request(req: &pb::ExportRequest) -> anyhow::Result<PlateImageOptions> {
        let backend = match req.plate_image_backend() {
            pb::RenderBackend::PathTrace => "path",
            _ => "raster",
        };
        PlateImageOptions::parse(req.plate_image_width, req.plate_image_height, &req.plate_image_background, backend)
    }

    /// The flat colour behind the plate: the key for a transparent image,
    /// the backdrop's horizon through the tone map otherwise.
    fn background(&self) -> [f32; 3] {
        match &self.backdrop {
            None => PLATE_KEY,
            Some(b) => b.background(1.0, PLATE_TONEMAP),
        }
    }
}

/// The tone curve of a plate image: Khronos PBR Neutral, exact for base
/// colours under unit light, so a filament's red is that red.
const PLATE_TONEMAP: cadpole_core::render::ToneMap = cadpole_core::render::ToneMap::Neutral;

/// One standalone plate image, rendered and named.
pub struct PlateImage {
    /// The plate, 1-based; 0 for the contact sheet.
    pub plate: u32,
    /// The plate's name, empty for a numbered one.
    pub name: String,
    /// The file name it is written under: [`plate_image_file_name`].
    pub path: String,
    pub image: cadpole_core::render::Image,
}

impl PlateImage {
    pub fn to_proto(&self) -> pb::PlateImage {
        pb::PlateImage {
            path: self.path.clone(),
            plate: self.plate,
            plate_name: self.name.clone(),
            width: self.image.width,
            height: self.image.height,
            png: self.image.to_png(),
        }
    }
}

/// `<stem>_plate_<n>.png`, or `<stem>_plate_<n>_<name>.png` for a named
/// plate, the name reduced to letters, digits and single underscores:
/// `Core (print in place)` is `gear_toy_plate_1_Core_print_in_place.png`.
pub fn plate_image_file_name(stem: &str, plate: u32, name: &str) -> String {
    let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let slug = slug.split('_').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("_");
    if slug.is_empty() { format!("{stem}_plate_{plate}.png") } else { format!("{stem}_plate_{plate}_{slug}.png") }
}

/// `<stem>_plates.png`: the contact sheet.
pub fn plate_sheet_file_name(stem: &str) -> String {
    format!("{stem}_plates.png")
}

/// The bed as a mesh: a slab over the machine's printable area
/// `[x0, y0, x1, y1]` whose top is just under z = 0, with its twelve edges
/// as polylines for the line overlay. A plate's scene ([`plate_scene`]) is
/// in the plate's own coordinates -- the writer's layout rule
/// ([`cadpole_export_3mf::bambu::layout_plates`], the one the project file and
/// the model view read) already applied -- so the picture shows the parts
/// where the project puts them.
fn bed_slab(area: [f64; 4]) -> (cadpole_core::tessellate::Mesh, Vec<Vec<Vec3>>) {
    let (x0, y0, x1, y1) = (area[0], area[1], area[2], area[3]);
    let z1 = BED_TOP;
    let z0 = z1 - BED_THICKNESS * (x1 - x0).max(y1 - y0);
    let c = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    // Six faces, each four vertices with the face's own normal, so the slab
    // shades flat and its edges fold at 90°.
    let faces: [([Vec3; 4], Vec3); 6] = [
        ([c(x0, y0, z1), c(x1, y0, z1), c(x1, y1, z1), c(x0, y1, z1)], Vec3::Z),
        ([c(x0, y1, z0), c(x1, y1, z0), c(x1, y0, z0), c(x0, y0, z0)], Vec3::new(0.0, 0.0, -1.0)),
        ([c(x0, y0, z0), c(x1, y0, z0), c(x1, y0, z1), c(x0, y0, z1)], Vec3::new(0.0, -1.0, 0.0)),
        ([c(x1, y1, z0), c(x0, y1, z0), c(x0, y1, z1), c(x1, y1, z1)], Vec3::Y),
        ([c(x0, y1, z0), c(x0, y0, z0), c(x0, y0, z1), c(x0, y1, z1)], Vec3::new(-1.0, 0.0, 0.0)),
        ([c(x1, y0, z0), c(x1, y1, z0), c(x1, y1, z1), c(x1, y0, z1)], Vec3::X),
    ];
    let mut mesh = cadpole_core::tessellate::Mesh::default();
    for (fi, (quad, n)) in faces.iter().enumerate() {
        let base = mesh.vertex_count() as u32;
        for v in quad {
            mesh.positions.extend_from_slice(&[v.x as f32, v.y as f32, v.z as f32]);
            mesh.normals.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        mesh.face_ids.extend_from_slice(&[fi as u32, fi as u32]);
    }
    let corners = |z: f64| [c(x0, y0, z), c(x1, y0, z), c(x1, y1, z), c(x0, y1, z)];
    let (top, bottom) = (corners(z1), corners(z0));
    let mut edges = Vec::with_capacity(12);
    for i in 0..4 {
        let j = (i + 1) % 4;
        edges.push(vec![top[i], top[j]]);
        edges.push(vec![bottom[i], bottom[j]]);
        edges.push(vec![bottom[i], top[i]]);
    }
    (mesh, edges)
}

/// The standalone image of one plate: [`plate_scene`]'s bodies standing on
/// the bed ([`bed_slab`] over the machine's printable `area`), from
/// [`PLATE_VIEW`], the camera fitted to the whole bed so every plate of a
/// project is drawn at one scale and nothing is clipped, under the product
/// rig with edges drawn. Transparent around the bed unless `opts` names a
/// backdrop.
pub fn plate_image_of(scene: &crate::RenderScene, area: [f64; 4], opts: &PlateImageOptions) -> cadpole_core::render::Image {
    use cadpole_core::render::{Backend, Camera, Item, Lighting, RenderOptions};
    let (mut lo, mut hi) = (Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN));
    for p in scene.points() {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    let (slab, slab_edges) = bed_slab(area);
    let slab_material = cadpole_core::context::Material { base_color: BED_COLOR, roughness: 0.6, ..Default::default() };
    let slab_creases = vec![90.0; slab_edges.len()];

    let mut o = RenderOptions {
        width: opts.width.max(1),
        height: opts.height.max(1),
        background: opts.background(),
        draw_grid: false,
        draw_edges: true,
        draw_silhouettes: true,
        // The ground-contact shadow is cast on the *background* plane, so
        // the bed would sit in a blocky grey halo that no key removes; the
        // bed is the ground here, and the parts' grounding comes from the
        // ambient occlusion at their feet.
        contact_shadow: false,
        lighting: Lighting::Product,
        tonemap: PLATE_TONEMAP,
        backdrop: opts.backdrop,
        backend: opts.backend,
        ..RenderOptions::default()
    };
    // The bed is the floor: the tracer's own ground plane would cut through it.
    o.path.ground = false;
    if opts.backend == Backend::Path {
        o.samples = 64;
    }
    if let Some(b) = &opts.backdrop {
        o.path.show_environment = true;
        o.path.ground_albedo = b.floor_albedo();
    }

    // The camera: the preset's direction and the product lens, then pulled
    // back until the bed and every part fit the frame.
    let (slo, shi) = slab.bbox().expect("the slab has vertices");
    let (flo, fhi) = (lo.min(slo), hi.max(shi));
    let mut camera = Camera::preset(PLATE_VIEW, flo, fhi, PLATE_MARGIN);
    camera.fov_degrees = PLATE_FOV_DEGREES;
    let aspect = o.width as f64 / o.height as f64;
    let slab_points = || slab.positions.chunks_exact(3).map(|p| Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
    camera.fit_points(|| scene.points().chain(slab_points()), PLATE_MARGIN, aspect);

    let mut items = crate::items_of(scene, None);
    items.push(Item {
        edges: &slab_edges,
        edge_dihedrals: &slab_creases,
        ..Item::new(&slab, &slab_material, &slab_edges)
    });
    let mut img = cadpole_core::render::render_with_stats(&items, &camera, &o).0;
    if opts.backdrop.is_none() {
        key_out(&mut img, PLATE_KEY);
    }
    img
}

/// The contact sheet: every plate image at half size, side by side,
/// `ceil(√N)` to a row as Bambu lays plates out, its plate number and name
/// under it, on the same background as the images (transparent stays
/// transparent).
pub fn plate_sheet_of(images: &[PlateImage], opts: &PlateImageOptions) -> cadpole_core::render::Image {
    let tiles: Vec<(String, cadpole_core::render::Image)> = images
        .iter()
        .map(|p| {
            let label = if p.name.is_empty() { format!("plate {}", p.plate) } else { format!("plate {}: {}", p.plate, p.name) };
            (label, downscale_by(&p.image, 2))
        })
        .collect();
    let cols = cadpole_export_3mf::bambu::plate_columns(images.len() as u32);
    let tile_h = tiles.first().map(|(_, t)| t.height).unwrap_or(1);
    // A label that reads at the tile's size: the 7 px font at 1 px per
    // 200 px of tile, 1 to 4.
    let scale = (tile_h / 200).clamp(1, 4);
    let bg = opts.background();
    let lum = 0.2126 * bg[0] + 0.7152 * bg[1] + 0.0722 * bg[2];
    let text = if lum > 0.3 { [52, 54, 58, 255] } else { [230, 232, 236, 255] };
    let mut sheet = cadpole_core::render::contact_sheet_with(&tiles, cols, bg, scale, text);
    if opts.backdrop.is_none() {
        key_out(&mut sheet, PLATE_KEY);
    }
    sheet
}

/// One body on a plate, as [`plate_plan`] places it: what `Model.plates`
/// carries, and what the plate thumbnails are rendered from.
#[derive(Clone, Debug)]
pub struct PlannedBody {
    pub body: BodyId,
    pub id_string: String,
    pub name: String,
    /// The print object the body belongs to: `export(name=)` when the call
    /// named one, else the body's own name.
    pub object: String,
    /// 1-based filament slot.
    pub slot: u32,
    /// The transform that takes the body's mesh -- a part body's, for an
    /// instance -- to where it stands on the plate: the instance's
    /// pose (never the exploded-view offset, which no export sees), then
    /// the plate's placement and its object's set-down onto the bed.
    pub transform: cadpole_core::Transform,
    /// The move from where the script put the body to where it stands in its
    /// plate's own coordinates (the bed's corner at the origin): the plate's
    /// centring and the object's set-down, without the plate's place on the
    /// grid. What the plate images draw with.
    pub local: Vec3,
}

/// One plate of the plan: its number and name, where it sits, and its
/// bodies in export order.
#[derive(Clone, Debug)]
pub struct PlannedPlate {
    /// 1-based.
    pub index: u32,
    /// Empty for a plate only ever numbered.
    pub name: String,
    pub placement: cadpole_export_3mf::bambu::PlatePlacement,
    pub bodies: Vec<PlannedBody>,
}

/// One `export()` call of the project with its bodies' slots: the project
/// writer's unit, since a call is one object (`name=`) or one object per
/// body. `bodies` is `(id, name, slot, per-triangle slots)` in call order.
#[derive(Clone, Debug)]
pub struct PlannedCall {
    /// 1-based plate number.
    pub plate: u32,
    pub name: Option<String>,
    pub bodies: Vec<(BodyId, String, u32, Vec<u32>)>,
}

/// The layout of one Bambu Studio project, decided once for
/// the export and the model view alike: the machine, the plates with their
/// names, the filament slots and colours, which body prints where and in
/// what, and the placement the export gives every body. See [`plate_plan`].
#[derive(Clone, Debug)]
pub struct PlatePlan {
    /// The bed the grid is laid out for, in mm: `machine.bed()`.
    pub bed: [f64; 2],
    /// The machine the plates are laid out for and the pieces fitted to.
    pub machine: cadpole_export_3mf::bambu::Machine,
    /// The template whose settings the project carries, and its asset name.
    pub template: Option<(cadpole_export_3mf::bambu::Template, String)>,
    /// Plates whose pieces cannot be printed as laid out, each saying why:
    /// the export refuses the project while any are left.
    pub refusals: Vec<String>,
    /// Filament slots, slot 1 first.
    pub filaments: Vec<cadpole_export_3mf::Filament>,
    /// One per plate, plate 1 first; an empty plate is listed empty.
    pub plates: Vec<PlannedPlate>,
    /// One per `export()` call, in `specs` order.
    pub calls: Vec<PlannedCall>,
    pub warnings: Vec<String>,
}

impl PlatePlan {
    /// A filament's label as the export writes it: its material name, else
    /// its colour as `#rrggbb`.
    pub fn filament_label(&self, slot: u32) -> String {
        match self.filaments.get(slot.max(1) as usize - 1) {
            Some(f) if !f.name.is_empty() => f.name.clone(),
            Some(f) => f.hex(),
            None => String::new(),
        }
    }

    /// A slot's colour, linear RGB; grey for a slot past the list.
    pub fn filament_color(&self, slot: u32) -> [f64; 3] {
        self.filaments.get(slot.max(1) as usize - 1).map(|f| f.color).unwrap_or([0.75, 0.75, 0.75])
    }
}

/// The plate layout of every `export(..., flavor=flavor.bambu)` call to
/// one path: what [`export_bambu_project`] writes and what
/// `Model.plates` shows, from one computation so that the preview is the
/// export.
///
/// Decided here: the plate each call lands on and the plates' names
/// ([`resolve_plates`]); the bed, from the first `printer=` (a second,
/// different one is a warning); one palette over every call, with
/// `extruder=` pins on top and a pinned body claiming no slot of its own
/// ([`palette_with`]); and where each plate's contents go, which is
/// [`cadpole_export_3mf::bambu::layout_plates`] over the bodies' footprints
/// -- Bambu's grid, and a plate's contents centred on the bed as a group.
///
/// **Cheap by construction.** The footprints are the bodies' *cached*
/// meshes' bounding boxes (every visible body's mesh is already made by the
/// time a model view asks) and the placements are transforms: nothing is
/// tessellated, baked or moved, and a
/// test holds this to. The export then meshes at each call's tolerance;
/// the view's boxes are at the document's, so the two agree to the chord
/// tolerance.
pub fn plate_plan(ctx: &mut Context, specs: &[cadpole_core::context::ExportSpec]) -> anyhow::Result<PlatePlan> {
    use cadpole_export_3mf::bambu::{layout_plates, plate_label, Footprint};
    let mut warnings = Vec::new();
    let (plate_of, plate_names) = resolve_plates(specs);
    let path = specs.first().map(|s| s.path.clone()).unwrap_or_default();
    let (machine, template) = project_machine(specs, &path, &mut warnings)?;
    let bed = machine.bed();

    // One palette over every call, so a colour is one slot project-wide.
    // A body pinned by `extruder=` everywhere it appears claims no slot for
    // its colour (the toy's lid prints in the ring's slot and the project
    // has four filaments, not five); a body pinned in one call and free in
    // another keeps a slot of its own for the free call.
    let mut all_ids: Vec<BodyId> = Vec::new();
    let mut first_pin: BTreeMap<BodyId, u32> = BTreeMap::new();
    let mut free: std::collections::HashSet<BodyId> = std::collections::HashSet::new();
    for spec in specs {
        for (k, id) in spec.bodies.iter().enumerate() {
            if !all_ids.contains(id) {
                all_ids.push(*id);
            }
            match spec.extruders.get(k) {
                Some(s) => {
                    first_pin.entry(*id).or_insert(*s);
                }
                None => {
                    free.insert(*id);
                }
            }
        }
    }
    let always_pinned: BTreeMap<BodyId, u32> =
        first_pin.iter().filter(|(id, _)| !free.contains(id)).map(|(id, s)| (*id, *s)).collect();
    let mut pal = palette_with(ctx, &all_ids, &always_pinned)?;
    warnings.append(&mut pal.warnings);
    // A slot a per-call pin names past the list opens it with that body's
    // colour. Two pins disagreeing about one slot's colour is a warning
    // (printing a body in a slot another colour owns is what a pin is for,
    // so that alone is not).
    let mut pinned: std::collections::HashMap<u32, (String, String)> = std::collections::HashMap::new();
    for spec in specs {
        for (id, slot) in spec.bodies.iter().zip(&spec.extruders) {
            let (colour, name) = {
                let b = ctx.body(*id)?;
                (hex(b.material.base_color), b.name.clone())
            };
            if pal.filaments.len() < *slot as usize {
                pal.filaments.resize(
                    *slot as usize,
                    cadpole_export_3mf::Filament { name: String::new(), color: unhex(&colour) },
                );
            }
            match pinned.get(slot) {
                None => {
                    pinned.insert(*slot, (colour, name));
                }
                Some((first_colour, first)) if *first_colour != colour => warnings.push(format!(
                    "export: extruder {slot} is pinned to both {first:?} ({first_colour}) and {name:?} ({colour}); \
                     the slot keeps {}",
                    pal.filaments[*slot as usize - 1].hex()
                )),
                Some(_) => {}
            }
        }
    }
    // A pin is the call's, not the body's: the same body in another call
    // without `extruder=` keeps the slot its colour gave it.
    let slot_of = |id: BodyId, pin: Option<&u32>| {
        let (slot, tri) =
            pal.bodies.iter().find(|p| p.body == id).map(|p| (p.slot, p.tri_slots.clone())).unwrap_or((1, Vec::new()));
        (pin.copied().unwrap_or(slot), tri)
    };

    // Every call's bodies with their slots, and every print object's
    // footprint on its plate -- one object for a call with `name=`, one per
    // body otherwise -- from the meshes the bodies already have, never new
    // ones. Each object is set down on the bed: its lowest point to z = 0.
    let mut calls = Vec::with_capacity(specs.len());
    let mut footprints: Vec<Footprint> = Vec::new();
    let mut drops: Vec<Vec<f64>> = Vec::with_capacity(specs.len());
    for (spec, plate) in specs.iter().zip(&plate_of) {
        let mut bodies = Vec::with_capacity(spec.bodies.len());
        let mut boxes = Vec::with_capacity(spec.bodies.len());
        for (k, id) in spec.bodies.iter().enumerate() {
            let name = ctx.body(*id)?.name.clone();
            let (slot, tri_slots) = slot_of(*id, spec.extruders.get(k));
            bodies.push((*id, name, slot, tri_slots));
            boxes.push(ctx.body_mesh_cached(*id)?.0.bbox());
        }
        let objects: Vec<Vec<usize>> =
            if spec.name.is_some() { vec![(0..boxes.len()).collect()] } else { (0..boxes.len()).map(|k| vec![k]).collect() };
        let mut drop = vec![0.0; boxes.len()];
        for members in objects {
            let (mut lo, mut hi) = (Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN));
            for (a, b) in members.iter().filter_map(|k| boxes[*k]) {
                lo = lo.min(a);
                hi = hi.max(b);
            }
            if lo.x > hi.x {
                continue;
            }
            footprints.push(Footprint { plate: *plate, lo: [lo.x, lo.y], hi: [hi.x, hi.y], height: hi.z - lo.z });
            for k in members {
                drop[k] = -lo.z;
            }
        }
        drops.push(drop);
        calls.push(PlannedCall { plate: *plate, name: spec.name.clone(), bodies });
    }
    // The prime towers: the room the template's tower takes, and
    // which plates print one -- those of more than one filament, counting
    // painted triangles' slots as the writer does.
    let filaments: Vec<usize> = (1..=plate_names.len() as u32)
        .map(|plate| {
            let mut slots: Vec<u32> = Vec::new();
            for call in calls.iter().filter(|c| c.plate == plate) {
                for (_, _, slot, tri) in &call.bodies {
                    for s in std::iter::once(*slot).chain(tri.iter().copied()) {
                        if s > 0 && !slots.contains(&s) {
                            slots.push(s);
                        }
                    }
                }
            }
            slots.len()
        })
        .collect();
    let towers = cadpole_export_3mf::bambu::TowerPlan {
        room: cadpole_export_3mf::bambu_settings::tower_room(template.as_ref().map(|(t, _)| t)),
        filaments,
    };
    let layout = layout_plates(&machine, plate_names.len() as u32, &footprints, &towers);
    let label = |n: u32| plate_label(n, plate_names.get(n as usize - 1).map(String::as_str).unwrap_or(""));
    let refusals: Vec<String> = layout.refusals(label).into_iter().map(|r| format!("export: {path}: {r}")).collect();
    warnings.extend(layout.warnings(label).into_iter().map(|w| format!("export: {path}: {w}")));
    let mut plates: Vec<PlannedPlate> = plate_names
        .iter()
        .zip(&layout.placements)
        .enumerate()
        .map(|(i, (name, placement))| PlannedPlate { index: i as u32 + 1, name: name.clone(), placement: *placement, bodies: Vec::new() })
        .collect();
    for (call, drop) in calls.iter().zip(&drops) {
        let plate = &mut plates[call.plate as usize - 1];
        let origin = plate.placement.origin;
        let shift = plate.placement.shift;
        for ((id, name, slot, _), dz) in call.bodies.iter().zip(drop) {
            // An instance body's mesh is its part's, drawn under the pose;
            // an ordinary body's -- a pattern copy included, which
            // carries a mesh of its own -- is already where it is.
            let rec = ctx.body(*id)?;
            let pose = match (rec.instance, rec.source()) {
                (Some(_), Some((_, pose))) => pose,
                _ => cadpole_core::Transform::identity(),
            };
            let local = Vec3::new(shift[0], shift[1], *dz);
            let moved = cadpole_core::Transform::translation(Vec3::new(origin[0], origin[1], 0.0) + local);
            plate.bodies.push(PlannedBody {
                body: *id,
                id_string: rec.id_string(),
                name: name.clone(),
                object: call.name.clone().unwrap_or_else(|| name.clone()),
                slot: *slot,
                transform: pose.then(moved),
                local,
            });
        }
    }
    Ok(PlatePlan { bed, machine, template, refusals, filaments: pal.filaments, plates, calls, warnings })
}

/// The machine of a project's printer table row: its printable
/// area and height, the corner it cannot print, what each nozzle reaches,
/// and its stock Bambu Studio presets.
pub fn machine_of_printer(p: &cadpole_print::Printer) -> cadpole_export_3mf::bambu::Machine {
    cadpole_script::plating::machine_of_printer(p)
}

/// The printer a Bambu project is laid out for when no call names one: the
/// X1 Carbon, a 256 mm bed with the X1 series' 250 mm height and
/// cutter corner.
pub const DEFAULT_PRINTER: &str = "bambu_x1c";

/// The machine and the template of a project's calls: the first
/// call's `template=` (its saved printer), else the first `printer=` (or
/// [`DEFAULT_PRINTER`]) with the first `bed=` replacing its bed. A later call
/// naming another template, printer or bed is a warning: one project is laid
/// out for one machine.
fn project_machine(
    specs: &[cadpole_core::context::ExportSpec],
    path: &str,
    warnings: &mut Vec<String>,
) -> anyhow::Result<(cadpole_export_3mf::bambu::Machine, Option<(cadpole_export_3mf::bambu::Template, String)>)> {
    let mut first = |f: &dyn Fn(&cadpole_core::context::ExportSpec) -> Option<String>| -> Option<String> {
        let mut seen: Option<String> = None;
        for s in specs {
            if let Some(v) = f(s) {
                match &seen {
                    None => seen = Some(v),
                    Some(have) if *have != v => warnings.push(format!(
                        "export: {path} is laid out for {have}; the call naming {v} keeps that layout"
                    )),
                    Some(_) => {}
                }
            }
        }
        seen
    };
    let template = specs.iter().find_map(|s| s.template.clone());
    first(&|s: &cadpole_core::context::ExportSpec| s.template.as_ref().map(|t| format!("template {:?}", t.asset)));
    first(&|s: &cadpole_core::context::ExportSpec| (!s.printer.is_empty()).then(|| format!("printer={:?}", s.printer)));
    first(&|s: &cadpole_core::context::ExportSpec| s.bed.map(|b| format!("bed={b:?}")));
    if let Some(t) = template {
        let parsed = cadpole_export_3mf::bambu::Template::from_parts(&t.application, &t.settings)
            .map_err(|e| anyhow::anyhow!("export: template {:?}: {e}", t.asset))?;
        // `printer=` naming the template's own machine is the usual case
        // (the script refuses another Bambu model); a custom bed, or a
        // printer that is not the template's, gives way to the template.
        let other = specs.iter().filter(|s| !s.printer.is_empty()).any(|s| {
            cadpole_print::printers::printer(&s.printer).and_then(|p| p.bambu).map(|b| b.model) != Some(parsed.printer_model())
        });
        if other || specs.iter().any(|s| s.bed.is_some()) {
            warnings.push(format!(
                "export: {path} is laid out for template {:?}'s {}; printer= and bed= give way to it",
                t.asset,
                parsed.printer_model()
            ));
        }
        return Ok((parsed.machine(), Some((parsed, t.asset))));
    }
    let id = specs.iter().find(|s| !s.printer.is_empty()).map(|s| s.printer.as_str()).unwrap_or(DEFAULT_PRINTER);
    let p = cadpole_print::printers::printer(id).ok_or_else(|| anyhow::anyhow!("export: unknown printer {id:?}"))?;
    let mut machine = machine_of_printer(p);
    if let Some(b) = specs.iter().find_map(|s| s.bed) {
        machine.area = [0.0, 0.0, b[0], b[1]];
        if b[2] > 0.0 {
            machine.height = b[2];
        }
        machine.exclude.clear();
        machine.nozzles.clear();
        machine.name = format!("{} (custom bed)", p.display_name);
    }
    Ok((machine, None))
}

/// `Model.plates`: the plates of every Bambu Studio project the
/// document's `export()` calls describe, in path order, for a UI to draw
/// with the meshes it already has. One [`plate_plan`] per path; a path some
/// call writes as a standard 3MF too is laid out from its Bambu calls, and
/// the export is what refuses it. Empty for a document without Bambu
/// exports, and the plan's warnings are the export's to report, so a model
/// without such exports is untouched.
pub fn document_plates(ctx: &mut Context) -> Vec<pb::Plate> {
    let specs: Vec<cadpole_core::context::ExportSpec> =
        ctx.exports.iter().filter(|s| s.format == "3mf" && s.flavor == cadpole_core::export::Flavor::Bambu).cloned().collect();
    let mut paths: Vec<&str> = Vec::new();
    for spec in &specs {
        if !paths.contains(&spec.path.as_str()) {
            paths.push(&spec.path);
        }
    }
    let mut out = Vec::new();
    for path in paths {
        let group: Vec<cadpole_core::context::ExportSpec> = specs.iter().filter(|s| s.path == path).cloned().collect();
        // A plan can only fail on a body the script never had, which a
        // recorded export cannot name; nothing to show is the right answer.
        let Ok(plan) = plate_plan(ctx, &group) else { continue };
        for p in &plan.plates {
            out.push(pb::Plate {
                index: p.index,
                name: p.name.clone(),
                path: path.to_owned(),
                bed_x: plan.bed[0],
                bed_y: plan.bed[1],
                origin: Some(Vec3::new(p.placement.origin[0], p.placement.origin[1], 0.0).to_proto()),
                bodies: p
                    .bodies
                    .iter()
                    .map(|b| {
                        let c = plan.filament_color(b.slot);
                        pb::PlateBody {
                            body_id: b.id_string.clone(),
                            object: b.object.clone(),
                            transform: Some(b.transform.to_proto()),
                            slot: b.slot,
                            color: Some(pb::Color { r: c[0], g: c[1], b: c[2] }),
                            filament: plan.filament_label(b.slot),
                        }
                    })
                    .collect(),
            });
        }
    }
    out
}

/// [`Context::to_model`] with `Model.plates` filled in: what every
/// evaluation answers with. The plates cost nothing to a document without
/// Bambu exports, and a transform per body to one with them.
pub fn model_with_plates(ctx: &mut Context, skip_mesh: bool, eval_millis: f64) -> pb::Model {
    let mut model = ctx.to_model(skip_mesh, eval_millis);
    model.plates = document_plates(ctx);
    model
}

/// What [`export_bambu_project`] produced: the project file, its warnings,
/// and the standalone plate images when they were asked for.
pub struct BambuProject {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
    /// One per plate that holds anything, in plate order.
    pub plate_images: Vec<PlateImage>,
    /// The contact sheet of `plate_images`, when there are any.
    pub plate_sheet: Option<PlateImage>,
}

/// Write every `export(..., flavor=flavor.bambu)` call to one path as one
/// Bambu Studio project: a plate per `plate=`, the call's bodies as
/// one object under `name=` or one object each, a filament slot per body
/// from `extruder=` or its colour, the plate grid laid out for `printer=`'s
/// bed, and a rendered thumbnail per plate (`thumbnails`; a test skips the
/// render). `images` asks for the standalone plate images and their contact
/// sheet as well, rendered from the same plate scenes the thumbnails are.
/// The layout is [`plate_plan`]'s, which the model view shares.
pub fn export_bambu_project(
    ctx: &mut Context,
    specs: &[cadpole_core::context::ExportSpec],
    title: &str,
    thumbnails: bool,
    images: Option<&PlateImageOptions>,
) -> anyhow::Result<BambuProject> {
    use cadpole_export_3mf::bambu::{Plate, Project, ProjectObject, ProjectPart};
    let plan = plate_plan(ctx, specs)?;
    if !plan.refusals.is_empty() {
        // A plate whose pieces cannot fit is refused before anything is
        // meshed or rendered: every such plate, by name.
        anyhow::bail!("{}", plan.refusals.join("; "));
    }
    let mut warnings = plan.warnings.clone();

    // Meshes first, each at its call's tolerance, so the parts can borrow.
    type Call = Vec<(String, cadpole_core::tessellate::Mesh, u32, Vec<u32>)>;
    let mut meshes: Vec<Call> = Vec::with_capacity(specs.len());
    for (spec, call) in specs.iter().zip(&plan.calls) {
        let mut out = Vec::with_capacity(call.bodies.len());
        for (id, name, slot, tri_slots) in &call.bodies {
            let mesh = ctx.body_mesh_with(*id, spec.tolerance)?.0.clone();
            out.push((name.clone(), mesh, *slot, tri_slots.clone()));
        }
        meshes.push(out);
    }
    let mut plates: Vec<Plate<'_>> =
        plan.plates.iter().map(|p| Plate { name: p.name.clone(), ..Default::default() }).collect();
    for (call, meshed) in plan.calls.iter().zip(&meshes) {
        let parts = |range: std::ops::Range<usize>| -> Vec<ProjectPart<'_>> {
            meshed[range]
                .iter()
                .map(|(name, mesh, slot, tri_slots)| ProjectPart {
                    name: name.clone(),
                    mesh,
                    slot: *slot,
                    tri_slots: tri_slots.clone(),
                })
                .collect()
        };
        let plate = &mut plates[call.plate as usize - 1];
        match &call.name {
            Some(name) => plate.objects.push(ProjectObject { name: name.clone(), parts: parts(0..meshed.len()) }),
            None => {
                for (k, (name, _, _, _)) in meshed.iter().enumerate() {
                    plate.objects.push(ProjectObject { name: name.clone(), parts: parts(k..k + 1) });
                }
            }
        }
    }
    let mut plate_images = Vec::new();
    if thumbnails || images.is_some() {
        // Per plate, every body on it with the slot it prints in, rendered
        // where it stands **in its filament's colour** -- a pinned body
        // shows the colour it will come off the printer in, as Bambu's own
        // thumbnails do, not the material the script gave it. One scene per
        // plate serves both the thumbnail in the file and the standalone
        // image beside it.
        for (i, (plate, planned)) in plates.iter_mut().zip(&plan.plates).enumerate() {
            if plate.objects.is_empty() {
                continue;
            }
            let coloured: Vec<(BodyId, Option<[f64; 3]>, Vec3)> = planned
                .bodies
                .iter()
                .map(|b| (b.body, plan.filaments.get(b.slot as usize - 1).map(|f| f.color), b.local))
                .collect();
            let scene = plate_scene(ctx, &coloured)?;
            if thumbnails {
                let (big, small) = plate_thumbnails(&scene);
                plate.thumbnail = Some(big);
                plate.thumbnail_small = Some(small);
            }
            if let Some(opts) = images {
                let n = i as u32 + 1;
                plate_images.push(PlateImage {
                    plate: n,
                    name: plate.name.clone(),
                    path: plate_image_file_name(title, n, &plate.name),
                    image: plate_image_of(&scene, plan.machine.area, opts),
                });
            }
        }
    }
    let plate_sheet = match images {
        Some(opts) if !plate_images.is_empty() => Some(PlateImage {
            plate: 0,
            name: String::new(),
            path: plate_sheet_file_name(title),
            image: plate_sheet_of(&plate_images, opts),
        }),
        _ => None,
    };
    let project = Project {
        title: title.to_owned(),
        plates,
        palette: plan.filaments.clone(),
        machine: plan.machine.clone(),
        template: plan.template.clone(),
    };
    let written = cadpole_export_3mf::bambu::write_project(&project).map_err(|e| anyhow::anyhow!("{e}"))?;
    // The writer lays the welded meshes out by the same rule the plan used
    // on the cached ones; its layout warnings repeat the plan's.
    let path = specs.first().map(|s| s.path.as_str()).unwrap_or("");
    for w in written.warnings {
        if !warnings.iter().any(|have| have.ends_with(&w)) {
            warnings.push(format!("export: {path}: {w}"));
        }
    }
    Ok(BambuProject { bytes: written.bytes, warnings, plate_images, plate_sheet })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_derives_the_wall_from_the_nozzle() {
        let p = profile("", 0.0, 0.6, 3.0, 0.0, "", 0.0, &[], &[]).unwrap();
        assert!((p.min_wall - 1.8).abs() < 1e-12);
        assert_eq!(p.max_overhang_degrees, 45.0);
        assert_eq!(p.bed, [256.0, 256.0, 256.0]);
    }

    #[test]
    fn a_single_bed_number_is_a_cube() {
        assert_eq!(profile("", 0.0, 0.0, 0.0, 0.0, "", 0.0, &[180.0], &[]).unwrap().bed, [180.0, 180.0, 180.0]);
        assert_eq!(
            profile("", 0.0, 0.0, 0.0, 0.0, "", 0.0, &[256.0, 256.0, 200.0], &[]).unwrap().bed,
            [256.0, 256.0, 200.0]
        );
    }

    #[test]
    fn a_named_printer_supplies_the_bed_and_call_site_arguments_still_win() {
        let p = profile("prusa_mk4", 0.0, 0.0, 0.0, 0.0, "", 0.0, &[], &[]).unwrap();
        assert_eq!(p.bed, [250.0, 210.0, 220.0]);
        assert_eq!(p.name, "prusa_mk4");
        assert_eq!(p.material.name, "PLA");
        // A nozzle at the call site wins, and drags min_wall with it.
        let p = profile("prusa_mk4", 0.0, 0.6, 0.0, 0.3, "PETG", 0.0, &[], &[]).unwrap();
        assert_eq!(p.bed, [250.0, 210.0, 220.0]);
        assert!((p.nozzle - 0.6).abs() < 1e-12);
        assert!((p.min_wall - 1.2).abs() < 1e-12);
        assert!((p.layer_height - 0.3).abs() < 1e-12);
        assert_eq!(p.material.name, "PETG");
    }

    #[test]
    fn an_unknown_printer_or_material_lists_the_ones_there_are() {
        let e = profile("makerbot", 0.0, 0.0, 0.0, 0.0, "", 0.0, &[], &[]).unwrap_err().to_string();
        assert!(e.contains("bambu_a1") && e.contains("prusa_mk4"), "{e}");
        let e = profile("", 0.0, 0.0, 0.0, 0.0, "unobtanium", 0.0, &[], &[]).unwrap_err().to_string();
        assert!(e.contains("PLA") && e.contains("PETG"), "{e}");
    }

    // ---- end to end, through the operation --------------------------------

    fn doc(src: &str) -> pb::Document {
        crate::document_from_scripts("t", vec![("main.star".into(), src.into())])
    }

    fn check(src: &str) -> pb::PrintCheckResponse {
        print_check(&pb::PrintCheckRequest { document: Some(doc(src)), ..Default::default() }).unwrap()
    }

    /// The Gridfinity bin's last boolean trims 0.25 mm off every perimeter
    /// foot with the body's own outline, and the mesh kernel's exact answer
    /// to that is a feather edge of triangles a few hundredths of a
    /// millimetre wide: 28 with no area at all, and a 0.03 mm "wall" along
    /// the base that `cadpole check` reported and the path tracer lit up as a
    /// bright patch through glass. The mesh a boolean hands back is cleaned
    /// of everything thinner than the tessellation tolerance
    /// (`cadpole_core::mesh_clean`), so the mesh that is rendered, exported and
    /// checked is the same clean one. This is that, on the real example at
    /// its defaults: closed, no internal wall, nothing thinner than the
    /// tolerance, and none of the print checks' thin spots at the base.
    #[test]
    fn the_gridfinity_bin_comes_out_of_the_mesh_kernel_without_slivers() {
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let src = std::fs::read_to_string(std::path::Path::new(&dir).join("gridfinity_bin.star"))
            .expect("examples/gridfinity_bin.star");
        let doc = crate::document_from_scripts("gridfinity_bin", vec![("main.star".into(), src)]);
        let tolerance = 0.05;
        let (mut ctx, _) = crate::evaluate_document_with(&doc, &Default::default(), tolerance, 0.0);
        let errors: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert!(errors.is_empty(), "the bin must evaluate cleanly: {errors:?}");
        let ids: Vec<BodyId> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
        assert_eq!(ids.len(), 1, "one bin");
        assert_eq!(ctx.body(ids[0]).unwrap().kind(), "mesh", "the footprint trim runs on the mesh kernel");
        let mesh = ctx.body_mesh(ids[0]).unwrap().0.clone();

        // Still a solid, by the one definition every part of Cadpole shares:
        // closed, consistently oriented, embedded. Removing a sliver
        // has to be a collapse, never a deletion — dropping the triangles
        // would open a hole and this is what would catch it.
        let v = cadpole_core::solid::validate(&mesh);
        assert!(v.is_solid(), "not a solid after cleaning: {:?}", v.failing());
        // And nothing degenerate is left for the weld to collapse. This is
        // the assertion that fails without the cleaning, where the bin's last boolean
        // leaves 28 zero-area triangles.
        assert_eq!(v.collapsed, 0, "{} degenerate triangles collapsed when welding", v.collapsed);
        assert!(v.degenerate.is_empty(), "{:?}", v.degenerate);
        // No needle either: no edge shorter than the tolerance survives, so
        // nothing is left for a slicer or a later weld to collapse.
        let mut shortest = (f64::INFINITY, Vec3::ZERO);
        for t in 0..mesh.triangle_count() {
            let tri = mesh.triangle(t).unwrap();
            for k in 0..3 {
                let l = (tri[(k + 1) % 3] - tri[k]).length();
                if l < shortest.0 {
                    shortest = (l, tri[k]);
                }
            }
        }
        assert!(shortest.0 >= tolerance, "an edge {:.4} mm long at {:?}", shortest.0, shortest.1);

        // What `cadpole check` says, against the default printer: no degenerate
        // warning, and no wall under the tolerance anywhere. Thin spots it
        // still reports are real and well above it — the exposed floor
        // between two feet's rounded corners at the outer wall reads about
        // 0.5 mm, because its centroid sits 0.15 mm from the wall's convex
        // edge and the probe's cone ray meets that wall (thickness.rs says
        // why that is left alone). Before the cleanup the same place read
        // 0.03 mm, from a sliver of that floor 0.03 mm wide.
        let report = cadpole_print::check(&mesh, &cadpole_print::Profile::default());
        assert!(report.mesh.watertight);
        assert_eq!(report.mesh.collapsed, 0);
        assert!(report.mesh.degenerate.is_empty(), "{:?}", report.mesh.degenerate);
        assert!(
            !report.issues.iter().any(|i| i.check == cadpole_print::Check::Degenerate),
            "{:?}",
            report.issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
        let under: Vec<_> = report.thickness.spots.iter().filter(|(_, mm)| *mm < tolerance).collect();
        assert!(under.is_empty(), "walls under the tessellation tolerance: {under:?}");
        assert!(report.thickness.min >= tolerance, "a wall {} mm thin", report.thickness.min);
    }

    #[test]
    fn a_thin_walled_box_is_flagged_at_the_right_thickness() {
        // 20 mm cube hollowed to a 0.6 mm shell, against the 0.8 mm default.
        // On the default B-rep kernel: the enclosed subtrahend becomes a void
        // in the result, and the checks run over both shells.
        let r = check(
            "outer = box(20, 20, 20)\n\
             inner = box(18.8, 18.8, 18.8, at=(0.6, 0.6, 0.6))\n\
             shell = subtract(outer, inner, name=\"shell\")\n",
        );
        let b = &r.bodies[0];
        assert!(b.watertight, "{b:?}");
        assert!(b.solid, "{b:?}");
        assert_eq!(b.components, 2, "the outer skin and the cavity");
        assert_eq!(b.genus, 0);
        assert!(!b.inverted_normals, "the cavity is a void, not the whole body inside out");
        let want = 20.0f64.powi(3) - 18.8f64.powi(3);
        assert!((b.volume_mm3 - want).abs() < want * 1e-5, "volume {} mm3, want {want}", b.volume_mm3);
        assert!((b.min_wall_mm - 0.6).abs() < 0.01, "min wall {}", b.min_wall_mm);
        assert!(b.thin_area_mm2 > 0.0);
        let i = b.issues.iter().find(|i| i.check == "thickness").expect("a thickness issue");
        assert_eq!(i.severity, "warning");
        assert!(i.message.contains("0.8 mm minimum"), "{}", i.message);
    }

    #[test]
    fn a_thick_walled_box_is_not_flagged() {
        let r = check(
            "outer = box(20, 20, 20)\n\
             inner = box(16, 16, 16, at=(2, 2, 2))\n\
             shell = subtract(outer, inner, name=\"shell\")\n",
        );
        let b = &r.bodies[0];
        assert!((b.min_wall_mm - 2.0).abs() < 0.01, "min wall {}", b.min_wall_mm);
        assert_eq!(b.thin_area_mm2, 0.0);
        assert!(b.issues.iter().all(|i| i.check != "thickness"), "{:?}", b.issues);
        assert!(r.ok);
    }

    #[test]
    fn a_model_bigger_than_the_bed_reports_the_excess() {
        let r = print_check(&pb::PrintCheckRequest {
            document: Some(doc("b = box(300, 100, 300)")),
            bed: vec![256.0, 256.0, 256.0],
            ..Default::default()
        })
        .unwrap();
        let b = &r.bodies[0];
        assert!(!b.fits_bed);
        let e = b.bed_excess_mm.as_ref().unwrap();
        assert!((e.x - 44.0).abs() < 1e-6, "{e:?}");
        assert_eq!(e.y, 0.0);
        assert!((e.z - 44.0).abs() < 1e-6, "{e:?}");
        assert!(!r.ok, "not fitting the machine is an error");
        let i = b.issues.iter().find(|i| i.check == "bed_fit").unwrap();
        assert!(i.message.contains("44 mm on X"), "{}", i.message);
    }

    /// The print-in-place rules run over the bodies of a mesh
    /// export, as warnings that name the export: `cadpole check` cannot tell a
    /// multi-material part from a plate of parts, so it advises and the
    /// script's `assert_printable([...])` decides.
    #[test]
    fn a_mesh_export_of_two_bodies_is_judged_as_one_print_with_warnings() {
        let src = "a = box(10, 10, 10, name=\"a\")\nb = box(10, 10, 10, at=(10.2, 0, 0), name=\"b\")\n";
        // No export: nothing between the bodies is judged.
        let r = check(src);
        assert!(r.ok);
        assert!(r.bodies.iter().all(|b| b.issues.iter().all(|i| i.check != "fuse")), "{:?}", r.bodies);
        // A STEP export is not a print.
        let r = check(&format!("{src}export([a, b], \"parts.step\")\n"));
        assert!(r.bodies.iter().all(|b| b.issues.iter().all(|i| i.check != "fuse")), "{:?}", r.bodies);
        // A 3MF is: the gap that would fuse is reported on the first body,
        // as a warning, with the export it is about.
        let r = check(&format!("{src}export([a, b], \"parts.3mf\")\n"));
        assert!(r.ok, "a warning, not an error: {:?}", r.bodies);
        let i = r.bodies[0].issues.iter().find(|i| i.check == "fuse").expect("a fuse finding on a");
        assert_eq!(i.severity, "warning");
        assert!(i.message.starts_with("parts.3mf: gap of 0.2 mm between a and b"), "{}", i.message);
        assert!(i.message.contains("between a/right and b/left"), "{}", i.message);
        assert!(i.at.is_some());
        assert!(r.bodies[1].issues.iter().all(|i| i.check != "fuse"), "once, on the first of the pair");
        // Restricting the check to one body leaves nothing to pair.
        let r = print_check(&pb::PrintCheckRequest {
            document: Some(doc(&format!("{src}export([a, b], \"parts.3mf\")\n"))),
            body_ids: vec!["a#0".into()],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(r.bodies.len(), 1);
        assert!(r.bodies[0].issues.iter().all(|i| i.check != "fuse"));
    }

    /// `cadpole check` takes what a whole-document export writes --
    /// hidden layers left out -- and `--layer` / `--body` name the bodies,
    /// adding up; a name that picks nothing is an error.
    #[test]
    fn the_check_takes_what_an_export_writes_and_names_bodies_by_layer_id_or_name() {
        let src = "walls = layer(\"walls\")\nsite = layer(\"site\", visible=False)\n\
                   a = box(10, 10, 10, name=\"a\", layer=walls)\n\
                   b = box(10, 10, 10, at=(20, 0, 0), name=\"b\")\n\
                   c = box(10, 10, 10, at=(40, 0, 0), name=\"c\", layer=site)\n";
        let run = |ids: &[&str], layers: &[&str], hidden: bool| {
            print_check(&pb::PrintCheckRequest {
                document: Some(doc(src)),
                body_ids: ids.iter().map(|s| s.to_string()).collect(),
                layer_names: layers.iter().map(|s| s.to_string()).collect(),
                include_hidden_layers: hidden,
                ..Default::default()
            })
        };
        let names = |r: anyhow::Result<pb::PrintCheckResponse>| -> Vec<String> {
            r.expect("check").bodies.iter().map(|b| b.name.clone()).collect()
        };
        assert_eq!(names(run(&[], &[], false)), ["a", "b"], "the hidden layer is left out");
        assert_eq!(names(run(&[], &[], true)), ["a", "b", "c"]);
        assert_eq!(names(run(&[], &["walls"], false)), ["a"]);
        assert_eq!(names(run(&[], &["site"], false)), ["c"], "a hidden layer named is checked");
        assert_eq!(names(run(&[], &["other"], false)), ["b"], "`other` is the implicit layer");
        assert_eq!(names(run(&["b"], &["walls"], false)), ["a", "b"], "by name, adding to the layer");
        assert_eq!(names(run(&["c#2"], &[], false)), ["c"], "by id, whatever its layer");
        let e = run(&[], &["wals"], false).unwrap_err().to_string();
        assert!(e.contains("no layer \"wals\"") && e.contains("walls, site"), "{e}");
        let e = run(&["nope"], &[], false).unwrap_err().to_string();
        assert!(e.contains("no body \"nope\""), "{e}");
        let all_hidden = "l = layer(\"ghost\", visible=False)\nl.add(box(10, 10, 10, name=\"a\"))\n";
        let e = print_check(&pb::PrintCheckRequest { document: Some(doc(all_hidden)), ..Default::default() })
            .unwrap_err()
            .to_string();
        assert!(e.contains("all 1 are on hidden layers") && e.contains("--include-hidden"), "{e}");
    }

    /// The parts of one `name=` object of a Bambu project are one
    /// piece on the printer, so `cadpole check` checks them fused: two 0.5 mm
    /// plates side by side are one 1 mm wall, a head resting on two piers is
    /// a bridge, and nothing between the parts is a `fuse` finding. Without
    /// `name=` the same parts are checked one by one, as before.
    #[test]
    fn a_named_object_is_checked_as_the_one_piece_a_slicer_prints() {
        let parts = "a = box(20, 0.5, 10, name=\"a\")\n\
                     b = box(20, 0.5, 10, at=(0, 0.5, 0), name=\"b\")\n\
                     p1 = box(5, 1, 10, at=(0, 5, 0), name=\"p1\")\n\
                     p2 = box(5, 1, 10, at=(14.5, 5, 0), name=\"p2\")\n\
                     head = box(9.5, 1, 2, at=(5, 5, 8), name=\"head\")\n";
        let named = check(&format!(
            "{parts}export([a, b, p1, p2, head], \"kit.3mf\", flavor=flavor.bambu, plate=\"L0\", name=\"L0\")\n"
        ));
        assert_eq!(named.bodies.len(), 1, "{:?}", named.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
        let o = &named.bodies[0];
        assert_eq!((o.name.as_str(), o.body_id.as_str()), ("L0", "kit.3mf:L0"));
        assert_eq!(o.fused_body_ids, ["a#0", "b#1", "p1#2", "p2#3", "head#4"]);
        assert!(o.solid, "{:?}", o.issues);
        assert!(o.issues.iter().all(|i| i.check != "thickness" && i.check != "fuse"), "{:?}", o.issues);
        assert!((o.min_wall_mm - 1.0).abs() < 0.01, "the two plates are one wall: {}", o.min_wall_mm);
        assert_eq!(o.unsupported_area_mm2, 0.0, "the head is a bridge between the piers: {:?}", o.issues);
        assert!((o.bridge_area_mm2 - 9.5).abs() < 1e-6 && o.bridges == 1, "{o:?}");
        assert!((o.max_bridge_mm - 10.0).abs() < 1e-9);
        assert!(o.issues.iter().any(|i| i.check == "fused" && i.message.contains("the 5 parts of object \"L0\"")));
        // The same parts as five objects: five reports, the plates thin, the
        // head floating in the air of its own report, fuse warnings between.
        let loose = check(&format!("{parts}export([a, b, p1, p2, head], \"kit.3mf\", flavor=flavor.bambu)\n"));
        assert_eq!(loose.bodies.len(), 5);
        assert!(loose.bodies[0].issues.iter().any(|i| i.check == "thickness"), "{:?}", loose.bodies[0].issues);
        assert!(loose.bodies.iter().any(|b| b.issues.iter().any(|i| i.check == "fuse")));
        // `--max-bridge` reaches the fused piece: at 9 mm the head is an
        // overhang again, and says it is a span.
        let r = print_check(&pb::PrintCheckRequest {
            document: Some(doc(&format!(
                "{parts}export([a, b, p1, p2, head], \"kit.3mf\", flavor=flavor.bambu, name=\"L0\")\n"
            ))),
            max_bridge: 9.0,
            ..Default::default()
        })
        .unwrap();
        let o = &r.bodies[0];
        assert!((o.unsupported_area_mm2 - 9.5).abs() < 1e-6, "{o:?}");
        assert!(o.issues.iter().any(|i| i.check == "overhang" && i.message.contains("max_bridge 9 mm")));
    }

    /// A story piece as a print kit makes it -- one part per
    /// filament, each joined on its own with `union(weld=)` -- a slab, the
    /// walls standing on it (one meeting the slab's corner only along an
    /// edge), and the frames with a 40-degree corbel under a lip. Every part
    /// is a solid and so is the whole; fused by a mesh boolean the parts'
    /// shared faces came back as doubled faces and pinched edges, and the
    /// check called the piece "NOT watertight". Taken apart into their boxes
    /// and joined on one grid, they are one solid of the welded whole's
    /// volume, and the corbel still carries the lip.
    #[test]
    fn a_piece_of_welded_parts_that_share_faces_is_checked_as_one_solid() {
        let kit = "slab = union(box(40, 30, 2.4, name=\"s0\"), weld=0.01, name=\"slab\")\n\
                   walls = union(box(40, 1.6, 24, at=(0, 0, 2.4), name=\"w0\"), box(1.6, 30, 24, at=(0, 0, 2.4), name=\"w1\"),\n\
                                 box(2, 2, 26.4, at=(40, 30, 0), name=\"post\"), weld=0.01, name=\"walls\")\n\
                   corbel = extrude(sketch(YZ.offset(10), name=\"cs\").polygon([(0, 18), (-0.4, 18), (0, 17.52)]), 8, name=\"corbel\")\n\
                   frames = union(box(8, 2, 2, at=(10, -0.4, 18), name=\"head\"), corbel, weld=0.01, name=\"frames\")\n\
                   export([slab, walls, frames], \"kit.3mf\", flavor=flavor.bambu, plate=\"L0\", name=\"L0\", extruder=[1, 2, 3])\n";
        let r = check(kit);
        assert_eq!(r.bodies.len(), 1, "{:?}", r.bodies.iter().map(|b| (&b.name, &b.issues)).collect::<Vec<_>>());
        let o = &r.bodies[0];
        assert_eq!(o.fused_body_ids.len(), 3);
        assert!(o.solid && o.watertight, "{:?}", o.issues);
        assert_eq!((o.non_manifold_edges, o.internal_faces, o.self_intersections), (0, 0, 0), "{o:?}");
        assert!(o.issues.iter().any(|i| i.check == "fused" && i.message.contains("are checked as the one piece")), "{:?}", o.issues);
        // The welded whole of the same boxes and corbel: the same volume,
        // to the bridge the weld puts along the post's edge contact.
        let whole = check(
            "u = union(box(40, 30, 2.4, name=\"s0\"), box(40, 1.6, 24, at=(0, 0, 2.4), name=\"w0\"),\n\
                       box(1.6, 30, 24, at=(0, 0, 2.4), name=\"w1\"), box(2, 2, 26.4, at=(40, 30, 0), name=\"post\"),\n\
                       box(8, 2, 2, at=(10, -0.4, 18), name=\"head\"),\n\
                       extrude(sketch(YZ.offset(10), name=\"cs\").polygon([(0, 18), (-0.4, 18), (0, 17.52)]), 8, name=\"corbel\"),\n\
                       weld=0.01, name=\"u\")\n",
        );
        let w = &whole.bodies[0];
        assert!(w.solid, "{:?}", w.issues);
        assert!((o.volume_mm3 - w.volume_mm3).abs() < 1e-3 * w.volume_mm3, "fused {} whole {}", o.volume_mm3, w.volume_mm3);
        assert!((o.unsupported_area_mm2 - w.unsupported_area_mm2).abs() < 0.05, "{} vs {}", o.unsupported_area_mm2, w.unsupported_area_mm2);
    }

    /// `support_free` keeps only the findings that mean "needs support",
    /// and the request's rules move them.
    #[test]
    fn support_free_keeps_only_what_needs_support() {
        let src = "post = box(4, 4, 30, name=\"post\")\n\
                   arm = box(14, 4, 4, at=(4, 0, 26), name=\"arm\")\n\
                   finger = box(2, 4, 8, at=(14, 0, 18), name=\"finger\")\n\
                   hook = union(post, arm, finger, name=\"hook\")\n";
        let ask = |area: f64, island: f64| {
            print_check(&pb::PrintCheckRequest {
                document: Some(doc(src)),
                support_free: true,
                max_overhang_area: area,
                min_island: island,
                ..Default::default()
            })
            .unwrap()
        };
        let r = ask(0.0, 0.0);
        assert!(!r.ok);
        let b = &r.bodies[0];
        assert!(!b.support_free);
        assert_eq!(b.islands, 1);
        assert!((b.deepest_island_mm - 8.0).abs() < 1e-6, "{}", b.deepest_island_mm);
        let support = ["overhang", "island", "cantilever", "horizontal_hole", "bridge"];
        assert!(b.issues.iter().all(|i| support.contains(&i.check.as_str())), "{:?}", b.issues);
        assert!(b.issues.iter().any(|i| i.check == "island" && i.message.contains("8 mm below")), "{:?}", b.issues);
        let r = ask(10.0, 9.0);
        assert!(r.ok, "{:?}", r.bodies[0].issues);
        assert!(r.bodies[0].support_free && r.bodies[0].issues.is_empty());
    }

    #[test]
    fn orientation_comes_back_with_its_reasoning() {
        let r = print_check(&pb::PrintCheckRequest {
            document: Some(doc("b = box(20, 20, 100)")),
            bed: vec![50.0, 120.0, 30.0],
            orient: true,
            ..Default::default()
        })
        .unwrap();
        let o = r.bodies[0].orientation.as_ref().expect("orient: true");
        assert!(o.changed, "{o:?}");
        assert!((o.height_mm - 20.0).abs() < 1e-6, "it has to lie down to fit: {o:?}");
        assert!(o.reasoning.contains("mm tall"), "{}", o.reasoning);
    }

    // ---- colour -----------------------------------------------------------

    fn export(src: &str, slicer: &str, group: &str) -> Vec<u8> {
        let d = doc(src);
        let (mut ctx, _) = crate::evaluate_document_with(&d, &std::collections::HashMap::new(), 0.0, 0.0);
        assert!(
            !ctx.diagnostics.iter().any(|x| x.severity == cadpole_core::context::Severity::Error),
            "{:?}",
            ctx.diagnostics
        );
        let ids: Vec<_> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
        export_3mf(&mut ctx, &ids, "t", slicer, group, None).unwrap().0
    }

    #[test]
    fn slots_are_assigned_in_order_of_first_use() {
        let bytes = export(
            "a = box(10, 10, 10, name=\"a\")\n\
             a.material(color=\"#112233\")\n\
             b = box(10, 10, 10, at=(20, 0, 0), name=\"b\")\n\
             b.material(color=\"#445566\")\n\
             c = box(10, 10, 10, at=(40, 0, 0), name=\"c\")\n\
             c.material(color=\"#112233\")\n",
            "prusa",
            "objects",
        );
        let z = cadpole_export_3mf::zip::read(&bytes).unwrap();
        let xml = z.text(cadpole_export_3mf::MODEL_PART).unwrap();
        assert_eq!(xml.matches("<base ").count(), 2, "two distinct colours, three bodies");
        assert!(xml.contains("displaycolor=\"#112233\""), "{xml}");
        // a and c share slot 1 (pindex 0), b is slot 2.
        let pindexes: Vec<&str> = xml.match_indices("pindex=\"").map(|(i, _)| &xml[i + 8..i + 9]).collect();
        assert_eq!(pindexes, vec!["0", "1", "0"]);
    }

    #[test]
    fn painted_faces_reach_the_file_as_per_triangle_colour() {
        let bytes = export(
            "b = box(10, 10, 10, name=\"b\")\n\
             b.material(color=\"#112233\")\n\
             paint(b, faces=b.faces(normal=(0, 0, 1)), color=\"#FF8800\")\n",
            "bambu",
            "objects",
        );
        let z = cadpole_export_3mf::zip::read(&bytes).unwrap();
        let xml = z.text(cadpole_export_3mf::MODEL_PART).unwrap();
        // The top face is two triangles; they and only they carry a property.
        assert_eq!(xml.matches(" p1=\"1\"").count(), 2, "{xml}");
        assert_eq!(xml.matches("paint_color=\"8\"").count(), 2, "slot 2 is the paint string \"8\"");
        assert_eq!(xml.matches("mmu_segmentation=\"8\"").count(), 2);
        assert!(xml.contains("displaycolor=\"#FF8800\""));
    }

    #[test]
    fn a_pinned_slot_keeps_its_number_and_leaves_a_gap() {
        let bytes = export(
            "b = box(10, 10, 10, name=\"b\")\n\
             b.material(color=\"#112233\")\n\
             paint(b, faces=b.faces(normal=(0, 0, 1)), color=\"#FF8800\", slot=4)\n",
            "prusa",
            "objects",
        );
        let z = cadpole_export_3mf::zip::read(&bytes).unwrap();
        let xml = z.text(cadpole_export_3mf::MODEL_PART).unwrap();
        assert_eq!(xml.matches("<base ").count(), 4, "slot 4 means four <base> entries: {xml}");
        assert!(xml.contains("mmu_segmentation=\"1C\""), "filament 4 is the string \"1C\": {xml}");
        assert_eq!(xml.matches(" p1=\"3\"").count(), 2, "0-based index of slot 4");
    }

    #[test]
    fn painting_a_whole_body_pins_its_filament() {
        let bytes = export(
            "a = box(10, 10, 10, name=\"a\")\n\
             b = box(10, 10, 10, at=(20, 0, 0), name=\"b\")\n\
             paint(b, color=\"#00FF00\", slot=3)\n",
            "prusa",
            "objects",
        );
        let z = cadpole_export_3mf::zip::read(&bytes).unwrap();
        let cfg = z.text(cadpole_export_3mf::PRUSA_MODEL_CONFIG).unwrap();
        assert!(cfg.contains("key=\"extruder\" value=\"3\""), "{cfg}");
        let xml = z.text(cadpole_export_3mf::MODEL_PART).unwrap();
        assert!(!xml.contains("mmu_segmentation"), "a whole body needs no per-triangle paint: {xml}");
    }

    #[test]
    fn grouping_part_puts_both_bodies_in_one_object() {
        let bytes = export(
            "a = box(10, 10, 10, name=\"a\")\n\
             b = box(10, 10, 10, at=(0, 0, 10), name=\"b\")\n\
             b.material(color=\"#00FF00\")\n",
            "prusa",
            "part",
        );
        let z = cadpole_export_3mf::zip::read(&bytes).unwrap();
        let xml = z.text(cadpole_export_3mf::MODEL_PART).unwrap();
        assert_eq!(xml.matches("<object ").count(), 1);
        assert_eq!(xml.matches("<item ").count(), 1);
        assert_eq!(xml.matches("<triangle ").count(), 24);
        let cfg = z.text(cadpole_export_3mf::PRUSA_MODEL_CONFIG).unwrap();
        assert!(cfg.contains("firstid=\"0\" lastid=\"11\"") && cfg.contains("firstid=\"12\" lastid=\"23\""), "{cfg}");
    }

    #[test]
    fn a_face_name_that_does_not_exist_is_a_script_error_not_a_silent_miss() {
        let d = doc("b = box(10, 10, 10, name=\"b\")\npaint(b, faces=[\"face:99\"], color=\"#FF0000\")\n");
        let (ctx, _) = crate::evaluate_document_with(&d, &std::collections::HashMap::new(), 0.0, 0.0);
        let msg = ctx.diagnostics.iter().map(|x| x.message.clone()).collect::<Vec<_>>().join("; ");
        assert!(msg.contains("face:99"), "{msg}");
    }

    #[test]
    fn the_analysis_render_draws_a_legend_and_the_plain_one_does_not() {
        let plain = crate::render(&pb::RenderRequest {
            document: Some(doc("b = box(20, 20, 20)")),
            width: 400,
            height: 300,
            ..Default::default()
        })
        .unwrap();
        let analysed = crate::render(&pb::RenderRequest {
            document: Some(doc("b = box(20, 20, 20)")),
            width: 400,
            height: 300,
            analysis: "thickness".into(),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(plain.png, analysed.png, "the analysis view has to look different");
        let bad = crate::render(&pb::RenderRequest {
            document: Some(doc("b = box(20, 20, 20)")),
            width: 64,
            height: 48,
            analysis: "zebra".into(),
            ..Default::default()
        });
        assert!(bad.unwrap_err().to_string().contains("thickness"), "an unknown view says what is available");
    }
}
