//! Mesh kernel binding: `manifold-rust` (Apache-2.0, a pure-Rust port of
//! Manifold v3.5.0). Everything that touches manifold lives here, so the rest
//! of the crate keeps talking in [`Mesh`] / [`Topology`] (docs/design.md).
//!
//! Why this exists: truck's `shapeops` cost grows with the *accumulated* face
//! count of the operands (measured: 58 → 353 ms for the six
//! successive foot unions of `examples/gridfinity_bin.star`), and it refuses
//! coincident faces outright. Manifold's boolean is BVH-accelerated and
//! topologically robust (Smith–Dodgson), so the cost follows the size of the
//! *touching* region rather than the whole body.
//!
//! What crosses the boundary:
//!
//! * **Positions and triangles.** Our [`Mesh`] is flat f32 arrays with one
//!   vertex per (position, normal, face) corner; manifold wants a welded,
//!   watertight index mesh. [`Mesh::welded`] does the welding and
//!   [`Mesh::is_watertight`] is the gate: a mesh that does not pass it
//!   is refused with a diagnostic instead of silently importing as empty.
//! * **Provenance.** Every triangle carries a `face_id`; manifold propagates
//!   it to every triangle of the result (`MeshGL::face_id`, the `faceID` of
//!   the C++ library), which is exactly the element map naming wants. We
//!   therefore *never* re-derive face groups geometrically: a face group in
//!   the result is the set of triangles whose provenance id is the same, and
//!   it keeps the name the face had before the boolean.
//!
//! Normals are not handed to manifold. They are rebuilt on the way out, one
//! vertex per (position, face group), and they are the *surface's*:
//! a vertex the boolean left where it was keeps the normal its operand had
//! ([`CarriedNormals`]); a vertex the boolean made, on an intersection
//! curve, takes the exact normal of its group's source surface, which
//! travels with the provenance — a recognised class's closed form
//! ([`SurfaceClass::normal_at`]), or the free-form patch itself, inverted
//! ([`crate::tessellate::FaceInfo::geometry`]); and a group with no surface
//! behind it (an imported or field mesh, the robust engine's regrouping) is
//! area-weighted inside the group, as every group was before. Either way a
//! cylindrical group shades smoothly while the boundary between two groups
//! stays a hard edge, which is what the B-rep tessellator does too.
//!
//! **The result is cleaned before it is named.** Manifold's output is exact,
//! which means it keeps every sliver two tessellated surfaces make where they
//! cross at a grazing angle or nearly coincide, down to its own epsilon of a
//! few nanometres. [`crate::mesh_clean`] collapses everything thinner than
//! the tessellation tolerance — the only accuracy the mesh ever had — inside
//! [`from_manifold`], before face groups, edges and vertices are derived, so
//! the renderer, the exporters, the print checks and the names all see one
//! mesh. It is the single place this happens; no consumer welds for itself.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use manifold_rust::manifold::Manifold;
use manifold_rust::types::{BooleanEngine, Error as MError, MeshGL, OpType};
use truck_modeling::{ParametricSurface, ParametricSurface3D, SPHint2D, Surface};

use crate::geom::Vec3;
use crate::kernel::{BoolOp, notes, point_to_vec3, trace, vector_to_vec3};
use crate::ksurface;
use crate::mesh_clean;
use crate::naming::{self, NamedFace, Role};
use crate::solid;
use crate::surface_class::SurfaceClass;
use crate::tangent::{self, TangentCase, TangentContact, TangentFace};
use crate::tessellate::{EdgeInfo, FaceInfo, Mesh, Topology, VertexInfo};
use crate::{CadpoleError, Result};
use crate::fmath;

/// A mesh body: triangles plus the face groups that name them.
/// `mesh.face_ids[t]` indexes `topology.faces`.
pub type MeshBody = (Mesh, Topology);

/// Weld distance used when handing a mesh to manifold, in mm. Tessellated
/// B-rep faces agree on shared edges to within float rounding, so this only
/// has to be smaller than any real feature: 0.1 µm is four orders of
/// magnitude below the tightest tolerance Cadpole accepts.
pub const WELD_EPS: f32 = 1e-4;

fn op_type(op: BoolOp) -> OpType {
    match op {
        BoolOp::Union => OpType::Add,
        BoolOp::Subtract => OpType::Subtract,
        BoolOp::Intersect => OpType::Intersect,
    }
}

/// Convert one of our meshes into a manifold solid, welding first and adding
/// `id_offset` to every provenance id so two operands can share one id space.
///
/// Fails rather than importing an empty solid when the welded mesh is not a
/// closed, consistently oriented 2-manifold: manifold would answer
/// `NotManifold`/`NotClosed` and every later operation would silently produce
/// nothing.
pub fn to_manifold(mesh: &Mesh, id_offset: u32, tolerance: f64) -> Result<Manifold> {
    if mesh.indices.is_empty() {
        return Err(CadpoleError::kernel("mesh kernel: the body has no triangles"));
    }
    let welded = mesh.welded(WELD_EPS);
    if !welded.is_watertight() {
        return Err(CadpoleError::kernel(format!(
            "mesh kernel: the body is not watertight after welding ({} triangles, {} vertices); \
             the mesh boolean needs a closed, orientable surface",
            welded.triangle_count(),
            welded.vertex_count()
        )));
    }
    // Manifold reads triangles as CCW seen from outside. Our tessellation is
    // outward already; a body that came in inside-out would otherwise be
    // read as an infinite solid with a hole in it, so flip it instead.
    let flip = welded.volume() < 0.0;

    // `mg.tolerance` is deliberately left at zero, so manifold simplifies at
    // its own floor (a few nanometres here) and the tessellation tolerance is
    // applied only by `mesh_clean` on the way out. Handing manifold the
    // tolerance instead was measured on `examples/gridfinity_bin.star`: its
    // in-boolean `simplify_topology` then removes the needles but leaves the
    // caps, and its `form_loop` answers one collapse at the bin's corner by
    // duplicating a vertex — a pinch that is a 2-manifold by index and two
    // non-manifold edges once welded, which the next boolean would silently
    // take down the soup path. `Manifold::simplify(tolerance)` on the result
    // goes further the other way, collapsing colinear edges across faces it
    // groups as coplanar at that tolerance: 5242 → 1848 triangles on the
    // bin, curved faces included, which is a shape change and not a
    // cleanup. `tolerance` is accepted so the signature says what the mesh
    // was built at; it is not forwarded.
    let _ = tolerance;
    let mut mg = MeshGL { num_prop: 3, ..MeshGL::default() };
    mg.vert_properties = welded.positions.clone();
    mg.tri_verts = Vec::with_capacity(welded.indices.len());
    for t in welded.indices.chunks_exact(3) {
        if flip {
            mg.tri_verts.extend_from_slice(&[t[0], t[2], t[1]]);
        } else {
            mg.tri_verts.extend_from_slice(t);
        }
    }
    mg.face_id = welded.face_ids.iter().map(|f| f + id_offset).collect();
    if mg.face_id.len() != welded.triangle_count() {
        // `welded` keeps face_ids and triangles in step; be explicit anyway,
        // because manifold rejects a short face_id array outright.
        mg.face_id.resize(welded.triangle_count(), id_offset);
    }

    let m = Manifold::from_mesh_gl_robust(&mg);
    if trace::enabled() {
        // Debug-only: the per-operand verdicts, ours beside manifold's. A
        // disagreement here points at a defect such as a fold; the
        // decision it leads to (which engine ran) is a note, not a trace.
        let report = solid::validate(&welded);
        let _ = trace::Span::start(format!(
            "to_manifold operand: {} | manifold says self-intersecting: {} | ours: {} crossings {} overlaps (pairs skipped: {}) | first crossing: {:?}",
            report.summary(),
            m.has_self_intersections(),
            report.crossings.len(),
            report.overlaps.len(),
            report.pairs_skipped,
            report.crossings.first().or(report.overlaps.first()),
        ));
        let _ = trace::Span::start(format!(
            "to_manifold folds: {} ; worst: {:?}",
            report.folds.len(),
            report
                .folds
                .iter()
                .take(4)
                .map(|f| format!("dot={:.4} at ({:.2},{:.2},{:.2}) tips {:.4} apart faces {}/{}", f.dot, f.at.x, f.at.y, f.at.z, f.tip_gap, f.faces.0, f.faces.1))
                .collect::<Vec<_>>()
        ));
    }
    match m.status() {
        MError::NoError => Ok(m),
        e => Err(CadpoleError::kernel(format!("mesh kernel: manifold refused the mesh: {e}"))),
    }
}

/// Manifold's own answer to "does this mesh intersect itself?", the verdict
/// its `Auto` boolean dispatch routes on: `true` sends the next boolean to
/// the robust engine, which keeps no face provenance. `None` when the mesh
/// is not one manifold can hold at all (not watertight after welding, or
/// refused on import), which is a different and louder problem.
///
/// Exposed so `cadpole inspect` can print it beside our validator's verdict.
/// The two are different predicates with different tolerances — manifold's
/// narrow phase read the impeller's fold as coincident surface and counts a
/// micron-long crossing, our validator reports the fold and ignores the
/// micron; on the slit-box fixture manifold says clean — and which one the
/// mesh kernel's `Auto` dispatch believes is otherwise invisible. Here
/// it is one line of the report.
pub fn self_intersects(mesh: &Mesh) -> Option<bool> {
    if mesh.indices.is_empty() {
        return None;
    }
    let welded = mesh.welded(WELD_EPS);
    if !welded.is_watertight() {
        return None;
    }
    let flip = welded.volume() < 0.0;
    let mut mg = MeshGL { num_prop: 3, ..MeshGL::default() };
    mg.vert_properties = welded.positions.clone();
    mg.tri_verts = Vec::with_capacity(welded.indices.len());
    for t in welded.indices.chunks_exact(3) {
        if flip {
            mg.tri_verts.extend_from_slice(&[t[0], t[2], t[1]]);
        } else {
            mg.tri_verts.extend_from_slice(t);
        }
    }
    let m = Manifold::from_mesh_gl_robust(&mg);
    match m.status() {
        MError::NoError => Some(m.has_self_intersections()),
        _ => None,
    }
}

/// Which of manifold's boolean engines a boolean runs on.
///
/// The exact engine (the port of the C++ reference) keeps face provenance,
/// so a result's faces are the operands' faces under the operands' names.
/// The rational-arithmetic robust engine keeps none: its result's face ids
/// are its own coplanar groups — one per planar patch, one per triangle on
/// a curved surface — and say nothing about which operand face a triangle
/// came from. `Auto` is manifold's own dispatch, which sends any operand its
/// narrow phase calls self-intersecting to the robust engine — and that
/// verdict fires on a fold, and on a crossing a micron long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Exact,
    Robust,
    Auto,
}

impl Engine {
    /// `"exact"`, `"robust"` or `"auto"`; anything else is `None`.
    pub fn parse(s: &str) -> Option<Engine> {
        match s {
            "exact" => Some(Engine::Exact),
            "robust" => Some(Engine::Robust),
            "auto" => Some(Engine::Auto),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Engine::Exact => "exact",
            Engine::Robust => "robust",
            Engine::Auto => "auto",
        }
    }

    fn manifold(self) -> BooleanEngine {
        match self {
            Engine::Exact => BooleanEngine::Exact,
            Engine::Robust => BooleanEngine::Robust,
            Engine::Auto => BooleanEngine::Auto,
        }
    }
}

/// The engine `CADPOLE_MESH_ENGINE` forces, for experiments, or `None` for the
/// default policy (exact first, robust only when exact refuses).
pub fn engine_from_env() -> Option<Engine> {
    std::env::var("CADPOLE_MESH_ENGINE").ok().and_then(|v| Engine::parse(&v))
}

/// Whether the shared-plane snap runs: always, unless
/// `CADPOLE_TANGENT_SNAP=0`, which is how a before/after on a whole example is
/// measured from one binary (`cadpole inspect` with and without). The
/// classification and its notes run either way. An experiment knob like
/// `CADPOLE_MESH_ENGINE`, not a setting: nothing in a document depends on it.
pub fn snap_enabled() -> bool {
    std::env::var("CADPOLE_TANGENT_SNAP").map(|v| v != "0").unwrap_or(true)
}

/// Boolean of two mesh bodies. The result's face groups keep the names of the
/// faces they came from (manifold's per-triangle provenance ids are exactly
/// the element map). A source face the boolean cut into several
/// pieces comes back as several connected components under one provenance id
/// and becomes `name#k`; so does a name two operands happen to share, which is
/// what a body united with a copy of itself produces. Both are ordered by
/// [`naming::order_pieces`], never by manifold's output order.
///
/// `tolerance` is the tessellation tolerance the operands were built at, in
/// mm; the result is cleaned of anything thinner ([`crate::mesh_clean`]).
///
/// The engine is the default policy unless `CADPOLE_MESH_ENGINE` says otherwise;
/// [`boolean_with`] takes the choice as an argument.
pub fn boolean(op: BoolOp, a: &MeshBody, b: &MeshBody, tolerance: f64) -> Result<MeshBody> {
    boolean_with(op, a, b, tolerance, engine_from_env())
}

/// [`boolean`] with the engine chosen by the caller: `None` is the policy
/// (exact first, robust only when exact refuses), `Some` forces one. An
/// argument rather than only the environment variable so a test can force
/// the robust engine without a process-global setting that every other test
/// running beside it would see.
pub fn boolean_with(op: BoolOp, a: &MeshBody, b: &MeshBody, tolerance: f64, engine: Option<Engine>) -> Result<MeshBody> {
    let _span = trace::Span::start(format!(
        "mesh {} a={}t/{}g b={}t/{}g",
        op.kind_name(),
        a.0.triangle_count(),
        a.1.faces.len(),
        b.0.triangle_count(),
        b.1.faces.len(),
    ));
    let offset = id_span(a);
    // The tangent case, stages 1 and 2: name every touching
    // contact between the operands, then make the planes they share exact
    // before manifold sees them. Classification reads the operands as they
    // came in; the snap hands back copies only when something moved.
    let eps = tangent::snap_eps(&a.0, &b.0);
    let fa = tangent::faces_of_body(a, eps);
    let fb = tangent::faces_of_body(b, eps);
    let contacts = tangent::classify(&fa, &fb, tolerance, tangent::Depth::Cheap);
    let (am, bm, snap) = if snap_enabled() {
        tangent::snap_shared_planes(&a.0, &fa, &b.0, &fb, eps)
    } else {
        (std::borrow::Cow::Borrowed(&a.0), std::borrow::Cow::Borrowed(&b.0), tangent::SnapReport::default())
    };
    let risk = say_what_touches(op, &contacts, &fa, &fb, &snap, eps, tolerance).into_iter().collect::<Vec<_>>();
    let groups = merged_groups(&a.1, &b.1, offset);
    let cause = || tangent::summary(&tangent::classify(&fa, &fb, tolerance, tangent::Depth::Full), &fa, &fb);
    // The carried normals are keyed by position, so they are entered
    // from the meshes this run hands to manifold -- snapped or as they came.
    let run = |am: &Mesh, bm: &Mesh| -> Result<MeshBody> {
        let ma = to_manifold(am, 0, tolerance)?;
        let mb = to_manifold(bm, offset, tolerance)?;
        let carried = {
            let _span = trace::Span::start(format!("carried normals a={}v b={}v", am.vertex_count(), bm.vertex_count()));
            let mut carried = CarriedNormals::new();
            carried.add_mesh(am, &a.1, 0);
            carried.add_mesh(bm, &b.1, offset);
            carried
        };
        finish(op, &ma, &mb, &groups, Some(&carried), tolerance, engine, &cause)
    };
    gated(op, &snap, &risk, || run(&am, &bm), || run(&a.0, &b.0))
}

/// Is this result what the caller's exit gate will accept: closed, wound
/// consistently, enclosing something? One weld and one edge map, the same
/// predicate `Context::check_mesh_result` applies, so "the snap made it
/// worse" means exactly "the snap turned an answer into a refusal".
fn sound(mesh: &Mesh) -> bool {
    solid::validate_with(mesh, &solid::SolidOptions { pair_limit: 0, ..Default::default() }).oriented()
}

