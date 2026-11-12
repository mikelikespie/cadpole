//! Plating a document's bodies: `print_plan(...)` and the `Plate` operation.
//!
//! The planner is `cadpole_print::plate`, meshes in and a plan out. This is
//! the part that knows the document: it reads each body's mesh, colour and
//! paint, asks the planner, makes the placed copies on a hidden `print`
//! layer (each copy carrying its source's paint), and records ordinary
//! `export(..., flavor=flavor.bambu, plate=...)` specs, so the Bambu project
//! writer -- plates, filament slots, prime tower, thumbnails -- writes the
//! file exactly as it writes a hand-made one. `docs/plating.md` is the
//! design.

use std::collections::BTreeMap;

use cadpole_core::context::{BambuTemplate, ExportSpec, PlateRef};
use cadpole_core::export::Flavor;
use cadpole_core::geom::Vec3;
use cadpole_core::tessellate::{Mesh, Topology};
use cadpole_core::{BodyId, Context, Span};
use cadpole_export_3mf::bambu::Machine;
use cadpole_print::plate::{self, Item, Options, Pin, Plan, PlanBody, Variant};

/// The layer the placed copies go on, hidden, when the script has not
/// declared it.
pub const PRINT_LAYER: &str = "print";

/// One thing to plate: a body, or a list that is one piece.
#[derive(Clone, Debug, Default)]
pub struct PlateItem {
    pub bodies: Vec<BodyId>,
    pub declared: bool,
    pub pin: Pin,
}

/// What to plate and how.
#[derive(Clone, Debug)]
pub struct PlateRequest {
    pub items: Vec<PlateItem>,
    /// The printer, by `printer()`'s names; empty is the X1 Carbon.
    pub printer: String,
    /// A Bambu Studio project to take settings (and so the machine) from.
    pub template: Option<BambuTemplate>,
    pub ams: u32,
    pub spacing: f64,
    pub joint_gap: f64,
    pub max_bridge: f64,
    pub variants: Vec<Variant>,
    /// Where the plan is written, as a Bambu project; `None` places the
    /// copies and records nothing.
    pub path: Option<String>,
    pub tolerance: Option<f64>,
    /// The layer the copies go on.
    pub layer: String,
    pub span: Option<Span>,
}

impl Default for PlateRequest {
    fn default() -> PlateRequest {
        PlateRequest {
            items: Vec::new(),
            printer: String::new(),
            template: None,
            ams: 4,
            spacing: plate::DEFAULT_SPACING,
            joint_gap: plate::DEFAULT_JOINT_GAP,
            max_bridge: cadpole_print::overhang::DEFAULT_MAX_BRIDGE,
            variants: vec![Variant::Colour],
            path: None,
            tolerance: None,
            layer: PRINT_LAYER.to_owned(),
            span: None,
        }
    }
}

/// What came of it.
#[derive(Clone, Debug)]
pub struct Plated {
    pub plan: Plan,
    pub options: Options,
    pub report: String,
    /// The body each plan body index stands for.
    pub sources: Vec<BodyId>,
    /// Per placement, the copies made, in the piece's body order.
    pub copies: Vec<Vec<BodyId>>,
    /// The printer the plan was made for, by id.
    pub printer: String,
}

/// The printer table's machine, as the Bambu writer lays plates out on it.
pub fn machine_of_printer(p: &cadpole_print::Printer) -> Machine {
    use cadpole_export_3mf::bambu::{Presets, Reach};
    Machine {
        name: p.display_name.to_owned(),
        area: [0.0, 0.0, p.bed[0], p.bed[1]],
        height: p.bed[2],
        exclude: p.exclude.to_vec(),
        nozzles: p.nozzles.iter().map(|n| Reach { area: n.area, height: n.height, hotends: n.hotends }).collect(),
        nozzle_diameter: p.nozzle,
        presets: p.bambu.map(|b| Presets {
            model: b.model.to_owned(),
            printer: b.printer.to_owned(),
            process: b.process.to_owned(),
            filament: b.filament.to_owned(),
            filament_id: b.filament_id.to_owned(),
        }),
    }
}

