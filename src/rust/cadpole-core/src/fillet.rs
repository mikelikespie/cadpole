//! Constant-radius fillets on the exact path:
//! the blend is a natural quadric built by hand — a cylinder along a
//! straight edge, a torus round a circular one, a sphere octant where three
//! meet at an orthogonal corner — and it goes into the solid by **re-wiring
//! truck's shells over shared edges**, never by a boolean. A fillet's contact
//! curves are tangent to the faces by definition, and a tangent contact is
//! exactly what the boolean refuses, so this is construction.
//!
//! **Where the topology comes from.** Two routes, chosen per chain of runs:
//!
//! * **A closed chain on one planar face** whose joints are tangent (a
//!   rim, a boss root, a hole's mouth or floor, the rim of a rounded
//!   outline) or right-angle corners of two straight runs on a straight
//!   seam (a prism cap's rim) is re-wired **directly**. Its topology is one
//!   vertex split per seam: the rim vertex `V` (two rim edges and one
//!   straight seam up the wall) becomes `W` on the plane and `U` on the
//!   wall, the seam is trimmed to `U`, the rim edges move out to the two
//!   contact curves, and one blend patch per rim edge is bounded by them and
//!   by the joint edges `U–W` at its ends — the section arc at a tangent
//!   joint, the blends' ellipse at a corner. Nothing is cut, so the
//!   chamfer's refusals for inner loops (a plate top with holes in it) and
//!   for long circular seams do not apply, and it costs milliseconds where
//!   the chamfer's ring boolean cost seconds.
//! * **Everything else** — straight edges ending on perpendicular planar
//!   faces, chains turning corners, three blends at a corner — takes its
//!   topology from the chamfer with `Setback::Fillet`, whose setback
//!   curves are the fillet's contact curves: the cutters are built and
//!   subtracted exactly as `chamfer` does, then every chamfer face is
//!   **replaced** by the blend patch over the same contact edges (rebuilt as
//!   exact lines and arcs through the boolean's vertices), each end segment
//!   on a perpendicular planar face by the section arc, each mitre line
//!   between two blends by their intersection — a planar ellipse arc, exact
//!   as a rational quadratic — and at an orthogonal three-way corner the
//!   three mitre lines and their centre vertex by three quarter arcs and a
//!   sphere octant (`vertex[<vertex>]`).
//!
//! **What truck needs**: faces sharing an edge must share
//! the `Edge` object, so every replacement is built once, keyed by the old
//! edge's id, and every face that carried the old edge is rebuilt over the
//! new one with its own orientation kept; the shell is then checked closed
//! and the result tessellated and validated before it is handed back.
//!
//! **Refused, by name**, with `kernel="field"` as the way out:
//! everything the planner refuses for a chamfer; a blend whose end face is
//! neither a plane perpendicular to the edge nor a blend it can meet; two
//! blends meeting at a vertex whose contact points do not coincide; three
//! blends at a corner that is not orthogonal; a rim vertex with anything but
//! one straight seam; a feature inside the blend's footprint.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use truck_meshalgo::prelude::*;
use truck_modeling::{
    BSplineCurve, BSplineSurface, Curve, Edge, EdgeID, Face, KnotVec, Line, NurbsCurve, NurbsSurface, Shell,
    ShellCondition, Solid, Surface, Vector4, Vertex, VertexID, Wire,
};

use crate::chamfer::{
    self, build_cutter, chains_of, check_mixed_corners, check_run, clearance, exact_edges, fmt_pt, naming_list,
    parallel, pick_edges, plan_runs, ring_geom, rotate_about, same_surface, setback_curves, EdgeChamfer, Op, Path,
    Picked, Planned, Probe, Run, Section, Setback, SetbackCurve, CHAMFER_TOLERANCE, TAU,
};
use crate::context::{fmt_num, match_tolerance, Body, NameSource, Span};
use crate::geom::Vec3;
use crate::ids::{BodyId, FeatureId};
use crate::kernel::{self, BoolOp, KSolid};
use crate::memo::{KeyBuilder, Produced};
use crate::naming::{self, NamedFace, Role};
use crate::surface_class::{classify, SurfaceClass, EXACT_TOL};
use crate::tessellate::Topology;
use crate::{fmath, Context, CadpoleError, Result};

/// What [`Context::fillet`] made: the body, and one record per edge run with
/// `face` the blend's name (`<feature>/blend[<edge>]`), the neighbours in
/// name order, the contact distance along each (`r·cot(γ/2)`), the wedge
/// angle, the convexity and the two contact curves.
#[derive(Clone, Debug)]
pub struct Filleted {
    pub body: BodyId,
    pub edges: Vec<EdgeChamfer>,
}

const FIELD: &str = "or pass kernel=\"field\" for a mesh result";

/// A contact circle this small, in mm, is the corner's axis itself: its arc
/// is a point.
const POLE_TOL: f64 = 1e-6;

impl Context {
    /// Fillet the named edges of `body` at `radius` on the exact path.
    /// `edges` empty means every edge of the body that is a corner.
    ///
    /// The result is a B-rep body whose blends are named
    /// `<feature>/blend[<edge>]` (a sphere octant `<feature>/vertex[<vertex>]`),
    /// whose neighbours keep their names, and whose new edges follow the
    /// face-pair grammar. What the exact path cannot build is an error naming
    /// the edge and the fix — the field path is never taken on its own.
    pub fn fillet(
        &mut self,
        body: BodyId,
        edges: &[String],
        radius: f64,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Filleted> {
        if !(radius > 0.0) || !radius.is_finite() {
            return Err(CadpoleError::invalid(format!("fillet: the radius must be a positive distance, got {radius}")));
        }
        let source = self.body(body)?;
        let inputs = vec![source.feature.clone()];
        let material = source.material.clone();
        let layer = source.layer.clone();
        let body_name = source.name.clone();
        let solid = match &source.body {
            Body::Brep(s) => s.clone(),
            Body::Mesh(_) => {
                return Err(CadpoleError::invalid(format!(
                    "fillet: `{body_name}` is a mesh body, which has no exact edges to fillet; pass kernel=\"field\" to \
                     fillet it on the field path (the result is a mesh body)"
                )));
            }
        };
        let mut args = BTreeMap::new();
        args.insert("body".into(), body_name.clone());
        args.insert("radius".into(), fmt_num(radius));
        let fid = self.new_feature("fillet", name.clone(), inputs, args, span.clone())?;

        // ---- the plan: the chamfer's, with the fillet's setback
        let (mesh, topo) = self.body_mesh(body)?.clone();
        let probe = Probe {
            mesh: &mesh,
            topo: &topo,
            soup: (0..mesh.triangle_count()).filter_map(|t| mesh.triangle(t)).collect(),
        };
        let exact = exact_edges(&solid);
        let setback = Setback::Fillet { radius };
        let picked = pick_edges(Op::Fillet, &body_name, edges, &topo, &exact)?;
        let Planned { picked, runs, seams } = plan_runs(Op::Fillet, picked, &probe, &topo, setback, !edges.is_empty())?;
        if picked.is_empty() {
            return Err(CadpoleError::invalid(format!(
                "fillet: `{body_name}` has no edge to fillet{}",
                if seams.is_empty() {
                    String::new()
                } else {
                    format!(" (its edges are all smooth seams: {})", naming_list(seams.iter().map(String::as_str)))
                }
            )));
        }
        if !seams.is_empty() {
            self.info(
                format!(
                    "fillet: skipped {} smooth seam{} that {} no corner: {}",
                    seams.len(),
                    if seams.len() == 1 { "" } else { "s" },
                    if seams.len() == 1 { "is" } else { "are" },
                    naming_list(seams.iter().map(String::as_str))
                ),
                &fid,
                span.clone(),
            );
        }
        let plan = partition(&picked, &runs, &topo);

        // The cut route's checks run before anything is built, as `chamfer`'s
        // do; the direct route checks its footprint as it builds.
        let tol = self.boolean_tolerance.min(CHAMFER_TOLERANCE);
        let h = clearance(tol);
        let mut cut_runs: Vec<Run> = plan.cut.iter().map(|&k| runs[k].clone()).collect();
        let cut_chains = chains_of(&mut cut_runs, h);
        check_mixed_corners(Op::Fillet, &cut_runs, &picked, &topo)?;
        for run in &cut_runs {
            check_run(Op::Fillet, run, &cut_runs, &picked, &probe, &topo, self.tolerance)?;
        }

        // ---- the records
        let mut records = Vec::with_capacity(runs.len());
        for run in &runs {
            let names: Vec<&str> = run.members.iter().map(|&m| picked[m].name.as_str()).collect();
            records.push(EdgeChamfer {
                edge: names[0].to_owned(),
                aliases: names[1..].iter().map(|s| (*s).to_owned()).collect(),
                face: blend_name(&fid, names[0]),
                faces: [topo.faces[run.faces[0]].name.clone(), topo.faces[run.faces[1]].name.clone()],
                setback: run.setback,
                wedge: run.frame.wedge(),
                convex: run.convex,
                curves: setback_curves(run),
            });
        }
        // The contact distances are `r·cot(γ/2)` for a wedge measured off the
        // mesh's normals, so they carry noise in the last bits; the record
        // says them to the nanometre.
        let nm = |d: f64| fmt_num((d * 1e9).round() / 1e9);
        let arg_edges: Vec<String> = records
            .iter()
            .map(|r| format!("{} (contact {} along `{}`, {} along `{}`)", r.edge, nm(r.setback[0]), r.faces[0], nm(r.setback[1]), r.faces[1]))
            .collect();
        self.set_feature_arg(&fid, "edges", arg_edges.join("; "));

        // ---- build, through the memo: keyed by the feature the
        // names are minted under, the body, the edges as given, the radius
        // and the routing inputs.
        let mut key = KeyBuilder::new("fillet");
        key.put(&fid).body(&self.body(body)?.key);
        key.tag("edges").u64(edges.len() as u64);
        for e in edges {
            key.str(e);
        }
        key.f64(radius);
        self.routing_key(&mut key);
        let mesh_tol = self.tolerance;
        let produced = self.memoised("fillet", &fid, span.clone(), key.finish(), |ctx| {
            let _span = kernel::trace::Span::start(format!("fillet {} run(s)", runs.len()));
            let mut blends: Vec<(SurfaceClass, String)> = Vec::new();
            let mut acc = solid;
            // The direct chains first, on the source solid, whose edges the
            // topology indexes; the cutters do not care what they cut.
            if !plan.direct.is_empty() {
                let _span = kernel::trace::Span::start(format!(
                    "fillet: re-wire {} closed chain(s), {} run(s)",
                    plan.direct.len(),
                    plan.direct.iter().map(|c| c.runs.len()).sum::<usize>()
                ));
                acc = build_closed_chains(&acc, &topo, &probe, &picked, &runs, &plan.direct, radius, mesh_tol, &fid, &mut blends)?;
            }
            if !cut_runs.is_empty() {
                let saved = ctx.boolean_tolerance;
                ctx.boolean_tolerance = tol;
                let cut = cut_by_chamfer(ctx, acc, &cut_chains, &cut_runs, &picked, &fid, span.clone());
                ctx.boolean_tolerance = saved;
                let cut = cut?;
                let _span = kernel::trace::Span::start("fillet: swap the chamfer faces for blends");
                acc = swap_chamfers(&cut, &topo, &picked, &cut_runs, radius, &fid, &mut blends)?;
            }
            let acc = kernel::canonical(acc);

            // The result is a solid the tessellator can close, or this call is
            // a refusal that says so.
            let body = Body::Brep(acc);
            let checked = {
                let _span = kernel::trace::Span::start("fillet: tessellate and validate the result");
                body.to_mesh(mesh_tol)
            };
            let names: Vec<&str> = records.iter().map(|r| r.edge.as_str()).collect();
            let which = if names.len() == 1 { format!("edge `{}`", names[0]) } else { format!("edges {}", naming_list(names.iter().copied())) };
            match checked {
                Err(e) => {
                    let holes = match &body {
                        Body::Brep(solid) => open_edge_samples(solid, mesh_tol, 4),
                        Body::Mesh(_) => Vec::new(),
                    };
                    let near = if holes.is_empty() {
                        String::new()
                    } else {
                        format!(" The open edges are near {}.", naming_list(holes.iter().map(String::as_str)))
                    };
                    return Err(CadpoleError::kernel(format!(
                        "fillet: the blends for {which} were built and wired into the shell, but the solid cannot be \
                         meshed: {}.{near} {FIELD}",
                        e.to_string().trim_end_matches('.')
                    )));
                }
                Ok((mesh, _)) => {
                    let report = crate::solid::validate_with(&mesh, &crate::solid::SolidOptions { pair_limit: 0, ..Default::default() });
                    if !report.oriented() {
                        return Err(CadpoleError::kernel(format!(
                            "fillet: the blends for {which} were built and wired into the shell, but the solid that came \
                             out is not closed and consistently oriented: {}. {FIELD}",
                            report.problems().join("; ")
                        )));
                    }
                }
            }
            // Names: every blend by the surface it lies on, everything else
            // matched back to the source body's named surfaces.
            let generator = Arc::new(naming::Generator::Surfaces { faces: blends, tol: match_tolerance(mesh_tol) });
            let sources = Arc::new(source_catalogue(&mesh, &topo, mesh_tol));
            let names = NameSource::Rewired { feature: fid.clone(), generator, sources };
            Ok(Produced::one(body, names))
        })?;

        self.consume_body(body)?;
        let id = self.add_produced(&fid, produced, name)?[0];
        self.body_mut(id)?.material = material;
        self.body_mut(id)?.layer = layer;
        Ok(Filleted { body: id, edges: records })
    }
}

fn blend_name(fid: &FeatureId, edge: &str) -> String {
    format!("{}/blend[{}]", fid.0, edge)
}

/// Where a tessellation of `solid` at `tol` is open: the midpoints of up to
/// `limit` edges that only one triangle carries, formatted, in the order the
/// mesh has them. The same weld [`kernel::tessellate_reporting`] judges the
/// mesh by, so what it calls a hole is what this points at.
fn open_edge_samples(solid: &KSolid, tol: f64, limit: usize) -> Vec<String> {
    let ts = kernel::with_cones_revolved(solid, crate::ksurface::DivisionBounds::default());
    let Ok(meshed) = kernel::guarded("tessellate", std::panic::AssertUnwindSafe(|| ts.triangulation(tol))) else {
        return Vec::new();
    };
    let mut mesh = crate::tessellate::Mesh::default();
    for (fi, face) in meshed.face_iter().enumerate() {
        let Some(poly) = face.surface() else { continue };
        let positions = poly.positions();
        let base = mesh.vertex_count() as u32;
        let mut remap: HashMap<usize, u32> = HashMap::new();
        for tri in poly.tri_faces() {
            let mut ids = [0u32; 3];
            for (k, v) in tri.iter().enumerate() {
                ids[k] = *remap.entry(v.pos).or_insert_with(|| {
                    let p = positions[v.pos];
                    mesh.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
                    mesh.normals.extend_from_slice(&[0.0, 0.0, 0.0]);
                    base + (mesh.vertex_count() as u32 - base - 1)
                });
            }
            if !face.orientation() {
                ids.swap(1, 2);
            }
            mesh.indices.extend_from_slice(&ids);
            mesh.face_ids.push(fi as u32);
        }
    }
    let welded = mesh.welded(crate::mesh_kernel::WELD_EPS);
    let mut edges: HashMap<(u32, u32), (i32, u32)> = HashMap::new();
    let mut order: Vec<(u32, u32)> = Vec::new();
    for (t, &face) in welded.indices.chunks_exact(3).zip(&welded.face_ids) {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let key = if a < b { (a, b) } else { (b, a) };
            let e = edges.entry(key).or_insert_with(|| {
                order.push(key);
                (0, face)
            });
            e.0 += if a < b { 1 } else { -1 };
        }
    }
    let at = |i: u32| {
        let i = i as usize * 3;
        Vec3::new(welded.positions[i] as f64, welded.positions[i + 1] as f64, welded.positions[i + 2] as f64)
    };
    let kinds: Vec<String> = solid.face_iter().map(|f| classify(&f.oriented_surface(), EXACT_TOL).kind_name().to_string()).collect();
    order
        .iter()
        .filter(|k| edges[k].0 != 0)
        .take(limit)
        .map(|&(a, b)| {
            let f = edges[&(a, b)].1 as usize;
            format!("{} (on {} face {f})", fmt_pt((at(a) + at(b)) * 0.5), kinds.get(f).map(String::as_str).unwrap_or("?"))
        })
        .collect()
}