/// The snap's gate: a boolean on the snapped operands, and when the
/// snap moved something and the result is not a solid, the same boolean on
/// the operands as they came — kept if *that* one is a solid, with a note.
///
/// Why it is needed: two spellings of one plane a hair apart are two sheets
/// manifold keeps apart, and a body that only *touches* another along an
/// edge or a line of that plane comes back as two surfaces a hair apart,
/// which is a solid by every test. Make the tie exact and the same contact
/// is a pinch — an edge where four triangles meet — and the exit gate
/// refuses it. `examples/gridfinity_baselid.star`'s foot on its slab is
/// that: a flush chamfer beside a cylinder tangent to it. The pinch is the
/// true geometry and the refusal would be honest, but the answer the script
/// had before is a solid, and the snap must not take one away: it repairs
/// spellings, and when the repair reveals a contact the kernel cannot
/// hold, the spelling that could is kept. The retry costs a second boolean,
/// only when the snap both moved something and lost the solid.
///
/// Notes: everything pushed before the gate (the snap's own note among
/// them) is kept in order; a declined snap adds why; and `risk` — the
/// contacts the snap cannot make exact, from [`say_what_touches`] — is
/// said only when the result shows their symptom: more than one shell, a
/// surface that is not a solid, or a refusal.
fn gated(
    op: BoolOp,
    snap: &tangent::SnapReport,
    risk: &[String],
    snapped: impl FnOnce() -> Result<MeshBody>,
    plain: impl FnOnce() -> Result<MeshBody>,
) -> Result<MeshBody> {
    let report_of = |m: &Mesh| solid::validate_with(m, &solid::SolidOptions { pair_limit: 0, ..Default::default() });
    let say_risk = |out: &Result<MeshBody>| {
        if risk.is_empty() {
            return;
        }
        let symptom = match out {
            Ok(body) => {
                let r = report_of(&body.0);
                !r.oriented() || r.components > 1
            }
            Err(_) => true,
        };
        if symptom {
            for n in risk {
                notes::push(n.clone());
            }
        }
    };
    if !snap.moved() {
        let out = snapped();
        say_risk(&out);
        return out;
    }
    let before = notes::drain();
    let out = snapped();
    let out_notes = notes::drain();
    let keep = |first: Vec<String>, second: Vec<String>| {
        for n in first.into_iter().chain(second) {
            notes::push(n);
        }
    };
    if let Ok(body) = &out {
        let report = report_of(&body.0);
        if !report.oriented() {
            let retry = plain();
            let retry_notes = notes::drain();
            if let Ok(again) = &retry {
                if sound(&again.0) {
                    keep(before, retry_notes);
                    notes::push(format!(
                        "mesh {}: the shared-plane snap was declined: with the planes made exact the result \
                         was not a solid ({}) -- the tie is a contact along an edge or a line, which pinches \
                         rather than joins -- so the boolean ran on the operands as they came, a hair apart \
                         there",
                        op.kind_name(),
                        report.problems().join("; ")
                    ));
                    say_risk(&retry);
                    return retry;
                }
            }
        }
    }
    keep(before, out_notes);
    say_risk(&out);
    out
}

/// The notes stage 2 owes the user before the boolean runs — what the snap
/// moved, and a touching pair the snap left alone because its gap is wider
/// than the band — and the one it may owe afterwards: the contact the snap
/// cannot make exact, a curved tangency or an edge or a corner resting in a
/// face, with the fix. That one is returned rather than pushed, because it
/// is only news when the result shows the symptom (two bodies that did not
/// merge, or a surface pinched to nothing): a cylinder tangent to its own
/// slot's side, or a foot's chamfer edge lying in the floor it also crosses,
/// unions perfectly well, and `gated` says nothing then. A coplanar contact
/// is said only through the snap note, because when nothing had to move it
/// is already an exact tie; a near miss is not searched for here (that is
/// the full classification, on a refusal).
fn say_what_touches(
    op: BoolOp,
    contacts: &[TangentContact],
    fa: &[TangentFace],
    fb: &[TangentFace],
    snap: &tangent::SnapReport,
    eps: f64,
    tolerance: f64,
) -> Option<String> {
    if trace::enabled() {
        let _ = trace::Span::start(format!(
            "tangent: {} contact(s), snap groups {} faces {} vertices {} max move {:.3e} declined {} (eps {:.3e}); first: {:?}",
            contacts.len(),
            snap.groups,
            snap.faces,
            snap.vertices,
            snap.max_move,
            snap.declined,
            eps,
            contacts.first().map(|c| tangent::describe(c, fa, fb)),
        ));
    }
    if let Some(note) = snap.note(op, eps) {
        notes::push(note);
    }
    // A touching pair whose planes are a gap apart wider than the band: not
    // snapped, by the rule, and then not joined either -- the mesh kernel
    // keeps two shells a hair apart and every check calls that a solid.
    // Said, with the number, because nothing else would say it.
    if let Some(c) = contacts.iter().find(|c| {
        matches!(c.case, TangentCase::Coplanar { sense: tangent::Sense::Opposed, .. }) && c.distance > eps && c.detail == "a gap"
    }) {
        notes::push(format!(
            "mesh {}: {} -- a gap wider than the {} snap band, so the tie is not made exact and the \
             two bodies stay apart there (a union keeps them as two shells). Move them together, \
             or overlap them by 0.5 mm",
            op.kind_name(),
            tangent::describe(c, fa, fb),
            tangent::um(eps)
        ));
    }
    let worth_a_note = |c: &&TangentContact| match c.case {
        TangentCase::CoincidentSurface { .. } | TangentCase::TangentLine { .. } | TangentCase::TangentPoint { .. } => true,
        TangentCase::EdgeOnFace { interior, .. } | TangentCase::VertexOnFace { interior, .. } => interior,
        TangentCase::Coplanar { .. } | TangentCase::NearMiss => false,
    };
    contacts.iter().find(worth_a_note).map(|c| {
        format!(
            "mesh {}: {}. {}",
            op.kind_name(),
            tangent::describe(c, fa, fb),
            tangent::fix(c, op, fa, fb, tolerance, tangent::Path::Mesh)
        )
    })
}

/// Smallest gap, in mm, at which two bounding boxes count as *separated* for
/// [`separated`]. Two solids whose boxes are this far apart cannot touch, cross
/// or share a face, so composing them adds no contact anywhere. It is two
/// orders of magnitude above [`WELD_EPS`] so a tool built by tessellating a
/// curved surface cannot creep across it through chordal sag.
pub const SEPARATION_EPS: f64 = 1e-2;

/// Are these two mesh bodies more than [`SEPARATION_EPS`] apart?
///
/// This is the mesh-side twin of [`crate::kernel::disposition`]'s `Separate`,
/// decided on bounding boxes alone because that is the half of the
/// classification that costs nothing and is all [`boolean_many`] needs of a fan
/// of tools: tools that cannot touch each other cannot cut each other, so their
/// union is their concatenation and no tool's faces can be split by another's.
///
/// Conservative in the safe direction — a pair reported *not* separated simply
/// stays on the pairwise path — and a body with no geometry is never separated
/// from anything, because there is nothing to be sure about.
pub fn separated(a: &Mesh, b: &Mesh) -> bool {
    match (a.bbox(), b.bbox()) {
        (Some(x), Some(y)) => !crate::kernel::boxes_within(x, y, SEPARATION_EPS),
        _ => false,
    }
}

/// `a` against **all** of `tools` in one boolean.
///
/// `a - t1 - t2 - ... - tn` is `a - (t1 u ... u tn)` and `a u t1 u ... u tn` is
/// `a u (t1 u ... u tn)`, so a fan of tools is one boolean against their union
/// — and when the tools are pairwise separated ([`disjoint_run`]) that union is
/// free: [`Manifold::compose`] concatenates them, halfedges and provenance ids
/// and all, without a boolean.
///
/// **The caller establishes that**, tool by tool with [`separated`], because it
/// is walking the operand list anyway and can stop at the first tool that
/// touches an earlier one. Handing this overlapping tools is not checked and is
/// wrong: `compose` would make a self-intersecting solid out of them, which
/// manifold would answer correctly but by a different route and with different
/// triangles.
///
/// **Why this is the whole optimisation.** A pairwise fold pays, at *every*
/// step, for the accumulated body rather than for the cut: manifold's `Auto`
/// dispatch scans each operand for self-intersections to choose between its
/// exact and its rational-arithmetic engine, that verdict is cached on the
/// solid it was computed for, and a fold throws the solid away and rebuilds it
/// from triangles every step. Subtracting 32 magnet sockets from a lid spent
/// 82% of six seconds re-answering "does this lid intersect itself" 32 times.
/// One boolean asks once.
///
/// **Order is preserved, deliberately.** Provenance ids are handed out in
/// operand order — `a` first, then each tool — which is the order the fold's
/// repeated re-basing produces, and [`from_manifold`] emits face groups in
/// provenance order. The result's faces therefore come back in the same order
/// under the same names as the fold's.
pub fn boolean_many(op: BoolOp, a: &MeshBody, tools: &[&MeshBody], tolerance: f64) -> Result<MeshBody> {
    boolean_many_with(op, a, tools, tolerance, engine_from_env())
}

/// [`boolean_many`] with the engine chosen by the caller; see [`boolean_with`].
pub fn boolean_many_with(
    op: BoolOp,
    a: &MeshBody,
    tools: &[&MeshBody],
    tolerance: f64,
    engine: Option<Engine>,
) -> Result<MeshBody> {
    match tools {
        [] => return Err(CadpoleError::kernel("mesh kernel: a boolean needs at least one tool")),
        [one] => return boolean_with(op, a, one, tolerance, engine),
        _ => {}
    }
    let _span = trace::Span::start(format!(
        "mesh {} a={}t/{}g x{} tools={}t",
        op.kind_name(),
        a.0.triangle_count(),
        a.1.faces.len(),
        tools.len(),
        tools.iter().map(|t| t.0.triangle_count()).sum::<usize>(),
    ));
    let mut groups: Vec<SourceFace> = Vec::new();
    push_groups(&mut groups, &a.1, id_span(a));
    // The tangent pre-pass, tool by tool: each tool is classified
    // against `a` and snapped onto `a`'s planes; `a` never moves, so the
    // batched boolean sees exactly the `a` the fold would, and the tools
    // cannot touch each other (the caller established that), so no tool is
    // snapped to another.
    let eps = tools.iter().fold(tangent::snap_eps(&a.0, &a.0), |e, t| e.max(tangent::snap_eps(&a.0, &t.0)));
    let fa = tangent::faces_of_body(a, eps);
    let mut tool_meshes: Vec<Mesh> = Vec::with_capacity(tools.len());
    let mut tool_faces: Vec<Vec<TangentFace>> = Vec::with_capacity(tools.len());
    let mut moved_any = tangent::SnapReport::default();
    let mut risk: Vec<String> = Vec::new();
    for t in tools {
        let ft = tangent::faces_of_body(t, eps);
        let contacts = tangent::classify(&fa, &ft, tolerance, tangent::Depth::Cheap);
        let (_, tm, snap) = if snap_enabled() {
            tangent::snap_shared_planes(&a.0, &fa, &t.0, &ft, eps)
        } else {
            (std::borrow::Cow::Borrowed(&a.0), std::borrow::Cow::Borrowed(&t.0), tangent::SnapReport::default())
        };
        risk.extend(say_what_touches(op, &contacts, &fa, &ft, &snap, eps, tolerance));
        moved_any.vertices += snap.vertices;
        tool_meshes.push(tm.into_owned());
        tool_faces.push(ft);
        push_groups(&mut groups, &t.1, id_span(t));
    }
    let cause = || {
        tool_faces
            .iter()
            .find_map(|ft| tangent::summary(&tangent::classify(&fa, ft, tolerance, tangent::Depth::Full), &fa, ft))
    };
    let run = |am: &Mesh, tms: &[&Mesh]| -> Result<MeshBody> {
        let ma = to_manifold(am, 0, tolerance)?;
        let mut carried = CarriedNormals::new();
        carried.add_mesh(am, &a.1, 0);
        let mut offset = id_span(a);
        let mut parts: Vec<Manifold> = Vec::with_capacity(tools.len());
        for (t, tm) in tools.iter().zip(tms) {
            parts.push(to_manifold(tm, offset, tolerance)?);
            carried.add_mesh(tm, &t.1, offset);
            offset += id_span(t);
        }
        let tool = Manifold::compose(&parts);
        match tool.status() {
            MError::NoError => {}
            e => {
                return Err(CadpoleError::kernel(format!(
                    "mesh kernel: the {} tools could not be combined: {e}",
                    tools.len()
                )));
            }
        }
        finish(op, &ma, &tool, &groups, Some(&carried), tolerance, engine, &cause)
    };
    let snapped_tools: Vec<&Mesh> = tool_meshes.iter().collect();
    let plain_tools: Vec<&Mesh> = tools.iter().map(|t| &t.0).collect();
    gated(op, &moved_any, &risk, || run(&a.0, &snapped_tools), || run(&a.0, &plain_tools))
}

/// Every one of `parts` as one body, when they neither touch nor cross:
/// the parts are laid out in one provenance id space in order, as
/// [`boolean_many`] lays out its tools, composed by [`Manifold::compose`] —
/// a concatenation, no boolean — and the result is checked twice: manifold
/// says whether any triangle of one part meets a triangle of another
/// (`has_self_intersections`, the test its exact engine's precondition
/// rests on), and the welded result must still be a closed, consistently
/// oriented surface, which two parts sharing a face or an edge are not,
/// because the weld joins them into a non-manifold edge. Either finding
/// answers `None`, and the caller's pairwise path — snaps, notes and all —
/// takes over; a `Some` is exactly what that path would have produced for
/// disjoint parts, with one clean and one naming pass instead of one per
/// step. A rigid copy cannot lie inside another copy of itself, so
/// containment, which neither check would see, does not arise for the
/// pattern this serves.
pub fn compose_disjoint(parts: &[&MeshBody], tolerance: f64) -> Result<Option<MeshBody>> {
    let [first, rest @ ..] = parts else {
        return Err(CadpoleError::kernel("mesh kernel: a compose needs at least one part"));
    };
    if rest.is_empty() {
        return Ok(Some((*first).clone()));
    }
    let _span = trace::Span::start(format!(
        "mesh compose x{} parts={}t",
        parts.len(),
        parts.iter().map(|p| p.0.triangle_count()).sum::<usize>(),
    ));
    // Notes pushed here belong to the composed result only: when the
    // parts turn out to touch, the pairwise path says its own.
    let before = notes::drain();
    let mut groups: Vec<SourceFace> = Vec::new();
    let mut manifolds: Vec<Manifold> = Vec::with_capacity(parts.len());
    let mut carried = CarriedNormals::new();
    let mut offset = 0u32;
    let built = (|| -> Result<Option<MeshBody>> {
        for p in parts {
            manifolds.push(to_manifold(&p.0, offset, tolerance)?);
            carried.add_mesh(&p.0, &p.1, offset);
            push_groups(&mut groups, &p.1, id_span(p));
            offset += id_span(p);
        }
        let composed = Manifold::compose(&manifolds);
        match composed.status() {
            MError::NoError => {}
            e => {
                return Err(CadpoleError::kernel(format!(
                    "mesh kernel: the {} parts could not be combined: {e}",
                    parts.len()
                )));
            }
        }
        if composed.has_self_intersections() {
            return Ok(None);
        }
        let out = from_manifold_with(&composed, &groups, Some(&carried), tolerance)?;
        Ok(sound(&out.0).then_some(out))
    })();
    let after = notes::drain();
    for n in before {
        notes::push(n);
    }
    if matches!(built, Ok(Some(_))) {
        for n in after {
            notes::push(n);
        }
    }
    built
}

/// The id space one operand occupies: its face groups, or one past the largest
/// provenance id its triangles carry when an imported mesh has more of those
/// than the topology has faces. Exactly what [`boolean`] uses as its offset,
/// so the batched path lays operands out in the order the fold does.
fn id_span(b: &MeshBody) -> u32 {
    id_span_of(&b.0, &b.1)
}