/// The printer a plan is made for when none is named: what a Bambu
/// project is laid out for by default.
pub const DEFAULT_PRINTER: &str = "bambu_x1c";

/// Plan, place the copies, and (with a path) record the project's exports.
pub fn plate_bodies(ctx: &mut Context, req: &PlateRequest) -> anyhow::Result<Plated> {
    let (options, printer) = options_for(req)?;

    // The bodies, once each, in the order the items name them.
    let mut sources: Vec<BodyId> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    for it in &req.items {
        let mut idx = Vec::new();
        for id in &it.bodies {
            let k = match sources.iter().position(|s| s == id) {
                Some(k) => k,
                None => {
                    sources.push(*id);
                    sources.len() - 1
                }
            };
            idx.push(k);
        }
        items.push(Item { bodies: idx, declared: it.declared, pin: it.pin.clone() });
    }
    if sources.is_empty() {
        anyhow::bail!("there is nothing to plate");
    }
    let paints = cadpole_core::paint::paints(ctx);
    let (meshes, names, colours) = load(ctx, &sources, &paints, req.tolerance)?;
    let bodies: Vec<PlanBody<'_>> = (0..sources.len())
        .map(|i| PlanBody {
            name: names[i].clone(),
            mesh: &meshes[i].0,
            topo: Some(&meshes[i].1),
            colours: colours[i].0.clone(),
            tri_colours: colours[i].1.clone(),
        })
        .collect();
    let plan = plate::plan(&bodies, &items, &options);
    let report = plan.report(&options);
    drop(bodies);

    // The copies, on the print layer.
    if !ctx.layers.iter().any(|l| l.name == req.layer) {
        ctx.declare_layer(cadpole_core::layers::LayerDecl {
            name: req.layer.clone(),
            doc: "The pieces as they print, laid out on their plates".to_owned(),
            visible: false,
            opacity: 1.0,
            color: None,
            span: req.span.clone(),
        })?;
    }
    let mut copies: Vec<Vec<BodyId>> = Vec::with_capacity(plan.placements.len());
    for pl in &plan.placements {
        let piece = &plan.pieces[pl.piece];
        let plate_no = pl.plate + 1;
        let mut ids = Vec::new();
        for &b in &piece.bodies {
            let src = sources[b];
            let src_id = ctx.body(src)?.id_string();
            let id = ctx.duplicate(src, &pl.transform, None, req.span.clone())?;
            {
                let rec = ctx.body_mut(id)?;
                rec.name = format!("{} (plate {plate_no})", names[b]);
                rec.visible = true;
            }
            ctx.set_layer(id, &req.layer)?;
            // Paint is recorded against a body; the copy gets its own.
            let new_id = ctx.body(id)?.id_string();
            let feature = ctx.body(id)?.feature.clone();
            for p in paints.iter().filter(|p| p.body_id == src_id) {
                let args = cadpole_core::paint::args(&new_id, &p.faces, p.color, p.slot);
                ctx.new_feature(cadpole_core::paint::KIND, None, vec![feature.clone()], args, req.span.clone())?;
            }
            ids.push(id);
        }
        copies.push(ids);
    }

    if let Some(path) = &req.path {
        for (k, pl) in plan.placements.iter().enumerate() {
            let piece = &plan.pieces[pl.piece];
            let plate = &plan.plates[pl.plate];
            let mono = plate.variant == Some(Variant::Mono);
            ctx.exports.push(ExportSpec {
                bodies: copies[k].clone(),
                path: path.clone(),
                format: "3mf".to_owned(),
                tolerance: req.tolerance,
                flavor: Flavor::Bambu,
                plate: Some(PlateRef::Name(plate.name.clone())),
                name: Some(piece.name.clone()),
                extruders: if mono { vec![1; copies[k].len()] } else { Vec::new() },
                printer: if req.template.is_some() { String::new() } else { printer.id.to_owned() },
                bed: None,
                template: req.template.clone(),
            });
        }
    }
    for n in &plan.notes {
        ctx.diagnostics.push(cadpole_core::context::Diagnostic::warning(format!("print_plan: {n}")).with_span(req.span.clone()));
    }
    Ok(Plated { plan, options, report, sources, copies, printer: printer.id.to_owned() })
}