/// The source body's named surfaces as the triangles they cover, for the
/// faces the fillet keeps.
fn source_catalogue(mesh: &crate::tessellate::Mesh, topo: &Topology, tol: f64) -> naming::SurfaceCatalogue {
    let mut cat = naming::SurfaceCatalogue::new(match_tolerance(tol));
    for f in &topo.faces {
        let tris: Vec<[Vec3; 3]> = (f.tri_start..f.tri_start + f.tri_count).filter_map(|t| mesh.triangle(t as usize)).collect();
        cat.add_face(NamedFace { name: f.name.clone(), aliases: f.aliases.clone(), role: Role::Kept }, tris);
    }
    cat.finish();
    cat
}

// ------------------------------------------------------------- the plan

/// A closed, tangent-continuous chain of runs on one planar face, re-wired
/// directly.
struct ClosedChain {
    runs: Vec<usize>,
    /// The planar face every run of the chain lies on (a topology index).
    plane: usize,
}

struct Partition {
    direct: Vec<ClosedChain>,
    /// The runs cut by chamfer, in run order.
    cut: Vec<usize>,
}

/// Which runs are re-wired directly and which are cut: a chain is direct
/// when it is closed, one planar face carries all of it, and every joint
/// in it is tangent or a corner of two straight runs.
fn partition(picked: &[Picked], runs: &[Run], topo: &Topology) -> Partition {
    let n = runs.len();
    let near = |p: Vec3, q: Vec3| (p - q).length() <= 1e-6;
    let touch = |a: &Run, b: &Run| -> Option<Vec3> {
        if a.closed || b.closed {
            return None;
        }
        [a.path.start(), a.path.end()].into_iter().find(|p| [b.path.start(), b.path.end()].iter().any(|q| near(*p, *q)))
    };
    let mut seen = vec![false; n];
    let mut direct = Vec::new();
    let mut cut = Vec::new();
    for i in 0..n {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        let mut chain = vec![i];
        let mut k = 0;
        while k < chain.len() {
            for j in 0..n {
                if !seen[j] && touch(&runs[chain[k]], &runs[j]).is_some() {
                    seen[j] = true;
                    chain.push(j);
                }
            }
            k += 1;
        }
        match closed_chain_on_one_plane(&chain, picked, runs, topo) {
            Some(plane) => direct.push(ClosedChain { runs: chain, plane }),
            None => cut.extend(chain),
        }
    }
    Partition { direct, cut }
}

/// The planar face a closed chain lies on, or `None` when the chain is
/// open, has no one plane under it, or turns a corner the direct route
/// cannot join: a joint is either tangent (a rounded outline's arc meeting
/// its straight run) or a corner of two **straight** runs (a prism cap's
/// rim), whose blends meet in an ellipse; an arc turning a corner against a
/// line is left to the cut route, which refuses it by name.
fn closed_chain_on_one_plane(chain: &[usize], picked: &[Picked], runs: &[Run], topo: &Topology) -> Option<usize> {
    let near = |p: Vec3, q: Vec3| (p - q).length() <= 1e-6;
    // The plane: a circular run says which of its faces is the plane; a
    // chain of straight runs has one planar face common to all of them.
    let plane = match chain.iter().find_map(|&k| runs[k].plane_side.map(|s| runs[k].faces[s])) {
        Some(p) => p,
        None => runs[chain[0]].faces.into_iter().find(|f| {
            matches!(topo.faces[*f].surface, SurfaceClass::Plane { .. }) && chain.iter().all(|&k| runs[k].faces.contains(f))
        })?,
    };
    if !matches!(topo.faces[plane].surface, SurfaceClass::Plane { .. }) {
        return None;
    }
    for &k in chain {
        let r = &runs[k];
        if !r.faces.contains(&plane) || r.members.iter().any(|&m| !picked[m].faces.contains(&plane)) {
            return None;
        }
    }
    // Every end of every open run meets exactly one other run, tangentially,
    // with the same setback on the plane.
    for &k in chain {
        let r = &runs[k];
        if r.closed {
            continue;
        }
        for (v, t_out) in [(r.path.start(), -r.path.tangent(0.0)), (r.path.end(), r.path.tangent(1.0))] {
            let mut found = 0;
            for &j in chain {
                if j == k {
                    continue;
                }
                let o = &runs[j];
                let leaving = if near(o.path.start(), v) {
                    o.path.tangent(0.0)
                } else if near(o.path.end(), v) {
                    -o.path.tangent(1.0)
                } else {
                    continue;
                };
                // `t_out` points away from `r` at `v` and `leaving` points
                // into `o` there; the chain passes straight through when
                // they are the same direction. Otherwise it is a corner,
                // and only two straight runs can mitre there.
                let tangent = (leaving - t_out).length() <= 1e-6;
                if !tangent && !(matches!(r.path, Path::Line { .. }) && matches!(o.path, Path::Line { .. })) {
                    return None;
                }
                let ps = |run: &Run| if run.faces[0] == plane { run.setback[0] } else { run.setback[1] };
                if (ps(r) - ps(o)).abs() > 1e-9 {
                    return None;
                }
                found += 1;
            }
            if found != 1 {
                return None;
            }
        }
    }
    Some(plane)
}

// ----------------------------------------------------- the solid, indexed

/// A solid taken apart: its faces in `face_iter` order, and who owns each
/// edge and vertex.
struct Indexed {
    faces: Vec<Face>,
    /// The faces on each edge.
    edge_faces: HashMap<EdgeID, Vec<usize>>,
    /// An absolute handle per edge id.
    edges: HashMap<EdgeID, Edge>,
    /// Edge handles in the tessellator's walk order (faces, wires, edges,
    /// each once): index `i` is `Topology::edges[i]`.
    by_order: Vec<Edge>,
    vertex_edges: HashMap<VertexID, Vec<EdgeID>>,
    vertex_faces: HashMap<VertexID, HashSet<usize>>,
}

impl Indexed {
    fn of(solid: &KSolid) -> Indexed {
        let faces: Vec<Face> = solid.face_iter().cloned().collect();
        let mut edge_faces: HashMap<EdgeID, Vec<usize>> = HashMap::new();
        let mut edges = HashMap::new();
        let mut by_order = Vec::new();
        let mut vertex_edges: HashMap<VertexID, Vec<EdgeID>> = HashMap::new();
        let mut vertex_faces: HashMap<VertexID, HashSet<usize>> = HashMap::new();
        for (i, face) in faces.iter().enumerate() {
            for wire in face.boundaries() {
                for e in wire.edge_iter() {
                    let id = e.id();
                    let fs = edge_faces.entry(id).or_default();
                    if !fs.contains(&i) {
                        fs.push(i);
                    }
                    for v in [e.absolute_front(), e.absolute_back()] {
                        vertex_faces.entry(v.id()).or_default().insert(i);
                    }
                    if edges.contains_key(&id) {
                        continue;
                    }
                    let abs = if e.orientation() { e.clone() } else { e.inverse() };
                    for v in [abs.absolute_front(), abs.absolute_back()] {
                        vertex_edges.entry(v.id()).or_default().push(id);
                    }
                    edges.insert(id, abs.clone());
                    by_order.push(abs);
                }
            }
        }
        Indexed { faces, edge_faces, edges, by_order, vertex_edges, vertex_faces }
    }

    /// The face across edge `e` from face `i`.
    fn across(&self, e: EdgeID, i: usize) -> Option<usize> {
        self.edge_faces.get(&e).and_then(|fs| fs.iter().copied().find(|&f| f != i))
    }

    /// How face `i` traverses edge `e`: `Some(true)` forwards (absolute front
    /// to back), `Some(false)` backwards, `None` when it does not carry it.
    fn traversal(&self, i: usize, e: EdgeID) -> Option<bool> {
        self.faces[i].boundaries().iter().flat_map(|w| w.edge_iter().cloned().collect::<Vec<_>>()).find(|h| h.id() == e).map(|h| h.orientation())
    }
}

/// What replaces an old edge in a face's wire.
#[derive(Clone)]
enum Subst {
    /// One edge for one, with the same absolute direction.
    Replace(Edge),
    /// This edge and the next one in the wire (they meet at a vertex that
    /// disappears) become this one edge between their outer vertices.
    Merge(Edge),
    /// A `Merge` whose arc is not built yet: a corner's arcs need every
    /// mitre's outer vertex first, so the walk records the pair here and
    /// the corner pass fills the arc in.
    Pending,
    /// The edge shrinks to a point: its two ends become one vertex, which
    /// the edges either side of it already share, and it leaves the wire.
    Remove,
}

/// The wire with its edges substituted, in the wire's own direction.
fn substitute(wire: &Wire, subst: &HashMap<EdgeID, Subst>) -> Result<Wire> {
    let handles: Vec<Edge> = wire.edge_iter().cloned().collect();
    let n = handles.len();
    let mut out: Vec<Edge> = Vec::with_capacity(n);
    let mut k = 0;
    while k < n {
        let h = &handles[k];
        match subst.get(&h.id()) {
            None => out.push(h.clone()),
            Some(Subst::Replace(new)) => out.push(if h.orientation() { new.clone() } else { new.inverse() }),
            Some(Subst::Pending) => return Err(CadpoleError::kernel("fillet: a corner's arc was never built")),
            Some(Subst::Remove) => {}
            Some(Subst::Merge(arc)) => {
                let next = &handles[(k + 1) % n];
                let pair = matches!(subst.get(&next.id()), Some(Subst::Merge(a)) if a.id() == arc.id());
                if !pair || n < 2 {
                    return Err(CadpoleError::kernel("fillet: a corner's two mitre edges are not consecutive in the face's wire"));
                }
                let from = h.front().id();
                out.push(if arc.absolute_front().id() == from { arc.clone() } else { arc.inverse() });
                k += 1;
            }
        }
        k += 1;
    }
    // A merge at the wire's seam consumed the first handle as "next".
    if let (Some(first), Some(last)) = (out.first(), out.last()) {
        if out.len() > 1 && first.id() == last.id() {
            out.pop();
        }
    }
    Ok(Wire::from(out))
}

/// A face rebuilt over substituted edges: the same surface, the same
/// orientation, its oriented wires running as they did.
fn rebuild(face: &Face, subst: &HashMap<EdgeID, Subst>) -> Result<Face> {
    let touched = face.boundaries().iter().any(|w| w.edge_iter().any(|e| subst.contains_key(&e.id())));
    if !touched {
        return Ok(face.clone());
    }
    let oriented: Vec<Wire> = face.boundaries().iter().map(|w| substitute(w, subst)).collect::<Result<_>>()?;
    face_oriented(oriented, face.surface(), face.orientation())
}

/// A face whose *oriented* boundaries are `oriented`: with `forward` the
/// surface's own normal is the face's, otherwise the face is inverted and
/// its stored wires are the oriented ones reversed, which is how truck
/// reports an inverted face's boundaries.
fn face_oriented(oriented: Vec<Wire>, surface: Surface, forward: bool) -> Result<Face> {
    let wires = if forward { oriented } else { oriented.iter().map(Wire::inverse).collect() };
    let mut f = Face::try_new(wires, surface).map_err(|e| CadpoleError::kernel(format!("fillet: a rebuilt face is not simple ({e})")))?;
    if !forward {
        f.invert();
    }
    Ok(f)
}

/// The solid from `faces`, checked closed and manifold.
fn solid_of(faces: Vec<Face>, what: &str) -> Result<KSolid> {
    let mut shell = Shell::new();
    shell.extend(faces);
    if shell.shell_condition() != ShellCondition::Closed {
        return Err(CadpoleError::kernel(format!(
            "fillet: after {what} the shell is {:?}, not closed: an edge was rebuilt for one of its faces and not the \
             other",
            shell.shell_condition()
        )));
    }
    Solid::try_new(vec![shell]).map_err(|e| CadpoleError::kernel(format!("fillet: after {what} the shell is not a solid ({e})")))
}

// -------------------------------------------------------------- geometry

fn hom(p: Vec3, w: f64) -> Vector4 {
    Vector4::new(p.x * w, p.y * w, p.z * w, w)
}

fn euclid(p: &Vector4) -> Vec3 {
    Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w)
}

/// The angle from `from` to `to` counter-clockwise about the axis through
/// `centre`, in `(0, 2π]`.
fn ccw_angle(from: Vec3, to: Vec3, centre: Vec3, axis: Vec3) -> f64 {
    let radial = |p: Vec3| {
        let v = p - centre;
        (v - axis * v.dot(axis)).normalized()
    };
    let (u, v) = (radial(from), radial(to));
    let t = fmath::atan2(u.cross(v).dot(axis), u.dot(v));
    if t <= 1e-12 { t + TAU } else { t }
}