/// [`id_span`] of a mesh and its topology held apart.
pub(crate) fn id_span_of(mesh: &Mesh, topo: &Topology) -> u32 {
    topo.faces.len().max(max_face_id(mesh) + 1) as u32
}

/// Append one operand's named faces to the id table, padding to its id span.
pub(crate) fn push_groups(out: &mut Vec<SourceFace>, t: &Topology, span: u32) {
    let base = out.len();
    for f in &t.faces {
        out.push(SourceFace {
            name: f.name.clone(),
            aliases: f.aliases.clone(),
            surface_kind: f.surface_kind,
            surface: f.surface.clone(),
            geometry: f.geometry.clone(),
        });
    }
    // A body may carry face ids past the end of its face list (an imported
    // mesh); keep the id space dense so indexing is total.
    while out.len() < base + span as usize {
        out.push(SourceFace::unnamed(out.len()));
    }
}

/// Run the boolean and rebuild one of our meshes from it. Shared by
/// [`boolean`] and [`boolean_many`] so the two cannot drift apart in engine
/// choice, error wording or naming. `cause` is the tangent classification
/// of the operands at full depth, asked for only when the exact engine
/// refuses, so the note that says the names are gone can also say which
/// contact most likely cost them.
fn finish(
    op: BoolOp,
    ma: &Manifold,
    mb: &Manifold,
    groups: &[SourceFace],
    carried: Option<&CarriedNormals>,
    tolerance: f64,
    forced: Option<Engine>,
    cause: &dyn Fn() -> Option<String>,
) -> Result<MeshBody> {
    // Exact engine first, robust engine only when exact refuses. The exact
    // engine (the port of the C++ reference) keeps face provenance; the
    // rational-arithmetic robust engine keeps none, so every triangle of its
    // result is its own face. manifold's `Auto` sends any operand its narrow
    // phase calls self-intersecting to the robust engine -- and that verdict
    // fires on crossings a micron long, which is what two tangent-continuous
    // patches tessellated independently do next to their shared edge. The
    // exact engine handles those without complaint: the impeller's wheel came
    // back with 443 faces from it and 160,857 from `Auto`. `forced` (from
    // `CADPOLE_MESH_ENGINE`, or a caller) overrides, for experiments.
    let mut out = ma.boolean_with_engine(mb, op_type(op), forced.unwrap_or(Engine::Exact).manifold());
    // Whether the result came from the engine that keeps no provenance, and
    // why. Said to the user either way, because the consequence is the same
    // and is invisible otherwise: one face per triangle, and every name on
    // the operands gone. Under `Auto` manifold does not say which engine it
    // chose, so that note says what would have decided it.
    // (how it came to run there, whether that is certain). `Auto` does not
    // say which engine it chose, so its note is conditional and its names
    // are left as they are: blanking them would lose the names on every
    // union `Auto` ran exactly, to protect against the ones it did not.
    let mut robust: Option<(String, bool)> = match forced {
        Some(Engine::Robust) => Some(("was forced onto the robust engine (CADPOLE_MESH_ENGINE=robust)".to_owned(), true)),
        Some(Engine::Auto) => Some((
            "ran with manifold's own engine choice (CADPOLE_MESH_ENGINE=auto), which uses the robust \
             engine whenever it calls an operand self-intersecting -- a fold, or a crossing a \
             micron long, both count -- and the exact engine otherwise; if it chose the robust \
             engine, this body"
                .to_owned(),
            false,
        )),
        _ => None,
    };
    if forced.is_none() && !matches!(out.status(), MError::NoError) {
        let refused = out.status();
        let _span = trace::Span::start(format!(
            "mesh {}: the exact engine refused ({refused}); retrying on the robust engine",
            op.kind_name(),
        ));
        out = ma.boolean_with_engine(mb, op_type(op), BooleanEngine::Robust);
        robust = Some((format!("was refused by the exact engine ({refused}) and ran on the robust engine"), true));
    }
    match out.status() {
        MError::NoError => {}
        e => {
            return Err(CadpoleError::kernel(format!("mesh {} failed: {e}", op.kind_name())));
        }
    }
    if out.num_tri() == 0 {
        return Err(CadpoleError::kernel(format!(
            "mesh {} produced an empty body (the operands may not overlap)",
            op.kind_name()
        )));
    }
    let groups: &[SourceFace] = match robust {
        Some((how, certain)) => {
            // The refusal is manifold's; the contact behind it is ours to
            // name (stage 2): the operands are classified at full
            // depth only now, on the path that pays for it anyway.
            let contact = if forced.is_none() {
                cause().map(|c| format!(". The contact most likely behind the refusal: {c}")).unwrap_or_default()
            } else {
                String::new()
            };
            notes::push(format!(
                "mesh {op} {how}. The robust engine keeps no face provenance: its result's faces are \
                 regrouped by flatness -- one face per planar patch, and on a curved surface one \
                 face per triangle -- and named face:N, so the names this body's operands had do \
                 not survive; `body.faces(...)` selections, fillets by face and STEP export of this \
                 body will not find them. The exact engine refuses an operand that crosses itself, \
                 even by a micron, or carries a fold (two adjacent triangles facing opposite ways); \
                 `cadpole inspect` shows which body and where{contact}",
                op = op.kind_name()
            ));
            // The ids the robust engine emits are its own coplanar groups,
            // not the provenance ids the operands went in with; looking them
            // up in the operands' name table would hand out operand names at
            // random. Index names say what is true.
            if certain { &[] } else { groups }
        }
        None => groups,
    };
    let body = from_manifold_with(&out, groups, carried, tolerance)?;
    if op == BoolOp::Union {
        return union_gate(&out, groups, carried, tolerance, body);
    }
    Ok(body)
}

/// Above this many triangles the union gate's pair search is skipped, as
/// the solid validator skips it ([`solid::SolidOptions::pair_limit`]):
/// unknown is not a defect, and a minute on a check nobody asked for is.
pub const UNION_PAIR_LIMIT: usize = 250_000;

/// The union's exit gate: a union never hands back a surface that
/// passes through itself or doubles a face without saying so.
///
/// The gate after every mesh boolean judges topology only -- closed,
/// manifold, wound consistently, enclosing something -- because crossings
/// are what the field path's fillet band makes by construction and the
/// search is the expensive half. A union is not that path, and its answer
/// is what a print script hands a slicer as one part: a union of boxes can
/// come back watertight but not a solid -- triangles a millimetre through
/// each other, faces doubled. So a union's result is also
/// searched for crossings and overlaps. When it has them, the cleanup is
/// the usual cause (collapsing a sliver thinner than the tolerance moves a
/// vertex, and a long sliver's neighbours can end up through each other),
/// so the same answer is rebuilt without it and kept, with a note, if that
/// is a solid; otherwise the union is refused, saying where.
fn union_gate(
    out: &Manifold,
    groups: &[SourceFace],
    carried: Option<&CarriedNormals>,
    tolerance: f64,
    body: MeshBody,
) -> Result<MeshBody> {
    let opts = solid::SolidOptions { pair_limit: UNION_PAIR_LIMIT, ..Default::default() };
    let report = solid::validate_with(&body.0, &opts);
    if !report.oriented() || report.embedded() {
        // Topology is the caller's exit gate's to judge, in its words.
        return Ok(body);
    }
    let raw = from_manifold_opts(out, groups, carried, tolerance, false)?;
    let again = solid::validate_with(&raw.0, &opts);
    if again.is_solid() {
        notes::push(format!(
            "mesh union: cleaning the result's slivers left a surface that passes through itself ({}), so the \
             slivers are kept: {} triangles instead of {}, a solid",
            report.problems().join("; "),
            raw.0.triangle_count(),
            body.0.triangle_count()
        ));
        return Ok(raw);
    }
    let at = report
        .crossings
        .first()
        .map(|c| c.at)
        .or(report.overlaps.first().map(|c| c.at))
        .map(|p| format!(" near ({:.3}, {:.3}, {:.3})", p.x, p.y, p.z))
        .unwrap_or_default();
    Err(CadpoleError::kernel(format!(
        "mesh union produced a surface that is not a solid{at}: {}. It is closed, but it passes through itself or \
         doubles a face, so a slicer would have to guess at its inside: the operands meet in faces that are nearly \
         but not exactly coplanar, or one of them already crossed itself. Overlap them by more where they meet; for \
         axis-aligned boxes, union(..., weld=) joins them exactly",
        report.problems().join("; ")
    )))
}

fn max_face_id(m: &Mesh) -> usize {
    m.face_ids.iter().copied().max().unwrap_or(0) as usize
}

/// One entry per provenance id: the name the source face carried, its aliases,
/// its surface kind, the analytic surface it was recognised as and, when it
/// was not, its own surface (the triangles of a face group are still on the
/// source's surface, so the surface is provenance like the name, and the
/// result's vertex normals are evaluated on it). Index is the id; `b`'s ids
/// start at `offset`. Duplicate names are *not* renamed here —
/// [`Topology::apply_names`] gives every face that ends up sharing a name its
/// canonical `#k` piece index.
#[derive(Clone, Debug)]
pub struct SourceFace {
    pub name: String,
    pub aliases: Vec<String>,
    pub surface_kind: &'static str,
    pub surface: SurfaceClass,
    /// [`FaceInfo::geometry`]: the free-form patch a class could not name.
    pub geometry: Option<Arc<Surface>>,
}

impl SourceFace {
    /// The entry for a provenance id no operand face stands behind: the
    /// index name, and no surface.
    fn unnamed(id: usize) -> SourceFace {
        SourceFace {
            name: naming::Kind::Face.index_name(id),
            aliases: Vec::new(),
            surface_kind: "group",
            surface: SurfaceClass::Other,
            geometry: None,
        }
    }
}

fn merged_groups(a: &Topology, b: &Topology, offset: u32) -> Vec<SourceFace> {
    let mut out: Vec<SourceFace> = Vec::with_capacity(offset as usize + b.faces.len());
    push_groups(&mut out, a, offset);
    push_groups(&mut out, b, b.faces.len() as u32);
    out
}

/// Triangles that share an edge *and* a provenance id are one face. A boolean
/// that cuts a source face in two leaves two components under one id: those
/// are the pieces docs/design.md calls `split`. Returns a dense component id per
/// triangle.
fn components(verts: &[[u32; 3]], prov: &[u32]) -> Vec<u32> {
    let n = verts.len();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    fn find(parent: &mut [u32], mut i: u32) -> u32 {
        while parent[i as usize] != i {
            parent[i as usize] = parent[parent[i as usize] as usize];
            i = parent[i as usize];
        }
        i
    }
    let mut per_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::with_capacity(n * 3 / 2);
    for (i, t) in verts.iter().enumerate() {
        for k in 0..3 {
            let (u, v) = (t[k], t[(k + 1) % 3]);
            per_edge.entry(if u < v { (u, v) } else { (v, u) }).or_default().push(i);
        }
    }
    for tris in per_edge.values() {
        for w in tris.windows(2) {
            if prov[w[0]] != prov[w[1]] {
                continue;
            }
            let (ra, rb) = (find(&mut parent, w[0] as u32), find(&mut parent, w[1] as u32));
            if ra != rb {
                parent[ra as usize] = rb;
            }
        }
    }
    let mut dense: HashMap<u32, u32> = HashMap::new();
    (0..n)
        .map(|i| {
            let r = find(&mut parent, i as u32);
            let next = dense.len() as u32;
            *dense.entry(r).or_insert(next)
        })
        .collect()
}


/// One triangle spelled out for the trace: corners, edge lengths, altitude.
fn describe_tri(pos: &[Vec3], t: &[u32; 3]) -> String {
    let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
    let area2 = (b - a).cross(c - a).length();
    let edges = [(b - a).length(), (c - b).length(), (a - c).length()];
    let longest = edges.iter().cloned().fold(0.0, f64::max);
    format!(
        "[{:?} ({:.3},{:.3},{:.3}) ({:.3},{:.3},{:.3}) ({:.3},{:.3},{:.3}) edges {:.4}/{:.4}/{:.4} min-altitude {:.5}]",
        t, a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z, edges[0], edges[1], edges[2],
        if longest > 0.0 { area2 / longest } else { 0.0 }
    )
}

/// Rebuild one of our meshes (plus its named topology) from a manifold solid.
/// `groups` names provenance ids; ids without an entry get `face:<id>`.
/// `tolerance` is the tessellation tolerance in mm: triangles thinner than it
/// are collapsed first (see the module notes and [`crate::mesh_clean`]).
/// Every vertex normal is evaluated on its group's surface (module notes);
/// [`from_manifold_with`] also takes the operands' own normals.
pub fn from_manifold(m: &Manifold, groups: &[SourceFace], tolerance: f64) -> Result<MeshBody> {
    from_manifold_with(m, groups, None, tolerance)
}

/// [`from_manifold`], with the normals the operands brought in: a vertex the
/// boolean left in place takes its operand's normal from `carried` before
/// anything is evaluated.
pub fn from_manifold_with(
    m: &Manifold,
    groups: &[SourceFace],
    carried: Option<&CarriedNormals>,
    tolerance: f64,
) -> Result<MeshBody> {
    from_manifold_opts(m, groups, carried, tolerance, true)
}

/// [`from_manifold_with`], with the cleanup optional: `clean = false` keeps
/// every sliver manifold made, which is what the union gate falls
/// back to when the cleanup's collapses left a surface that crosses itself.
fn from_manifold_opts(
    m: &Manifold,
    groups: &[SourceFace],
    carried: Option<&CarriedNormals>,
    tolerance: f64,
    clean: bool,
) -> Result<MeshBody> {
    let mg = m.get_mesh_gl(-1);
    let stride = mg.num_prop as usize;
    if stride < 3 {
        return Err(CadpoleError::kernel("mesh kernel: manifold returned a mesh without positions"));
    }
    let nv = mg.num_vert();
    let pos: Vec<Vec3> = (0..nv)
        .map(|i| {
            let o = i * stride;
            Vec3::new(
                mg.vert_properties[o] as f64,
                mg.vert_properties[o + 1] as f64,
                mg.vert_properties[o + 2] as f64,
            )
        })
        .collect();
    let ntri = mg.num_tri();

    // Provenance id per triangle, then the connected components inside each
    // id: one component is one face of the result, so a source face the
    // boolean cut in two gives two. Components are ordered by provenance id
    // and then by `naming::order_pieces`, never by manifold's output order.
    let mut raw: Vec<u32> = (0..ntri).map(|t| mg.face_id.get(t).copied().unwrap_or(0)).collect();
    let mut src_verts: Vec<[u32; 3]> = (0..ntri)
        .map(|t| {
            let v = mg.get_tri_verts(t);
            [v[0], v[1], v[2]]
        })
        .collect();

    // Clean here, on manifold's shared-vertex triangles and before anything
    // reads them: the face pieces, their areas and centroids (which order the
    // `#k` names), the edge chains and the vertex normals are all derived
    // from what is left, so a sliver never becomes a face, an edge or a
    // crease. Manifold's positions are already welded — one vertex per
    // point — which is what lets every operation be a topological edit.
    let folds_before = if trace::enabled() && clean { solid::folds(&pos, &src_verts, &raw) } else { Vec::new() };
    if clean {
        let _span = trace::Span::start(format!("mesh clean {ntri}t tol={tolerance}"));
        let report = mesh_clean::clean(&pos, &mut src_verts, &mut raw, tolerance);
        if trace::enabled() {
            // Debug-only: the fold census before and after, with the worst
            // pairs spelled out. What the clean did about them, when it did
            // not finish, is the note below.
            let after = solid::folds(&pos, &src_verts, &raw);
            let _ = trace::Span::start(format!(
                "from_manifold folds: {} from manifold, {} after clean; worst after: {:?}; clean report {report:?}",
                folds_before.len(),
                after.len(),
                after
                    .iter()
                    .take(3)
                    .map(|f| format!(
                        "dot={:.4} tips {:.4} apart {} vs {}",
                        f.dot,
                        f.tip_gap,
                        describe_tri(&pos, &src_verts[f.tris.0 as usize]),
                        describe_tri(&pos, &src_verts[f.tris.1 as usize])
                    ))
                    .collect::<Vec<_>>()
            ));
        }
        // A clean that stopped early or gave up is the user's business: the
        // body keeps what it would have removed, and the next boolean on it
        // may pay for that. `Context` attaches this to the feature.
        if let Some(note) = report.note() {
            notes::push(note);
        }
    }
    from_indexed(&pos, src_verts, raw, groups, carried, tolerance)
}