/// The planner's options for a request: the printer's (or the template's)
/// machine, and the request's numbers.
fn options_for(req: &PlateRequest) -> anyhow::Result<(Options, &'static cadpole_print::Printer)> {
    let printer_id = if req.printer.is_empty() { DEFAULT_PRINTER } else { req.printer.as_str() };
    let printer = cadpole_print::printer(printer_id)
        .ok_or_else(|| anyhow::anyhow!("unknown printer {printer_id:?}; Cadpole knows {}", cadpole_print::printer_list()))?;
    let mut profile = printer.profile();
    let (machine, tower) = match &req.template {
        Some(t) => {
            let tpl = cadpole_export_3mf::bambu_settings::Template::from_parts(&t.application, &t.settings)
                .map_err(|e| anyhow::anyhow!("template {:?}: {e}", t.asset))?;
            let m = tpl.machine();
            profile.bed = [m.area[2] - m.area[0], m.area[3] - m.area[1], m.height];
            (m, cadpole_export_3mf::bambu_settings::tower_room(Some(&tpl)))
        }
        None => (machine_of_printer(printer), cadpole_export_3mf::bambu_settings::tower_room(None)),
    };
    let options = Options {
        profile,
        max_bridge: req.max_bridge,
        region: machine.shared(),
        exclude: machine.exclude.clone(),
        tower: tower.map(|r| r.span()),
        ams: req.ams,
        spacing: req.spacing,
        joint_gap: req.joint_gap,
        cell: 1.0,
        variants: if req.variants.is_empty() { vec![Variant::Colour] } else { req.variants.clone() },
        printer: printer.id.to_owned(),
    };
    Ok((options, printer))
}

type Loaded = (Vec<(Mesh, Topology)>, Vec<String>, Vec<(Vec<String>, Vec<u8>)>);

/// Each body's mesh, name, and filaments with the per-triangle paint.
fn load(ctx: &mut Context, sources: &[BodyId], paints: &[cadpole_core::paint::Paint], tolerance: Option<f64>) -> anyhow::Result<Loaded> {
    let mut meshes: Vec<(Mesh, Topology)> = Vec::with_capacity(sources.len());
    let mut names = Vec::new();
    let mut colours: Vec<(Vec<String>, Vec<u8>)> = Vec::new();
    for id in sources {
        let (name, base, id_string) = {
            let b = ctx.body(*id)?;
            (b.name.clone(), cadpole_core::paint::hex(b.material.base_color), b.id_string())
        };
        let (mesh, topo) = ctx.body_mesh_with(*id, tolerance)?.clone();
        let mut list = vec![base];
        let mut tri: Vec<u8> = Vec::new();
        for p in paints.iter().filter(|p| p.body_id == id_string && !p.hex.is_empty()) {
            if p.faces.is_empty() {
                // The whole body: its filament is the paint's, and earlier
                // face paint is painted over.
                list = vec![p.hex.clone()];
                tri.clear();
                continue;
            }
            let k = match list.iter().position(|c| c == &p.hex) {
                Some(k) => k,
                None => {
                    list.push(p.hex.clone());
                    list.len() - 1
                }
            };
            if tri.is_empty() {
                tri = vec![0; mesh.triangle_count()];
            }
            let faces: Vec<u32> =
                p.faces.iter().filter_map(|n| topo.faces.iter().position(|f| &f.name == n).map(|i| i as u32)).collect();
            for (t, f) in mesh.face_ids.iter().enumerate() {
                if faces.contains(f) {
                    tri[t] = k.min(255) as u8;
                }
            }
        }
        names.push(name);
        colours.push((list, tri));
        meshes.push((mesh, topo));
    }
    Ok((meshes, names, colours))
}

/// One piece judged as it stands.
#[derive(Clone, Debug)]
pub struct Judged {
    pub name: String,
    pub verdict: plate::score::Verdict,
    /// A print-in-place piece keeps its modelled pose whatever the verdict,
    /// so it is never called worse.
    pub in_place: bool,
}