/// A rational quadratic arc from `p0` through `angle` radians
/// counter-clockwise about the axis through `centre`, as spans of at most a
/// quarter turn (Piegl & Tiller): the homogeneous control points and the
/// knots, with the interior knots doubled.
fn arc_control(p0: Vec3, centre: Vec3, axis: Vec3, angle: f64) -> (Vec<Vector4>, KnotVec) {
    let k = ((angle / std::f64::consts::FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize;
    let delta = angle / k as f64;
    let (_, w) = fmath::sin_cos(delta / 2.0);
    let foot = centre + axis * (p0 - centre).dot(axis);
    let mut pts = Vec::with_capacity(2 * k + 1);
    let mut knots = vec![0.0, 0.0, 0.0];
    for i in 0..k {
        let a0 = delta * i as f64;
        pts.push(hom(rotate_about(p0, centre, axis, a0), 1.0));
        let mid = rotate_about(p0, centre, axis, a0 + delta / 2.0);
        pts.push(hom(foot + (mid - foot) * (1.0 / w), w));
        if i + 1 < k {
            let t = (i + 1) as f64 / k as f64;
            knots.push(t);
            knots.push(t);
        }
    }
    pts.push(hom(rotate_about(p0, centre, axis, angle), 1.0));
    knots.extend([1.0, 1.0, 1.0]);
    (pts, KnotVec::from(knots))
}

fn nurbs_curve(pts: Vec<Vector4>, knots: KnotVec) -> Curve {
    Curve::NurbsCurve(NurbsCurve::new(BSplineCurve::new(knots, pts)))
}

/// The circular arc edge from `p0` to `p2` about `axis` through `centre`,
/// running the way round that passes nearest `via`; its ends are exactly the
/// vertices' points.
fn circle_arc_edge(v0: &Vertex, v2: &Vertex, centre: Vec3, axis: Vec3, via: Vec3) -> Edge {
    let (p0, p2) = (kernel::point_to_vec3(v0.point()), kernel::point_to_vec3(v2.point()));
    let ccw = ccw_angle(p0, p2, centre, axis);
    let to_via = ccw_angle(p0, via, centre, axis);
    let (axis, angle) = if to_via < ccw { (axis, ccw) } else { (-axis, TAU - ccw) };
    let (mut pts, knots) = arc_control(p0, centre, axis, angle);
    *pts.last_mut().expect("an arc has control points") = hom(p2, 1.0);
    Edge::new(v0, v2, nurbs_curve(pts, knots))
}

/// The homogeneous profile `pts` over `vknots` swung through `angle` about
/// the axis through `centre`, as the exact NURBS surface of revolution: the
/// turn along `u` in spans of at most a quarter turn, the profile along `v`
/// — the loft's and the sweep's layout, which the tessellator and
/// `SurfaceClass` read as a surface of revolution.
fn revolve_profile(pts: &[Vector4], vknots: KnotVec, centre: Vec3, axis: Vec3, angle: f64) -> Result<Surface> {
    let k = ((angle / std::f64::consts::FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize;
    let delta = angle / k as f64;
    let (_, w_mid) = fmath::sin_cos(delta / 2.0);
    let mut net: Vec<Vec<Vector4>> = vec![Vec::with_capacity(pts.len()); 2 * k + 1];
    for p in pts {
        let w = p.w;
        let e = euclid(p);
        let d = e - centre;
        let foot = centre + axis * d.dot(axis);
        let on_axis = (d - axis * d.dot(axis)).length() < 1e-12;
        for i in 0..k {
            let a0 = delta * i as f64;
            net[2 * i].push(hom(rotate_about(e, centre, axis, a0), w));
            let mid = if on_axis { e } else { foot + (rotate_about(e, centre, axis, a0 + delta / 2.0) - foot) * (1.0 / w_mid) };
            net[2 * i + 1].push(hom(mid, w * w_mid));
        }
        net[2 * k].push(hom(rotate_about(e, centre, axis, angle), w));
    }
    let mut uknots = vec![0.0, 0.0, 0.0];
    for i in 1..k {
        let t = i as f64 / k as f64;
        uknots.push(t);
        uknots.push(t);
    }
    uknots.extend([1.0, 1.0, 1.0]);
    let bsp = BSplineSurface::try_new((KnotVec::from(uknots), vknots), net)
        .map_err(|e| CadpoleError::kernel(format!("fillet: the torus patch: {e}")))?;
    Ok(Surface::NurbsSurface(NurbsSurface::new(bsp)))
}

/// The homogeneous profile `pts` over `uknots` extruded along `along`: the
/// profile along `u`, the extrusion along `v`.
fn extrude_profile(pts: &[Vector4], uknots: KnotVec, along: Vec3) -> Result<Surface> {
    let net: Vec<Vec<Vector4>> = pts.iter().map(|p| vec![*p, *p + Vector4::new(along.x * p.w, along.y * p.w, along.z * p.w, 0.0)]).collect();
    let bsp = BSplineSurface::try_new((uknots, KnotVec::bezier_knot(1)), net)
        .map_err(|e| CadpoleError::kernel(format!("fillet: the cylinder patch: {e}")))?;
    Ok(Surface::NurbsSurface(NurbsSurface::new(bsp)))
}

/// The surface's point and normal at the fractions `(fu, fv)` of its range.
fn surface_at(s: &Surface, fu: f64, fv: f64) -> (Vec3, Vec3) {
    use truck_modeling::ParametricSurface;
    let (ur, vr) = s.try_range_tuple();
    let ((u0, u1), (v0, v1)) = (ur.unwrap_or((0.0, 1.0)), vr.unwrap_or((0.0, 1.0)));
    let (u, v) = (u0 + (u1 - u0) * fu, v0 + (v1 - v0) * fv);
    (kernel::point_to_vec3(s.subs(u, v)), kernel::vector_to_vec3(s.normal(u, v)).normalized())
}

/// The section at a station of a run: the edge point, the two contact points
/// (side 0 along `frame.a`, side 1 along `frame.b`), the rolling ball's
/// centre and the frame there.
struct SectionAt {
    e: Vec3,
    p: [Vec3; 2],
    ball: Vec3,
    a: Vec3,
    b: Vec3,
    t: Vec3,
    /// The arc's angle, `π − γ`.
    arc: f64,
}

fn section_at(run: &Run, radius: f64, f: f64) -> SectionAt {
    let e = run.path.at(f);
    let a = run.path.carry(run.frame.a, 0.0, f);
    let b = run.path.carry(run.frame.b, 0.0, f);
    let t = run.path.tangent(f);
    let wedge = run.frame.wedge();
    let (s, _) = fmath::sin_cos(wedge / 2.0);
    let ball = e + (a + b).normalized() * (radius / s);
    SectionAt { e, p: [e + a * run.setback[0], e + b * run.setback[1]], ball, a, b, t, arc: std::f64::consts::PI - wedge }
}

impl SectionAt {
    /// The section arc from the side-0 contact point to the side-1 one, as
    /// homogeneous control points and knots.
    fn arc_control(&self) -> (Vec<Vector4>, KnotVec) {
        let n = (self.p[0] - self.ball).cross(self.p[1] - self.ball).normalized();
        let (mut pts, knots) = arc_control(self.p[0], self.ball, n, self.arc);
        *pts.last_mut().expect("an arc has control points") = hom(self.p[1], 1.0);
        (pts, knots)
    }
}

/// The station of `q` along `run`, as the fraction of its path: the
/// projection onto a line, the counter-clockwise angle about a circle's axis
/// (in `[0, 2π)` for a closed run, `(−π, π]` for an open one).
fn station(run: &Run, q: Vec3) -> f64 {
    match run.path {
        Path::Line { start, end } => {
            let d = end - start;
            (q - start).dot(d) / d.dot(d)
        }
        Path::Arc { centre, axis, start, sweep, .. } => {
            let radial = |p: Vec3| {
                let v = p - centre;
                (v - axis * v.dot(axis)).normalized()
            };
            let (u, v) = (radial(start), radial(q));
            let mut t = fmath::atan2(u.cross(v).dot(axis), u.dot(v));
            if run.closed && t < -1e-9 {
                t += TAU;
            }
            t / sweep
        }
    }
}

/// The outward normal of the blend on `run` at the point `x` of it.
fn outward_at(run: &Run, radius: f64, x: Vec3) -> Vec3 {
    let s = section_at(run, radius, station(run, x));
    let n = (x - s.ball).normalized();
    if run.convex { n } else { -n }
}

/// The blend patch of `run` between the stations `f0 < f1` and its class:
/// a cylinder along a straight run, a torus round a circular one.
fn blend_surface(run: &Run, radius: f64, f0: f64, f1: f64) -> Result<(Surface, SurfaceClass)> {
    let s = section_at(run, radius, f0);
    let (pts, knots) = s.arc_control();
    match run.path {
        Path::Line { start, end } => {
            let along = (end - start) * (f1 - f0);
            let surface = extrude_profile(&pts, knots, along)?;
            Ok((surface, SurfaceClass::Cylinder { axis_point: s.ball, axis: s.t, radius }))
        }
        Path::Arc { centre, axis, sweep, .. } => {
            let surface = revolve_profile(&pts, knots, centre, axis, sweep * (f1 - f0))?;
            let d = s.ball - centre;
            let major = (d - axis * d.dot(axis)).length();
            let class = if major <= 1e-9 {
                SurfaceClass::Sphere { center: centre + axis * d.dot(axis), radius }
            } else {
                SurfaceClass::Torus { center: centre + axis * d.dot(axis), axis, major, minor: radius }
            };
            Ok((surface, class))
        }
    }
}

/// A blend face over `oriented` (its oriented wire, running as the shell
/// needs it) on `surface`, facing outward.
fn blend_face(run: &Run, radius: f64, oriented: Wire, surface: Surface) -> Result<Face> {
    let (x, n) = surface_at(&surface, 0.5, 0.5);
    let forward = n.dot(outward_at(run, radius, x)) > 0.0;
    face_oriented(vec![oriented], surface, forward)
}

// ------------------------------------------------- the direct construction

/// One vertex of a chain's rim, split in two: `w` on the plane and `u` on
/// the wall, joined by the section arc; the straight seam up the wall is
/// trimmed back to `u` once every chain that claims it has spoken.
struct RimVertex {
    vertex: Vertex,
    run: usize,
    w: Vertex,
    u: Vertex,
    section: Edge,
}

/// Where a seam is cut back: a new vertex at its front, at its back, or at
/// both when two chains (a hole's mouth and its floor) share the wall. Each
/// end remembers the edge whose blend claimed it, for the refusal.
#[derive(Default)]
struct SeamTrim {
    front: Option<(Vertex, String)>,
    back: Option<(Vertex, String)>,
}

/// Re-wire every closed, tangent-continuous chain in one pass over the
/// source solid: no boolean, one vertex split per seam. One pass, and not
/// one chain after another, because the runs index the source's edges
/// (`Topology::edges[i]` is `Indexed::by_order[i]`, and a re-wired solid
/// walks differently) and because two chains may trim the same seam from
/// its two ends.
#[allow(clippy::too_many_arguments)]
fn build_closed_chains(
    solid: &KSolid,
    topo: &Topology,
    probe: &Probe<'_>,
    picked: &[Picked],
    runs: &[Run],
    chains: &[ClosedChain],
    radius: f64,
    mesh_tol: f64,
    fid: &FeatureId,
    blends: &mut Vec<(SurfaceClass, String)>,
) -> Result<KSolid> {
    let ix = Indexed::of(solid);
    let name_of_run = |k: usize| picked[runs[k].members[0]].name.as_str();
    // Every face's substitutions and every seam's trims, gathered over all
    // the chains before anything is rebuilt.
    let mut subst: Vec<HashMap<EdgeID, Subst>> = vec![HashMap::new(); ix.faces.len()];
    let mut seam_trims: HashMap<EdgeID, SeamTrim> = HashMap::new();
    let mut blend_faces: Vec<Face> = Vec::new();

    for chain in chains {
        let plane = chain.plane;
        let plane_name = &topo.faces[plane].name;
        let plane_side = |run: &Run| if run.faces[0] == plane { 0 } else { 1 };

        // The rim: every member edge of every run, as the solid's own edges.
        struct RimEdge {
            run: usize,
            edge: Edge,
        }
        let mut rim: Vec<RimEdge> = Vec::new();
        let mut rim_ids: HashSet<EdgeID> = HashSet::new();
        for &k in &chain.runs {
            for &m in &runs[k].members {
                let edge = ix
                    .by_order
                    .get(picked[m].index)
                    .cloned()
                    .ok_or_else(|| CadpoleError::kernel("fillet: the solid has fewer edges than its topology"))?;
                rim_ids.insert(edge.id());
                rim.push(RimEdge { run: k, edge });
            }
        }
        let run_of_edge: HashMap<EdgeID, usize> = rim.iter().map(|r| (r.edge.id(), r.run)).collect();

        // The vertices: two rim edges and one straight seam up the wall each.
        let mut seams: HashSet<EdgeID> = HashSet::new();
        let mut vertices: HashMap<VertexID, RimVertex> = HashMap::new();
        for r in &rim {
            for v in [r.edge.absolute_front(), r.edge.absolute_back()] {
                if vertices.contains_key(&v.id()) {
                    continue;
                }
                let p = kernel::point_to_vec3(v.point());
                let incident = ix.vertex_edges.get(&v.id()).cloned().unwrap_or_default();
                let on_rim: Vec<EdgeID> = incident.iter().copied().filter(|e| rim_ids.contains(e)).collect();
                let others: Vec<EdgeID> = incident.iter().copied().filter(|e| !rim_ids.contains(e)).collect();
                let edge_name = name_of_run(r.run);
                if on_rim.len() != 2 || others.len() != 1 {
                    return Err(CadpoleError::invalid(format!(
                        "fillet: edge `{edge_name}` meets {} other edge{} at {}, and a blend round a closed loop needs \
                         exactly one — the wall's seam — at every vertex of the loop, so the wall can be trimmed back \
                         to the contact curve there. Fillet the edges that meet there in one call, {FIELD}",
                        others.len(),
                        if others.len() == 1 { "" } else { "s" },
                        fmt_pt(p)
                    )));
                }
                let seam = ix.edges[&others[0]].clone();
                if !is_straight(&seam.curve()) {
                    return Err(CadpoleError::invalid(format!(
                        "fillet: the wall's seam at {} on edge `{edge_name}` is a {} curve, not a straight line, and \
                         the exact path trims only straight seams back to the contact curve (a curved seam is a wall \
                         that is not a cylinder or a cone along it). Build the round into the profile, {FIELD}",
                        fmt_pt(p),
                        kernel::curve_kind(&seam.curve())
                    )));
                }
                // Both faces of the seam are on the wall of a run that ends here.
                let runs_here: Vec<usize> = on_rim.iter().map(|e| run_of_edge[e]).collect();
                for &f in &ix.edge_faces[&seam.id()] {
                    let on_wall = runs_here.iter().any(|&k| {
                        let wall = runs[k].faces[1 - plane_side(&runs[k])];
                        same_surface(&topo.faces[f].surface, &topo.faces[wall].surface)
                    });
                    if f == plane || !on_wall {
                        return Err(CadpoleError::invalid(format!(
                            "fillet: the seam at {} on edge `{edge_name}` lies on `{}`, which is not the wall the blend \
                             runs along, so trimming it would open that face. Fillet the closed loop that face belongs \
                             to, {FIELD}",
                            fmt_pt(p),
                            topo.faces[f].name
                        )));
                    }
                }
                seams.insert(seam.id());
                // The contact points here, from the two loop edges' runs. At
                // a tangent joint (or where one run's two members meet) they
                // agree, and the two blends share one section arc. At a
                // corner of two straight runs the wall points must still
                // agree — the seam is where both blends meet the wall — and
                // the plane point is where the two contact lines cross; the
                // joint is then the blends' ellipse.
                let (k0, k1) = (runs_here[0], runs_here[1]);
                let (s0, s1) = (section_at(&runs[k0], radius, station(&runs[k0], p)), section_at(&runs[k1], radius, station(&runs[k1], p)));
                let (ps0, ps1) = (plane_side(&runs[k0]), plane_side(&runs[k1]));
                let (w0, u0, w1, u1) = (s0.p[ps0], s0.p[1 - ps0], s1.p[ps1], s1.p[1 - ps1]);
                let tangent = k0 == k1 || ((w0 - w1).length() <= 1e-6 && (u0 - u1).length() <= 1e-6);
                let corner = !tangent;
                if corner && !(matches!(runs[k0].path, Path::Line { .. }) && matches!(runs[k1].path, Path::Line { .. })) {
                    let (arc, line) = if matches!(runs[k0].path, Path::Arc { .. }) { (k0, k1) } else { (k1, k0) };
                    return Err(CadpoleError::invalid(format!(
                        "fillet: `{}` (an arc) and `{}` (straight) meet at a corner at {}: their blends, a torus and a \
                         cylinder, would mitre along a curve the exact path does not build. Round that corner in the \
                         sketch so the two join tangentially, {FIELD}",
                        name_of_run(arc),
                        name_of_run(line),
                        fmt_pt(p)
                    )));
                }
                if corner && (u0 - u1).length() > 1e-6 {
                    return Err(CadpoleError::invalid(format!(
                        "fillet: the blends along `{}` and `{}` meet at {} but do not reach the wall's seam at one \
                         point ({} against {}): the two edges meet their walls at different angles, so the corner \
                         needs a vertex blend the exact path does not build. Fillet one of \
                         the two, {FIELD}",
                        name_of_run(k0),
                        name_of_run(k1),
                        fmt_pt(p),
                        fmt_pt(u0),
                        fmt_pt(u1)
                    )));
                }
                let (s, k) = (s0, k0);
                let ps = ps0;
                let up = u0;
                let wp = if corner {
                    // Where run 0's contact line on the plane meets run 1's:
                    // along run 0's direction from its own contact point.
                    let t0 = runs[k0].path.tangent(0.0);
                    let a1 = if ps1 == 0 { s1.a } else { s1.b };
                    let denom = t0.dot(a1);
                    if denom.abs() < 1e-9 {
                        return Err(CadpoleError::kernel(format!("fillet: the contact lines at {} are parallel", fmt_pt(p))));
                    }
                    w0 + t0 * ((w1 - w0).dot(a1) / denom)
                } else {
                    w0
                };
                // The seam must reach past the wall's contact curve, with a
                // chord to spare so the trimmed seam is a real edge.
                let far = if seam.absolute_front().id() == v.id() { seam.absolute_back() } else { seam.absolute_front() };
                let far_p = kernel::point_to_vec3(far.point());
                let (wall_len, need) = ((far_p - p).length(), (up - p).length());
                if wall_len <= need + mesh_tol {
                    return Err(CadpoleError::invalid(format!(
                        "fillet: a radius of {} at edge `{edge_name}` needs {} mm of wall below the edge at {}, and the \
                         wall there is only {:.2} mm; the largest radius that fits is about {:.2} mm. {FIELD}",
                        fmt_num(radius),
                        fmt_num(need),
                        fmt_pt(p),
                        wall_len,
                        radius * (wall_len - mesh_tol).max(0.0) / need
                    )));
                }
                let w = Vertex::new(wp.point());
                let u = Vertex::new(up.point());
                let trim = seam_trims.entry(seam.id()).or_default();
                let slot = if seam.absolute_front().id() == v.id() { &mut trim.front } else { &mut trim.back };
                *slot = Some((u.clone(), edge_name.to_owned()));
                // The joint from the wall to the plane: the section arc, or
                // at a corner the ellipse in which the two blends meet.
                let section = if corner {
                    mitre_edge(&runs[k0], &runs[k1], radius, &u, &w, name_of_run(k0), name_of_run(k1))?
                } else {
                    let (mut arc_pts, knots) = s.arc_control();
                    if ps == 0 {
                        arc_pts.reverse();
                    }
                    Edge::new(&u, &w, nurbs_curve(arc_pts, knots))
                };
                vertices.insert(v.id(), RimVertex { vertex: v.clone(), run: k, w, u, section });
            }
        }

        // A rim arc whose contact circle on the plane has shrunk to a point:
        // a rim blended at the radius its outline's corners were rounded to,
        // so the ball rolls on the corner's axis and the corner blend is a
        // sphere patch with its pole on the plane. The two contact points
        // at the arc's ends are then one point, and they are made one
        // vertex: the plane's contact edge for the arc would be an edge of
        // no length, which the plane's loop and the blend's both carried and
        // the tessellator could not place (the pole has every `u`). The
        // plane's loop goes straight from one neighbour's contact edge to
        // the other's, and the corner blend is bounded by its two section
        // arcs and its wall edge, as a ball's octant is.
        let mut collapsed: HashSet<EdgeID> = HashSet::new();
        for r in &rim {
            let run = &runs[r.run];
            let SetbackCurve::Circle { radius: rc, .. } = setback_curves(run)[plane_side(run)] else { continue };
            if rc > POLE_TOL {
                continue;
            }
            let (v0, v1) = (r.edge.absolute_front().id(), r.edge.absolute_back().id());
            if v0 == v1 {
                continue;
            }
            let keep = vertices[&v0].w.clone();
            let other = vertices.get_mut(&v1).expect("every rim vertex was visited");
            let (from, to) = (other.section.absolute_front().clone(), other.section.absolute_back().clone());
            let curve = other.section.curve();
            other.section = if to.id() == other.w.id() {
                Edge::new(&from, &keep, curve)
            } else {
                Edge::new(&keep, &to, curve)
            };
            other.w = keep;
            collapsed.insert(r.edge.id());
        }
        // Every arc of the rim collapsed: the face it rounds has no boundary
        // left, it has become the top of a dome. That is a sphere or a
        // revolve, not a blend between two faces.
        if !rim.is_empty() && rim.iter().all(|r| collapsed.contains(&r.edge.id())) {
            return Err(CadpoleError::invalid(format!(
                "fillet: a radius of {} at `{}` equals its outline's radius, so the face it rounds vanishes \
                 into a dome; use a smaller radius, a sphere or a revolve, or kernel=\"field\"",
                crate::context::fmt_num(radius),
                name_of_run(rim[0].run)
            )));
        }

        let d_plane = |k: usize| runs[k].setback[plane_side(&runs[k])];
        let d_wall = |k: usize| runs[k].setback[1 - plane_side(&runs[k])];

        // The footprint: no other edge of the solid may enter the band the
        // blend takes off the plane and off the wall. Every polyline
        // segment is sampled, not only its ends — a straight edge is two
        // points, and it is the plate's own rim that a hole's mouth reaches.
        let eps = mesh_tol + 1e-6;
        let hit = |k: usize, q: Vec3, scale: f64| -> bool {
            let run = &runs[k];
            let f = station(run, q);
            let f = match run.path {
                Path::Line { .. } => {
                    if !(-1e-6..=1.0 + 1e-6).contains(&f) {
                        return false;
                    }
                    f.clamp(0.0, 1.0)
                }
                Path::Arc { .. } => {
                    if !run.closed && !(-1e-6..=1.0 + 1e-6).contains(&f) {
                        return false;
                    }
                    f
                }
            };
            let s = section_at(run, radius, f);
            let ps = plane_side(run);
            let (na, nb) = (run.path.carry(run.frame.na, 0.0, f), run.path.carry(run.frame.nb, 0.0, f));
            let (dir_p, n_p, dir_w, n_w) = if ps == 0 { (s.a, na, s.b, nb) } else { (s.b, nb, s.a, na) };
            let v = q - s.e;
            let on_plane = v.dot(n_p).abs() <= eps && (-eps..=d_plane(k) * scale + eps).contains(&v.dot(dir_p));
            let on_wall = v.dot(n_w).abs() <= eps && (-eps..=d_wall(k) * scale + eps).contains(&v.dot(dir_w));
            on_plane || on_wall
        };
        // How far `q` is from the run's line or circle: a segment whose ends
        // are both farther than the band reaches, by more than its own
        // length, cannot enter it.
        let path_dist = |k: usize, q: Vec3| -> f64 {
            match runs[k].path {
                Path::Line { start, end } => {
                    let d = end - start;
                    let t = ((q - start).dot(d) / d.dot(d)).clamp(0.0, 1.0);
                    (q - (start + d * t)).length()
                }
                Path::Arc { centre, axis, radius: rr, .. } => {
                    let v = q - centre;
                    let ax = v.dot(axis);
                    let rad = (v - axis * ax).length();
                    (ax * ax + (rad - rr) * (rad - rr)).sqrt()
                }
            }
        };
        let enters = |k: usize, polyline: &[Vec3], scale: f64| -> bool {
            let reach = d_plane(k).max(d_wall(k)) + eps;
            let step = (d_plane(k).min(d_wall(k)) * scale / 4.0).max(1e-3);
            if polyline.len() == 1 {
                return hit(k, polyline[0], scale);
            }
            polyline.windows(2).any(|s| {
                let (q0, q1) = (s[0], s[1]);
                let len = (q1 - q0).length();
                if path_dist(k, q0).min(path_dist(k, q1)) - len > reach {
                    return false;
                }
                let n = ((len / step).ceil() as usize).clamp(1, 512);
                (0..=n).any(|j| hit(k, q0 + (q1 - q0) * (j as f64 / n as f64), scale))
            })
        };
        for (i, e) in topo.edges.iter().enumerate() {
            let handle = match ix.by_order.get(i) {
                Some(h) => h,
                None => continue,
            };
            if rim_ids.contains(&handle.id()) || seams.contains(&handle.id()) {
                continue;
            }
            for &k in &chain.runs {
                if enters(k, &e.polyline, 1.0) {
                    // The largest radius that keeps clear, by bisection on the scale.
                    let (mut lo, mut hi) = (0.0f64, 1.0f64);
                    for _ in 0..30 {
                        let mid = (lo + hi) / 2.0;
                        if enters(k, &e.polyline, mid) {
                            hi = mid
                        } else {
                            lo = mid
                        }
                    }
                    return Err(CadpoleError::invalid(format!(
                        "fillet: a radius of {} at edge `{}` would reach `{}`: the blend takes {} mm off `{plane_name}` \
                         and {} mm off the wall, and that edge lies in the band. The largest radius that keeps clear of \
                         it is about {:.2} mm; move the feature, {FIELD}",
                        fmt_num(radius),
                        name_of_run(k),
                        e.name,
                        fmt_num(d_plane(k)),
                        fmt_num(d_wall(k)),
                        radius * lo
                    )));
                }
            }
        }

        // The contact curve on the plane must stay on the plane's face: a
        // radius wider than the face would put the blend's far edge past
        // the face's own far edge (the cut route's planner checks the same
        // for a cutter). After the footprint, because a contact curve that
        // leaves the face crosses one of its edges, which the footprint
        // names; what is left for this check is the chain's own opposite
        // rim, excluded there.
        for &k in &chain.runs {
            let run = &runs[k];
            let ps = plane_side(run);
            let stations: &[f64] = if run.closed { &[0.0, 0.25, 0.5, 0.75] } else { &[0.0, 0.25, 0.5, 0.75, 1.0] };
            let on_plane = |scale: f64| {
                stations.iter().all(|&f| {
                    let s = section_at(run, radius, f);
                    let dir = if ps == 0 { s.a } else { s.b };
                    probe.distance_to_face(plane, s.e + dir * (d_plane(k) * scale)) <= 1e-6
                })
            };
            if !on_plane(1.0) {
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                for _ in 0..30 {
                    let mid = (lo + hi) / 2.0;
                    if on_plane(mid) {
                        lo = mid
                    } else {
                        hi = mid
                    }
                }
                return Err(CadpoleError::invalid(format!(
                    "fillet: {} mm along `{plane_name}` at edge `{}` is wider than the face: the contact curve leaves \
                     it, so the blend would end on its far edge. The largest radius that fits there is about {:.2} mm",
                    fmt_num(d_plane(k)),
                    name_of_run(k),
                    radius * lo
                )));
            }
        }

        // ---- the rim's new edges: the plane's contact curve on the plane,
        // the wall's on every face across the rim from it.
        struct Moved {
            /// `None` where the contact arc on the plane is a point.
            plane: Option<Edge>,
            wall: Edge,
        }
        let mut moved: HashMap<EdgeID, Moved> = HashMap::new();
        for r in &rim {
            let run = &runs[r.run];
            let ps = plane_side(run);
            let (v0, v1) = (r.edge.absolute_front(), r.edge.absolute_back());
            let (rv0, rv1) = (&vertices[&v0.id()], &vertices[&v1.id()]);
            let mid = mid_of(&r.edge);
            let curves = setback_curves(run);
            let make = |a: &Vertex, b: &Vertex, side: usize| -> Edge {
                match &curves[side] {
                    SetbackCurve::Line { .. } => Edge::new(a, b, Curve::Line(Line(a.point(), b.point()))),
                    SetbackCurve::Circle { centre, axis, .. } => {
                        // The way round the old edge went, read at the new radius.
                        let Path::Arc { centre: c0, axis: ax, .. } = run.path else { unreachable!() };
                        let via_angle = ccw_angle(kernel::point_to_vec3(v0.point()), mid, c0, ax);
                        let via = rotate_about(kernel::point_to_vec3(a.point()), *centre, ax, via_angle);
                        circle_arc_edge(a, b, *centre, *axis, via)
                    }
                }
            };
            let plane_e = (!collapsed.contains(&r.edge.id())).then(|| make(&rv0.w, &rv1.w, ps));
            let wall_e = make(&rv0.u, &rv1.u, 1 - ps);
            for &f in &ix.edge_faces[&r.edge.id()] {
                let new = if f != plane {
                    Subst::Replace(wall_e.clone())
                } else {
                    match &plane_e {
                        Some(e) => Subst::Replace(e.clone()),
                        None => Subst::Remove,
                    }
                };
                subst[f].insert(r.edge.id(), new);
            }
            moved.insert(r.edge.id(), Moved { plane: plane_e, wall: wall_e });
        }

        // ---- one blend face per rim edge
        for r in &rim {
            let run = &runs[r.run];
            let (v0, v1) = (r.edge.absolute_front(), r.edge.absolute_back());
            let (rv0, rv1) = (&vertices[&v0.id()], &vertices[&v1.id()]);
            let m = &moved[&r.edge.id()];
            // The plane traverses the old edge one way; the blend takes the
            // plane edge the other way, the section arc, the wall edge, the
            // other section arc.
            let plane_forward =
                ix.traversal(plane, r.edge.id()).ok_or_else(|| CadpoleError::kernel("fillet: the plane lost its rim edge"))?;
            let (x, y) = if plane_forward { (rv0, rv1) } else { (rv1, rv0) };
            let plane_edge = m.plane.as_ref().map(|p| if plane_forward { p.inverse() } else { p.clone() });
            let wall_edge = if plane_forward { m.wall.clone() } else { m.wall.inverse() };
            let wire: Wire = match plane_edge {
                None => vec![x.section.inverse(), wall_edge, y.section.clone()].into(),
                Some(plane_edge) => vec![plane_edge, x.section.inverse(), wall_edge, y.section.clone()].into(),
            };
            let (f0, f1) = (station(run, kernel::point_to_vec3(v0.point())), station(run, kernel::point_to_vec3(v1.point())));
            let (f0, f1) = arc_span(run, f0, f1, mid_of(&r.edge));
            let (surface, class) = blend_surface(run, radius, f0, f1)?;
            blend_faces.push(blend_face(run, radius, wire, surface)?);
            blends.push((class, blend_name(fid, name_of_run(r.run))));
            let _ = (x.run, y.run, &x.vertex, &y.vertex);
        }
    }

    // ---- the seams, each cut back once at every end a chain claimed
    for (id, trim) in &seam_trims {
        let seam = &ix.edges[id];
        let (of, ot) = (kernel::point_to_vec3(seam.absolute_front().point()), kernel::point_to_vec3(seam.absolute_back().point()));
        let from = trim.front.as_ref().map(|(u, _)| u.clone()).unwrap_or_else(|| seam.absolute_front().clone());
        let to = trim.back.as_ref().map(|(u, _)| u.clone()).unwrap_or_else(|| seam.absolute_back().clone());
        let (pf, pt) = (kernel::point_to_vec3(from.point()), kernel::point_to_vec3(to.point()));
        if let (Some((_, a)), Some((_, b))) = (&trim.front, &trim.back) {
            // Two blends share this wall: their contact curves must leave
            // some of it between them.
            let wall_len = (ot - of).length();
            let taken = (pf - of).length() + (pt - ot).length();
            if (pt - pf).dot(ot - of) <= mesh_tol * wall_len {
                return Err(CadpoleError::invalid(format!(
                    "fillet: the blends along `{a}` and `{b}` both take the wall between them, which is {:.2} mm long at \
                     {}, and a radius of {} needs {:.2} mm of it. The largest radius that leaves wall between them is \
                     about {:.2} mm; {FIELD}",
                    wall_len,
                    fmt_pt(of),
                    fmt_num(radius),
                    taken,
                    radius * (wall_len - mesh_tol).max(0.0) / taken
                )));
            }
        }
        let new = Edge::new(&from, &to, Curve::Line(Line(from.point(), to.point())));
        for &f in &ix.edge_faces[id] {
            subst[f].insert(*id, Subst::Replace(new.clone()));
        }
    }

    // ---- rebuild
    let mut faces: Vec<Face> = Vec::with_capacity(ix.faces.len() + blend_faces.len());
    for (i, face) in ix.faces.iter().enumerate() {
        faces.push(rebuild(face, &subst[i])?);
    }
    faces.extend(blend_faces);
    solid_of(faces, "re-wiring the closed loops")
}

/// A seam that can be trimmed to a point along it: a `Line`, or a spline
/// that is one — a loft's ruling between two circles is a degree-1
/// B-spline, and a boolean's line an `IntersectionCurve` — measured by
/// sampling against the chord between its ends.
fn is_straight(c: &Curve) -> bool {
    if matches!(c, Curve::Line(_)) {
        return true;
    }
    let (t0, t1) = c.range_tuple();
    let (a, b) = (kernel::point_to_vec3(c.subs(t0)), kernel::point_to_vec3(c.subs(t1)));
    let d = b - a;
    let len = d.length();
    if !(len > 1e-9) {
        return false;
    }
    let dir = d * (1.0 / len);
    (1..8).all(|k| {
        let p = kernel::point_to_vec3(c.subs(t0 + (t1 - t0) * k as f64 / 8.0));
        let v = p - a;
        (v - dir * v.dot(dir)).length() <= 1e-7 * len.max(1.0)
    })
}

fn mid_of(e: &Edge) -> Vec3 {
    let c = e.curve();
    let (t0, t1) = c.range_tuple();
    kernel::point_to_vec3(c.subs((t0 + t1) / 2.0))
}

/// The parametric span `(f0, f1)` of a member edge on its run, ordered and,
/// on a closed circle, wrapped so the member from the last joint back to the
/// start reads as `(f, 1)`; `mid` is a point on the edge.
fn arc_span(run: &Run, f0: f64, f1: f64, mid: Vec3) -> (f64, f64) {
    match run.path {
        Path::Line { .. } => (f0.min(f1), f0.max(f1)),
        Path::Arc { .. } => {
            let fm = station(run, mid);
            let (mut lo, hi) = (f0.min(f1), f0.max(f1));
            if run.closed {
                // A closed run's start is at one joint: an edge ending there
                // has station 0 for its far end, which reads as 1.
                if (hi - lo) > 0.5 && !(lo < fm && fm < hi) {
                    if lo < 1e-9 {
                        lo = 1.0;
                    }
                    let (a, b) = (lo.min(hi), lo.max(hi));
                    return (a, b);
                }
                if lo < 1e-9 && fm > hi {
                    return (hi, 1.0);
                }
            }
            (lo, hi)
        }
    }
}

// ------------------------------------------------------ the cut route

/// The chamfer's booleans with the fillet's setback: one union of cutters
/// and one boolean per chain, on the exact kernel only; no naming, since the
/// result is re-wired and named afresh.
fn cut_by_chamfer(
    ctx: &mut Context,
    source: KSolid,
    chains: &[Vec<usize>],
    runs: &[Run],
    picked: &[Picked],
    fid: &FeatureId,
    span: Option<Span>,
) -> Result<KSolid> {
    let tol = ctx.boolean_tolerance;
    let mut acc = source;
    for chain in chains {
        let _span = kernel::trace::Span::start(format!("fillet: chamfer a chain of {} run(s)", chain.len()));
        let mut tool: Option<KSolid> = None;
        for &k in chain {
            let run = &runs[k];
            let edge = picked[run.members[0]].name.as_str();
            let cutter = build_cutter(Op::Fillet, run, edge)?;
            tool = Some(match tool {
                None => cutter.solid,
                Some(t) => kernel::boolean(BoolOp::Union, &t, &cutter.solid, tol).map_err(|e| {
                    CadpoleError::kernel(format!(
                        "fillet: the cutter that trims the faces round edge `{edge}` could not be joined to its chain's at \
                         the corner they share: {}. Fillet the two in separate calls, {FIELD}",
                        e.to_string().trim_end_matches('.')
                    ))
                })?,
            });
        }
        let tool = tool.expect("a chain has a run");
        let convex = runs[chain[0]].convex;
        let op = if convex { BoolOp::Subtract } else { BoolOp::Union };
        let edges: Vec<&str> = chain.iter().map(|&k| picked[runs[k].members[0]].name.as_str()).collect();
        acc = kernel::boolean(op, &acc, &tool, tol).map_err(|e| {
            CadpoleError::kernel(format!(
                "fillet: the exact kernel could not {} the cutter that trims the faces round {}: {}. The blend's \
                 contact curves are the setback curves of a chamfer at r·cot(γ/2), and that chamfer is what could \
                 not be cut here. {FIELD}",
                op.kind_name(),
                if edges.len() == 1 { format!("edge `{}`", edges[0]) } else { format!("edges {}", naming_list(edges.into_iter())) },
                e.to_string().trim_end_matches('.')
            ))
        })?;
        ctx.absorb_kernel_notes(fid, span.clone());
    }
    Ok(acc)
}

/// The classes a run's chamfer face was cut with.
fn chamfer_class(run: &Run) -> SurfaceClass {
    let s = Section::new(run);
    match run.path {
        Path::Line { .. } => SurfaceClass::Plane { origin: s.pa, normal: s.chamfer_normal },
        Path::Arc { centre, axis, start, .. } => {
            let ring = ring_geom(run, &s).expect("a circular run");
            let radial = (start - centre).normalized();
            chamfer::revolved_class(centre, axis, radial, ring.chamfer[0], ring.chamfer[1])
        }
    }
}

/// The name a face of the cut solid had on the source, by its surface.
fn source_face_name(class: &SurfaceClass, topo: &Topology) -> String {
    topo.faces
        .iter()
        .find(|f| same_surface(&f.surface, class))
        .map(|f| f.name.clone())
        .unwrap_or_else(|| format!("a {} face", class.kind_name()))
}

/// Replace every chamfer face of `cut` by its blend, every end segment by the
/// section arc, every mitre by the blends' intersection, and every three-way
/// corner by three quarter arcs and a sphere octant.
fn swap_chamfers(
    cut: &KSolid,
    topo: &Topology,
    picked: &[Picked],
    runs: &[Run],
    radius: f64,
    fid: &FeatureId,
    blends: &mut Vec<(SurfaceClass, String)>,
) -> Result<KSolid> {
    let ix = Indexed::of(cut);
    let classes: Vec<SurfaceClass> = ix.faces.iter().map(|f| classify(&f.oriented_surface(), EXACT_TOL)).collect();
    let chamfer_classes: Vec<SurfaceClass> = runs.iter().map(chamfer_class).collect();
    let name_of_run = |k: usize| picked[runs[k].members[0]].name.as_str();
    // Which run each face is the chamfer face of.
    let face_run: Vec<Option<usize>> =
        classes.iter().map(|c| chamfer_classes.iter().position(|cc| same_surface(c, cc))).collect();
    for (k, run) in runs.iter().enumerate() {
        if !face_run.iter().any(|r| *r == Some(k)) {
            return Err(CadpoleError::kernel(format!(
                "fillet: the chamfer face that carries the blend's topology for edge `{}` is not in the cut solid",
                name_of_run(k)
            )));
        }
        let _ = run;
    }
    // A corner: a vertex every face of which is a chamfer face, of three
    // distinct runs.
    let corner_runs = |v: VertexID| -> Option<Vec<usize>> {
        let fs = ix.vertex_faces.get(&v)?;
        let mut ks: Vec<usize> = fs.iter().map(|&f| face_run[f]).collect::<Option<Vec<_>>>()?;
        ks.sort_unstable();
        ks.dedup();
        (ks.len() == 3 && fs.len() == 3).then_some(ks)
    };

    let mut new_edges: HashMap<EdgeID, Edge> = HashMap::new();
    // Per chamfer face, its substitutions (mitre pairs merge per face).
    let mut face_subst: Vec<HashMap<EdgeID, Subst>> = vec![HashMap::new(); ix.faces.len()];
    // The corner arcs, keyed by (corner vertex, run); and the corners.
    let mut corner_arcs: HashMap<(VertexID, usize), Edge> = HashMap::new();
    struct Corner {
        centre: Vec3,
        runs: Vec<usize>,
        /// The outer vertex on the mitre edge between runs `(i, j)`.
        outer: HashMap<(usize, usize), Vertex>,
        vertex_name: String,
    }
    let mut corners: HashMap<VertexID, Corner> = HashMap::new();
    let mut new_faces: Vec<Face> = Vec::with_capacity(ix.faces.len() + 8);

    for (i, face) in ix.faces.iter().enumerate() {
        let Some(k) = face_run[i] else { continue };
        let run = &runs[k];
        let edge_name = name_of_run(k);
        let wires = face.boundaries();
        if wires.len() != 1 {
            return Err(CadpoleError::invalid(format!(
                "fillet: the blend along `{edge_name}` would have {} loops: something crosses the band the fillet \
                 takes off its faces. Move that feature clear of the edge, {FIELD}",
                wires.len()
            )));
        }
        let handles: Vec<Edge> = wires[0].edge_iter().cloned().collect();
        for h in &handles {
            let id = h.id();
            let (va, vb) = (h.absolute_front().clone(), h.absolute_back().clone());
            let (pa, pb) = (kernel::point_to_vec3(va.point()), kernel::point_to_vec3(vb.point()));
            if new_edges.contains_key(&id) {
                if let Some(new) = new_edges.get(&id) {
                    face_subst[i].insert(id, Subst::Replace(new.clone()));
                }
                continue;
            }
            let g = ix.across(id, i).ok_or_else(|| CadpoleError::kernel("fillet: an edge of the cut solid has one face"))?;
            let other_run = face_run[g];
            if other_run == Some(k) {
                // The seam between two pieces of one cone: the section arc.
                let new = section_edge(run, radius, &va, &vb, pa);
                new_edges.insert(id, new.clone());
                face_subst[i].insert(id, Subst::Replace(new));
                continue;
            }
            if let Some(side) = (0..2).find(|&s| same_surface(&classes[g], &topo.faces[run.faces[s]].surface)) {
                // A contact edge: the exact line or arc through the boolean's vertices.
                let new = contact_edge(run, side, &va, &vb, mid_of(h));
                new_edges.insert(id, new.clone());
                face_subst[i].insert(id, Subst::Replace(new));
                continue;
            }
            if let Some(j) = other_run {
                // A mitre with another run's blend.
                let corner = [&va, &vb].into_iter().find(|v| corner_runs(v.id()).is_some());
                if let Some(m) = corner {
                    let outer = if m.id() == va.id() { vb.clone() } else { va.clone() };
                    let ks = corner_runs(m.id()).expect("a corner");
                    let entry = corners.entry(m.id()).or_insert_with(|| Corner {
                        centre: Vec3::ZERO,
                        runs: ks.clone(),
                        outer: HashMap::new(),
                        vertex_name: String::new(),
                    });
                    entry.outer.insert((k.min(j), k.max(j)), outer.clone());
                    // The arc for run `k` at this corner is built once both of
                    // its outer vertices are known: below, after the walk.
                    face_subst[i].insert(id, Subst::Pending);
                    continue;
                }
                let new = mitre_edge(run, &runs[j], radius, &va, &vb, edge_name, name_of_run(j))?;
                new_edges.insert(id, new.clone());
                face_subst[i].insert(id, Subst::Replace(new));
                continue;
            }
            // An end on a planar face perpendicular to the run.
            let f_end = (station(run, (pa + pb) * 0.5)).round().clamp(0.0, 1.0);
            let t = run.path.tangent(f_end);
            if let SurfaceClass::Plane { normal, .. } = &classes[g] {
                if parallel(*normal, t, 1e-6) {
                    let new = section_edge(run, radius, &va, &vb, run.path.at(f_end));
                    new_edges.insert(id, new.clone());
                    face_subst[i].insert(id, Subst::Replace(new));
                    continue;
                }
            }
            let g_name = source_face_name(&classes[g], topo);
            return Err(CadpoleError::invalid(format!(
                "fillet: the blend along `{edge_name}` ends on `{g_name}` at {}, which is neither a planar face \
                 perpendicular to the edge nor another blend of this call: an oblique end, or a face the blend runs \
                 into, needs a vertex blend the exact path does not build. Fillet the closed \
                 loop the edge belongs to, fillet the edge along `{g_name}` in the same call so the two meet, build the \
                 round into the profile, {FIELD}",
                fmt_pt((pa + pb) * 0.5)
            )));
        }
    }

    // The corners: orthogonal, equal radius (one call), the three ball
    // centres one point.
    let mut octants: Vec<(VertexID, Face, SurfaceClass, String)> = Vec::new();
    for (m, corner) in corners.iter_mut() {
        let ks = corner.runs.clone();
        // The original vertex: the runs' common end.
        let v = {
            let r0 = &runs[ks[0]];
            let ends = [r0.path.start(), r0.path.end()];
            let r1 = &runs[ks[1]];
            ends.into_iter()
                .find(|e| (*e - r1.path.start()).length() <= 1e-6 || (*e - r1.path.end()).length() <= 1e-6)
                .ok_or_else(|| CadpoleError::kernel("fillet: three blends meet at a corner none of their edges share"))?
        };
        let leaving: Vec<Vec3> = ks
            .iter()
            .map(|&k| {
                let r = &runs[k];
                if (r.path.start() - v).length() <= 1e-6 { r.path.tangent(0.0) } else { -r.path.tangent(1.0) }
            })
            .collect();
        let names: Vec<&str> = ks.iter().map(|&k| name_of_run(k)).collect();
        let orthogonal = (0..3).all(|a| (0..3).all(|b| a == b || leaving[a].dot(leaving[b]).abs() <= 1e-6))
            && ks.iter().all(|&k| (runs[k].frame.wedge() - std::f64::consts::FRAC_PI_2).abs() <= 1e-6)
            && ks.iter().all(|&k| matches!(runs[k].path, Path::Line { .. }))
            && ks.iter().all(|&k| runs[k].convex);
        if !orthogonal {
            return Err(CadpoleError::invalid(format!(
                "fillet: three blends meet at {} (edges {}) and the corner is not orthogonal — the edges are not \
                 mutually perpendicular with their faces at right angles — so the vertex needs an N-sided patch the \
                 exact path does not build; only the sphere octant is. Fillet two of the \
                 edges and leave the third sharp, {FIELD}",
                fmt_pt(v),
                naming_list(names.iter().copied())
            )));
        }
        let centre = v + (leaving[0] + leaving[1] + leaving[2]) * radius;
        corner.centre = centre;
        corner.vertex_name = topo
            .vertices
            .iter()
            .find(|x| (x.position - v).length() <= 1e-6)
            .map(|x| x.name.clone())
            .unwrap_or_else(|| fmt_pt(v));
        // The three arcs: run k's from its outer vertex with run j to its outer vertex with run l.
        let outer = |a: usize, b: usize| -> Result<Vertex> {
            corner.outer.get(&(a.min(b), a.max(b))).cloned().ok_or_else(|| CadpoleError::kernel("fillet: a corner is missing a mitre edge"))
        };
        for (n, &k) in ks.iter().enumerate() {
            let (j, l) = (ks[(n + 1) % 3], ks[(n + 2) % 3]);
            let (pj, pl) = (outer(k, j)?, outer(k, l)?);
            let (qj, ql) = (kernel::point_to_vec3(pj.point()), kernel::point_to_vec3(pl.point()));
            for q in [qj, ql] {
                if ((q - centre).length() - radius).abs() > 1e-6 || (q - centre).dot(leaving[n]).abs() > 1e-6 {
                    return Err(CadpoleError::kernel(format!(
                        "fillet: the corner at {} is not where the three blends' contact curves end ({} is {:.4} mm off \
                         the sphere)",
                        fmt_pt(v),
                        fmt_pt(q),
                        ((q - centre).length() - radius).abs()
                    )));
                }
            }
            let (_, w) = fmath::sin_cos(std::f64::consts::FRAC_PI_4);
            let pts = vec![hom(qj, 1.0), hom(qj + ql - centre, w), hom(ql, 1.0)];
            let arc = Edge::new(&pj, &pl, nurbs_curve(pts, KnotVec::bezier_knot(2)));
            corner_arcs.insert((*m, k), arc);
        }
        // The octant: the profile arc from the pole (run 0's opposite vertex)
        // to run 0's first outer vertex, swung a quarter turn about run 0's
        // direction through the centre onto its other outer vertex.
        let (k0, k1, k2) = (ks[0], ks[1], ks[2]);
        let (p01, p02, p12) = (outer(k0, k1)?, outer(k0, k2)?, outer(k1, k2)?);
        let (q01, q02, q12) = (kernel::point_to_vec3(p01.point()), kernel::point_to_vec3(p02.point()), kernel::point_to_vec3(p12.point()));
        let axis = leaving[0];
        let turn = if (rotate_about(q01, centre, axis, std::f64::consts::FRAC_PI_2) - q02).length() <= 1e-6 {
            std::f64::consts::FRAC_PI_2
        } else if (rotate_about(q01, centre, -axis, std::f64::consts::FRAC_PI_2) - q02).length() <= 1e-6 {
            -std::f64::consts::FRAC_PI_2
        } else {
            return Err(CadpoleError::kernel(format!("fillet: the octant at {} does not close a quarter turn", fmt_pt(v))));
        };
        let (_, w) = fmath::sin_cos(std::f64::consts::FRAC_PI_4);
        let profile = vec![hom(q12, 1.0), hom(q12 + q01 - centre, w), hom(q01, 1.0)];
        let surface = revolve_profile(&profile, KnotVec::bezier_knot(2), centre, axis * turn.signum(), turn.abs())?;
        // The wire: each arc the way the blend that owns it does not go.
        let arcs = [(k0, p01.clone(), p02.clone()), (k1, p01.clone(), p12.clone()), (k2, p02.clone(), p12.clone())];
        let mut loop_edges: Vec<Edge> = Vec::new();
        for (k, _, _) in &arcs {
            let arc = corner_arcs[&(*m, *k)].clone();
            // The blend face of run k traverses the arc as its merged mitre
            // pair did: from the outer vertex its wire reaches first. Read the
            // direction off the chamfer face's wire.
            let fi = (0..ix.faces.len()).find(|&f| face_run[f] == Some(*k) && ix.vertex_faces.get(m).map(|s| s.contains(&f)).unwrap_or(false)).expect("the run's chamfer face at the corner");
            let wire = &ix.faces[fi].boundaries()[0];
            let hs: Vec<Edge> = wire.edge_iter().cloned().collect();
            let n = hs.len();
            let pos = (0..n).find(|&p| hs[p].back().id() == *m).expect("the corner on the wire");
            let from = hs[pos].front().id();
            // The blend goes from `from` through the corner to the next outer
            // vertex; the octant goes the other way.
            let blend_forward = arc.absolute_front().id() == from;
            loop_edges.push(if blend_forward { arc.inverse() } else { arc.clone() });
        }
        // Chain them into a loop.
        let mut ordered = vec![loop_edges[0].clone()];
        let mut rest: Vec<Edge> = loop_edges[1..].to_vec();
        while !rest.is_empty() {
            let tail = ordered.last().expect("a loop").back().id();
            let pos = rest.iter().position(|e| e.front().id() == tail).ok_or_else(|| CadpoleError::kernel(format!("fillet: the octant's arcs at {} do not chain", fmt_pt(v))))?;
            ordered.push(rest.remove(pos));
        }
        let wire: Wire = ordered.into();
        let (x, nrm) = surface_at(&surface, 0.5, 0.5);
        let forward = nrm.dot((x - centre).normalized()) > 0.0;
        let face = face_oriented(vec![wire], surface, forward)?;
        let name = format!("{}/vertex[{}]", fid.0, corner.vertex_name);
        octants.push((*m, face, SurfaceClass::Sphere { center: centre, radius }, name));
    }
    // Now that the corner arcs exist, put them into the chamfer faces' maps.
    for (i, _) in ix.faces.iter().enumerate() {
        let Some(k) = face_run[i] else { continue };
        let keys: Vec<EdgeID> = face_subst[i].iter().filter(|(_, s)| matches!(s, Subst::Pending)).map(|(id, _)| *id).collect();
        for id in keys {
            let h = &ix.edges[&id];
            let m = [h.absolute_front(), h.absolute_back()].into_iter().find(|v| corners.contains_key(&v.id())).expect("a corner").id();
            let arc = corner_arcs[&(m, k)].clone();
            face_subst[i].insert(id, Subst::Merge(arc));
        }
    }

    // ---- rebuild
    for (i, face) in ix.faces.iter().enumerate() {
        match face_run[i] {
            None => {
                let mut subst: HashMap<EdgeID, Subst> = HashMap::new();
                for w in face.boundaries() {
                    for h in w.edge_iter() {
                        if let Some(n) = new_edges.get(&h.id()) {
                            subst.insert(h.id(), Subst::Replace(n.clone()));
                        }
                    }
                }
                new_faces.push(rebuild(face, &subst)?);
            }
            Some(k) => {
                let run = &runs[k];
                let oriented = substitute(&face.boundaries()[0], &face_subst[i])?;
                // The patch spans the wire's stations along the run.
                let mut lo = f64::INFINITY;
                let mut hi = f64::NEG_INFINITY;
                for h in oriented.edge_iter() {
                    for v in [h.front(), h.back()] {
                        let f = station(run, kernel::point_to_vec3(v.point()));
                        lo = lo.min(f);
                        hi = hi.max(f);
                    }
                }
                let (surface, class) = blend_surface(run, radius, lo, hi)?;
                new_faces.push(blend_face(run, radius, oriented, surface)?);
                blends.push((class, blend_name(fid, name_of_run(k))));
            }
        }
    }
    for (_, face, class, name) in octants {
        new_faces.push(face);
        blends.push((class, name));
    }
    solid_of(new_faces, "swapping the chamfer faces for blends")
}

/// The section arc between two vertices of a run's chamfer face at one
/// station: `near` is a point at that station (its edge point, or a point
/// of the seam).
fn section_edge(run: &Run, radius: f64, va: &Vertex, vb: &Vertex, near: Vec3) -> Edge {
    let s = section_at(run, radius, station(run, near));
    let (pa, pb) = (kernel::point_to_vec3(va.point()), kernel::point_to_vec3(vb.point()));
    let n = (pa - s.ball).cross(pb - s.ball).normalized();
    let (mut pts, knots) = arc_control(pa, s.ball, n, s.arc);
    *pts.last_mut().expect("an arc has control points") = hom(pb, 1.0);
    Edge::new(va, vb, nurbs_curve(pts, knots))
}

/// A contact edge of a run on side `side`: the exact line, or the exact
/// setback circle's arc through `via`, between the boolean's vertices.
fn contact_edge(run: &Run, side: usize, va: &Vertex, vb: &Vertex, via: Vec3) -> Edge {
    match &setback_curves(run)[side] {
        SetbackCurve::Line { .. } => Edge::new(va, vb, Curve::Line(Line(va.point(), vb.point()))),
        SetbackCurve::Circle { centre, axis, .. } => circle_arc_edge(va, vb, *centre, *axis, via),
    }
}

/// The mitre between two blends of equal radius: the planar ellipse arc in
/// which their cylinders meet, exact as a rational quadratic — the affine
/// image of a circular arc, since the intersection of two equal cylinders
/// with meeting axes is an ellipse with semi-axes `r` (along the common
/// normal of the axes) and `r / sin(θ/2)` (θ the angle between the axes).
fn mitre_edge(a: &Run, b: &Run, radius: f64, va: &Vertex, vb: &Vertex, name_a: &str, name_b: &str) -> Result<Edge> {
    let (pa, pb) = (kernel::point_to_vec3(va.point()), kernel::point_to_vec3(vb.point()));
    let refuse = |why: String| {
        CadpoleError::invalid(format!(
            "fillet: the blends along `{name_a}` and `{name_b}` meet at {} and their contact curves do not: {why}, so \
             the vertex needs a blend the exact path does not build. Fillet one of the two, \
             {FIELD}",
            fmt_pt((pa + pb) * 0.5)
        ))
    };
    let (Path::Line { .. }, Path::Line { .. }) = (&a.path, &b.path) else {
        return Err(refuse("one of them is circular".into()));
    };
    let (sa, sb) = (section_at(a, radius, 0.0), section_at(b, radius, 0.0));
    let (ta, tb) = (sa.t, sb.t);
    // The axes' closest points.
    let (ba, bb) = (sa.ball, sb.ball);
    let n = ta.cross(tb);
    if n.length() < 1e-9 {
        return Err(refuse("their edges are parallel".into()));
    }
    let n = n.normalized();
    let gap = (bb - ba).dot(n).abs();
    if gap > 1e-6 {
        return Err(refuse(format!("the blends' axes pass {gap:.3} mm apart")));
    }
    // C: solve ba + s ta = bb + u tb (in the plane of the axes).
    let w = bb - ba;
    let (aa, ab, bb2) = (ta.dot(ta), ta.dot(tb), tb.dot(tb));
    let det = aa * bb2 - ab * ab;
    let s = (w.dot(ta) * bb2 - w.dot(tb) * ab) / det;
    let centre = ba + ta * s;
    let dist = |p: Vec3, t: Vec3| {
        let v = p - centre;
        (v - t * v.dot(t)).length()
    };
    for p in [pa, pb] {
        for t in [ta, tb] {
            if (dist(p, t) - radius).abs() > 1e-6 {
                return Err(refuse(format!(
                    "the contact point at {} is {:.3} mm from where the other blend touches",
                    fmt_pt(p),
                    (dist(p, t) - radius).abs()
                )));
            }
        }
    }
    // The ellipse's frame in the plane through C, pa and pb.
    let pn = (pa - centre).cross(pb - centre).normalized();
    if n.dot(pn).abs() > 1e-6 {
        return Err(refuse("the mitre does not lie in a plane through the axes' crossing".into()));
    }
    let e1 = n;
    let e2 = pn.cross(e1).normalized();
    let cos_half = e2.dot(ta).abs();
    if (cos_half - e2.dot(tb).abs()).abs() > 1e-6 {
        return Err(refuse("the mitre plane does not bisect the axes".into()));
    }
    let semi_b = radius / (1.0 - cos_half * cos_half).max(1e-12).sqrt();
    let unit = |p: Vec3| -> (f64, f64) {
        let v = p - centre;
        (v.dot(e1) / radius, v.dot(e2) / semi_b)
    };
    let (ua, ub) = (unit(pa), unit(pb));
    let (phi_a, phi_b) = (fmath::atan2(ua.1, ua.0), fmath::atan2(ub.1, ub.0));
    let mut d = phi_b - phi_a;
    if d > std::f64::consts::PI {
        d -= TAU;
    }
    if d < -std::f64::consts::PI {
        d += TAU;
    }
    if d.abs() >= std::f64::consts::PI - 1e-9 {
        return Err(refuse("the mitre would be half the ellipse".into()));
    }
    let (sm, cm) = fmath::sin_cos(phi_a + d / 2.0);
    let (_, w) = fmath::sin_cos(d.abs() / 2.0);
    let p1 = centre + e1 * (radius * cm / w) + e2 * (semi_b * sm / w);
    let pts = vec![hom(pa, 1.0), hom(p1, w), hom(pb, 1.0)];
    let curve = nurbs_curve(pts, KnotVec::bezier_knot(2));
    // Every point of it is on both cylinders.
    for k in 1..8 {
        let q = kernel::point_to_vec3(curve.subs(k as f64 / 8.0));
        if (dist(q, ta) - radius).abs() > 1e-6 || (dist(q, tb) - radius).abs() > 1e-6 {
            return Err(CadpoleError::kernel(format!(
                "fillet: the mitre ellipse between `{name_a}` and `{name_b}` leaves the blends by {:.2e} mm",
                (dist(q, ta) - radius).abs().max((dist(q, tb) - radius).abs())
            )));
        }
    }
    Ok(Edge::new(va, vb, curve))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Plane;
    use crate::kernel::KernelChoice;
    use crate::sketch::{Loop, Sketch, P2};
    use crate::solid;
    use crate::tessellate::EdgeInfo;
    use std::f64::consts::PI;

    fn a_box(ctx: &mut Context, w: f64, d: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
        ctx.extrude("box", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    fn a_cylinder(ctx: &mut Context, at: P2, z: f64, r: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY.offset(z)).with_loop(Loop::circle(at, r));
        ctx.extrude("cylinder", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    /// The body's volume from a fine tessellation of the exact solid (1 µm
    /// chords), so a closed form is met to the blends' chord error at that
    /// tolerance and not the display mesh's.
    fn volume(ctx: &mut Context, id: BodyId) -> f64 {
        let s = ctx.body(id).unwrap().solid().unwrap().clone();
        Body::Brep(s).to_mesh(0.001).unwrap().0.volume()
    }

    fn assert_volume(v: f64, expect: f64, rel: f64) {
        assert!((v - expect).abs() <= rel * expect, "volume {v}, expected {expect} (off by {:.2e})", (v - expect).abs() / expect);
    }

    fn names_of(ctx: &mut Context, id: BodyId) -> Vec<String> {
        let mut n: Vec<String> = ctx.body_mesh(id).unwrap().1.faces.iter().map(|f| f.name.clone()).collect();
        n.sort();
        n
    }

    fn edges_named(ctx: &mut Context, id: BodyId, pred: impl Fn(&EdgeInfo) -> bool) -> Vec<String> {
        ctx.body_mesh(id).unwrap().1.edges.iter().filter(|e| pred(e)).map(|e| e.name.clone()).collect()
    }

    fn at_z(e: &EdgeInfo, z: f64) -> bool {
        (e.start.z - z).abs() < 1e-6 && (e.end.z - z).abs() < 1e-6
    }

    fn top_rim(ctx: &mut Context, id: BodyId, z: f64) -> Vec<String> {
        edges_named(ctx, id, |e| at_z(e, z))
    }

    /// The gate every result is held to: a solid by every test, no fold,
    /// every face a recognised surface, no fallback name, still exact.
    fn assert_clean(ctx: &mut Context, id: BodyId) {
        let (mesh, topo) = ctx.body_mesh(id).unwrap().clone();
        let report = solid::validate(&mesh);
        assert!(report.is_solid(), "{}", report.problems().join("; "));
        assert!(report.folds.is_empty(), "{} folds", report.folds.len());
        for f in &topo.faces {
            assert!(f.surface.is_recognised(), "{} is {}", f.name, f.surface_kind);
            let local = naming::local_of(&f.name);
            let fallback =
                local.strip_prefix("face[").map(|r| r.trim_end_matches(']').chars().all(|c| c.is_ascii_digit())).unwrap_or(false);
            assert!(!fallback, "a fallback name: {}", f.name);
        }
        assert_eq!(ctx.body(id).unwrap().kind(), "brep");
    }

    fn refusal(ctx: &mut Context, body: BodyId, edges: &[String], r: f64) -> String {
        match ctx.fillet(body, edges, r, None, None) {
            Ok(_) => panic!("expected a refusal"),
            Err(e) => e.to_string(),
        }
    }

    /// A box's top rim at 2 mm: four cylinder faces of that radius meeting
    /// in four elliptical mitres, the neighbours kept, and the volume in
    /// closed form: each edge loses `(1 − π/4) r²` per unit length, and at
    /// each corner the two blends' wedges overlap in the region
    /// `{x<r, y<r, z<r}` above the mitre — subtracted twice along the
    /// corner's `r` of each edge and given back once: the overlap of two
    /// perpendicular quarter-round cutters of radius r is `(2/3 − π/4 + ...)`;
    /// measured against the exact figure `r³(1 − π/4)·2 − r³(2/3)·(1 − ...)`.
    /// Concretely, per corner the box loses `r³ (1 − π/4) · 2` along the two
    /// edge ends less the doubly-counted part, which by integrating the
    /// two-cylinder intersection is `r³ (2/3 − π/4 + 1/... )`; the test
    /// derives it numerically once and pins it.
    #[test]
    fn a_box_top_rim_is_four_cylinders_meeting_in_ellipses() {
        let (w, d, h, r) = (30.0, 20.0, 10.0, 2.0);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, w, d, h, "blk");
        let rim = top_rim(&mut ctx, blk, h);
        assert_eq!(rim.len(), 4, "{rim:?}");
        let out = ctx.fillet(blk, &rim, r, Some("round".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);

        let names = names_of(&mut ctx, out.body);
        assert_eq!(names.len(), 10, "{names:?}");
        for e in &rim {
            assert!(names.contains(&format!("round/blend[{e}]")), "{names:?}");
        }
        for kept in ["blk/top", "blk/bottom", "blk/front", "blk/back", "blk/left", "blk/right"] {
            assert!(names.contains(&kept.to_owned()), "{names:?}");
        }
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        for f in topo.faces.iter().filter(|f| f.name.starts_with("round/")) {
            let SurfaceClass::Cylinder { radius, axis, .. } = f.surface else { panic!("{}: {:?}", f.name, f.surface) };
            assert!((radius - r).abs() < 1e-6, "{}: radius {radius}", f.name);
            assert!(axis.z.abs() < 1e-9, "a rim blend's axis is horizontal: {axis:?}");
        }
        // Four mitres between blends, each a quarter ellipse with semi-axes
        // r and r√2: its length is what an ellipse of that shape has.
        let mitres: Vec<&EdgeInfo> = topo.edges.iter().filter(|e| e.faces.iter().all(|n| n.starts_with("round/"))).collect();
        assert_eq!(mitres.len(), 4, "four mitres");
        for m in &mitres {
            assert_eq!(m.curve_kind, "nurbs");
            // From (r in, r in, h) on the top down to (0, 0, h − r) on the vertical edge.
            let ends = [m.start, m.end];
            assert!(ends.iter().any(|p| (p.z - h).abs() < 1e-6), "{:?}", ends);
            assert!(ends.iter().any(|p| (p.z - (h - r)).abs() < 1e-6), "{:?}", ends);
        }
        // Volume: the closed form, the wedges along every edge less the
        // corner overlaps, each of which is the volume of the region above
        // both quarter-cylinders' sections: r³ (2/3 − π/4 + ... ) — derived
        // by integrating x + y < ... ; numerically (1 - PI/4) * r² per unit
        // length, and per corner the double count is r³ · (2/3 − π/4 + 1/2·(π/2 − 4/3)) —
        // simplified below to the integral of the two quarter-round profiles.
        let per_len = (1.0 - PI / 4.0) * r * r;
        // The overlap at one corner: the region {0<x<r, 0<y<r, 0<z<r} that is
        // outside *both* blend cylinders (x−r)²+(z−r)² > r² and (y−r)²+(z−r)² > r²
        // is subtracted once in each edge's `per_len·r` and is really only
        // gone once, so give back its volume: ∫₀ʳ s(z)² dz with
        // s(z) = r − √(r² − (z − r)²) the width outside one cylinder at height z.
        let overlap = {
            let n = 4000;
            let mut sum = 0.0;
            for i in 0..n {
                let z = r * (i as f64 + 0.5) / n as f64;
                let s = r - (r * r - (z - r) * (z - r)).max(0.0).sqrt();
                sum += s * s * (r / n as f64);
            }
            sum
        };
        let expect = w * d * h - per_len * 2.0 * (w + d) + 4.0 * overlap;
        let v = volume(&mut ctx, out.body);
        // The blends are tessellated, so the figure is met to the chord
        // error of four cylinders, not to the bit.
        assert_volume(v, expect, 2e-4);
        // The records: contact r along each face, a right-angle wedge, lines.
        assert_eq!(out.edges.len(), 4);
        for rec in &out.edges {
            assert_eq!(rec.setback, [r, r]);
            assert!((rec.wedge - PI / 2.0).abs() < 1e-9);
            assert!(rec.convex);
            assert!(rec.face.starts_with("round/blend["));
            assert!(matches!(rec.curves[0], SetbackCurve::Line { .. }));
        }
        // The chord error is the cylinders': never more than the tolerance
        // asked for, and not zero.
        let s = ctx.body(out.body).unwrap().solid().unwrap().clone();
        let t = kernel::tessellate_reporting(&s, 0.05).unwrap();
        let chord = t.chord.map(|e| e.max).unwrap_or(0.0);
        assert!(chord > 0.0 && chord <= 0.05 + 1e-9, "chord {chord}");
    }

    /// All twelve edges of a box: twelve cylinder blends and eight sphere
    /// octants, one at every corner, the volume of the rounded box in closed
    /// form: `V − (1 − π/4) r² Σl + (corners)`, where every corner's cube
    /// `r³` is replaced by the sphere octant and the three quarter-cylinder
    /// wedges meeting there — the rounded box's volume is exactly
    /// `(w−2r)(d−2r)(h−2r) + 2r[(w−2r)(d−2r) + ...] + πr²[(w−2r)+(d−2r)+(h−2r)] + 4/3 π r³`.
    #[test]
    fn all_twelve_edges_of_a_box_make_a_rounded_box_with_sphere_octants() {
        let (w, d, h, r) = (24.0, 16.0, 8.0, 1.5);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, w, d, h, "blk");
        let out = ctx.fillet(blk, &[], r, Some("round".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);
        let names = names_of(&mut ctx, out.body);
        assert_eq!(names.len(), 26, "{names:?}");
        assert_eq!(names.iter().filter(|n| n.starts_with("round/blend[")).count(), 12);
        assert_eq!(names.iter().filter(|n| n.starts_with("round/vertex[")).count(), 8, "{names:?}");
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        for f in topo.faces.iter().filter(|f| f.name.starts_with("round/vertex[")) {
            let SurfaceClass::Sphere { radius, .. } = f.surface else { panic!("{}: {:?}", f.name, f.surface) };
            assert!((radius - r).abs() < 1e-6);
        }
        let (iw, id, ih) = (w - 2.0 * r, d - 2.0 * r, h - 2.0 * r);
        let expect = iw * id * ih
            + 2.0 * r * (iw * id + iw * ih + id * ih)
            + PI * r * r * (iw + id + ih)
            + 4.0 / 3.0 * PI * r * r * r;
        let v = volume(&mut ctx, out.body);
        assert_volume(v, expect, 3e-4);
    }

    /// A chain of two edges turning a corner mitres there and ends on the
    /// perpendicular side faces at its free ends.
    #[test]
    fn a_chain_that_turns_a_corner_mitres_there_and_ends_on_the_side_faces() {
        let (w, d, h, r) = (30.0, 20.0, 10.0, 2.0);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, w, d, h, "blk");
        let two: Vec<String> = edges_named(&mut ctx, blk, |e| {
            let front = e.start.y.min(e.end.y) < -d / 2.0 + 1e-6 && (e.start.y - e.end.y).abs() < 1e-6;
            let right = e.start.x.min(e.end.x) > w / 2.0 - 1e-6 && (e.start.x - e.end.x).abs() < 1e-6;
            at_z(e, h) && (front || right)
        });
        assert_eq!(two.len(), 2, "{two:?}");
        let out = ctx.fillet(blk, &two, r, None, None).unwrap();
        assert_clean(&mut ctx, out.body);
        assert_eq!(names_of(&mut ctx, out.body).len(), 8);
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        assert_eq!(topo.edges.iter().filter(|e| e.faces.iter().all(|n| n.starts_with("fillet_1/"))).count(), 1);
        // The end arcs lie in the side faces: an edge between a blend and
        // blk/left or blk/back, a quarter circle of length πr/2.
        let ends: Vec<&EdgeInfo> = topo
            .edges
            .iter()
            .filter(|e| e.faces.iter().any(|n| n.starts_with("fillet_1/")) && e.faces.iter().any(|n| n == "blk/left" || n == "blk/back"))
            .collect();
        assert_eq!(ends.len(), 2, "{:?}", ends.iter().map(|e| &e.name).collect::<Vec<_>>());
        for e in ends {
            // A quarter circle, measured along the display polyline (which
            // chords it short by a few hundredths).
            assert_eq!(e.curve_kind, "nurbs");
            assert!((e.length - PI * r / 2.0).abs() < 0.05, "{}: {}", e.name, e.length);
        }
    }

    /// A cylinder's rim: no boolean at all — the rim's two arcs move out to
    /// the two contact circles, the wall's seams are trimmed, and two torus
    /// halves close the gap; Pappus gives the volume.
    #[test]
    fn a_cylinder_rim_is_a_torus_by_re_wiring() {
        let (rad, h, r) = (10.0, 12.0, 1.5);
        let mut ctx = Context::new();
        let post = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 0.0, rad, h, "post");
        let rim = top_rim(&mut ctx, post, h);
        assert_eq!(rim.len(), 2);
        let out = ctx.fillet(post, &rim, r, Some("round".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);
        assert_eq!(out.edges.len(), 1);
        let names = names_of(&mut ctx, out.body);
        let tori: Vec<&String> = names.iter().filter(|n| n.starts_with("round/blend[")).collect();
        assert_eq!(tori.len(), 2, "{names:?}");
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        for f in topo.faces.iter().filter(|f| f.name.starts_with("round/")) {
            let SurfaceClass::Torus { major, minor, axis, .. } = f.surface else { panic!("{}: {:?}", f.name, f.surface) };
            assert!((major - (rad - r)).abs() < 1e-6 && (minor - r).abs() < 1e-6, "{}: {major} {minor}", f.name);
            assert!(axis.z.abs() > 0.999);
        }
        // Pappus: the section removed is a square less a quarter disc,
        // (1 − π/4) r², whose centroid sits at distance rad − r + (r − 4r/(3(4−π)))... use
        // the exact revolved figure: V = π rad² h − 2π ∫ over the removed region of ρ dA.
        // Removed region in (ρ, z): rad−r < ρ < rad, h−r < z < h, outside the circle
        // centred (rad−r, h−r) of radius r. ∫ρ dA = ∫ over the square − ∫ over the quarter disc.
        let square = (rad * rad - (rad - r) * (rad - r)) / 2.0 * r; // ∫ρ dρ dz over the square
        // ∫ρ dA over the quarter disc centred at ρc = rad − r: area·ρc + first moment about its centre:
        let quarter = PI * r * r / 4.0 * (rad - r) + (r * r * r / 3.0);
        let expect = PI * rad * rad * h - TAU * (square - quarter);
        let v = volume(&mut ctx, out.body);
        assert_volume(v, expect, 2e-3);
    }

    /// A boss on a plate: the root is an inner loop of the plate's top — the
    /// case the chamfer refuses — and the bead re-wires without a boolean:
    /// an inner torus, concave, with Pappus's volume added.
    #[test]
    fn a_boss_root_takes_an_inner_torus_bead() {
        let (w, d, t, rb, hb, r) = (40.0, 40.0, 5.0, 8.0, 10.0, 1.0);
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, w, d, t, "plate");
        let boss = a_cylinder(&mut ctx, P2::new(0.0, 0.0), t - 1.0, rb, hb + 1.0, "boss");
        let part = ctx.boolean(BoolOp::Union, plate, &[boss], KernelChoice::Brep, Some("part".into()), None).unwrap();
        let root: Vec<String> = edges_named(&mut ctx, part, |e| at_z(e, t) && !e.convex);
        assert_eq!(root.len(), 2, "{root:?}");
        let out = ctx.fillet(part, &root, r, Some("bead".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);
        assert!(!out.edges[0].convex);
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        let tori: Vec<_> = topo.faces.iter().filter(|f| f.name.starts_with("bead/blend[")).collect();
        assert_eq!(tori.len(), 2);
        for f in &tori {
            let SurfaceClass::Torus { major, minor, .. } = f.surface else { panic!("{}: {:?}", f.name, f.surface) };
            assert!((major - (rb + r)).abs() < 1e-6 && (minor - r).abs() < 1e-6, "{major} {minor}");
        }
        assert!(topo.faces.iter().any(|f| f.name == "plate/top"));
        // Added: the fillet's section (1 − π/4) r² revolved: ∫ρ dA over the
        // square [rb, rb+r]×[t, t+r] less the quarter disc centred (rb+r, t+r).
        let square = ((rb + r) * (rb + r) - rb * rb) / 2.0 * r;
        let quarter = PI * r * r / 4.0 * (rb + r) - r * r * r / 3.0;
        let expect = w * d * t + PI * rb * rb * hb + TAU * (square - quarter);
        let v = volume(&mut ctx, out.body);
        assert_volume(v, expect, 2e-3);
    }

    /// A blind hole's floor: a concave closed loop that is the floor's own
    /// outer boundary; and the hole's mouth, an inner loop of the plate, is
    /// a convex torus — both by re-wiring.
    #[test]
    fn a_hole_mouth_and_a_blind_floor_both_re_wire() {
        let (w, d, t, rb, r) = (30.0, 30.0, 8.0, 4.0, 1.0);
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, w, d, t, "plate");
        let bore = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 3.0, rb, 10.0, "bore");
        let part = ctx.boolean(BoolOp::Subtract, plate, &[bore], KernelChoice::Brep, Some("part".into()), None).unwrap();
        let mut both: Vec<String> = edges_named(&mut ctx, part, |e| at_z(e, t) && e.length < TAU * rb);
        both.extend(edges_named(&mut ctx, part, |e| at_z(e, 3.0)));
        assert_eq!(both.len(), 4, "{both:?}");
        let out = ctx.fillet(part, &both, r, Some("round".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);
        assert_eq!(out.edges.len(), 2);
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        let tori: Vec<_> = topo.faces.iter().filter(|f| f.name.starts_with("round/blend[")).collect();
        assert_eq!(tori.len(), 4, "{:?}", tori.iter().map(|f| &f.name).collect::<Vec<_>>());
        // Mouth: removed (1 − π/4) r² revolved at ρ ≈ rb..rb+r; floor: added at the same radii.
        let square = ((rb + r) * (rb + r) - rb * rb) / 2.0 * r;
        let mouth = square - (PI * r * r / 4.0 * (rb + r) - r * r * r / 3.0);
        let floor = square - (PI * r * r / 4.0 * (rb + r) - r * r * r / 3.0);
        let expect = w * d * t - PI * rb * rb * (t - 3.0) - TAU * mouth + TAU * floor;
        let v = volume(&mut ctx, out.body);
        assert_volume(v, expect, 2e-3);
    }

    /// The rim of an extruded rounded rectangle: four straight runs and four
    /// quarter-circle runs joined tangentially — one closed chain, re-wired
    /// directly, cylinders and torus quarters sharing their section arcs.
    #[test]
    fn a_rounded_outline_rim_is_one_tangent_chain_of_cylinders_and_tori() {
        let (w, d, h, rc, r) = (30.0, 20.0, 8.0, 4.0, 1.5);
        let mut ctx = Context::new();
        let s = Sketch::new(Plane::XY).with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), w, d, rc));
        let blk = ctx.extrude("extrude", &s, None, h, BTreeMap::new(), Some("blk".into()), None).unwrap()[0];
        let rim = top_rim(&mut ctx, blk, h);
        assert_eq!(rim.len(), 8, "{rim:?}");
        let out = ctx.fillet(blk, &rim, r, Some("round".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        let blends: Vec<_> = topo.faces.iter().filter(|f| f.name.starts_with("round/blend[")).collect();
        assert_eq!(blends.len(), 8);
        assert_eq!(blends.iter().filter(|f| matches!(f.surface, SurfaceClass::Cylinder { .. })).count(), 4);
        assert_eq!(blends.iter().filter(|f| matches!(f.surface, SurfaceClass::Torus { .. })).count(), 4);
        // Volume: the section (1 − π/4) r² along the straight length plus
        // the corner tori (Pappus at radius rc − r + centroid offset).
        let straight = 2.0 * (w - 2.0 * rc) + 2.0 * (d - 2.0 * rc);
        let square = (rc * rc - (rc - r) * (rc - r)) / 2.0 * r;
        let quarter = PI * r * r / 4.0 * (rc - r) + r * r * r / 3.0;
        let corners = TAU * (square - quarter);
        let area = w * d - (4.0 - PI) * rc * rc;
        let expect = area * h - (1.0 - PI / 4.0) * r * r * straight - corners;
        let v = volume(&mut ctx, out.body);
        assert_volume(v, expect, 2e-3);
    }

    /// The rim of an outline whose corners `fillet` itself rounded, at the
    /// corners' own radius: each corner blend is then a sphere patch whose
    /// contact curve on the top shrinks to a point (a torus of major radius
    /// zero). It used to fail to mesh ("the boundary segment ... passes
    /// through boundary vertex 8") while the same rim on an extruded
    /// rounded rectangle built; both must build, to the same volume, with a
    /// sphere at every corner.
    #[test]
    fn a_rim_at_the_corner_radius_of_a_filleted_outline_builds_sphere_corners() {
        for r in [1.0, 2.0] {
            let (w, h) = (10.0, 10.0);
            let mut ctx = Context::new();
            let blk = a_box(&mut ctx, w, w, h, "blk");
            let verticals = edges_named(&mut ctx, blk, |e| (e.start.x - e.end.x).abs() < 1e-6 && (e.start.y - e.end.y).abs() < 1e-6);
            let rounded = ctx.fillet(blk, &verticals, r, Some("corners".into()), None).unwrap().body;
            let rim = top_rim(&mut ctx, rounded, h);
            assert_eq!(rim.len(), 8, "{rim:?}");
            let out = ctx.fillet(rounded, &rim, r, Some("round".into()), None).unwrap_or_else(|e| panic!("r={r}: {e}"));
            assert_clean(&mut ctx, out.body);
            let v = volume(&mut ctx, out.body);

            let mut ctx2 = Context::new();
            let s = Sketch::new(Plane::XY).with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), w, w, r));
            let ext = ctx2.extrude("extrude", &s, None, h, BTreeMap::new(), Some("ext".into()), None).unwrap()[0];
            let rim2 = top_rim(&mut ctx2, ext, h);
            let out2 = ctx2.fillet(ext, &rim2, r, Some("round".into()), None).unwrap();
            let v2 = volume(&mut ctx2, out2.body);
            assert_volume(v, v2, 1e-4);
            let topo = ctx.body_mesh(out.body).unwrap().1.clone();
            let spheres = topo.faces.iter().filter(|f| matches!(f.surface, SurfaceClass::Sphere { .. })).count();
            assert_eq!(spheres, 4, "{:?}", topo.faces.iter().map(|f| (&f.name, f.surface_kind)).collect::<Vec<_>>());
        }
    }

    /// A cylinder's top rim at the cylinder's own radius: both rim arcs
    /// collapse to the pole and the top face has no boundary left. It is a
    /// dome, and is refused by name instead of failing as "a rebuilt face is
    /// not simple".
    #[test]
    fn a_rim_radius_equal_to_the_outline_is_a_dome_and_says_so() {
        let mut ctx = Context::new();
        let post = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 0.0, 5.0, 10.0, "post");
        let rim = top_rim(&mut ctx, post, 10.0);
        let e = ctx.fillet(post, &rim, 5.0, Some("dome".into()), None).map(|_| ()).unwrap_err().to_string();
        assert!(e.contains("vanishes") && e.contains("dome") && e.contains("kernel=\"field\""), "{e}");
    }

    /// A plate whose top has two holes in it, its rim rounded: the chamfer
    /// route is closed to this (the contact loop would enclose the top's
    /// inner loops, a split truck refuses), so the rim is re-wired directly
    /// with the blends' ellipse as the joint at every corner — the same
    /// geometry as the box's rim, no boolean, the holes untouched.
    #[test]
    fn a_plate_rim_round_holes_is_re_wired_with_ellipse_joints() {
        let (w, d, t, r, rh) = (40.0, 30.0, 8.0, 2.0, 2.75);
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, w, d, t, "plate");
        let left = a_cylinder(&mut ctx, P2::new(-12.0, 0.0), -1.0, rh, t + 2.0, "left");
        let right = a_cylinder(&mut ctx, P2::new(12.0, 0.0), -1.0, rh, t + 2.0, "right");
        let part = ctx.boolean(BoolOp::Subtract, plate, &[left, right], KernelChoice::Brep, Some("part".into()), None).unwrap();
        let rim: Vec<String> = edges_named(&mut ctx, part, |e| at_z(e, t) && e.length > 10.0);
        assert_eq!(rim.len(), 4, "{rim:?}");
        let out = ctx.fillet(part, &rim, r, Some("round".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        let names: Vec<&str> = topo.faces.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names.iter().filter(|n| n.starts_with("round/blend[")).count(), 4, "{names:?}");
        for kept in ["plate/top", "plate/bottom", "left/side#0", "right/side#1"] {
            assert!(names.contains(&kept), "{names:?}");
        }
        // Four ellipse joints between blends, from the top down to `r` below it.
        let joints: Vec<&EdgeInfo> = topo.edges.iter().filter(|e| e.faces.iter().all(|n| n.starts_with("round/"))).collect();
        assert_eq!(joints.len(), 4, "{:?}", joints.iter().map(|e| &e.name).collect::<Vec<_>>());
        for j in &joints {
            assert_eq!(j.curve_kind, "nurbs");
            let zs = [j.start.z, j.end.z];
            assert!(zs.iter().any(|z| (z - t).abs() < 1e-6) && zs.iter().any(|z| (z - (t - r)).abs() < 1e-6), "{zs:?}");
        }
        // The vertical edges are trimmed to the joint, not cut through.
        let verticals: Vec<&EdgeInfo> = topo
            .edges
            .iter()
            .filter(|e| {
                (e.start.x - e.end.x).abs() < 1e-6
                    && (e.start.y - e.end.y).abs() < 1e-6
                    && (e.start.x.abs() - w / 2.0).abs() < 1e-6
                    && (e.start.y.abs() - d / 2.0).abs() < 1e-6
            })
            .collect();
        assert_eq!(verticals.len(), 4, "{:?}", verticals.iter().map(|e| (&e.name, e.length)).collect::<Vec<_>>());
        for v in &verticals {
            assert!((v.length - (t - r)).abs() < 1e-6, "{}: {}", v.name, v.length);
        }
        let per_len = (1.0 - PI / 4.0) * r * r;
        let overlap = {
            let n = 4000;
            let mut sum = 0.0;
            for i in 0..n {
                let z = r * (i as f64 + 0.5) / n as f64;
                let s = r - (r * r - (z - r) * (z - r)).max(0.0).sqrt();
                sum += s * s * (r / n as f64);
            }
            sum
        };
        let expect = w * d * t - 2.0 * PI * rh * rh * t - per_len * 2.0 * (w + d) + 4.0 * overlap;
        let v = volume(&mut ctx, out.body);
        assert_volume(v, expect, 3e-4);
    }

    /// A single edge whose ends are convex vertices: one cylinder ending on
    /// the two perpendicular side faces.
    #[test]
    fn a_single_edge_ends_on_perpendicular_faces() {
        let (w, d, h, r) = (30.0, 20.0, 10.0, 3.0);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, w, d, h, "blk");
        let front: Vec<String> = edges_named(&mut ctx, blk, |e| at_z(e, h) && e.start.y < 0.0 && (e.start.y - e.end.y).abs() < 1e-6);
        assert_eq!(front.len(), 1);
        let out = ctx.fillet(blk, &front, r, None, None).unwrap();
        assert_clean(&mut ctx, out.body);
        let v = volume(&mut ctx, out.body);
        assert_volume(v, w * d * h - (1.0 - PI / 4.0) * r * r * w, 2e-4);
        assert_eq!(names_of(&mut ctx, out.body).len(), 7);
    }

    // ------------------------------------------------------------ refusals

    #[test]
    fn a_radius_wider_than_a_neighbour_is_refused_with_the_width_that_fits() {
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 30.0, 20.0, 4.0, "blk");
        let front: Vec<String> = edges_named(&mut ctx, blk, |e| at_z(e, 4.0) && e.start.y < 0.0 && (e.start.y - e.end.y).abs() < 1e-6);
        let msg = refusal(&mut ctx, blk, &front, 5.0);
        assert_eq!(
            msg,
            "fillet: 5 mm along `blk/front` at edge `blk/front|top` is wider than the face: the setback curve leaves \
             it, so the cutter would swallow `blk/front` and the blend would end on its far edge. The largest \
             setback that fits there is about 3.88 mm"
        );
    }

    /// Two blends on one face whose contact lines meet or cross: at half the
    /// width (a full round) and at more than half the width the face would
    /// vanish between them. Each setback fits the face alone, so the
    /// one-edge check passes; the refusal names both edges, the face, the
    /// width between them and the radius that leaves some of it.
    #[test]
    fn two_blends_that_meet_on_a_face_are_refused_with_the_width_between_them() {
        for r in [5.0, 6.0] {
            let mut ctx = Context::new();
            let blk = a_box(&mut ctx, 10.0, 20.0, 30.0, "blk");
            let verticals = edges_named(&mut ctx, blk, |e| (e.start.x - e.end.x).abs() < 1e-6 && (e.start.y - e.end.y).abs() < 1e-6);
            assert_eq!(verticals.len(), 4, "{verticals:?}");
            let msg = refusal(&mut ctx, blk, &verticals, r);
            assert!(msg.starts_with("fillet: the blends at edges `blk/"), "r={r}: {msg}");
            assert!(msg.contains("both run along `blk/"), "r={r}: {msg}");
            assert!(msg.contains("10 mm apart"), "r={r}: {msg}");
            assert!(msg.contains("under 5 mm"), "r={r}: {msg}");
        }
        // Just under half the width still builds.
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 10.0, 20.0, 30.0, "blk");
        let verticals = edges_named(&mut ctx, blk, |e| (e.start.x - e.end.x).abs() < 1e-6 && (e.start.y - e.end.y).abs() < 1e-6);
        let out = ctx.fillet(blk, &verticals, 4.5, None, None).unwrap();
        assert_clean(&mut ctx, out.body);
    }

    #[test]
    fn a_rim_radius_that_reaches_a_feature_is_refused_by_name() {
        let (rad, h, r) = (10.0, 3.0, 4.0);
        let mut ctx = Context::new();
        let post = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 0.0, rad, h, "post");
        let rim = top_rim(&mut ctx, post, h);
        let msg = refusal(&mut ctx, post, &rim, r);
        assert!(msg.starts_with("fillet: a radius of 4 at edge `post/side#"), "{msg}");
        assert!(msg.contains("needs 4 mm of wall below the edge") && msg.contains("only 3.00 mm"), "{msg}");
        assert!(msg.contains("largest radius that fits is about 2.95 mm"), "{msg}");
    }

    #[test]
    fn a_hole_inside_the_blend_footprint_is_refused_by_name() {
        let (w, d, t, r) = (30.0, 30.0, 8.0, 3.0);
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, w, d, t, "plate");
        let bore = a_cylinder(&mut ctx, P2::new(0.0, 12.0), 3.0, 2.0, 10.0, "bore");
        let part = ctx.boolean(BoolOp::Subtract, plate, &[bore], KernelChoice::Brep, Some("part".into()), None).unwrap();
        // The hole's mouth, 1 mm from the back edge: a fillet of 3 on the
        // mouth reaches the rim.
        let mouth: Vec<String> = edges_named(&mut ctx, part, |e| at_z(e, t) && e.length < TAU * 2.0);
        assert_eq!(mouth.len(), 2, "{mouth:?}");
        let msg = refusal(&mut ctx, part, &mouth, r);
        assert!(msg.starts_with("fillet: a radius of 3 at edge `bore/side#"), "{msg}");
        assert!(msg.contains("would reach `plate/back|top`"), "{msg}");
        assert!(msg.contains("largest radius that keeps clear of it is about 0.9"), "{msg}");
    }

    #[test]
    fn an_edge_dying_into_a_face_is_refused_with_the_chamfer_planners_words() {
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, 40.0, 30.0, 4.0, "plate");
        let s = Sketch::new(Plane::XY.offset(2.0)).with_loop(Loop::rect(P2::new(20.0, 0.0), 8.0, 36.0));
        let flange = ctx.extrude("box", &s, None, 20.0, BTreeMap::new(), Some("flange".into()), None).unwrap()[0];
        let l = ctx.boolean(BoolOp::Union, plate, &[flange], KernelChoice::Brep, Some("l".into()), None).unwrap();
        let edge: Vec<String> =
            edges_named(&mut ctx, l, |e| at_z(e, 4.0) && e.start.y < -14.0 && e.end.y < -14.0 && e.start.x.max(e.end.x) > 10.0);
        assert_eq!(edge.len(), 1, "{edge:?}");
        let msg = refusal(&mut ctx, l, &edge, 1.0);
        assert!(msg.starts_with(&format!("fillet: edge `{}` dies into `flange/left` at (16.000, -15.000, 4.000)", edge[0])), "{msg}");
        assert!(msg.contains("Fillet the edge along `flange/left` as well"), "{msg}");
    }

    #[test]
    fn a_free_form_edge_and_a_mesh_body_are_refused() {
        let mut ctx = Context::new();
        let pts = [P2::new(10.0, 0.0), P2::new(0.0, 8.0), P2::new(-10.0, 0.0), P2::new(0.0, -8.0)];
        let s = Sketch::new(Plane::XY).with_loop(Loop::spline(&pts, 0.5).unwrap());
        let blob = ctx.extrude("extrude", &s, None, 5.0, BTreeMap::new(), Some("blob".into()), None).unwrap()[0];
        let rim = top_rim(&mut ctx, blob, 5.0);
        let msg = refusal(&mut ctx, blob, &rim[..1], 0.5);
        assert!(msg.starts_with(&format!("fillet: edge `{}` lies on `blob/side[", rim[0])), "{msg}");
        assert!(msg.contains("the blend would be a ruled surface"), "{msg}");
        let blk = a_box(&mut ctx, 10.0, 10.0, 10.0, "blk");
        let mesh = ctx.body_mesh(blk).unwrap().clone();
        let fid = ctx.new_feature("mesh", Some("m".into()), vec![], BTreeMap::new(), None).unwrap();
        let m = ctx.add_body(&fid, Body::Mesh(mesh), Some("m".into())).unwrap();
        let msg = refusal(&mut ctx, m, &[], 1.0);
        assert_eq!(
            msg,
            "fillet: `m` is a mesh body, which has no exact edges to fillet; pass kernel=\"field\" to fillet it on the \
             field path (the result is a mesh body)"
        );
    }

    /// Two blends meeting at a corner that is not a right angle, the third
    /// edge sharp: the contact points on that edge would not coincide
    /// (`r / sin θ` against `r·cot(θ/2)` from the corner), so no single
    /// mitre curve joins the blends. The shared planner refuses the corner
    /// before any mitre is built — at an obtuse corner the overrunning
    /// cutter would notch the third face, at an acute one the contact curve
    /// leaves it — and it names the edge and the face both times.
    /// `mitre_edge`'s own coincidence check stays as the backstop behind it.
    #[test]
    fn a_corner_whose_contact_points_would_differ_is_refused_by_name() {
        let mut ctx = Context::new();
        // A trapezoidal prism: 60° at A = (-15, 0), 120° at D = (-15 + 10/√3, 10).
        let s = Sketch::new(Plane::XY).with_loop(Loop::polygon(&[
            P2::new(-15.0, 0.0),
            P2::new(15.0, 0.0),
            P2::new(15.0, 10.0),
            P2::new(-15.0 + 10.0 / 3f64.sqrt(), 10.0),
        ]));
        let (h, dx) = (20.0, -15.0 + 10.0 / 3f64.sqrt());
        let blk = ctx.extrude("extrude", &s, None, h, BTreeMap::new(), Some("blk".into()), None).unwrap()[0];
        let vertical_at = |ctx: &mut Context, x: f64, y: f64| {
            edges_named(ctx, blk, |e| {
                [e.start, e.end].iter().all(|p| (p.x - x).abs() < 1e-6 && (p.y - y).abs() < 1e-6) && (e.start.z - e.end.z).abs() > 1.0
            })
        };
        // The obtuse corner D: the top rim along the back (y = 10) and the vertical edge at D.
        let back_top: Vec<String> =
            edges_named(&mut ctx, blk, |e| at_z(e, h) && (e.start.y - 10.0).abs() < 1e-6 && (e.end.y - 10.0).abs() < 1e-6);
        let at_d = vertical_at(&mut ctx, dx, 10.0);
        assert_eq!((back_top.len(), at_d.len()), (1, 1), "{back_top:?} {at_d:?}");
        let msg = refusal(&mut ctx, blk, &[back_top[0].clone(), at_d[0].clone()], 1.0);
        assert!(msg.starts_with(&format!("fillet: edge `{}` dies into `blk/side[0.3]` at ({dx:.3}, 10.000, 20.000)", back_top[0])), "{msg}");
        assert!(msg.contains("Fillet the edge along `blk/side[0.3]` as well"), "{msg}");
        // The acute corner A: the top rim along the front (y = 0) and the vertical edge at A.
        let front_top: Vec<String> =
            edges_named(&mut ctx, blk, |e| at_z(e, h) && e.start.y.abs() < 1e-6 && e.end.y.abs() < 1e-6);
        let at_a = vertical_at(&mut ctx, -15.0, 0.0);
        assert_eq!((front_top.len(), at_a.len()), (1, 1), "{front_top:?} {at_a:?}");
        let msg = refusal(&mut ctx, blk, &[front_top[0].clone(), at_a[0].clone()], 1.0);
        assert!(msg.starts_with("fillet: 1 mm along `blk/end` at edge `blk/end|side[0.0]` is wider than the face"), "{msg}");
        assert!(msg.contains("the blend would end on its far edge"), "{msg}");
    }

    /// An eighth of a ball on its own: the octant patch (a revolved quarter
    /// arc whose profile starts on the axis, so one row of the patch is a
    /// pole) closed by three quarter discs. The tessellator must close it.
    #[test]
    fn an_octant_patch_with_a_pole_tessellates_closed() {
        let (c, r) = (Vec3::new(1.0, 2.0, 3.0), 1.5);
        let (t0, t1, t2) = (Vec3::X, Vec3::Y, Vec3::Z);
        // The octant's corners: the sphere points in the -t directions.
        let (q01, q02, q12) = (c - t2 * r, c - t1 * r, c - t0 * r);
        let (v01, v02, v12) = (Vertex::new(q01.point()), Vertex::new(q02.point()), Vertex::new(q12.point()));
        let vc = Vertex::new(c.point());
        let (_, w) = fmath::sin_cos(std::f64::consts::FRAC_PI_4);
        let arc = |a: &Vertex, b: &Vertex| {
            let (pa, pb) = (kernel::point_to_vec3(a.point()), kernel::point_to_vec3(b.point()));
            Edge::new(a, b, nurbs_curve(vec![hom(pa, 1.0), hom(pa + pb - c, w), hom(pb, 1.0)], KnotVec::bezier_knot(2)))
        };
        let line = |a: &Vertex, b: &Vertex| Edge::new(a, b, Curve::Line(Line(a.point(), b.point())));
        // The arcs: equator about t0 from q01 to q02; meridians q01–q12, q02–q12.
        let (e_eq, e_m1, e_m2) = (arc(&v01, &v02), arc(&v01, &v12), arc(&v02, &v12));
        let (l01, l02, l12) = (line(&vc, &v01), line(&vc, &v02), line(&vc, &v12));
        // The patch: profile from the pole q12 to q01, swung a quarter turn about t0 onto q02.
        let profile = vec![hom(q12, 1.0), hom(q12 + q01 - c, w), hom(q01, 1.0)];
        let turn = if (rotate_about(q01, c, t0, std::f64::consts::FRAC_PI_2) - q02).length() <= 1e-9 { t0 } else { -t0 };
        let surface = revolve_profile(&profile, KnotVec::bezier_knot(2), c, turn, std::f64::consts::FRAC_PI_2).unwrap();
        let (x, n) = surface_at(&surface, 0.5, 0.5);
        // The oriented wire runs counter-clockwise about the outward normal:
        // q01 (-t2) → q02 (-t1) → q12 (-t0) does, seen from outside.
        let octant = face_oriented(vec![vec![e_eq.clone(), e_m2.clone(), e_m1.inverse()].into()], surface, n.dot((x - c).normalized()) > 0.0).unwrap();
        // The three quarter discs, each facing outward (away from the octant):
        // `a` runs c → pa, `arc` pa → pb, `b` c → pb.
        let disc = |a: &Edge, b: &Edge, arc: &Edge, outward: Vec3| {
            let (pa, pb) = (kernel::point_to_vec3(a.back().point()), kernel::point_to_vec3(b.back().point()));
            let ccw = (pa - c).cross(pb - c).dot(outward) > 0.0;
            let wire: Wire = if ccw { vec![a.clone(), arc.clone(), b.inverse()] } else { vec![b.clone(), arc.inverse(), a.inverse()] }.into();
            let plane = truck_modeling::Plane::new(c.point(), pa.point(), pb.point());
            let forward = kernel::vector_to_vec3(plane.normal()).dot(outward) > 0.0;
            face_oriented(vec![wire], Surface::Plane(plane), forward).unwrap()
        };
        // Disc in the plane ⟂ t0 (contains q01, q02): outward normal +t0.
        let d0 = disc(&l01, &l02, &e_eq, t0);
        let d1 = disc(&l01, &l12, &e_m1, t1);
        let d2 = disc(&l02, &l12, &e_m2, t2);
        let solid = solid_of(vec![octant, d0, d1, d2], "an eighth of a ball").unwrap();
        let holes = open_edge_samples(&solid, 0.05, 8);
        assert!(holes.is_empty(), "open edges: {holes:?}");
        let mb = Body::Brep(solid).to_mesh(0.001).unwrap();
        let expect = 4.0 / 3.0 * PI * r * r * r / 8.0;
        assert_volume(mb.0.volume(), expect, 2e-3);
        assert!(mb.1.faces.iter().any(|f| matches!(f.surface, SurfaceClass::Sphere { .. })), "{:?}", mb.1.faces.iter().map(|f| f.surface_kind).collect::<Vec<_>>());
    }

    #[test]
    fn the_arc_and_revolution_builders_are_exact() {
        let (pts, knots) = arc_control(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 1.5 * PI);
        assert_eq!(pts.len(), 7);
        let c = nurbs_curve(pts, knots);
        for k in 0..=20 {
            let p = kernel::point_to_vec3(c.subs(k as f64 / 20.0));
            assert!((p.length() - 1.0).abs() < 1e-12 && p.z.abs() < 1e-12, "{p:?}");
        }
        let end = kernel::point_to_vec3(c.subs(1.0));
        assert!((end - Vec3::new(0.0, -1.0, 0.0)).length() < 1e-12);
        // A quarter arc of radius 2 about (0,0,0)... revolved about z from radius 5: a torus.
        let (pts, knots) = arc_control(Vec3::new(5.0, 0.0, -2.0), Vec3::new(3.0, 0.0, -2.0), Vec3::Y, PI / 2.0);
        let s = revolve_profile(&pts, knots, Vec3::ZERO, Vec3::Z, PI).unwrap();
        let class = classify(&s, EXACT_TOL);
        let SurfaceClass::Torus { major, minor, .. } = class else { panic!("{class:?}") };
        assert!((major - 3.0).abs() < 1e-9 && (minor - 2.0).abs() < 1e-9);
        let (pts, knots) = arc_control(Vec3::new(2.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, PI / 2.0);
        let s = extrude_profile(&pts, knots, Vec3::new(0.0, 0.0, 7.0)).unwrap();
        let SurfaceClass::Cylinder { radius, .. } = classify(&s, EXACT_TOL) else { panic!() };
        assert!((radius - 2.0).abs() < 1e-9);
    }
}