/// The tail of [`from_manifold_with`], shared with every other producer of
/// an index mesh that carries a provenance id per triangle -- the exact
/// union of boxes (`crate::gridjoin`) is the other: welded
/// positions, triangles wound outward, an id per triangle indexing
/// `groups`, already clean. One face per connected run of triangles under
/// one id, ordered by id and then by `naming::order_pieces`; every vertex
/// normal from its group's surface; edges and vertices named from the
/// faces. Nothing is cleaned here.
pub fn from_indexed(
    pos: &[Vec3],
    src_verts: Vec<[u32; 3]>,
    raw: Vec<u32>,
    groups: &[SourceFace],
    carried: Option<&CarriedNormals>,
    tolerance: f64,
) -> Result<MeshBody> {
    let nv = pos.len();
    let ntri = src_verts.len();
    let comp = components(&src_verts, &raw);
    let ncomp = comp.iter().copied().max().map(|m| m as usize + 1).unwrap_or(0);
    if trace::enabled() {
        let ids: std::collections::HashSet<u32> = raw.iter().copied().collect();
        let mut seen = std::collections::HashSet::with_capacity(nv);
        let mut used = std::collections::HashSet::with_capacity(nv);
        for t in &src_verts {
            for &v in t {
                used.insert(v);
                let p = pos[v as usize];
                seen.insert((p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
            }
        }
        let _ = trace::Span::start(format!(
            "from_manifold: {ntri}t, {} provenance ids (max {}), {ncomp} components, {} vertices used at {} distinct positions, {} named groups",
            ids.len(),
            ids.iter().max().copied().unwrap_or(0),
            used.len(),
            seen.len(),
            groups.len()
        ));
    }
    let mut c_area = vec![0.0f64; ncomp];
    let mut c_centroid = vec![Vec3::ZERO; ncomp];
    let mut c_normal = vec![Vec3::ZERO; ncomp];
    let mut c_prov = vec![0u32; ncomp];
    for t in 0..ntri {
        let c = comp[t] as usize;
        let v = src_verts[t];
        let (a, b, cc) = (pos[v[0] as usize], pos[v[1] as usize], pos[v[2] as usize]);
        let n = (b - a).cross(cc - a);
        let area = n.length() / 2.0;
        c_area[c] += area;
        c_centroid[c] = c_centroid[c] + (a + b + cc) * (area / 3.0);
        c_normal[c] = c_normal[c] + n;
        c_prov[c] = raw[t];
    }
    for c in 0..ncomp {
        if c_area[c] > 0.0 {
            c_centroid[c] = c_centroid[c] * (1.0 / c_area[c]);
        }
    }
    let mut by_prov: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (c, p) in c_prov.iter().enumerate() {
        by_prov.entry(*p).or_default().push(c);
    }
    let mut group_of_comp = vec![0u32; ncomp];
    let mut group_comp: Vec<usize> = Vec::with_capacity(ncomp);
    for comps in by_prov.values() {
        let pieces: Vec<(Vec3, Vec3, f64)> =
            comps.iter().map(|&c| (c_centroid[c], c_normal[c].normalized(), c_area[c])).collect();
        for &pos in naming::order_pieces(&pieces).iter() {
            let c = comps[pos];
            group_of_comp[c] = group_comp.len() as u32;
            group_comp.push(c);
        }
    }

    // Triangles sorted by group so every face owns a contiguous run, with the
    // manifold order preserved inside a group.
    let mut order: Vec<usize> = (0..ntri).collect();
    order.sort_by_key(|&t| (group_of_comp[comp[t] as usize], t));

    let tri_verts: Vec<[u32; 3]> = order.iter().map(|&t| src_verts[t]).collect();
    let tri_group: Vec<u32> = order.iter().map(|&t| group_of_comp[comp[t] as usize]).collect();
    let tri_normal: Vec<Vec3> = tri_verts
        .iter()
        .map(|t| {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            (b - a).cross(c - a)
        })
        .collect();

    // The winding at every (vertex, group): the sum of the group's triangle
    // normals there. It orients the surface's normal, and it *is* the normal
    // — area-weighted — for a group with no surface.
    let mut acc: HashMap<(u32, u32), Vec3> = HashMap::with_capacity(tri_verts.len());
    for (i, t) in tri_verts.iter().enumerate() {
        for &v in t {
            let e = acc.entry((v, tri_group[i])).or_insert(Vec3::ZERO);
            *e = *e + tri_normal[i];
        }
    }

    let ngroups = group_comp.len();
    // One source per group, looked up once: its name, and the surface its
    // vertex normals are evaluated on.
    let prov_of = |g: u32| -> u32 { group_comp.get(g as usize).map(|&c| c_prov[c]).unwrap_or(g) };
    let sources: Vec<SourceFace> = (0..ngroups as u32)
        .map(|g| {
            let id = prov_of(g);
            groups.get(id as usize).cloned().unwrap_or_else(|| SourceFace::unnamed(id as usize))
        })
        .collect();
    let mut on_surface: Vec<GroupSurface> =
        sources.iter().map(|s| GroupSurface::new(&s.surface, s.geometry.as_deref())).collect();
    // A vertex counts as on its surface within the accuracy the mesh ever
    // had: manifold's new vertices lie on the operands' *triangles*, which
    // are chords of the surface, a tolerance off it and not a weld epsilon.
    let accept = tolerance.max(WELD_EPS as f64);
    let mut stats = NormalStats::default();
    let vertex_pass = trace::Span::start(format!("from_manifold vertices: {ntri}t in {ngroups} groups"));

    let mut mesh = Mesh::default();
    let mut remap: HashMap<(u32, u32), u32> = HashMap::with_capacity(acc.len());
    let mut faces: Vec<FaceInfo> = Vec::with_capacity(ngroups);
    let mut area = 0.0f64;
    let mut centroid = Vec3::ZERO;
    let mut normal_acc = Vec3::ZERO;
    let mut tri_start = 0u32;
    let mut current = tri_group.first().copied().unwrap_or(0);

    for (i, t) in tri_verts.iter().enumerate() {
        let g = tri_group[i];
        if g != current {
            faces.push(finish_face(&sources[current as usize], tri_start, mesh.triangle_count() as u32, area, centroid, normal_acc));
            tri_start = mesh.triangle_count() as u32;
            area = 0.0;
            centroid = Vec3::ZERO;
            normal_acc = Vec3::ZERO;
            current = g;
        }
        let mut ids3 = [0u32; 3];
        for (k, &v) in t.iter().enumerate() {
            ids3[k] = *remap.entry((v, g)).or_insert_with(|| {
                let p = pos[v as usize];
                let winding = acc.get(&(v, g)).copied().unwrap_or(Vec3::ZERO);
                let n = vertex_normal(&mut on_surface[g as usize], carried, prov_of(g), p, winding, accept, &mut stats);
                mesh.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
                mesh.normals.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
                (mesh.positions.len() / 3 - 1) as u32
            });
        }
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let tri_area = tri_normal[i].length() / 2.0;
        area += tri_area;
        centroid = centroid + (a + b + c) * (tri_area / 3.0);
        normal_acc = normal_acc + tri_normal[i];
        mesh.indices.extend_from_slice(&ids3);
        mesh.face_ids.push(g);
    }
    if !tri_verts.is_empty() {
        faces.push(finish_face(&sources[current as usize], tri_start, mesh.triangle_count() as u32, area, centroid, normal_acc));
    }
    drop(vertex_pass);
    if trace::enabled() {
        let _ = trace::Span::start(format!("from_manifold normals: {}", stats.summary()));
    }

    let mut topo = Topology { faces, edges: Vec::new(), vertices: Vec::new() };
    build_edges(&pos, &tri_verts, &tri_group, &tri_normal, &mut topo);
    // Manifold's provenance carried the names this far; `apply_names` gives
    // the pieces of a split (or of a name two operands shared) their canonical
    // `#k` index, and derives every edge and vertex name from the face names.
    let named: Vec<NamedFace> = sources
        .iter()
        .take(topo.faces.len())
        .map(|s| NamedFace { name: s.name.clone(), aliases: s.aliases.clone(), role: Role::Kept })
        .collect();
    topo.apply_names(named);
    Ok((mesh, topo))
}

fn finish_face(source: &SourceFace, tri_start: u32, tri_end: u32, area: f64, centroid: Vec3, normal_acc: Vec3) -> FaceInfo {
    FaceInfo {
        name: source.name.clone(),
        aliases: Vec::new(),
        surface_kind: source.surface_kind,
        surface: source.surface.clone(),
        geometry: source.geometry.clone(),
        centroid: if area > 0.0 { centroid * (1.0 / area) } else { centroid },
        normal: normal_acc.normalized(),
        area,
        tri_start,
        tri_count: tri_end - tri_start,
        edges: Vec::new(),
        edge_ids: Vec::new(),
        v_dir: Vec3::ZERO,
    }
}

// ------------------------------------------------------------ vertex normals

/// The vertex normals a boolean's operands brought in, keyed by position and
/// provenance id. A boolean leaves nearly every vertex where it was — only
/// the intersection curves are new — and such a vertex's normal is still the
/// one its operand had: truck's evaluation on a tessellated B-rep face, or
/// what an earlier boolean gave it. Looking it up is a hash probe; evaluating
/// it again would be an inversion on a free-form group, per vertex, per
/// boolean of a chain.
///
/// Only faces with a surface behind them are entered, so a group that has
/// none (an imported or field mesh) is area-weighted afresh, whatever normals
/// its mesh carried.
pub struct CarriedNormals {
    cells: HashMap<(i64, i64, i64, u32), Vec<(Vec3, Vec3)>>,
    len: usize,
}

impl Default for CarriedNormals {
    fn default() -> Self {
        Self::new()
    }
}

impl CarriedNormals {
    pub fn new() -> CarriedNormals {
        CarriedNormals { cells: HashMap::new(), len: 0 }
    }

    /// How many normals are held.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Enter one operand's vertices, its face `f` under the provenance id
    /// `id_offset + f` — the offset [`to_manifold`] was given for it.
    pub fn add(&mut self, body: &MeshBody, id_offset: u32) {
        let (mesh, topo) = body;
        self.add_mesh(mesh, topo, id_offset);
    }

    /// [`add`](Self::add) on a mesh and its topology held apart: what the
    /// snap hands manifold is the operand's positions moved by up to
    /// the band under the operand's own topology, and the normals are keyed
    /// by the positions manifold will hand back.
    pub fn add_mesh(&mut self, mesh: &Mesh, topo: &Topology, id_offset: u32) {
        if mesh.normals.len() < mesh.positions.len() {
            return;
        }
        let with_surface: Vec<bool> =
            topo.faces.iter().map(|f| f.surface.is_recognised() || f.geometry.is_some()).collect();
        // A vertex belongs to one face: the mesh has one per (position, face).
        let mut face_of: Vec<u32> = vec![u32::MAX; mesh.vertex_count()];
        for (t, tri) in mesh.indices.chunks_exact(3).enumerate() {
            let f = mesh.face_ids.get(t).copied().unwrap_or(0);
            for &v in tri {
                let slot = &mut face_of[v as usize];
                if *slot == u32::MAX {
                    *slot = f;
                }
            }
        }
        for (i, &f) in face_of.iter().enumerate() {
            if f == u32::MAX || !with_surface.get(f as usize).copied().unwrap_or(false) {
                continue;
            }
            let p = Vec3::new(mesh.positions[i * 3] as f64, mesh.positions[i * 3 + 1] as f64, mesh.positions[i * 3 + 2] as f64);
            let n = Vec3::new(mesh.normals[i * 3] as f64, mesh.normals[i * 3 + 1] as f64, mesh.normals[i * 3 + 2] as f64);
            if !(n.length() > 0.5) {
                continue;
            }
            let c = weld_cell(p);
            self.cells.entry((c.0, c.1, c.2, f + id_offset)).or_default().push((p, n));
            self.len += 1;
        }
    }

    /// The normal an operand's face `prov` had at `p`, within [`WELD_EPS`].
    pub fn get(&self, p: Vec3, prov: u32) -> Option<Vec3> {
        let eps = WELD_EPS as f64;
        let c = weld_cell(p);
        let probe = |key: (i64, i64, i64, u32)| {
            self.cells.get(&key).and_then(|hits| hits.iter().find(|(q, _)| (*q - p).length() <= eps).map(|(_, n)| *n))
        };
        // A vertex the boolean left alone comes back at its own coordinates,
        // so its own cell answers; the neighbours are for the copy of a
        // shared vertex the weld replaced by the other face's.
        if let Some(n) = probe((c.0, c.1, c.2, prov)) {
            return Some(n);
        }
        for dx in -1..=1i64 {
            for dy in -1..=1i64 {
                for dz in -1..=1i64 {
                    if (dx, dy, dz) == (0, 0, 0) {
                        continue;
                    }
                    if let Some(n) = probe((c.0 + dx, c.1 + dy, c.2 + dz, prov)) {
                        return Some(n);
                    }
                }
            }
        }
        None
    }
}

/// The cell of a [`WELD_EPS`]-sized spatial hash `p` falls in.
fn weld_cell(p: Vec3) -> (i64, i64, i64) {
    let eps = WELD_EPS as f64;
    ((p.x / eps).floor() as i64, (p.y / eps).floor() as i64, (p.z / eps).floor() as i64)
}

/// Where the vertex normals of one result came from: one count per vertex,
/// a vertex being one (position, face group) pair. The trace prints it; the
/// tests read it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NormalStats {
    /// The operand's own normal, at a vertex the boolean left in place.
    pub carried: usize,
    /// A recognised class's closed form.
    pub closed_form: usize,
    /// Inverted onto a free-form patch.
    pub inverted: usize,
    /// Inversions that started from a search of the whole patch, because
    /// there was no seed yet or the seeded search did not land on it. A cost
    /// counter: these are the expensive ones.
    pub presearched: usize,
    /// Area-weighted: the group has no surface (an imported or field mesh,
    /// or the robust engine's regrouping).
    pub no_surface: usize,
    /// Area-weighted: the vertex is farther from the surface than the
    /// tolerance, so it is not on it.
    pub off_surface: usize,
    /// Area-weighted: the surface's normal leans more than 60° from the
    /// triangles meeting at the vertex — an inversion onto the wrong sheet.
    pub rejected: usize,
    /// Area-weighted: the inversion did not converge, or the surface has no
    /// normal there (a cone's apex, a degenerate corner of a patch).
    pub failed: usize,
}

impl NormalStats {
    /// The vertices that got the surface's normal.
    pub fn exact(&self) -> usize {
        self.carried + self.closed_form + self.inverted
    }

    /// The vertices that fell back to the area-weighted normal.
    pub fn fallback(&self) -> usize {
        self.no_surface + self.off_surface + self.rejected + self.failed
    }

    pub fn summary(&self) -> String {
        format!(
            "{} vertices: {} carried, {} closed-form, {} inverted ({} from a whole-patch search); \
             {} area-weighted ({} no surface, {} off the surface, {} disagreeing with the triangles, {} failed)",
            self.exact() + self.fallback(),
            self.carried,
            self.closed_form,
            self.inverted,
            self.presearched,
            self.fallback(),
            self.no_surface,
            self.off_surface,
            self.rejected,
            self.failed,
        )
    }
}

/// One face group's surface, as [`from_manifold`] evaluates normals on it.
pub(crate) struct GroupSurface<'a> {
    class: &'a SurfaceClass,
    geometry: Option<&'a Surface>,
    /// The parameter the last inversion on this group found: the seed for
    /// the next. A boolean's new vertices run along an intersection curve
    /// and come out in order, so the previous one is close.
    seed: Option<(f64, f64)>,
}