impl Judged {
    /// Placed worse than the best by more than `tolerance`.
    pub fn worse(&self, tolerance: f64) -> bool {
        !self.in_place && !self.verdict.ok(tolerance)
    }
}

/// Judge `ids` as placed: the pieces the planner finds in them (a
/// print-in-place group stays one piece, and is not turned), each as it
/// stands against the best orientation found.
pub fn judge_orientation(ctx: &mut Context, ids: &[BodyId], req: &PlateRequest, along: Option<Vec3>) -> anyhow::Result<Vec<Judged>> {
    let (options, _) = options_for(req)?;
    let paints = cadpole_core::paint::paints(ctx);
    let (meshes, names, colours) = load(ctx, ids, &paints, req.tolerance)?;
    let bodies: Vec<PlanBody<'_>> = (0..ids.len())
        .map(|i| PlanBody {
            name: names[i].clone(),
            mesh: &meshes[i].0,
            topo: Some(&meshes[i].1),
            colours: colours[i].0.clone(),
            tri_colours: colours[i].1.clone(),
        })
        .collect();
    let items: Vec<Item> = (0..ids.len()).map(|i| Item { bodies: vec![i], ..Item::default() }).collect();
    let mut notes = Vec::new();
    let groups = plate::pieces::join(&bodies, &items, &options, &mut notes);
    let pin = Pin { along, ..Pin::default() };
    Ok(groups
        .into_iter()
        .map(|g| {
            let in_place = g.kind == plate::PieceKind::PrintInPlace;
            let (mesh, topo) = if g.bodies.len() == 1 {
                (meshes[g.bodies[0]].0.clone(), Some(&meshes[g.bodies[0]].1))
            } else {
                (plate::pieces::without_contacts(&g.bodies.iter().map(|&b| &meshes[b].0).collect::<Vec<_>>()), None)
            };
            Judged { name: g.name, verdict: plate::score::judge(&mesh, topo, &pin, &options), in_place }
        })
        .collect())
}

/// The model direction a script names: `(x, y, z)` or one of `"-z"`, `"+x"`,
/// ..., `"top"`/`"bottom"`/`"front"`/`"back"`/`"left"`/`"right"`.
pub fn direction_word(s: &str) -> Option<Vec3> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "+x" | "x" | "right" => Vec3::X,
        "-x" | "left" => -Vec3::X,
        "+y" | "y" | "back" => Vec3::Y,
        "-y" | "front" => -Vec3::Y,
        "+z" | "z" | "top" => Vec3::Z,
        "-z" | "bottom" => -Vec3::Z,
        _ => return None,
    })
}

/// A small map the report data is built from, so the builtin and the
/// operation say the same thing.
pub fn piece_rows(p: &Plated) -> Vec<BTreeMap<&'static str, String>> {
    let mut out = Vec::new();
    for pl in &p.plan.placements {
        let piece = &p.plan.pieces[pl.piece];
        let c = &piece.candidates[pl.candidate];
        let base = piece.baseline.map(|b| &piece.candidates[b]);
        let mut row = BTreeMap::new();
        row.insert("name", piece.name.clone());
        row.insert("plate", p.plan.plates[pl.plate].name.clone());
        row.insert("kind", piece.kind.as_str().to_owned());
        row.insert("orientation", c.label.clone());
        row.insert("support_area", format!("{}", c.support_area));
        row.insert("modelled_support_area", base.map(|b| format!("{}", b.support_area)).unwrap_or_default());
        row.insert("spin", format!("{}", pl.spin));
        row.insert("reason", piece.reason.clone());
        // Every orientation fully scored, best first: what the choice was
        // made from.
        row.insert(
            "considered",
            piece
                .candidates
                .iter()
                .filter(|k| k.full)
                .map(|k| format!("{}: cost {:.1}, {:.1} mm² support, {} islands", k.label, k.cost, k.support_area, k.islands))
                .collect::<Vec<_>>()
                .join("; "),
        );
        out.push(row);
    }
    out
}