impl<'a> GroupSurface<'a> {
    pub(crate) fn new(class: &'a SurfaceClass, geometry: Option<&'a Surface>) -> GroupSurface<'a> {
        GroupSurface { class, geometry, seed: None }
    }

    /// Is there a surface to evaluate a normal on at all?
    pub(crate) fn has_surface(&self) -> bool {
        self.class.is_recognised() || self.geometry.is_some()
    }
}

/// Newton steps for an inversion seeded by the previous vertex, and for one
/// that starts from a search of the whole patch. Newton either converges in
/// a handful of steps or oscillates, and a step is six surface evaluations.
const SEEDED_TRIALS: usize = 24;
const PRESEARCH_TRIALS: usize = 48;

/// How far the surface's normal may lean from the triangles that meet at the
/// vertex, as a cosine: 60°. The triangles are chords of the surface, so
/// their normals are within the facet half-angle of its; an answer past this
/// came from the wrong sheet — the far side of a revolved patch — and is
/// refused.
const AGREEMENT_COS: f64 = 0.5;

/// The normal of `group`'s surface at `p`, oriented like `winding` (the sum
/// of the triangle normals meeting at the vertex), or `None`, with the reason
/// counted in `stats`. `accept` is how far `p` may be from the surface.
pub(crate) fn surface_normal(
    group: &mut GroupSurface,
    p: Vec3,
    winding: Vec3,
    accept: f64,
    stats: &mut NormalStats,
) -> Option<Vec3> {
    if group.class.is_recognised() {
        if !(group.class.distance(p) <= accept) {
            stats.off_surface += 1;
            return None;
        }
        let Some(n) = group.class.normal_at(p) else {
            stats.failed += 1;
            return None;
        };
        return match oriented(n, winding) {
            Some(n) => {
                stats.closed_form += 1;
                Some(n)
            }
            None => {
                stats.rejected += 1;
                None
            }
        };
    }
    let Some(surface) = group.geometry else {
        stats.no_surface += 1;
        return None;
    };
    let point = p.point();
    let attempt = |hint: SPHint2D, trials: usize| -> Option<((f64, f64), f64)> {
        let (u, v) = ksurface::nearest_parameter(surface, point, hint, trials)?;
        let off = (point_to_vec3(surface.subs(u, v)) - p).length();
        Some(((u, v), off))
    };
    // The previous vertex's parameter, or for a group's first vertex the
    // middle of the patch: Newton from there lands on a blade-sized patch
    // nearly always, and the whole-patch search below is what catches the
    // rest — it costs 289 evaluations, and the wheel has a thousand groups.
    let seed = group.seed.or_else(|| match surface.try_range_tuple() {
        (Some((u0, u1)), Some((v0, v1))) => Some((0.5 * (u0 + u1), 0.5 * (v0 + v1))),
        _ => None,
    });
    let mut found = seed
        .and_then(|(u, v)| attempt(SPHint2D::Parameter(u, v), SEEDED_TRIALS))
        .filter(|(_, off)| *off <= accept);
    if found.is_none() {
        stats.presearched += 1;
        found = attempt(SPHint2D::None, PRESEARCH_TRIALS);
    }
    let Some(((u, v), off)) = found else {
        stats.failed += 1;
        return None;
    };
    if !(off <= accept) {
        stats.off_surface += 1;
        return None;
    }
    let n = vector_to_vec3(surface.normal(u, v));
    if !(n.length() > 1e-9) || !(n.x.is_finite() && n.y.is_finite() && n.z.is_finite()) {
        stats.failed += 1;
        return None;
    }
    match oriented(n, winding) {
        Some(n) => {
            group.seed = Some((u, v));
            stats.inverted += 1;
            Some(n)
        }
        None => {
            stats.rejected += 1;
            None
        }
    }
}

/// `n` as a unit vector on `winding`'s side — the surface's natural sign is
/// not the face's: a hole's cylinder faces its axis — or `None` when the two
/// disagree by more than [`AGREEMENT_COS`]. A zero winding (a vertex with
/// only degenerate triangles) orients nothing and refuses nothing.
fn oriented(n: Vec3, winding: Vec3) -> Option<Vec3> {
    let n = n.normalized();
    let len = winding.length();
    if !(len > 0.0) {
        return Some(n);
    }
    let d = n.dot(winding) / len;
    let n = if d < 0.0 { -n } else { n };
    (d.abs() >= AGREEMENT_COS).then_some(n)
}

/// The normal one result vertex gets (module notes): its operand's, the
/// surface's, or — with the reason counted — the area-weighted one.
fn vertex_normal(
    group: &mut GroupSurface,
    carried: Option<&CarriedNormals>,
    prov: u32,
    p: Vec3,
    winding: Vec3,
    accept: f64,
    stats: &mut NormalStats,
) -> Vec3 {
    if group.has_surface() {
        if let Some(n) = carried.and_then(|c| c.get(p, prov)).and_then(|n| oriented(n, winding)) {
            stats.carried += 1;
            return n;
        }
        if let Some(n) = surface_normal(group, p, winding, accept, stats) {
            return n;
        }
    } else {
        stats.no_surface += 1;
    }
    winding.normalized()
}

/// Edges of a mesh body: the boundaries between two face groups, chained into
/// polylines. A boolean's new edges therefore show up as real named edges with
/// a crease angle, which is what the renderer and `body.edges()` read.
fn build_edges(
    pos: &[Vec3],
    tri_verts: &[[u32; 3]],
    tri_group: &[u32],
    tri_normal: &[Vec3],
    topo: &mut Topology,
) {
    // Undirected mesh edge -> the (group, triangle) pairs that use it.
    let mut per_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::with_capacity(tri_verts.len() * 3 / 2);
    for (i, t) in tri_verts.iter().enumerate() {
        for k in 0..3 {
            let (u, v) = (t[k], t[(k + 1) % 3]);
            per_edge.entry(if u < v { (u, v) } else { (v, u) }).or_default().push(i);
        }
    }
    // Segments grouped by the unordered pair of face groups they separate.
    let mut by_pair: BTreeMap<(u32, u32), Vec<((u32, u32), f64, bool)>> = BTreeMap::new();
    for (e, tris) in &per_edge {
        if tris.len() != 2 {
            continue;
        }
        let (g0, g1) = (tri_group[tris[0]], tri_group[tris[1]]);
        if g0 == g1 {
            continue;
        }
        let (n0, n1) = (tri_normal[tris[0]], tri_normal[tris[1]]);
        let (l0, l1) = (n0.length(), n1.length());
        let (deg, convex) = if l0 > 0.0 && l1 > 0.0 {
            let n0 = n0 * (1.0 / l0);
            let n1 = n1 * (1.0 / l1);
            let angle = fmath::acos(n0.dot(n1).clamp(-1.0, 1.0)).to_degrees();
            // Convex when the far corner of the first triangle lies behind the
            // second triangle's plane (the same test the B-rep path uses).
            let opp = tri_verts[tris[0]].iter().copied().find(|v| *v != e.0 && *v != e.1).unwrap_or(e.0);
            let convex = (pos[opp as usize] - pos[e.0 as usize]).dot(n1) <= 0.0;
            (angle, convex)
        } else {
            (-1.0, true)
        };
        let key = if g0 < g1 { (g0, g1) } else { (g1, g0) };
        by_pair.entry(key).or_default().push((*e, deg, convex));
    }

    for (pair, segs) in by_pair {
        let mut angle_of: HashMap<(u32, u32), (f64, bool)> = HashMap::with_capacity(segs.len());
        let mut raw: Vec<(u32, u32)> = Vec::with_capacity(segs.len());
        for (e, deg, convex) in &segs {
            angle_of.insert(*e, (*deg, *convex));
            raw.push(*e);
        }
        for chain in chain_segments(&raw) {
            let polyline: Vec<Vec3> = chain.iter().map(|&v| pos[v as usize]).collect();
            if polyline.len() < 2 {
                continue;
            }
            let length: f64 = polyline.windows(2).map(|w| (w[1] - w[0]).length()).sum();
            let mut angles: Vec<f64> = Vec::new();
            let mut votes = 0i32;
            for w in chain.windows(2) {
                let k = if w[0] < w[1] { (w[0], w[1]) } else { (w[1], w[0]) };
                if let Some(&(deg, convex)) = angle_of.get(&k) {
                    angles.push(deg);
                    votes += if convex { 1 } else { -1 };
                }
            }
            angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let deg = angles.get(angles.len() / 2).copied().unwrap_or(-1.0);
            let i = topo.edges.len();
            let name = naming::Kind::Edge.index_name(i);
            for g in [pair.0, pair.1] {
                if let Some(f) = topo.faces.get_mut(g as usize) {
                    f.edge_ids.push(i as u32);
                }
            }
            topo.edges.push(EdgeInfo {
                name,
                curve_kind: "polyline",
                start: polyline[0],
                end: *polyline.last().unwrap(),
                length,
                polyline,
                faces: Vec::new(),
                face_ids: vec![pair.0, pair.1],
                dihedral_degrees: deg,
                convex: votes >= 0,
            });
        }
    }

    // Vertices: the ends of the chains — where three or more groups meet.
    let mut at: HashMap<u64, u32> = HashMap::new();
    let ends: Vec<(u64, Vec3, Vec<u32>)> = topo
        .edges
        .iter()
        .flat_map(|e| {
            let f = e.face_ids.clone();
            [(quantize(e.start), e.start, f.clone()), (quantize(e.end), e.end, f)]
        })
        .collect();
    for (key, p, faces) in ends {
        let next = at.len() as u32;
        let i = *at.entry(key).or_insert(next);
        if i == next {
            topo.vertices.push(VertexInfo {
                name: naming::Kind::Vertex.index_name(next as usize),
                position: p,
                faces: Vec::new(),
                face_ids: Vec::new(),
            });
        }
        let v = &mut topo.vertices[i as usize];
        for g in faces {
            if !v.face_ids.contains(&g) {
                v.face_ids.push(g);
            }
        }
    }
}

fn quantize(p: Vec3) -> u64 {
    let q = |v: f64| (v / 1e-6).round() as i64 as u64;
    q(p.x) ^ q(p.y).rotate_left(21) ^ q(p.z).rotate_left(42)
}

/// Chain undirected segments into polylines. Closed loops come back with the
/// first vertex repeated at the end. Deterministic: the starting vertex of
/// every chain is the smallest available index.
fn chain_segments(segs: &[(u32, u32)]) -> Vec<Vec<u32>> {
    let mut adj: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(a, b) in segs {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    for v in adj.values_mut() {
        v.sort_unstable();
    }
    let mut used: BTreeSet<(u32, u32)> = BTreeSet::new();
    let key = |a: u32, b: u32| if a < b { (a, b) } else { (b, a) };
    let mut out = Vec::new();
    // Open chains first (they start at a vertex of degree != 2), then loops.
    let starts: Vec<u32> = adj
        .iter()
        .filter(|(_, n)| n.len() != 2)
        .map(|(v, _)| *v)
        .chain(adj.keys().copied())
        .collect();
    for s in starts {
        loop {
            let Some(&next) = adj.get(&s).and_then(|n| n.iter().find(|&&w| !used.contains(&key(s, w)))) else {
                break;
            };
            let mut chain = vec![s, next];
            used.insert(key(s, next));
            let mut cur = next;
            let mut prev = s;
            loop {
                let Some(&w) = adj
                    .get(&cur)
                    .and_then(|n| n.iter().find(|&&w| w != prev && !used.contains(&key(cur, w))))
                else {
                    break;
                };
                used.insert(key(cur, w));
                chain.push(w);
                prev = cur;
                cur = w;
                if cur == s {
                    break;
                }
            }
            out.push(chain);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Plane;
    use crate::kernel;
    use crate::sketch::{Loop, P2, Sketch};
    use crate::tessellate;

    /// Tessellated and named the way `Context` does it, so these tests see the
    /// generator names the mesh kernel actually has to carry through.
    fn brep_mesh(plane: Plane, l: Loop, h: f64, feature: &str) -> MeshBody {
        brep_mesh_at(plane, l, h, feature, 0.05)
    }

    fn brep_mesh_at(plane: Plane, l: Loop, h: f64, feature: &str, tol: f64) -> MeshBody {
        let sketch = Sketch::new(plane).with_loop(l);
        let solid = kernel::extrude(&sketch, h).unwrap().remove(0);
        let mut mb = tessellate::tessellate(&solid, tol).unwrap();
        let g = naming::Generator::extrude(&sketch, h, "box");
        let probes = mb.1.probes(&mb.0);
        let named = naming::name_faces(&g, feature, &probes);
        mb.1.apply_names(named);
        mb
    }

    fn boxy(x: f64, y: f64, w: f64, d: f64, z: f64, h: f64, feature: &str) -> MeshBody {
        brep_mesh(Plane::XY.offset(z), Loop::rect(P2::new(x, y), w, d), h, feature)
    }

    /// A cylinder standing on `z`, axis along Z.
    fn post(cx: f64, cy: f64, r: f64, z: f64, h: f64, feature: &str) -> MeshBody {
        brep_mesh(Plane::XY.offset(z), Loop::circle(P2::new(cx, cy), r), h, feature)
    }

    /// A plate with a round hole through it.
    fn plate_with_hole(w: f64, d: f64, h: f64, hole_at: P2, hole_r: f64, feature: &str) -> MeshBody {
        let sketch = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), w, d))
            .with_loop(Loop::circle(hole_at, hole_r));
        let solid = kernel::extrude(&sketch, h).unwrap().remove(0);
        let mut mb = tessellate::tessellate(&solid, 0.05).unwrap();
        let g = naming::Generator::extrude(&sketch, h, "box");
        let probes = mb.1.probes(&mb.0);
        mb.1.apply_names(naming::name_faces(&g, feature, &probes));
        mb
    }

    /// The union gate refuses a closed surface that passes through
    /// itself -- here two overlapping boxes composed without a boolean, so
    /// the "result" is two shells running through each other -- whatever
    /// the cleanup does; a real union of the same two passes it.
    #[test]
    fn the_union_gate_refuses_a_surface_that_passes_through_itself() {
        let a = boxy(0.0, 0.0, 10.0, 10.0, 0.0, 10.0, "a");
        let b = boxy(5.0, 5.0, 10.0, 10.0, 5.0, 10.0, "b");
        let offset = id_span(&a);
        let ma = to_manifold(&a.0, 0, 0.05).unwrap();
        let mb = to_manifold(&b.0, offset, 0.05).unwrap();
        let both = Manifold::compose(&[ma, mb]);
        let groups = merged_groups(&a.1, &b.1, offset);
        let body = from_manifold_with(&both, &groups, None, 0.05).unwrap();
        let err = union_gate(&both, &groups, None, 0.05, body).unwrap_err().to_string();
        assert!(err.contains("passes through itself"), "{err}");
        assert!(err.contains("union(..., weld=)"), "{err}");
        let u = boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        assert!(solid::validate(&u.0).is_solid());
    }

    /// The boolean without the coplanar snap: no classification, no
    /// snap, the operands handed to manifold as they came. The "before"
    /// column of every measurement below.
    fn unsnapped(op: BoolOp, a: &MeshBody, b: &MeshBody) -> Result<MeshBody> {
        let offset = id_span(a);
        let ma = to_manifold(&a.0, 0, 0.05)?;
        let mb = to_manifold(&b.0, offset, 0.05)?;
        finish(op, &ma, &mb, &merged_groups(&a.1, &b.1, offset), None, 0.05, None, &|| None)
    }

    /// What a measurement reads off a result: faces, triangles, folds, shells,
    /// the solid verdict, and which engine ran (the robust one says so in a
    /// note). `notes` are the ones the run pushed.
    fn census(r: &Result<MeshBody>, notes: &[String]) -> String {
        match r {
            Ok((mesh, topo)) => {
                let v = solid::validate(mesh);
                format!(
                    "{} faces, {} tris, {} folds, {} shell{}, {}, volume {:.4}, engine {}",
                    topo.faces.len(),
                    mesh.triangle_count(),
                    v.folds.len(),
                    v.components,
                    if v.components == 1 { "" } else { "s" },
                    if v.is_solid() { "solid" } else { "NOT a solid" },
                    mesh.volume(),
                    if notes.iter().any(|n| n.contains("robust engine")) { "robust" } else { "exact" },
                )
            }
            Err(e) => format!("error: {e}"),
        }
    }

    /// Before (no snap) and after (the boolean as it now runs) for one pair.
    fn before_after(op: BoolOp, a: &MeshBody, b: &MeshBody) -> (String, String, Vec<String>) {
        notes::drain();
        let before = unsnapped(op, a, b);
        let before_notes = notes::drain();
        let after = boolean(op, a, b, 0.05);
        let after_notes = notes::drain();
        (census(&before, &before_notes), census(&after, &after_notes), after_notes)
    }

    /// Coplanar pairs as they reach the mesh kernel: exactly coplanar, and coplanar to within
    /// the float noise a second spelling of one plane carries (0.04 µm, well
    /// inside the snap band and well outside what any tessellation can
    /// promise). Every result is measured before and after the snap; the
    /// table goes to stderr and the assertions below pin what it shows.
    fn tangent_cases() -> Vec<(&'static str, BoolOp, MeshBody, MeshBody)> {
        let mut out: Vec<(&'static str, BoolOp, MeshBody, MeshBody)> = Vec::new();
        for (label, dz) in [("exact", 0.0), ("+0.04 µm gap", 4e-5), ("-0.04 µm overlap", -4e-5)] {
            let plate = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
            // (a) a box standing on a box: its bottom inside the plate's top.
            out.push((Box::leak(format!("box on box, {label}").into_boxed_str()), BoolOp::Union, plate.clone(), boxy(0.0, 0.0, 10.0, 10.0, 10.0 + dz, 6.0, "b")));
            // (a') a box beside a box, tops level: the side faces touch, the tops are flush.
            out.push((Box::leak(format!("box beside box, tops flush, {label}").into_boxed_str()), BoolOp::Union, plate.clone(), boxy(15.0, 0.0, 10.0, 20.0, dz, 10.0, "b")));
            // (b) a cylinder standing on a plate.
            out.push((Box::leak(format!("cylinder on plate, {label}").into_boxed_str()), BoolOp::Union, plate.clone(), post(2.0, -1.0, 3.0, 10.0 + dz, 6.0, "post")));
            // (a'') a pocket whose floor is the plate's bottom: subtract, flush.
            out.push((Box::leak(format!("through pocket flush with the bottom, {label}").into_boxed_str()), BoolOp::Subtract, plate.clone(), boxy(0.0, 0.0, 6.0, 6.0, dz, 4.0, "pocket")));
        }
        // (c) a boss whose side is tangent to a hole, through the plate — and
        // the same boss with the interference the diagnostic asks for.
        let holed = plate_with_hole(20.0, 20.0, 5.0, P2::new(0.0, 0.0), 4.0, "plate");
        out.push(("boss tangent inside a hole", BoolOp::Union, holed.clone(), post(2.0, 0.0, 2.0, -1.0, 8.0, "boss")));
        out.push(("boss with 0.1 mm interference", BoolOp::Union, holed, post(2.0, 0.0, 2.1, -1.0, 8.0, "boss")));
        out
    }

    /// Stage 1 of the tangent case, measured. The table goes to stderr;
    /// the assertions are what it shows:
    ///
    /// * a pair already on one plane to the bit is untouched — nothing moves,
    ///   nothing is said, and the result is the old one bit for bit;
    /// * a pair whose planes differ by float noise (0.04 µm) comes back
    ///   unsnapped as **two shells** — a box or a cylinder floating a hair above
    ///   the plate it stands on, a flush pocket left as a closed void under a
    ///   0.04 µm floor — and every check called that a solid. After the snap
    ///   each is one shell with the exact result's face count;
    /// * a flush pair with the same noise carried a 0.012 mm³ error in its
    ///   volume from the sliver band; after the snap the volume is exact;
    /// * the exact engine accepts every snapped pair, and none has a fold;
    /// * a boss tangent inside a hole is *named*, not fixed: the mesh kernel
    ///   cannot make a curved contact exact, the two
    ///   bodies meet without merging, and the note says so and by how much
    ///   to change the radius. With that interference the union is one body
    ///   and the note is gone.
    #[test]
    fn the_tangent_cases_measured_before_and_after_the_snap() {
        eprintln!("\n| case | op | before (no snap) | after | notes |\n| --- | --- | --- | --- | --- |");
        for (label, op, a, b) in tangent_cases() {
            let (before, after, after_notes) = before_after(op, &a, &b);
            eprintln!(
                "| {label} | {} | {before} | {after} | {} |",
                op.kind_name(),
                after_notes.iter().map(|n| n.chars().take(160).collect::<String>()).collect::<Vec<_>>().join(" // ")
            );
            let before_body = unsnapped(op, &a, &b).unwrap();
            let after_body = boolean(op, &a, &b, 0.05).unwrap();
            let v = solid::validate(&after_body.0);
            assert!(v.is_solid(), "{label}: {}", v.summary());
            assert!(v.folds.is_empty(), "{label}: {:?}", v.folds);
            assert!(!after_notes.iter().any(|n| n.contains("robust engine")), "{label}: the exact engine accepts the snapped pair: {after_notes:?}");
            if label.ends_with("exact") {
                assert_eq!(before_body.0.positions, after_body.0.positions, "{label}: bit-identical");
                assert_eq!(before_body.0.indices, after_body.0.indices, "{label}: bit-identical");
                assert!(after_notes.is_empty(), "{label}: an exact tie is not news: {after_notes:?}");
            }
            if label.contains("+0.04 µm gap") && !label.starts_with("box beside box") {
                let expected_faces = if label.starts_with("cylinder") { 9 } else { 11 };
                assert!(before.contains("2 shells"), "{label}: before the snap the bodies never met: {before}");
                assert_eq!(v.components, 1, "{label}: {after}");
                assert_eq!(after_body.1.faces.len(), expected_faces, "{label}: {after}");
                assert!(after_notes.iter().any(|n| n.starts_with(&format!("mesh {}: snapped ", op.kind_name()))), "{label}: {after_notes:?}");
            }
            if label.starts_with("box beside box") && !label.ends_with("exact") {
                assert!((before_body.0.volume() - 6000.0).abs() > 1e-3, "{label}: the sliver band costs volume unsnapped: {}", before_body.0.volume());
                assert!((after_body.0.volume() - 6000.0).abs() < 1e-9, "{label}: exact after the snap: {}", after_body.0.volume());
            }
            if label == "boss tangent inside a hole" {
                assert_eq!(v.components, 2, "the bodies meet without merging: {after}");
                let note = after_notes.iter().find(|n| n.starts_with("mesh union: tangent cylinders, one inside the other: `plate/")).unwrap_or_else(|| panic!("{after_notes:?}"));
                assert!(note.contains(" and `boss/"), "{note}");
                assert!(note.contains("touch along a line (axes 2.000 mm apart, the difference of the radii 4.000 and 2.000; 0.00 µm from exact tangency, axes 0.0 µrad apart). Give `boss/"), "{note}");
                assert!(note.ends_with("at least 0.100 mm of interference (radius 2.100 instead of 2.000) so the contact becomes a crossing (0.5 mm is the usual overlap), or as much clearance so the two do not meet; the mesh kernel cannot make a curved contact exact, so the result is two bodies that meet without merging, or a pinched seam"), "{note}");
            }
            if label == "boss with 0.1 mm interference" {
                assert_eq!(v.components, 1, "{after}");
                assert!(after_notes.is_empty(), "{after_notes:?}");
            }
        }
    }

    /// The snap note, word for word: what moved, by how much, within what,
    /// and why (the diagnostics text is a contract with the reader).
    #[test]
    fn the_snap_note_is_snapshot() {
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let b = boxy(0.0, 0.0, 10.0, 10.0, 10.0 + 4e-5, 6.0, "b");
        notes::drain();
        boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        let notes = notes::drain();
        assert_eq!(
            notes,
            vec![
                "mesh union: snapped 1 face group (2 faces: `a/top`, `b/bottom`) onto 1 shared plane, 12 \
                 vertices moved, the furthest by 0.04 µm — within the 0.10 µm band, so two spellings of one \
                 plane become one and manifold's exact engine resolves the contact as a tie instead of a \
                 sliver or a gap"
            ]
        );
    }

    /// A pair that only crosses — no coplanar faces, nothing tangent — is
    /// not touched by the snap: no copy is made, no vertex moves, nothing is
    /// said, and the boolean's bytes are those of the unsnapped boolean.
    #[test]
    fn a_pair_that_only_crosses_is_bit_identical_with_the_snap_enabled() {
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let b = boxy(5.0, 5.0, 12.0, 12.0, 3.0, 9.0, "b");
        let eps = tangent::snap_eps(&a.0, &b.0);
        let fa = tangent::faces_of_body(&a, eps);
        let fb = tangent::faces_of_body(&b, eps);
        assert!(tangent::classify(&fa, &fb, 0.05, tangent::Depth::Full).is_empty());
        let (am, bm, report) = tangent::snap_shared_planes(&a.0, &fa, &b.0, &fb, eps);
        assert!(matches!(am, std::borrow::Cow::Borrowed(_)) && matches!(bm, std::borrow::Cow::Borrowed(_)), "no copy when nothing moves");
        assert_eq!(report, tangent::SnapReport::default());
        notes::drain();
        let with = boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        assert!(notes::drain().is_empty());
        let without = unsnapped(BoolOp::Union, &a, &b).unwrap();
        assert_eq!(with.0.positions, without.0.positions);
        assert_eq!(with.0.indices, without.0.indices);
        assert_eq!(with.0.face_ids, without.0.face_ids);
        assert_eq!(
            with.1.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            without.1.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>()
        );
    }

    /// The bound, as a property over every case above and a few more
    /// spellings of the same plane: no vertex moves further than the band,
    /// a vertex that does not move keeps its bits, and a plane further away
    /// than the band is not a spelling of the same plane — it is left where
    /// it is and named instead.
    #[test]
    fn the_snap_never_moves_a_vertex_further_than_its_band() {
        let mut cases = tangent_cases();
        for dz in [8e-5, -8e-5, 2.5e-5] {
            let plate = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
            cases.push(("box on box, more noise", BoolOp::Union, plate.clone(), boxy(3.0, -2.0, 8.0, 12.0, 10.0 + dz, 6.0, "b")));
            cases.push(("cylinder on plate, more noise", BoolOp::Union, plate, post(-1.0, 2.0, 4.0, 10.0 + dz, 3.0, "post")));
        }
        for (label, _, a, b) in &cases {
            let eps = tangent::snap_eps(&a.0, &b.0);
            let fa = tangent::faces_of_body(a, eps);
            let fb = tangent::faces_of_body(b, eps);
            let (am, bm, report) = tangent::snap_shared_planes(&a.0, &fa, &b.0, &fb, eps);
            assert!(report.max_move <= eps, "{label}: {report:?} eps {eps}");
            for (input, output) in [(&a.0, &*am), (&b.0, &*bm)] {
                assert_eq!(input.indices, output.indices, "{label}: only positions change");
                for v in 0..input.vertex_count() {
                    let p = |m: &Mesh| Vec3::new(m.positions[v * 3] as f64, m.positions[v * 3 + 1] as f64, m.positions[v * 3 + 2] as f64);
                    let d = (p(output) - p(input)).length();
                    assert!(d <= eps + 1e-12, "{label}: vertex {v} moved {d} > {eps}");
                    if d == 0.0 {
                        assert_eq!(input.positions[v * 3..v * 3 + 3].iter().map(|x| x.to_bits()).collect::<Vec<_>>(), output.positions[v * 3..v * 3 + 3].iter().map(|x| x.to_bits()).collect::<Vec<_>>(), "{label}: an unmoved vertex keeps its bits");
                    }
                }
            }
        }
        // Beyond the band (0.2 µm at this size, where the band is 0.1 µm)
        // nothing moves, and the union says why it stayed two shells.
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let b = boxy(0.0, 0.0, 10.0, 10.0, 10.0 + 2e-4, 6.0, "b");
        let eps = tangent::snap_eps(&a.0, &b.0);
        assert!((eps - 1e-4).abs() < 1e-12, "the floor: {eps}");
        let fa = tangent::faces_of_body(&a, eps);
        let fb = tangent::faces_of_body(&b, eps);
        let (am, bm, report) = tangent::snap_shared_planes(&a.0, &fa, &b.0, &fb, eps);
        assert!(matches!(am, std::borrow::Cow::Borrowed(_)) && matches!(bm, std::borrow::Cow::Borrowed(_)));
        assert!(!report.moved());
        notes::drain();
        let u = boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        assert_eq!(solid::validate(&u.0).components, 2);
        let notes = notes::drain();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(
            notes[0],
            "mesh union: faces `a/top` and `b/bottom` are coplanar within 0.20 µm, a gap (normals 0.0 µrad apart), \
             facing each other: the bodies touch there over 100.000 mm² -- a gap wider than the 0.10 µm snap band, \
             so the tie is not made exact and the two bodies stay apart there (a union keeps them as two shells). \
             Move them together, or overlap them by 0.5 mm"
        );
    }

    /// Stage 2's classifier on the contacts the snap does not resolve, each
    /// named with its faces and numbers: a cylinder lying on a plate (tangent
    /// along a line), a box tilted onto one edge (edge-on-face), and a box
    /// tilted onto one corner (vertex-on-face). The tilted boxes are what a
    /// pinched union looks like before it is pinched.
    #[test]
    fn a_line_an_edge_and_a_corner_resting_on_a_face_are_named() {
        use crate::geom::Transform;
        let plate = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let eps = tangent::snap_eps(&plate.0, &plate.0);
        let fa = tangent::faces_of_body(&plate, eps);
        let first = |b: &MeshBody| {
            let fb = tangent::faces_of_body(b, eps);
            let c = tangent::classify(&fa, &fb, 0.05, tangent::Depth::Cheap);
            let head = c.first().cloned();
            let text = head.as_ref().map(|c| tangent::describe(c, &fa, &fb)).unwrap_or_default();
            (head, text)
        };

        // A roller along X, radius 3, resting on the plate's top at z = 10.
        let roller = brep_mesh(Plane::YZ.offset(-8.0), Loop::circle(P2::new(0.0, 13.0), 3.0), 16.0, "roller");
        let (c, text) = first(&roller);
        assert!(matches!(c.as_ref().map(|c| &c.case), Some(TangentCase::TangentLine { kind: "a cylinder on a plane" })), "{c:?}");
        assert!(text.starts_with("tangent a cylinder on a plane: `a/top` and `roller/"), "{text}");
        assert!(text.ends_with("touch along a line (axis 3.000 mm from the plane, radius 3.000; 0.00 µm from exact tangency, axes 0.0 µrad apart)"), "{text}");

        // A 4 mm box tilted 45° about its front bottom edge, standing on that
        // edge in the middle of the top face.
        let cube = boxy(0.0, 0.0, 4.0, 4.0, 0.0, 4.0, "b");
        let tilt = Transform::rotation(Vec3::new(0.0, -2.0, 0.0), Vec3::X, std::f64::consts::FRAC_PI_4).then(Transform::translation(Vec3::new(0.0, 0.0, 10.0)));
        let on_edge = (cube.0.transformed(&tilt), cube.1.transformed(&tilt));
        let (c, text) = first(&on_edge);
        assert!(matches!(c.as_ref().map(|c| &c.case), Some(TangentCase::EdgeOnFace { plane_in_a: true, interior: true })), "{c:?}");
        assert!(text.starts_with("edge-on-face contact: an edge of `b/") && text.ends_with("lies in `a/top` within 0.00 µm"), "{text}");
        // The union is told so, with the fix.
        notes::drain();
        let _ = boolean(BoolOp::Union, &plate, &on_edge, 0.05);
        let notes = notes::drain();
        assert!(notes.iter().any(|n| n.starts_with("mesh union: edge-on-face contact: an edge of `b/") && n.ends_with("lies in `a/top` within 0.00 µm. A contact along an edge or at a corner encloses nothing, so the result is pinched to nothing there: overlap the bodies by at least 0.5 mm where they touch, or keep them as two bodies")), "{notes:?}");

        // Tilted again about Y through that edge's end, so only one corner
        // stays on the plate.
        let tilt2 = Transform::rotation(Vec3::new(0.0, -2.0, 0.0), Vec3::X, std::f64::consts::FRAC_PI_4)
            .then(Transform::rotation(Vec3::new(-2.0, -2.0, 0.0), Vec3::Y, -0.35))
            .then(Transform::translation(Vec3::new(0.0, 0.0, 10.0)));
        let on_corner = (cube.0.transformed(&tilt2), cube.1.transformed(&tilt2));
        let lowest = on_corner.0.bbox().unwrap().0.z;
        assert!((lowest - 10.0).abs() < 1e-6, "the corner rests on the plate: {lowest}");
        let (c, text) = first(&on_corner);
        assert!(matches!(c.as_ref().map(|c| &c.case), Some(TangentCase::VertexOnFace { plane_in_a: true, interior: true })), "{c:?}");
        assert!(text.starts_with("vertex-on-face contact: a corner of `b/") && text.ends_with("rests on `a/top` within 0.00 µm"), "{text}");
    }

    #[test]
    fn round_trip_through_manifold_preserves_volume_and_groups() {
        let (mesh, topo) = boxy(0.0, 0.0, 10.0, 20.0, 0.0, 5.0, "a");
        let m = to_manifold(&mesh, 0, 0.05).unwrap();
        assert!((m.volume() - 1000.0).abs() < 1e-6, "volume {}", m.volume());
        let groups: Vec<SourceFace> = topo
            .faces
            .iter()
            .map(|f| SourceFace {
                name: f.name.clone(),
                aliases: f.aliases.clone(),
                surface_kind: f.surface_kind,
                surface: f.surface.clone(),
                geometry: f.geometry.clone(),
            })
            .collect();
        let (out, out_topo) = from_manifold(&m, &groups, 0.05).unwrap();
        assert_eq!(out_topo.faces.len(), 6, "a box round trips as six groups");
        assert!((out.volume() - 1000.0).abs() < 1e-4);
        assert!(out.welded(WELD_EPS).is_watertight());
        assert_eq!(out_topo.edges.len(), 12, "a box has twelve edges");
        for f in &out_topo.faces {
            assert_eq!(f.edges.len(), 4, "{} has {:?}", f.name, f.edges);
        }
        assert_eq!(out_topo.vertices.len(), 8);
    }

    #[test]
    fn union_volumes_add_up_and_names_survive() {
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let b = boxy(8.0, 6.0, 15.0, 12.0, 2.0, 8.0, "b");
        let (va, vb) = (a.0.volume(), b.0.volume());
        let u = boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        let i = boolean(BoolOp::Intersect, &a, &b, 0.05).unwrap();
        let (vu, vi) = (u.0.volume(), i.0.volume());
        assert!(((vu + vi) - (va + vb)).abs() < 1e-6 * (va + vb), "{vu} + {vi} != {va} + {vb}");
        let d = boolean(BoolOp::Subtract, &a, &b, 0.05).unwrap();
        assert!((d.0.volume() - (va - vi)).abs() < 1e-6 * va);
        // Every result is a closed surface.
        for r in [&u, &i, &d] {
            assert!(r.0.welded(WELD_EPS).is_watertight());
        }
        // A face of `a` that the boolean did not touch keeps its name, and the
        // triangles that carry it are the ones the topology points at.
        let bottom = u.1.faces.iter().find(|f| f.normal.z < -0.99).expect("bottom face");
        assert!(a.1.faces.iter().any(|f| f.name == bottom.name), "{} is not one of a's names", bottom.name);
        assert!(bottom.tri_count > 0);
        for t in bottom.tri_start..bottom.tri_start + bottom.tri_count {
            assert_eq!(u.0.face_ids[t as usize], u.1.faces.iter().position(|f| f.name == bottom.name).unwrap() as u32);
        }
    }

    /// Two operands that leave a rim thinner than the tolerance: `b` stands
    /// on `a` with its side 0.02 mm in from `a`'s, so the exact union has a
    /// 0.02 mm ledge of `a`'s top face running the length of `b` — a strip
    /// of triangles no wider than that, which the tessellation was never
    /// accurate enough to promise and which the cleanup takes away. What is
    /// left is closed, has no triangle thinner than the tolerance, encloses
    /// the union's volume to within the ledge, and carries only names the
    /// operands had.
    #[test]
    fn a_rim_thinner_than_the_tolerance_does_not_survive_the_boolean() {
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let b = boxy(0.02, 0.0, 10.0, 20.0, 9.5, 5.0, "b");
        let u = boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        let (mesh, topo) = &u;
        let welded = mesh.welded(WELD_EPS);
        assert!(welded.is_closed_manifold());
        assert_eq!(welded.triangle_count(), mesh.triangle_count(), "no triangle collapses when welding");
        let thinnest = (0..mesh.triangle_count())
            .map(|t| {
                let [p, q, r] = mesh.triangle(t).unwrap();
                mesh_clean::min_altitude(p, q, r)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(thinnest >= 0.05, "a triangle {thinnest} mm wide survived");
        let exact = 20.0 * 20.0 * 10.0 + 10.0 * 20.0 * 4.5;
        let ledge = 0.02 * 20.0 * 4.5;
        assert!((mesh.volume() - exact).abs() <= ledge + 1e-6, "volume {} vs {exact}", mesh.volume());
        for f in &topo.faces {
            let base = f.name.split('#').next().unwrap();
            assert!(base.starts_with("a/") || base.starts_with("b/"), "{} is not an operand's name", f.name);
        }
    }

    #[test]
    fn coincident_faces_union_where_the_brep_kernel_fails() {
        // Two boxes sharing the whole z=10 / z=0 plane: truck #57 / #114.
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let b = boxy(0.0, 0.0, 10.0, 10.0, 10.0, 6.0, "b");
        let u = boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        assert!((u.0.volume() - (4000.0 + 600.0)).abs() < 1e-6, "volume {}", u.0.volume());
        assert!(u.0.welded(WELD_EPS).is_watertight());
    }

    /// Four prisms in a row, clear of each other, standing through a plate:
    /// `examples/gridfinity_baselid.star`'s magnet sockets in miniature.
    fn fan(n: usize) -> (MeshBody, Vec<MeshBody>) {
        let plate = boxy(-50.0, -10.0, 200.0, 40.0, 0.0, 10.0, "plate");
        let tools = (0..n)
            .map(|i| boxy(-40.0 + 20.0 * i as f64, 0.0, 6.0, 6.0, -5.0, 20.0, &format!("t{i}")))
            .collect();
        (plate, tools)
    }

    fn fold(op: BoolOp, a: &MeshBody, tools: &[&MeshBody]) -> MeshBody {
        let mut acc = a.clone();
        for t in tools {
            acc = boolean(op, &acc, t, 0.05).unwrap();
        }
        acc
    }

    /// Everything a selection, an export or a print check can read: the names,
    /// how much material there is, and how the triangles are apportioned
    /// between the faces. [`boolean_many`] has to agree with the fold on all of
    /// it — that is what makes it an optimisation rather than a change.
    fn observable(b: &MeshBody) -> String {
        let mut out = format!("{:.9} {}t {}f", b.0.volume(), b.0.triangle_count(), b.1.faces.len());
        for f in &b.1.faces {
            out.push_str(&format!("|{} {:?} {} {}", f.name, f.aliases, f.surface_kind, f.tri_count));
        }
        for e in &b.1.edges {
            out.push_str(&format!("|{}", e.name));
        }
        for v in &b.1.vertices {
            out.push_str(&format!("|{}", v.name));
        }
        out
    }

    #[test]
    fn one_boolean_against_many_tools_answers_what_the_fold_answers() {
        for op in [BoolOp::Subtract, BoolOp::Union] {
            for n in [2usize, 3, 4] {
                let (plate, tools) = fan(n);
                let refs: Vec<&MeshBody> = tools.iter().collect();
                let folded = fold(op, &plate, &refs);
                let batched = boolean_many(op, &plate, &refs, 0.05).unwrap();
                assert_eq!(
                    observable(&folded),
                    observable(&batched),
                    "{} of {n} tools",
                    op.kind_name()
                );
                assert!(batched.0.welded(WELD_EPS).is_watertight());
            }
        }
    }

    /// One tool is the pairwise boolean, exactly — no separate code path to
    /// keep in step.
    #[test]
    fn a_batch_of_one_is_the_pairwise_boolean() {
        let (plate, tools) = fan(1);
        let one = boolean(BoolOp::Subtract, &plate, &tools[0], 0.05).unwrap();
        let many = boolean_many(BoolOp::Subtract, &plate, &[&tools[0]], 0.05).unwrap();
        assert_eq!(one.0.positions, many.0.positions);
        assert_eq!(one.0.indices, many.0.indices);
        assert_eq!(observable(&one), observable(&many));
    }

    #[test]
    fn separated_is_true_only_with_a_real_gap() {
        let (plate, tools) = fan(4);
        for (i, t) in tools.iter().enumerate() {
            assert!(!separated(&plate.0, &t.0), "tool {i} stands through the plate");
            for (j, o) in tools.iter().enumerate().skip(i + 1) {
                assert!(separated(&t.0, &o.0), "tools {i} and {j} are 14 mm apart");
            }
        }
        // Overlapping, and merely touching, both count as touching: boxes that
        // share a face are never composed, because composing them would make
        // coincident surface out of two boundaries.
        let over = boxy(-38.0, 0.0, 6.0, 6.0, -5.0, 20.0, "over");
        let flush = boxy(-34.0, 0.0, 6.0, 6.0, -5.0, 20.0, "flush");
        assert!(!separated(&tools[0].0, &over.0));
        assert!(!separated(&tools[0].0, &flush.0));
        // And a hair beyond the epsilon is separated.
        let clear = boxy(-33.98, 0.0, 6.0, 6.0, -5.0, 20.0, "clear");
        assert!(separated(&tools[0].0, &clear.0));
    }

    /// The empty and single cases are errors and shortcuts, not panics.
    #[test]
    fn a_batch_needs_at_least_one_tool() {
        let (plate, _) = fan(0);
        assert!(boolean_many(BoolOp::Subtract, &plate, &[], 0.05).is_err());
    }

    #[test]
    fn results_are_deterministic() {
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let b = boxy(5.0, 5.0, 12.0, 12.0, 3.0, 9.0, "b");
        let one = boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        let two = boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        assert_eq!(one.0.indices, two.0.indices);
        assert_eq!(one.0.positions, two.0.positions);
        assert_eq!(one.0.face_ids, two.0.face_ids);
        assert_eq!(
            one.1.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            two.1.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>()
        );
        assert_eq!(
            one.1.edges.iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            two.1.edges.iter().map(|e| e.name.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_non_watertight_mesh_is_refused_not_silently_empty() {
        let mut m = Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            face_ids: vec![0],
            colors: Vec::new(),
        };
        let e = match to_manifold(&m, 0, 0.05) {
            Ok(_) => panic!("a single triangle is not a solid"),
            Err(e) => e.to_string(),
        };
        assert!(e.contains("watertight"), "{e}");
        m.indices.clear();
        m.face_ids.clear();
        assert!(to_manifold(&m, 0, 0.05).is_err());
    }

    #[test]
    fn a_split_face_gets_canonical_piece_names() {
        // A slab right across `a`'s +X wall: that wall comes back as two
        // disconnected pieces under one provenance id.
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let b = boxy(10.0, 0.0, 20.0, 30.0, 3.0, 3.0, "b");
        let u = boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
        let names: Vec<&str> = u.1.faces.iter().map(|f| f.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "names must be unique: {names:?}");
        assert!(names.iter().any(|n| n.contains('#')), "a split face is named name#k: {names:?}");
        // Every piece name is a source name plus `#k`, never an invention, and
        // the unsuffixed name stays on as a group alias.
        for f in u.1.faces.iter().filter(|f| f.name.contains('#')) {
            let base = f.name.split('#').next().unwrap().to_owned();
            assert!(f.aliases.contains(&base), "{} lost its group name", f.name);
            assert!(
                a.1.faces.iter().chain(b.1.faces.iter()).any(|s| s.name == base),
                "{base} is not a source name"
            );
        }
    }

    /// The robust engine is the one that loses face names, and the user is
    /// told so through the note sink rather than a trace line. Forced by
    /// argument, not by `CADPOLE_MESH_ENGINE`: the variable is process-global
    /// and every other test in this binary would see it (`engine_from_env`
    /// is what reads it, and `Engine::parse` is its whole vocabulary).
    #[test]
    fn running_on_the_robust_engine_is_said_in_a_note() {
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let b = boxy(8.0, 6.0, 15.0, 12.0, 2.0, 8.0, "b");
        notes::drain();
        let exact = boolean_with(BoolOp::Union, &a, &b, 0.05, None).unwrap();
        assert!(notes::drain().is_empty(), "the exact engine has nothing to say");

        let robust = boolean_with(BoolOp::Union, &a, &b, 0.05, Some(Engine::Robust)).unwrap();
        let notes = notes::drain();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("mesh union was forced onto the robust engine"), "{}", notes[0]);
        assert!(notes[0].contains("keeps no face provenance"), "what happened: {}", notes[0]);
        assert!(notes[0].contains("do not survive"), "what it means: {}", notes[0]);
        // And the note is true: the exact result carries the operands' names,
        // the robust one carries none of them. Its ids are its own coplanar
        // groups, which must not be read as the operands' provenance -- that
        // would name a face `a/top` because it happened to be group 2.
        assert!(exact.1.faces.iter().all(|f| f.name.starts_with("a/") || f.name.starts_with("b/")), "{:?}", exact.1.faces.iter().map(|f| &f.name).collect::<Vec<_>>());
        assert!(robust.1.faces.iter().all(|f| f.name.starts_with("face:")), "{:?}", robust.1.faces.iter().map(|f| &f.name).collect::<Vec<_>>());
        assert!((robust.0.volume() - exact.0.volume()).abs() < 1e-3 * exact.0.volume());

        assert_eq!(Engine::parse("robust"), Some(Engine::Robust));
        assert_eq!(Engine::parse("exact"), Some(Engine::Exact));
        assert_eq!(Engine::parse("auto"), Some(Engine::Auto));
        assert_eq!(Engine::parse("fast"), None);
    }

    /// Manifold's verdict beside ours, for `cadpole inspect`: a box does not
    /// intersect itself, a fold counts as coincident surface to manifold and
    /// not to our validator, and an open mesh has no verdict at all.
    #[test]
    fn manifolds_own_self_intersection_verdict() {
        use crate::solid::fixtures::{holed_box, overlapping_boxes, slit_box};
        let (bx, _) = boxy(0.0, 0.0, 10.0, 10.0, 0.0, 5.0, "a");
        assert_eq!(self_intersects(&bx), Some(false));
        assert_eq!(self_intersects(&overlapping_boxes()), Some(true), "two shells through each other");
        // The slit box is a solid to us, with one fold; manifold has a
        // verdict on it either way, and `cadpole inspect` prints both.
        let slit = slit_box(0.05);
        let ours = solid::validate(&slit);
        assert!(ours.is_solid() && ours.folds.len() == 1, "{}", ours.summary());
        let theirs = self_intersects(&slit);
        assert!(theirs.is_some(), "manifold takes a closed manifold mesh");
        eprintln!("manifold on the slit box (fold, tips 0.05 apart): self-intersecting = {theirs:?}");
        assert_eq!(self_intersects(&holed_box(10.0, 10.0, 10.0)), None, "an open surface has no verdict");
        assert_eq!(self_intersects(&Mesh::default()), None);
    }

    /// Uniting a body with a copy of itself: the two operands carry identical
    /// names, and the result has to keep them apart without inventing any.
    #[test]
    fn a_body_united_with_its_own_copy_keeps_one_name_per_face() {
        let a = boxy(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "a");
        let mut copy = a.clone();
        let t = crate::geom::Transform::translation(Vec3::new(10.0, 0.0, 0.0));
        copy = (copy.0.transformed(&t), copy.1.transformed(&t));
        assert_eq!(
            a.1.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            copy.1.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            "a copy keeps the source's names"
        );
        let u = boolean(BoolOp::Union, &a, &copy, 0.05).unwrap();
        let names: Vec<String> = u.1.faces.iter().map(|f| f.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "names must be unique: {names:?}");
        for n in &names {
            let base = n.split('#').next().unwrap();
            assert!(base.starts_with("a/"), "{n} is not one of a's names");
        }
    }

    // ------------------------------------------------- vertex normals

    /// A cylinder standing on the XY plane, tessellated coarsely: at 0.2 mm
    /// on a 5 mm radius a facet spans about 32°, so an area-weighted vertex
    /// normal at a rim vertex — one triangle of one facet against two of the
    /// next — leans several degrees off the radial, which is what makes the
    /// assertions below bite.
    fn cyl(x: f64, y: f64, r: f64, z: f64, h: f64, feature: &str) -> MeshBody {
        brep_mesh_at(Plane::XY.offset(z), Loop::circle(P2::new(x, y), r), h, feature, 0.2)
    }

    fn boxy_at(x: f64, y: f64, w: f64, d: f64, z: f64, h: f64, feature: &str, tol: f64) -> MeshBody {
        brep_mesh_at(Plane::XY.offset(z), Loop::rect(P2::new(x, y), w, d), h, feature, tol)
    }

    fn vertex(mesh: &Mesh, i: usize) -> (Vec3, Vec3) {
        let p = Vec3::new(mesh.positions[i * 3] as f64, mesh.positions[i * 3 + 1] as f64, mesh.positions[i * 3 + 2] as f64);
        let n = Vec3::new(mesh.normals[i * 3] as f64, mesh.normals[i * 3 + 1] as f64, mesh.normals[i * 3 + 2] as f64);
        (p, n)
    }

    /// Every distinct vertex the face group's triangles use.
    fn group_vertices(mesh: &Mesh, f: &FaceInfo) -> Vec<u32> {
        let (s, e) = (f.tri_start as usize * 3, (f.tri_start + f.tri_count) as usize * 3);
        let mut out: Vec<u32> = mesh.indices[s..e].to_vec();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The area-weighted normal of vertex `i` from the triangles that use it
    /// — what a vertex with no source surface gets.
    fn area_weighted(mesh: &Mesh, i: u32) -> Vec3 {
        let mut sum = Vec3::ZERO;
        for (t, tri) in mesh.indices.chunks_exact(3).enumerate() {
            if tri.contains(&i) {
                let [a, b, c] = mesh.triangle(t).unwrap();
                sum = sum + (b - a).cross(c - a);
            }
        }
        sum.normalized()
    }

    /// The unit radial of a cylinder class at `p`: the exact normal, up to sign.
    fn radial(class: &SurfaceClass, p: Vec3) -> Vec3 {
        let SurfaceClass::Cylinder { axis_point, axis, .. } = class else { panic!("not a cylinder: {class:?}") };
        let d = p - *axis_point;
        (d - *axis * d.dot(*axis)).normalized()
    }

    fn is_cylinder(f: &FaceInfo) -> bool {
        matches!(f.surface, SurfaceClass::Cylinder { .. })
    }

    /// A box out through a cylinder's wall: the box's top and bottom cut the
    /// wall along arcs, its sides along vertical lines.
    fn cylinder_with_a_box_through_its_wall() -> (MeshBody, MeshBody) {
        (cyl(0.0, 0.0, 5.0, 0.0, 10.0, "c"), boxy_at(6.0, 0.0, 4.0, 3.0, 2.0, 6.0, "b", 0.2))
    }

    #[test]
    fn a_cylindrical_group_keeps_the_exact_radial_normal_through_a_union() {
        let (c, b) = cylinder_with_a_box_through_its_wall();
        assert!(c.1.faces.iter().any(is_cylinder), "the wall is recognised: {:?}", c.1.faces.iter().map(|f| f.surface.kind_name()).collect::<Vec<_>>());
        let u = boolean(BoolOp::Union, &c, &b, 0.2).unwrap();
        let mut checked = 0;
        let mut worst_area_weighted = 0.0f64;
        for f in u.1.faces.iter().filter(|f| is_cylinder(f)) {
            for v in group_vertices(&u.0, f) {
                let (p, n) = vertex(&u.0, v as usize);
                let exact = radial(&f.surface, p);
                assert!((n - exact).length() < 1e-6, "{} at {p:?}: {n:?} vs {exact:?}", f.name);
                worst_area_weighted = worst_area_weighted.max((area_weighted(&u.0, v) - exact).length());
                checked += 1;
            }
        }
        assert!(checked > 20, "{checked} vertices on the wall");
        // The old answer would have failed the assertion above at the rim.
        assert!(worst_area_weighted > 0.02, "area weighting is off by {worst_area_weighted} at worst");
    }

    #[test]
    fn a_vertex_on_the_intersection_curve_gets_each_groups_own_normal() {
        let (c, b) = cylinder_with_a_box_through_its_wall();
        let u = boolean(BoolOp::Union, &c, &b, 0.2).unwrap();
        // The box's top (z = 8) meets the wall along an arc. Every vertex on
        // it is one vertex of the wall's group and one of the top's, at one
        // position: the first radial, the second straight up.
        let top = u
            .1
            .faces
            .iter()
            .find(|f| matches!(f.surface, SurfaceClass::Plane { normal, .. } if normal.z > 0.99) && (f.centroid.z - 8.0).abs() < 1e-6)
            .expect("the box's top");
        let key = |p: Vec3| ((p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64, (p.z * 1e5).round() as i64);
        let top_at: HashMap<_, Vec3> = group_vertices(&u.0, top)
            .into_iter()
            .map(|v| {
                let (p, n) = vertex(&u.0, v as usize);
                (key(p), n)
            })
            .collect();
        let mut shared = 0;
        for f in u.1.faces.iter().filter(|f| is_cylinder(f)) {
            for v in group_vertices(&u.0, f) {
                let (p, n) = vertex(&u.0, v as usize);
                let Some(&top_n) = top_at.get(&key(p)) else { continue };
                assert!((n - radial(&f.surface, p)).length() < 1e-6, "the wall's copy at {p:?}: {n:?}");
                assert!((top_n - Vec3::Z).length() < 1e-6, "the top's copy at {p:?}: {top_n:?}");
                shared += 1;
            }
        }
        assert!(shared >= 3, "{shared} vertices on the arc");
    }

    /// The same manifold rebuilt twice: with the operands' surfaces known,
    /// and with none. The second is the plain area-weighted mesh, vertex for
    /// vertex; the first differs only in normals.
    #[test]
    fn a_mesh_with_no_source_surfaces_keeps_area_weighted_normals() {
        let (c, b) = cylinder_with_a_box_through_its_wall();
        let offset = id_span(&c);
        let ma = to_manifold(&c.0, 0, 0.2).unwrap();
        let mb = to_manifold(&b.0, offset, 0.2).unwrap();
        let out = ma.boolean_with_engine(&mb, OpType::Add, BooleanEngine::Exact);
        let known = merged_groups(&c.1, &b.1, offset);
        let stripped: Vec<SourceFace> =
            known.iter().cloned().map(|s| SourceFace { surface: SurfaceClass::Other, geometry: None, ..s }).collect();
        let (plain, _) = from_manifold(&out, &stripped, 0.2).unwrap();
        for i in 0..plain.vertex_count() as u32 {
            let (_, n) = vertex(&plain, i as usize);
            let aw = area_weighted(&plain, i);
            assert!((n - aw).length() < 1e-5, "vertex {i}: {n:?} vs area-weighted {aw:?}");
        }
        let (exact, _) = from_manifold(&out, &known, 0.2).unwrap();
        assert_eq!(plain.positions, exact.positions);
        assert_eq!(plain.indices, exact.indices);
        assert_eq!(plain.face_ids, exact.face_ids);
        let moved = plain.normals.chunks_exact(3).zip(exact.normals.chunks_exact(3)).filter(|(a, b)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs() > 1e-3).count();
        assert!(moved > 0, "knowing the surfaces changes some normal");
    }

    #[test]
    fn normals_follow_the_winding_so_a_holes_wall_faces_its_axis() {
        let plate = boxy_at(0.0, 0.0, 20.0, 20.0, 0.0, 10.0, "plate", 0.2);
        let drill = cyl(0.0, 0.0, 5.0, -1.0, 12.0, "drill");
        let d = boolean(BoolOp::Subtract, &plate, &drill, 0.2).unwrap();
        let mut checked = 0;
        for f in d.1.faces.iter().filter(|f| is_cylinder(f)) {
            for v in group_vertices(&d.0, f) {
                let (p, n) = vertex(&d.0, v as usize);
                let exact = radial(&f.surface, p);
                assert!((n + exact).length() < 1e-6, "a hole's wall faces its axis at {p:?}: {n:?} vs {exact:?}");
                assert!(n.dot(area_weighted(&d.0, v)) > 0.5, "and agrees with the triangles that use it");
                checked += 1;
            }
        }
        assert!(checked > 20, "{checked} vertices on the hole's wall");
    }

    /// Every way the surface's normal is declined, counted.
    #[test]
    fn the_fallback_fires_and_is_counted() {
        let wall = SurfaceClass::Cylinder { axis_point: Vec3::ZERO, axis: Vec3::Z, radius: 5.0 };
        let mut group = GroupSurface::new(&wall, None);
        assert!(group.has_surface());
        let mut stats = NormalStats::default();
        let on = Vec3::new(5.0, 0.0, 1.0);
        let n = surface_normal(&mut group, on, Vec3::X, 0.05, &mut stats).expect("on the wall");
        assert!((n - Vec3::X).length() < 1e-12);
        // Orientation follows the winding, not the class's natural sign.
        let n = surface_normal(&mut group, on, -Vec3::X, 0.05, &mut stats).expect("on the wall, inside out");
        assert!((n + Vec3::X).length() < 1e-12);
        assert_eq!(stats.closed_form, 2);
        // Farther from the surface than the tolerance: not on it.
        assert_eq!(surface_normal(&mut group, Vec3::new(5.2, 0.0, 1.0), Vec3::X, 0.05, &mut stats), None);
        assert_eq!(stats.off_surface, 1);
        // Triangles facing 90° away from the surface's normal: the wrong sheet.
        assert_eq!(surface_normal(&mut group, on, Vec3::Z, 0.05, &mut stats), None);
        assert_eq!(stats.rejected, 1);
        // No normal there: a cone's apex.
        let cone = SurfaceClass::Cone { apex: Vec3::ZERO, axis: Vec3::Z, half_angle: 0.5 };
        let mut apex = GroupSurface::new(&cone, None);
        assert_eq!(surface_normal(&mut apex, Vec3::ZERO, Vec3::Z, 0.05, &mut stats), None);
        assert_eq!(stats.failed, 1);
        // No surface at all.
        let other = SurfaceClass::Other;
        let mut none = GroupSurface::new(&other, None);
        assert!(!none.has_surface());
        assert_eq!(surface_normal(&mut none, on, Vec3::X, 0.05, &mut stats), None);
        assert_eq!(stats.no_surface, 1);
        assert_eq!((stats.exact(), stats.fallback()), (2, 4));
        assert!(stats.summary().contains("2 closed-form"), "{}", stats.summary());
    }

    /// A ruled patch between two rails, one of them tilted: the compressor
    /// wheel's blade in miniature, a surface no class names.
    fn twisted_patch() -> Surface {
        use truck_modeling::{BSplineCurve, BSplineSurface, KnotVec};
        let line = |p: Vec3, q: Vec3| BSplineCurve::new(KnotVec::bezier_knot(1), vec![p.point(), q.point()]);
        Surface::BSplineSurface(BSplineSurface::homotopy(
            line(Vec3::new(0.0, 0.0, 0.0), Vec3::new(30.0, 0.0, 0.0)),
            line(Vec3::new(0.0, 20.0, 5.0), Vec3::new(30.0, 20.0, -5.0)),
        ))
    }

    #[test]
    fn a_free_form_group_inverts_the_vertex_onto_its_patch() {
        let s = twisted_patch();
        let other = SurfaceClass::Other;
        let mut group = GroupSurface::new(&other, Some(&s));
        assert!(group.has_surface());
        let mut stats = NormalStats::default();
        for (u, v) in [(0.3, 0.6), (0.31, 0.62), (0.9, 0.1), (0.05, 0.95)] {
            let p = point_to_vec3(s.subs(u, v));
            let exact = vector_to_vec3(s.normal(u, v)).normalized();
            let n = surface_normal(&mut group, p, exact, 0.05, &mut stats).expect("on the patch");
            assert!((n - exact).length() < 1e-9, "({u}, {v}): {n:?} vs {exact:?}");
            let m = surface_normal(&mut group, p, -exact, 0.05, &mut stats).expect("on the patch, inside out");
            assert!((m + exact).length() < 1e-9, "({u}, {v}): {m:?} vs {exact:?}");
        }
        assert_eq!(stats.inverted, 8);
        assert_eq!(stats.presearched, 0, "the middle of the patch seeds the first vertex, the rest are seeded by the previous one: {}", stats.summary());
        // A seed that leads nowhere: the whole patch is searched, and counted.
        group.seed = Some((f64::NAN, f64::NAN));
        let p = point_to_vec3(s.subs(0.7, 0.2));
        let exact = vector_to_vec3(s.normal(0.7, 0.2)).normalized();
        let n = surface_normal(&mut group, p, exact, 0.05, &mut stats).expect("found from the whole patch");
        assert!((n - exact).length() < 1e-9);
        assert_eq!((stats.inverted, stats.presearched), (9, 1), "{}", stats.summary());
        // A millimetre off the patch is not on it.
        let p = point_to_vec3(s.subs(0.5, 0.5)) + vector_to_vec3(s.normal(0.5, 0.5)).normalized();
        assert_eq!(surface_normal(&mut group, p, Vec3::Z, 0.05, &mut stats), None);
        assert_eq!(stats.off_surface, 1);
    }

    #[test]
    fn a_moved_topology_moves_its_free_form_surfaces() {
        let s = twisted_patch();
        let mut topo = Topology::default();
        topo.faces.push(FaceInfo { geometry: Some(Arc::new(s.clone())), ..Default::default() });
        let t = crate::geom::Transform::rotation(Vec3::ZERO, Vec3::Z, 0.7)
            .then(crate::geom::Transform::translation(Vec3::new(1.0, 2.0, 3.0)));
        let moved = topo.transformed(&t);
        let g = moved.faces[0].geometry.as_ref().expect("the patch moves with the face");
        for (u, v) in [(0.3, 0.6), (0.0, 1.0)] {
            let expected = t.point(point_to_vec3(s.subs(u, v)));
            assert!((point_to_vec3(g.subs(u, v)) - expected).length() < 1e-9);
        }
    }

    #[test]
    fn carried_normals_answer_within_the_weld_epsilon_and_only_for_faces_with_a_surface() {
        let (c, _) = cylinder_with_a_box_through_its_wall();
        let mut carried = CarriedNormals::new();
        carried.add(&c, 7);
        assert!(!carried.is_empty());
        let f = c.1.faces.iter().position(is_cylinder).unwrap();
        let v = group_vertices(&c.0, &c.1.faces[f])[0];
        let (p, n) = vertex(&c.0, v as usize);
        let prov = 7 + f as u32;
        assert_eq!(carried.get(p, prov), Some(n));
        // A hair away — the other face's copy the weld kept — still answers;
        // past the epsilon, or under another provenance, nothing does.
        assert_eq!(carried.get(p + Vec3::new(0.5e-4, 0.0, 0.0), prov), Some(n));
        assert_eq!(carried.get(p + Vec3::new(3e-4, 0.0, 0.0), prov), None);
        assert_eq!(carried.get(p, prov + 1000), None);
        let mut stripped = c.clone();
        for f in &mut stripped.1.faces {
            f.surface = SurfaceClass::Other;
            f.geometry = None;
        }
        let mut none = CarriedNormals::new();
        none.add(&stripped, 0);
        assert!(none.is_empty(), "a face with no surface carries nothing: {}", none.len());
    }
}
