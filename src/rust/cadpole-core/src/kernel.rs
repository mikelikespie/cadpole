//! B-rep kernel binding. Everything that touches `truck` lives here so the
//! rest of the crate (and the scripting layer) stays kernel-agnostic.

use std::collections::HashMap;

use truck_meshalgo::prelude::*;
use truck_modeling::{
    builder, Curve, Face, Line, Point3, Rad, RevolutedCurve, SPHint2D, Shell, Solid, Surface, Vector3, Vertex,
    Wire,
};

use crate::curve::{arc3_params, Curve as Curve3, Segment3};
use crate::geom::{Plane, Transform, Vec3};
use crate::ksurface::{DivisionBounds, KCurve, KSurface, DEFAULT_MAX_NORMAL_DEGREES};
use crate::loft::{Cap, Layout, LoftPlan, PlannedSection, RibbonEnd, RuledCap, SectionMatch};
use crate::sketch::{arc_params, Loop, P2, Region, Segment, Sketch};
use crate::{CadpoleError, Result};
use crate::fmath;

/// The kernel's solid type.
pub type KSolid = Solid;

/// A tessellated solid: one shared vertex table, a mesh per face and a
/// polyline per edge, all by vertex id, closed by construction
/// ([`crate::brepmesh`]).
pub use crate::brepmesh::MeshedSolid;

pub const DEFAULT_TOLERANCE: f64 = 0.05;

/// Default geometric tolerance for boolean operations, in mm. Deliberately
/// coarser than the display tolerance: measured on examples/bracket.star, the
/// three booleans cost 3959 ms at 0.01, 1552 ms at 0.05 and 659 ms at 0.1
/// while producing a byte-identical mesh (same triangle count, same volume to
/// 8 digits). Past ~0.3 truck starts failing on tangent cases, so this is the
/// fast end of the safe range rather than the coarsest value that "works".
pub const DEFAULT_BOOLEAN_TOLERANCE: f64 = 0.1;

/// Build a truck wire from a loop, mapped onto `plane`.
pub(crate) fn loop_to_wire(l: &Loop, plane: &Plane) -> Result<Wire> {
    if l.segments.is_empty() {
        return Err(CadpoleError::invalid("empty loop"));
    }
    let to3 = |p: P2| plane.to_world(p.x, p.y).point();
    // Share vertices between consecutive segments so the wire is connected.
    let first = builder::vertex(to3(l.segments[0].start()));
    let mut prev = first.clone();
    let mut wire = Wire::new();
    let n = l.segments.len();
    for (i, seg) in l.segments.iter().enumerate() {
        let last = i + 1 == n;
        let end_v = if last { first.clone() } else { builder::vertex(to3(seg.end())) };
        let edge = match seg {
            Segment::Line { .. } => builder::line(&prev, &end_v),
            Segment::Arc { via, .. } => builder::circle_arc(&prev, &end_v, to3(*via)),
            Segment::Bezier { ctrl, .. } => builder::bezier(&prev, &end_v, ctrl.iter().map(|c| to3(*c)).collect()),
        };
        wire.push_back(edge);
        prev = end_v;
    }
    Ok(wire)
}

fn region_to_face(r: &Region, plane: &Plane) -> Result<truck_modeling::Face> {
    let mut wires = vec![loop_to_wire(&r.outer, plane)?];
    for h in &r.holes {
        wires.push(loop_to_wire(h, plane)?);
    }
    builder::try_attach_plane(&wires)
        .map_err(|e| CadpoleError::kernel(format!("cannot make planar face: {e}")))
}

/// Extrude every region of the sketch by `distance` along the plane normal
/// (negative distances extrude the other way). Returns one solid per region.
pub fn extrude(sketch: &Sketch, distance: f64) -> Result<Vec<KSolid>> {
    let _span = trace::Span::start(format!("extrude d={distance} loops={}", sketch.loops.len()));
    if !distance.is_finite() {
        return Err(CadpoleError::invalid(format!("extrude distance must be finite, got {distance}")));
    }
    if distance == 0.0 {
        return Err(CadpoleError::invalid("extrude distance must be non-zero"));
    }
    let regions = sketch.face_regions()?;
    if regions.is_empty() {
        return Err(CadpoleError::invalid("sketch has no closed regions to extrude"));
    }
    let dir = (sketch.plane.normal * distance).vector();
    let mut out = Vec::new();
    for r in &regions {
        let face = region_to_face(r, &sketch.plane)?;
        // truck expects the face normal to agree with the sweep direction for
        // an outward-oriented solid; flip when extruding against the normal.
        let face = if distance < 0.0 { face.inverse() } else { face };
        out.push(builder::tsweep(&face, dir));
    }
    Ok(out)
}

/// Revolve every region about the axis through `origin` along `axis`.
pub fn revolve(sketch: &Sketch, origin: Vec3, axis: Vec3, angle_rad: f64) -> Result<Vec<KSolid>> {
    let _span = trace::Span::start(format!("revolve angle={:.1}deg loops={}", angle_rad.to_degrees(), sketch.loops.len()));
    // truck's rotational sweep never returns for a NaN angle or axis, and a
    // sweep is one native operation no deadline can interrupt, so this is
    // the last place it can be refused.
    if !angle_rad.is_finite() {
        return Err(CadpoleError::invalid(format!("revolve angle must be finite, got {angle_rad}")));
    }
    let all_finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if !all_finite(axis) || !all_finite(origin) {
        return Err(CadpoleError::invalid("revolve axis and origin must be finite"));
    }
    if angle_rad == 0.0 {
        return Err(CadpoleError::invalid("revolve angle must be non-zero"));
    }
    if axis.length() == 0.0 {
        return Err(CadpoleError::invalid("revolve axis must be non-zero"));
    }
    let regions = sketch.face_regions()?;
    if regions.is_empty() {
        return Err(CadpoleError::invalid("sketch has no closed regions to revolve"));
    }
    let axis = axis.normalized();
    for r in &regions {
        crosses_axis(r, &sketch.plane, origin, axis)?;
    }
    let mut out = Vec::new();
    for r in &regions {
        let face = region_to_face(r, &sketch.plane)?;
        // Same rule as `extrude`, and it was missing here: truck orients the
        // swept solid from the face normal against the sweep direction, so a
        // profile facing the wrong way produces a solid whose normals all
        // point inward. Every revolved example in the corpus was inside out --
        // negative volume, and `cadpole check` calling it a void -- because an XZ
        // profile's normal is -Y while a positive turn about +Z moves it
        // toward +Y.
        //
        // The sweep direction at a point is `axis x radial`, reversed for a
        // negative angle. Measured at the profile's centroid rather than at
        // one vertex, so a profile that straddles the axis is judged by where
        // its material actually is.
        let face = if revolve_is_left_handed(r, &sketch.plane, origin, axis, angle_rad) {
            face.inverse()
        } else {
            face
        };
        out.push(builder::rsweep(&face, origin.point(), axis.vector(), Rad(angle_rad)));
    }
    Ok(out)
}

/// Refuse a profile that lies in a plane containing the axis and has material
/// on both sides of it: in a full turn every point passes through the
/// material on the other side, and at any angle the two sides do not join
/// into one solid of revolution. truck builds that
/// solid without complaint and the failure surfaced later, in tessellation,
/// as a parameter-domain message that never mentioned the axis. Touching the
/// axis is fine (a cone's apex, a solid of revolution's centre line).
fn crosses_axis(r: &Region, plane: &Plane, origin: Vec3, axis: Vec3) -> Result<()> {
    let n = plane.normal;
    let in_plane = axis.dot(n).abs() < 1e-9 && (origin - plane.origin).dot(n).abs() < 1e-9;
    if !in_plane {
        return Ok(());
    }
    // In-plane unit vector across the axis: the signed distance from it.
    let across = axis.cross(n).normalized();
    let (mut lo, mut hi, mut reach) = (0.0f64, 0.0f64, 0.0f64);
    for p in r.outer.sample() {
        let w = plane.to_world(p.x, p.y);
        let d = (w - origin).dot(across);
        lo = lo.min(d);
        hi = hi.max(d);
        reach = reach.max((w - origin).length());
    }
    // Only float noise counts as touching: a crossing of 1e-8 mm on a 10 mm
    // profile is still one truck cannot build, and it failed later, in
    // tessellation, with the old message.
    let eps = 1e-12 * reach.max(1.0);
    if lo < -eps && hi > eps {
        // Enough digits that a small crossing does not read as 0.000.
        let mm = |v: f64| if v >= 0.001 { format!("{v:.3}") } else { format!("{v:.1e}") };
        return Err(CadpoleError::invalid(format!(
            "revolve: the profile crosses the axis — it reaches {} mm on one side and {} mm on the \
             other — and material on both sides of the axis cannot be swept into one solid. Keep the \
             profile on one side of the axis (touching it is fine), or move the axis",
            mm(hi),
            mm(-lo)
        )));
    }
    Ok(())
}

/// Does sweeping this profile about the axis turn it inside out?
///
/// True when the profile's normal opposes the direction the sweep carries it,
/// which is the case `revolve` corrects by flipping the face.
///
/// A profile whose centroid sits *on* the axis has no radial direction and so
/// no handedness to get wrong: `false`, leave it alone.
fn revolve_is_left_handed(r: &Region, plane: &Plane, origin: Vec3, axis: Vec3, angle_rad: f64) -> bool {
    let pts: Vec<Vec3> = r.outer.sample().iter().map(|p| plane.to_world(p.x, p.y)).collect();
    if pts.is_empty() {
        return false;
    }
    let centroid = pts.iter().fold(Vec3::ZERO, |a, b| a + *b) * (1.0 / pts.len() as f64);
    let d = centroid - origin;
    let radial = d - axis * d.dot(axis);
    if radial.length() < 1e-9 {
        return false;
    }
    let sweep = axis.cross(radial.normalized()) * angle_rad.signum();
    plane.normal.dot(sweep) < 0.0
}

/// One stacked section, ready to be stitched: either the wires of one region
/// of a profile, the single wire of a curve section, or the single vertex of
/// a point section. A profile keeps the loops it came from, because the tip
/// fan needs to know whether a segment was a straight line or an arc to give
/// its face an exact, non-degenerate surface; a curve section keeps its curve
/// for the same reason, and for its cap.
enum Rung<'a> {
    Wires { wires: Vec<Wire>, loops: Vec<&'a Loop>, plane: Plane },
    /// A curve section: its wire, one edge per segment, and — when a ruled
    /// cap closes this end — the same loop with the ribbon's vertices
    /// planted, sharing every original vertex with `wire`.
    Curve { wire: Wire, curve: &'a Curve3, cap: Option<CapWire> },
    Apex(Vertex),
}

/// A capped section's loop with the ribbon's extra vertices:
/// `owner[i]` says which of the section's segments edge `i` is a piece of.
/// The ribbon is built on this wire, and the side face next to the section
/// takes its pieces as boundary edges while keeping the uncut segment's curve
/// as its surface — so the cap's vertices reach no other face.
struct CapWire {
    wire: Wire,
    owner: Vec<usize>,
}

impl Rung<'_> {
    /// The wires of a section that has any: one per loop of a profile, one
    /// for a curve, none for a point.
    fn wires(&self) -> &[Wire] {
        match self {
            Rung::Wires { wires, .. } => wires,
            Rung::Curve { wire, .. } => std::slice::from_ref(wire),
            Rung::Apex(_) => &[],
        }
    }

    /// The boundary edges a side face uses along segment `e` of loop `l` of
    /// this section: the segment's own edge, or — on a section closed by a
    /// ruled cap — the pieces the cap cut it into, in order.
    fn chain(&self, l: usize, e: usize) -> Vec<truck_modeling::Edge> {
        if let Rung::Curve { cap: Some(cap), .. } = self {
            if l == 0 {
                let pieces: Vec<truck_modeling::Edge> = cap
                    .wire
                    .edge_iter()
                    .zip(&cap.owner)
                    .filter(|(_, o)| **o == e)
                    .map(|(edge, _)| edge.clone())
                    .collect();
                if !pieces.is_empty() {
                    return pieces;
                }
            }
        }
        self.wires().get(l).and_then(|w| w.edge_iter().nth(e).cloned()).into_iter().collect()
    }
}

/// The tolerance a freshly stitched 3D loft is meshed at to read the sign of
/// its volume. Coarse on purpose: the sign of a closed, consistently
/// oriented shell's volume does not depend on how finely it is chorded, and
/// this runs once per loft.
const ORIENT_TOL: f64 = 0.25;

/// A loft through matched sections, on the **exact B-rep kernel**
/// (docs/design.md).
///
/// Every side face is a B-spline surface skinned through the corresponding
/// segment of every section ([`crate::skin`]). With `ruled=True` that skin is
/// taken pairwise at degree 1 in the section direction, where the algorithm
/// reduces *exactly* to `BSplineSurface::homotopy` — the ruled surface truck
/// builds between two curves, and the reason a loft needs no boolean at all.
/// With `ruled=False` one skin at degree 3 spans every section at once, so the
/// column is a single face that passes through each section and is C² across
/// the ones in the middle rather than a stack of bands with a crease at every
/// section plane. The two end sections become planar caps and the whole thing
/// is one `Solid`, so the seams are interior edges of a single solid rather
/// than coincident faces the boolean kernel would have to reconcile.
///
/// **Why B-rep and what it costs.** The corner band between two concentric
/// circular arcs of equal angular span comes out *exact*: both arcs are
/// rational curves with the same weights, so interpolating their homogeneous
/// control points interpolates their Euclidean ones, and a surface whose
/// cross-sections are circles of linearly varying radius at linearly varying
/// height is a cone. That is what a 45° chamfer of a filleted vertical edge
/// is, which is why Gridfinity's foot is a loft and not a chamfer. Routing the
/// loft through the mesh kernel instead would have given a faceted cone, lost
/// STEP and lost `faces(kind=)` — for no gain, because a loft is not a boolean
/// and so cannot hit the coincident-face failures that force booleans onto the
/// mesh kernel. The cost is that every ruled face is a NURBS patch (truck has
/// no cone surface type), so even the *flat* sides of a loft report
/// `kind="nurbs"` rather than `"plane"` until `SurfaceClass` recognition lands,
/// and a later boolean against a lofted body is a curved-pair boolean, which is
/// the slow class — which is exactly what `Context::boolean`'s face budget is
/// there to catch. The patch that *is* a cone is still meshed as one:
/// [`with_cones_revolved`] hands the tessellator a `RevolutedCurve` of its
/// ruling line — the same exact surface — because truck tessellates a NURBS
/// cone as a grid and a revolved line as a strip, and the difference is a
/// factor of sixty on a solid of revolution.
///
/// **Sections in space.** A plan in the ordered layout — any `Curve`
/// section, or a ruled cap — is stitched by [`stitch_ordered`]: the same
/// skinned sides, in list order, with the caps oriented *topologically* (the
/// start cap uses the first wire's edges backwards, the end cap the last
/// wire's forwards, so the shell closes by construction) and the whole solid
/// then turned outward by the sign of its volume, because with no common
/// normal there is no "up" to orient a cap by. A stacked plan takes exactly
/// the code it always took.
pub fn loft(plan: &LoftPlan) -> Result<Vec<KSolid>> {
    let _span = trace::Span::start(format!(
        "loft {} sections {} bands {} regions",
        plan.sections.len(),
        plan.bands(),
        plan.regions.len()
    ));
    let up = match &plan.layout {
        // The rungs are always stacked along +normal, whichever order the
        // script listed them in, so "up" is the section normal and nothing
        // else.
        Layout::Stacked { normal, .. } => *normal,
        Layout::Ordered => {
            let mut rungs: Vec<Rung<'_>> = Vec::with_capacity(plan.sections.len());
            for s in &plan.sections {
                match s {
                    PlannedSection::Point(p) => rungs.push(Rung::Apex(builder::vertex(p.point()))),
                    PlannedSection::Curve(c) => {
                        let e = if rungs.is_empty() { 0 } else { 1 };
                        let is_end = rungs.len() + 1 == plan.sections.len();
                        let rc = if rungs.is_empty() || is_end { plan.ruled_caps[e].as_ref() } else { None };
                        let (wire, cap) = curve_to_wires(c, rc)?;
                        rungs.push(Rung::Curve { wire, curve: c, cap });
                    }
                    PlannedSection::Profile { .. } => {
                        return Err(CadpoleError::kernel("loft: a planar section in an ordered plan"));
                    }
                }
            }
            return Ok(vec![guarded("loft", std::panic::AssertUnwindSafe(|| stitch_ordered(&rungs, plan)))??]);
        }
    };
    let order = plan.ascending();
    let mut out = Vec::with_capacity(plan.regions.len());
    for (outer, holes) in &plan.regions {
        let mut rungs: Vec<Rung<'_>> = Vec::with_capacity(order.len());
        for &si in &order {
            match &plan.sections[si] {
                PlannedSection::Point(p) => rungs.push(Rung::Apex(builder::vertex(p.point()))),
                PlannedSection::Profile { plane, loops, .. } => {
                    let mut ws = Vec::with_capacity(1 + holes.len());
                    let mut ls = Vec::with_capacity(1 + holes.len());
                    ws.push(loop_to_wire(&loops[*outer], plane)?);
                    ls.push(&loops[*outer]);
                    for h in holes {
                        ws.push(loop_to_wire(&loops[*h], plane)?);
                        ls.push(&loops[*h]);
                    }
                    rungs.push(Rung::Wires { wires: ws, loops: ls, plane: *plane });
                }
                PlannedSection::Curve(_) => {
                    return Err(CadpoleError::kernel("loft: a curve section in a stacked plan"));
                }
            }
        }
        out.push(guarded("loft", std::panic::AssertUnwindSafe(|| stitch(&rungs, up, plan.ruled)))??);
    }
    Ok(out)
}

/// Caps plus sides, stitched into one closed solid.
///
/// A point section is **not** a section here: it is a fan of faces onto a
/// vertex ([`apex_fan`]), because a patch degenerate along one parametric edge
/// cannot be inverted at the apex and tessellates to nothing. So the point
/// sections are peeled off the ends first and only the genuine profile
/// sections are skinned.
///
/// `ruled` decides how those profile sections are spanned, and it is the same
/// skinner either way: pairwise at degree 1 — one face per band, which is the
/// ruled surface truck's `homotopy` builds, bit for bit — or all of them at
/// once at degree 3, which is one face per column of the whole loft.
fn stitch(rungs: &[Rung<'_>], up: Vec3, ruled: bool) -> Result<KSolid> {
    let mut shell = Shell::new();
    if let Rung::Wires { wires, .. } = &rungs[0] {
        shell.push(cap(wires, -up)?);
    }
    if matches!(rungs[0], Rung::Apex(_)) && matches!(rungs[1], Rung::Apex(_)) {
        return Err(CadpoleError::invalid("loft: two point sections in a row is not a solid"));
    }
    if let (Rung::Apex(v), Rung::Wires { wires, loops, plane }) = (&rungs[0], &rungs[1]) {
        shell.extend(apex_fan(&wires[0], &planar_geoms(loops[0], plane, false), v, false)?);
    }
    let profiles: Vec<&Rung<'_>> =
        rungs.iter().filter(|r| matches!(r, Rung::Wires { .. })).collect();
    if ruled {
        for (k, pair) in profiles.windows(2).enumerate() {
            shell.extend(
                side_faces(pair, 1)
                    .map_err(|e| CadpoleError::kernel(format!("loft band {k}: {e}")))?,
            );
        }
    } else if profiles.len() >= 2 {
        shell.extend(side_faces(&profiles, crate::skin::MAX_V_DEGREE)?);
    }
    let last = rungs.len() - 1;
    if let (Rung::Wires { wires, loops, plane }, Rung::Apex(v)) = (&rungs[last - 1], &rungs[last]) {
        shell.extend(apex_fan(&wires[0], &planar_geoms(loops[0], plane, true), v, true)?);
    }
    if let Rung::Wires { wires, .. } = &rungs[last] {
        shell.push(cap(wires, up)?);
    }
    Solid::try_new(vec![shell]).map_err(|e| {
        CadpoleError::kernel(format!(
            "loft did not close into a solid ({e}); the sections may cross or touch each other"
        ))
    })
}

/// The side faces spanning a run of profile sections: one per segment of each
/// loop, skinned through that segment's curve in every section.
///
/// The correspondence is already decided — [`crate::loft::LoftPlan`] paired
/// the loops and the segments before any NURBS existed — so this walks loop
/// `l`, segment `s` and hands the `K + 1` curves at that address to the
/// skinner.
///
/// **One v direction for the whole run, not one per face.** Two neighbouring
/// side faces meet along the spoke edge that runs from one section's corner
/// vertex to the next's, and each of them carries that edge as a boundary
/// curve of its own surface. Those two curves are the same curve only because
/// they interpolate the same section corners at the same parameters through
/// the same knot vector, so the section parameters are averaged over every
/// column of every segment of the loft at once and the factorised
/// interpolation matrix is shared. The textbook algorithm averages over the
/// columns of one surface; averaging over the loft is the same computation with a
/// wider sum, and it is what makes the faces meet exactly.
fn side_faces(rungs: &[&Rung<'_>], max_v_degree: usize) -> Result<Vec<Face>> {
    let mut sections: Vec<&[Wire]> = Vec::with_capacity(rungs.len());
    for r in rungs {
        match r {
            Rung::Wires { .. } | Rung::Curve { .. } => sections.push(r.wires()),
            Rung::Apex(_) => {
                return Err(CadpoleError::kernel("loft: a point section reached the skinner"))
            }
        }
    }
    let k = sections.len() - 1;
    // `edges[section][loop][segment]`, so a column is a fixed (loop, segment).
    let edges: Vec<Vec<Vec<truck_modeling::Edge>>> = sections
        .iter()
        .map(|ws| ws.iter().map(|w| w.edge_iter().cloned().collect()).collect())
        .collect();
    for (s, sec) in edges.iter().enumerate() {
        if sec.len() != edges[0].len() {
            return Err(CadpoleError::invalid(format!(
                "loft section {s} has {} loops but section 0 has {}",
                sec.len(),
                edges[0].len()
            )));
        }
        for (l, segs) in sec.iter().enumerate() {
            if segs.len() != edges[0][l].len() {
                return Err(CadpoleError::invalid(format!(
                    "loft section {s} has {} segments in loop {l} but section 0 has {}; sections \
                     pair segment by segment, so the counts have to agree (match=\"arclen\" \
                     splits them until they do)",
                    segs.len(),
                    edges[0][l].len()
                )));
            }
        }
    }

    // Every column's sections, made compatible, in the order the faces come.
    let mut nets = Vec::new();
    for (l, segs) in edges[0].iter().enumerate() {
        for e in 0..segs.len() {
            let curves = (0..=k).map(|s| edges[s][l][e].oriented_curve().lift_up()).collect();
            nets.push(crate::skin::make_compatible(curves)?);
        }
    }
    let vdir = crate::skin::VDirection::new(&nets, max_v_degree)?;

    // A spoke is shared by the two faces that meet along it, keyed by the pair
    // of vertices it joins — the same bookkeeping `try_wire_homotopy` does.
    let mut spokes: HashMap<
        (truck_modeling::VertexID, truck_modeling::VertexID),
        truck_modeling::Edge,
    > = HashMap::new();
    let mut faces = Vec::with_capacity(nets.len());
    let mut column = 0;
    for (l, segs) in edges[0].iter().enumerate() {
        for e in 0..segs.len() {
            let net = &nets[column];
            column += 1;
            let surface = vdir.skin(net)?;
            // The surface at `u = u_min` and `u = u_max` is the B-spline over
            // its first and last row of control points: the spoke curves, by
            // construction on the surface and through every section's corner.
            let rows = surface.control_points();
            let (first, last) = (rows[0].clone(), rows[rows.len() - 1].clone());
            let (bottom, top) = (&edges[0][l][e], &edges[k][l][e]);
            let front = spoke(&mut spokes, bottom.front(), top.front(), k == 1, vdir.knots(), first)?;
            let back = spoke(&mut spokes, bottom.back(), top.back(), k == 1, vdir.knots(), last)?;
            // Along the two end sections the boundary is the pieces a ruled
            // cap cut the segment into, when there are any: the same curve,
            // more vertices, so the cap's ribbon can share them.
            let mut boundary: Vec<truck_modeling::Edge> = rungs[0].chain(l, e);
            boundary.push(back);
            boundary.extend(rungs[k].chain(l, e).into_iter().rev().map(|edge| edge.inverse()));
            boundary.push(front.inverse());
            let boundary: Wire = boundary.into();
            faces.push(
                Face::try_new(
                    vec![boundary],
                    Surface::NurbsSurface(truck_modeling::NurbsSurface::new(surface)),
                )
                .map_err(|e| CadpoleError::kernel(format!("loft side face: {e}")))?,
            );
        }
    }
    Ok(faces)
}

/// The loft's skinner run through the wires of consecutive *stations* of a
/// sweep: one rung per station, the same `side_faces` — pairwise at
/// degree 1 for a ruled segment, one cubic skin for an approximated one —
/// so a sweep's seams are the loft's shared spokes and its far wire is the
/// last station's, ready to be swept on from.
pub(crate) fn skin_through_wires(stations: &[Vec<Wire>], max_v_degree: usize) -> Result<Vec<Face>> {
    let rungs: Vec<Rung<'_>> = stations
        .iter()
        .map(|ws| Rung::Wires { wires: ws.clone(), loops: Vec::new(), plane: Plane::XY })
        .collect();
    let refs: Vec<&Rung<'_>> = rungs.iter().collect();
    side_faces(&refs, max_v_degree)
}

/// Sweep `profile` (as drawn) along `path` under `frame`, with no twist or
/// scale, at the default tolerance: one solid per region of the profile
/// (`crate::sweep`). The kernel entry a chamfer cutter takes for an
/// arc or spline edge: the section in the
/// plane normal to the edge at its start, swept along it — a straight edge
/// is a `tsweep`, a circular one an `rsweep`, both exact, and a spline edge
/// a measured NURBS skin.
pub fn sweep_along_wire(profile: &Sketch, path: &Curve3, frame: crate::sweep::Frame) -> Result<Vec<KSolid>> {
    let spec = crate::sweep::SweepSpec {
        profile,
        path,
        frame,
        up: None,
        twist: 0.0,
        scale: 1.0,
        tolerance: DEFAULT_TOLERANCE,
    };
    Ok(crate::sweep::sweep(&spec)?.solids)
}

/// The edge from one section's corner vertex to the last section's, shared
/// between the two faces that meet along it.
///
/// With two sections the spoke *is* the straight line between the two
/// vertices — a degree-1 B-spline over `[0,0,1,1]` through two points is that
/// line — so it is built as one, which is both the honest representation and
/// what keeps a ruled loft's edges the `Curve::Line`s they have always been.
/// With more, it is the surface's own boundary curve.
fn spoke(
    map: &mut HashMap<(truck_modeling::VertexID, truck_modeling::VertexID), truck_modeling::Edge>,
    from: &Vertex,
    to: &Vertex,
    straight: bool,
    knots: &truck_modeling::KnotVec,
    control_points: Vec<truck_modeling::Vector4>,
) -> Result<truck_modeling::Edge> {
    let key = (from.id(), to.id());
    if let Some(e) = map.get(&key) {
        return Ok(e.clone());
    }
    let edge = if straight {
        builder::line(from, to)
    } else {
        let curve = truck_modeling::BSplineCurve::try_new(knots.clone(), control_points)
            .map_err(|e| CadpoleError::kernel(format!("loft spoke: {e}")))?;
        truck_modeling::Edge::new(from, to, Curve::NurbsCurve(truck_modeling::NurbsCurve::new(curve)))
    };
    map.insert(key, edge.clone());
    Ok(edge)
}

/// A planar cap whose outward normal points along `want`.
fn cap(ws: &[Wire], want: Vec3) -> Result<Face> {
    let f = builder::try_attach_plane(ws).map_err(|e| CadpoleError::kernel(format!("loft cap: {e}")))?;
    let outward = match f.oriented_surface() {
        Surface::Plane(p) => vector_to_vec3(p.normal()),
        _ => want,
    };
    Ok(if outward.dot(want) < 0.0 { f.inverse() } else { f })
}

/// The edge of one segment in space between two existing vertices. A helix
/// has no exact edge (it is not rational) and gets the cubic stand-in of
/// [`crate::curve::helix_edge_curve`], within `HELIX_EDGE_TOL` of it.
pub(crate) fn segment_edge(seg: &Segment3, from: &Vertex, to: &Vertex) -> truck_modeling::Edge {
    match seg {
        Segment3::Line { .. } => builder::line(from, to),
        Segment3::Arc { via, .. } => builder::circle_arc(from, to, via.point()),
        Segment3::Bezier { ctrl, .. } => builder::bezier(from, to, ctrl.iter().map(|c| c.point()).collect()),
        Segment3::Helix { .. } => match seg.helix() {
            Some(h) => truck_modeling::Edge::new(from, to, crate::curve::helix_edge_curve(&h)),
            None => builder::line(from, to),
        },
    }
}

/// Build a truck wire from a closed curve in space, sharing vertices between
/// consecutive segments so the wire is connected, and the seam vertex between
/// the last segment and the first — and, when a ruled cap closes this end,
/// the cap's cut loop as a second wire that shares every one of those
/// vertices and adds its own between them.
fn curve_to_wires(c: &Curve3, cap: Option<&RuledCap>) -> Result<(Wire, Option<CapWire>)> {
    if c.segments.is_empty() {
        return Err(CadpoleError::invalid("empty curve"));
    }
    if !c.closed {
        return Err(CadpoleError::invalid("loft: an open curve is not a section"));
    }
    let n = c.segments.len();
    let vertices: Vec<Vertex> = c.segments.iter().map(|s| builder::vertex(s.start().point())).collect();
    let mut wire = Wire::new();
    for (i, seg) in c.segments.iter().enumerate() {
        wire.push_back(segment_edge(seg, &vertices[i], &vertices[(i + 1) % n]));
    }
    let Some(rc) = cap else { return Ok((wire, None)) };
    let pieces = &rc.curve.segments;
    let m = pieces.len();
    if rc.owner.len() != m || m < n {
        return Err(CadpoleError::kernel("loft cap: the cut loop does not match its section"));
    }
    // A piece that starts a segment starts at that segment's own vertex;
    // the others start at a new vertex of their own.
    let starts: Vec<Vertex> = (0..m)
        .map(|i| {
            if i == 0 || rc.owner[i] != rc.owner[i - 1] {
                vertices[rc.owner[i]].clone()
            } else {
                builder::vertex(pieces[i].start().point())
            }
        })
        .collect();
    // A segment the cap did not cut is the section's own edge, the very same
    // one: the side face next to it and the ribbon must share it, or the
    // shell does not close.
    let originals: Vec<truck_modeling::Edge> = wire.edge_iter().cloned().collect();
    let mut cut = Wire::new();
    for (i, piece) in pieces.iter().enumerate() {
        let starts_segment = i == 0 || rc.owner[i] != rc.owner[i - 1];
        let ends_segment = i + 1 == m || rc.owner[i + 1] != rc.owner[i];
        if starts_segment && ends_segment {
            cut.push_back(originals[rc.owner[i]].clone());
        } else {
            cut.push_back(segment_edge(piece, &starts[i], &starts[(i + 1) % m]));
        }
    }
    Ok((wire, Some(CapWire { wire: cut, owner: rc.owner.clone() })))
}

/// [`stitch`] for a plan in the ordered layout: curve sections in
/// list order, skinned by the same [`side_faces`] — pairwise at degree 1 when
/// ruled, once at degree 3 when not, so `ruled=False` through curve sections
/// is the same skin — each end closed the way the plan says.
///
/// Orientation is settled in two steps. Within the shell it is *topological*:
/// every side face uses its lower wire's edges forwards and its upper wire's
/// backwards (that is how [`side_faces`] lays a face out, and how
/// `try_wire_homotopy` always did), so the start cap is built on the first
/// wire and inverted, and the end cap on the last wire as is; every edge is
/// then used once each way and `Solid::try_new`'s closedness check passes by
/// construction. Whether the whole thing faces out or in is then read off the
/// sign of its volume, and an inside-out solid has every face inverted. That
/// is deterministic and needs no notion of "up": reversing the list order, or
/// drawing every section the other way round, gives the same outward solid.
fn stitch_ordered(rungs: &[Rung<'_>], plan: &LoftPlan) -> Result<KSolid> {
    let mut shell = Shell::new();
    let last = rungs.len() - 1;
    if matches!(rungs[0], Rung::Apex(_)) && matches!(rungs[1], Rung::Apex(_)) {
        return Err(CadpoleError::invalid("loft: two point sections in a row is not a solid"));
    }
    if matches!(rungs[0], Rung::Curve { .. }) {
        shell.extend(cap_ordered(&rungs[0], plan.caps[0], plan.ruled_caps[0].as_ref(), false)?);
    }
    if let (Rung::Apex(v), Rung::Curve { wire, curve, .. }) = (&rungs[0], &rungs[1]) {
        shell.extend(apex_fan(wire, &curve_geoms(curve, false), v, false)?);
    }
    let profiles: Vec<&Rung<'_>> = rungs.iter().filter(|r| matches!(r, Rung::Curve { .. })).collect();
    if plan.ruled {
        for (k, pair) in profiles.windows(2).enumerate() {
            shell.extend(side_faces(pair, 1).map_err(|e| CadpoleError::kernel(format!("loft band {k}: {e}")))?);
        }
    } else if profiles.len() >= 2 {
        shell.extend(side_faces(&profiles, crate::skin::MAX_V_DEGREE)?);
    }
    if let (Rung::Curve { wire, curve, .. }, Rung::Apex(v)) = (&rungs[last - 1], &rungs[last]) {
        shell.extend(apex_fan(wire, &curve_geoms(curve, true), v, true)?);
    }
    if matches!(rungs[last], Rung::Curve { .. }) {
        shell.extend(cap_ordered(&rungs[last], plan.caps[1], plan.ruled_caps[1].as_ref(), true)?);
    }
    let mut solid = Solid::try_new(vec![shell]).map_err(|e| {
        CadpoleError::kernel(format!(
            "loft did not close into a solid ({e}); the sections may cross or touch each other"
        ))
    })?;
    let volume = soup_volume(&solid, ORIENT_TOL)?;
    if volume < 0.0 {
        solid.not();
    }
    Ok(solid)
}

/// The faces closing one end of an ordered loft: a planar cap on the wire, or
/// the ruled ribbon of [`ribbon`] on the cap's cut wire. `at_end` says which
/// end, which decides the edge direction the cap must use to close the shell.
fn cap_ordered(rung: &Rung<'_>, cap: Cap, ruled: Option<&RuledCap>, at_end: bool) -> Result<Vec<Face>> {
    let Rung::Curve { wire, curve, cap: cap_wire } = rung else {
        return Err(CadpoleError::kernel("loft cap: not a curve section"));
    };
    match cap {
        Cap::Plane => {
            let f = builder::try_attach_plane(&[wire.clone()]).map_err(|e| {
                CadpoleError::kernel(format!(
                    "loft cap: {e} (the section leaves its best-fit plane by up to {:.4} mm)",
                    planar_deviation_of(curve)
                ))
            })?;
            Ok(vec![if at_end { f } else { f.inverse() }])
        }
        Cap::Ruled => {
            let rc = ruled.ok_or_else(|| CadpoleError::kernel("loft cap: a ruled cap without a layout"))?;
            let cw = cap_wire.as_ref().ok_or_else(|| CadpoleError::kernel("loft cap: a ruled cap without its wire"))?;
            if at_end { ribbon(&cw.wire, rc) } else { ribbon(&cw.wire.inverse(), rc) }
        }
    }
}

/// How far a curve's control points stray from one plane, for a diagnostic.
fn planar_deviation_of(c: &Curve3) -> f64 {
    let pts: Vec<Vec3> = c.segments.iter().flat_map(|s| s.control_points()).collect();
    let centroid = pts.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / pts.len().max(1) as f64);
    let mut n = Vec3::ZERO;
    for i in 0..pts.len() {
        n = n + (pts[i] - centroid).cross(pts[(i + 1) % pts.len()] - centroid);
    }
    if n.length() < 1e-12 {
        return 0.0;
    }
    let n = n.normalized();
    let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        let h = (*p - centroid).dot(n);
        (lo.min(h), hi.max(h))
    });
    hi - lo
}

/// A closed loop ruled against itself (`cap="ruled"`): the loop's
/// edges `0..split` are one half, the rest — reversed — the other, and each
/// pair of matched edges gets the same ruled patch a loft band gets, with a
/// straight ruling edge between consecutive pairs. The wire's own edges are
/// the ribbon's boundary, so the cap shares them with the sides and the
/// shell closes.
///
/// The two corners — the seam and the half-way vertex — are where a ruling
/// would have zero length, and a patch pinched to a point there is the
/// construction that produces no triangles (and
/// `try_wire_homotopy` on two wires sharing a vertex would ask for a
/// zero-length edge, which truck refuses outright). So the plan classified
/// each corner ([`RibbonEnd`]): two collinear lines through it form the
/// neighbouring patch's straight ruling and need no face; two coplanar
/// segments get a planar face bounded by them and the chord between their far
/// ends, from `try_attach_plane`, which is exact because the plan checked the
/// plane.
///
/// Faces come out in order from the seam to the half-way point, which is the
/// order the namer plants `start#k` / `end#k` in.
fn ribbon(w: &Wire, rc: &RuledCap) -> Result<Vec<Face>> {
    let edges: Vec<truck_modeling::Edge> = w.edge_iter().cloned().collect();
    let m = edges.len();
    let k = rc.split;
    if m != 2 * k || k < 2 {
        return Err(CadpoleError::kernel(format!(
            "loft cap: the ribbon's halves have {k} and {} edges",
            m.saturating_sub(k)
        )));
    }
    let a: Vec<truck_modeling::Edge> = edges[..k].to_vec();
    let b: Vec<truck_modeling::Edge> = (0..k).map(|i| edges[m - 1 - i].inverse()).collect();
    // The straight rulings between consecutive pairs: `rulings[i]` runs from
    // the start of `a[i]` to the start of `b[i]`, for the interior vertices.
    let rulings: Vec<Option<truck_modeling::Edge>> =
        (0..=k).map(|i| (i >= 1 && i < k).then(|| builder::line(a[i].front(), b[i].front()))).collect();
    let ruling = |i: usize| rulings[i].clone().expect("an interior ruling");
    let [seam, far] = rc.ends;
    let mut faces = Vec::with_capacity(k + 2);
    for i in 0..k {
        if i == 0 {
            match seam {
                RibbonEnd::Straight => continue,
                RibbonEnd::Flat => {
                    faces.push(flat_tip(vec![a[0].clone(), ruling(1), b[0].inverse()], "seam")?);
                    continue;
                }
            }
        }
        if i == k - 1 {
            match far {
                RibbonEnd::Straight => continue,
                RibbonEnd::Flat => {
                    faces.push(flat_tip(
                        vec![a[k - 1].clone(), b[k - 1].inverse(), ruling(k - 1).inverse()],
                        "half-way point",
                    )?);
                    continue;
                }
            }
        }
        let mut boundary: Vec<truck_modeling::Edge> = vec![a[i].clone()];
        if i == k - 2 && far == RibbonEnd::Straight {
            boundary.push(a[k - 1].clone());
            boundary.push(b[k - 1].inverse());
        } else {
            boundary.push(ruling(i + 1));
        }
        boundary.push(b[i].inverse());
        if i == 1 && seam == RibbonEnd::Straight {
            boundary.push(b[0].inverse());
            boundary.push(a[0].clone());
        } else {
            boundary.push(ruling(i).inverse());
        }
        let c0 = a[i].oriented_curve().lift_up();
        let c1 = b[i].oriented_curve().lift_up();
        let surface = Surface::NurbsSurface(truck_modeling::NurbsSurface::new(
            truck_modeling::BSplineSurface::homotopy(c0, c1),
        ));
        faces.push(
            Face::try_new(vec![boundary.into()], surface)
                .map_err(|e| CadpoleError::kernel(format!("loft cap: ribbon piece {i}: {e}")))?,
        );
    }
    Ok(faces)
}

/// The planar face closing one corner of a ribbon: `try_attach_plane` on the
/// boundary, which takes the plane's normal from the boundary's winding.
fn flat_tip(boundary: Vec<truck_modeling::Edge>, where_: &str) -> Result<Face> {
    builder::try_attach_plane(&[boundary.into()])
        .map_err(|e| CadpoleError::kernel(format!("loft cap: the corner at the {where_}: {e}")))
}

/// How far a sampled point may sit from the cone a patch is supposed to be,
/// per millimetre of the patch's distance from the origin, before the patch
/// is not called one. A homotopy between two concentric arcs is that cone to
/// about 1e-13 of its coordinates; three decades of slack still leaves
/// anything a script could draw on purpose — an arc a micron off centre —
/// on the NURBS path, and the surface truck is then asked to invert boundary
/// points on agrees with the patch three decades inside its own 1e-6.
const CONE_EPS: f64 = 1e-9;

/// `s`, with every face whose NURBS patch is a cone re-planted on the
/// `RevolutedCurve` of one of its ruling lines: the same surface, exactly,
/// and the one truck tessellates as a strip. Only [`tessellate_reporting`]
/// sees the copy; the model keeps its patches.
///
/// **The defect.** A loft band between two concentric arcs is a cone (a
/// cylinder when the radii agree), and so is an extruded arc, and the
/// homotopy each is built on already *is* that cone — [`loft`] says why. But
/// it is a NURBS patch, and truck's NURBS tessellator refines a patch by
/// testing each cell against the bilinear interpolant of its four corners and
/// halving **both** parameters when the test fails. A cell of a cone fails
/// because of the arc's curvature, and the height direction is halved along
/// with it, so a band 1 mm tall meshed the same 64 × 60 grid as one 17 mm
/// tall — 7,552 triangles a half-band at tolerance 0.05, measured, and the
/// same again for the side of a 1 mm disc — and a solid of revolution lofted
/// through twelve circles was 300,000 triangles that every boolean and the
/// render BVH then paid for. A `RevolutedCurve` divides its two directions
/// independently — the ruling line into its two ends, the turn by the chord
/// rule — so the same face is a strip of two triangles per rim segment: 118
/// instead of 7,552 for that half-band.
///
/// **Why a copy, and why here.** The first cut of this planted the
/// `RevolutedCurve` on the loft's own faces, and `examples/lamp_shade.star`
/// stopped building: its shell is `subtract(cone, cone, kernel="brep")`, two
/// concentric cones 1.5 mm apart, and `truck_shapeops` cuts that pair of
/// NURBS patches but reports the pair of revolved lines as meeting "not
/// transversally". Tessellation is the only consumer that cares which
/// representation a cone has, so it is the only one that sees the change:
/// booleans, `faces(kind=)`, naming and the STEP writer all keep the patch
/// they always had. `mapped` rebuilds the topology in face order with every
/// edge still shared, so the mesh closes exactly as before, and
/// `crate::tessellate` pairs meshed faces with the model's by that order.
///
/// A patch is recognised, not remembered: [`cone_of`] asks the surface, so a
/// loft band, an extruded arc and a boolean's trimmed piece of either all
/// qualify, however they were made.
///
/// **And the same for a skinned loft**. `loft(ruled=False)` through a
/// stack of concentric circles is one patch, not a cone but still a surface of
/// revolution, and truck grids it for exactly the same reason: 21,550
/// triangles for the compressor wheel's hub where the 36-band ruled version
/// was 5,852. [`revolution_of`] is [`cone_of`]'s general case — the meridian
/// of the patch rather than a ruling line — and it is tried second, so a cone
/// is still recognised as a cone and nothing about the ruled path moves.
///
/// **And every other patch gets Cadpole's division**. The surfaces the
/// copy carries are [`KSurface`]s: a recognised cone or revolution keeps
/// truck's per-direction `RevolutedCurve` division, a plane its boundary-only
/// one, and the spline patches that remain — twisted blades, spline lofts,
/// arcs about different axes — divide per direction and per knot span by the
/// chordal bound and the normal bound of `crate::ksurface`, instead of
/// truck's both-directions halving. The same `mapped` call, the same face
/// order, the same shared edges; only the interior grid of those faces
/// changes.
///
/// **And the edges carry the normal bound too.** The copy is not `mapped`
/// but rebuilt edge by edge, the way truck's own `shell_tessellation`
/// does it, because an edge's polyline must know what the faces it bounds
/// need of it: a blade's straight ruling would otherwise stay two points
/// long whatever the faces' grids do ([`KSurface::edge_pieces`]). One
/// [`KCurve`] per edge id, shared by every face that bounds it, so the mesh
/// closes exactly as before.
pub(crate) fn with_cones_revolved(s: &KSolid, bounds: DivisionBounds) -> TessellationSolid {
    use truck_topology::{Edge, EdgeID, Face, Shell, Vertex, VertexID, Wire};
    let surface_of = |surface: &Surface| match recognise_revolved(surface) {
        Some(revolved) => KSurface::trucks(revolved),
        None => KSurface::bounded(surface.clone(), bounds),
    };
    // The faces' surfaces once, and which of them each edge bounds.
    let surfaces: Vec<KSurface> = s.face_iter().map(|f| surface_of(&f.surface())).collect();
    let mut bounded_by: HashMap<EdgeID<Curve>, Vec<usize>> = HashMap::new();
    for (fi, face) in s.face_iter().enumerate() {
        for wire in face.absolute_boundaries() {
            for edge in wire.edge_iter() {
                bounded_by.entry(edge.id()).or_default().push(fi);
            }
        }
    }
    let vmap: HashMap<VertexID<Point3>, Vertex<Point3>> = s.vertex_iter().map(|v| (v.id(), v.mapped(|p| *p))).collect();
    let mut emap: HashMap<EdgeID<Curve>, Edge<Point3, KCurve>> = HashMap::new();
    let mut fi = 0;
    let shells: Vec<Shell<Point3, KCurve, KSurface>> = s
        .boundaries()
        .iter()
        .map(|shell| {
            shell
                .face_iter()
                .map(|face| {
                    let wires: Vec<Wire<Point3, KCurve>> = face
                        .absolute_boundaries()
                        .iter()
                        .map(|wire| {
                            wire.edge_iter()
                                .map(|edge| {
                                    let new_edge = emap.entry(edge.id()).or_insert_with(|| {
                                        let (front, back) = (edge.absolute_front(), edge.absolute_back());
                                        let pieces = bounded_by
                                            .get(&edge.id())
                                            .map(|faces| {
                                                faces.iter().map(|&f| surfaces[f].edge_pieces(front.point(), back.point())).max().unwrap_or(1)
                                            })
                                            .unwrap_or(1);
                                        Edge::debug_new(&vmap[&front.id()], &vmap[&back.id()], KCurve::new(edge.curve(), pieces))
                                    });
                                    if edge.orientation() {
                                        new_edge.clone()
                                    } else {
                                        new_edge.inverse()
                                    }
                                })
                                .collect()
                        })
                        .collect();
                    let mut new_face = Face::debug_new(wires, surfaces[fi].clone());
                    fi += 1;
                    if !face.orientation() {
                        new_face.invert();
                    }
                    new_face
                })
                .collect()
        })
        .collect();
    // Unchecked, in every build: the copy has the kernel's own topology, and
    // a shell the kernel handed over open is the driver's to refuse with the
    // edges named ([`open_shell`]), not a debug assertion's to panic on
    // outside `guarded` (which is what `debug_new` did in a test build).
    truck_topology::Solid::new_unchecked(shells)
}

/// The solid truck's tessellator is handed: the model's topology over
/// [`KCurve`]s and [`KSurface`]s, built by [`with_cones_revolved`].
pub type TessellationSolid = truck_topology::Solid<Point3, KCurve, KSurface>;

/// What the tessellator will see a surface as: `plane`, `revolved` (a
/// recognised cone, cylinder or surface of revolution) or the surface's own
/// kind. For tests and `cadpole inspect`.
pub fn tessellation_kind(surface: &Surface) -> &'static str {
    match recognise_revolved(surface) {
        Some(Surface::Plane(_)) => "plane",
        Some(_) => "revolved",
        None => surface_kind(surface),
    }
}

/// What the tessellator meshes a patch as: the cone or surface of
/// revolution [`cone_of`] and [`revolution_of`] recognise (the loft's
/// layout, the curve along `u`), or the plane a flat patch lies in
/// ([`plane_of`]), or the patch itself under Cadpole's division.
///
/// A sweep's revolved faces are laid out the other way round — the
/// profile along `u`, the turn along `v` — and are *not* recognised
/// transposed on purpose: re-planted on a `RevolutedCurve`, the two such
/// faces a boolean had trimmed (a chamfer cutter's cylinder and cone, cut by
/// a plate) leave their shared groove-bottom arc open (4 and 16 unmatched
/// edges, measured), where the same faces as
/// NURBS patches under Cadpole's per-direction division mesh closed. The
/// division bounds a rational patch by sampling, so a cone or torus patch
/// costs a strip either way.
fn recognise_revolved(surface: &Surface) -> Option<Surface> {
    if let Some(r) = cone_of(surface).or_else(|| revolution_of(surface)) {
        return Some(r);
    }
    let Surface::NurbsSurface(ns) = surface else { return None };
    // A flat patch — a radial line swept round an axis — is a
    // plane, and truck meshes a plane from its boundary alone; under the
    // division it left seams open, measured on the same cutter.
    plane_of(ns)
}

/// The plane a **rational** NURBS patch lies in, when every control point
/// does (within [`CONE_EPS`] of the coordinates' scale) — oriented by the
/// patch's own parametric normal, so the face's orientation keeps its
/// meaning. Rational only: a flat rational patch is a radial line swung
/// about an axis (a sweep's annulus), which the division left open
/// along its seams; a flat *polynomial* patch is a loft's ruled band
/// between two straight lines, which has always meshed under the division
/// and whose fingerprints (`existing_lofts_are_unchanged_by_the_curve_section_path`)
/// are held to the bit.
fn plane_of(ns: &truck_modeling::NurbsSurface<truck_modeling::Vector4>) -> Option<Surface> {
    let cps: Vec<truck_modeling::Vector4> = ns.control_points().iter().flatten().copied().collect();
    if cps.iter().all(|p| (p.w - 1.0).abs() < 1e-12) {
        return None;
    }
    let pts: Vec<Vec3> = cps
        .iter()
        .map(|p| if p.w.abs() > 1e-12 { Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w) } else { Vec3::new(p.x, p.y, p.z) })
        .collect();
    if pts.len() < 3 {
        return None;
    }
    let ((u0, u1), (v0, v1)) = ns.range_tuple();
    let (pu, pv) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    let n = vector_to_vec3(ns.normal(pu, pv));
    if !n.length().is_finite() || n.length() < 0.5 {
        return None;
    }
    let origin = point_to_vec3(ns.subs(pu, pv));
    let eps = CONE_EPS * pts.iter().map(|p| p.length()).fold(1.0, f64::max);
    if pts.iter().any(|p| (*p - origin).dot(n).abs() > eps) {
        return None;
    }
    let su = vector_to_vec3(ns.uder(pu, pv));
    let sv = vector_to_vec3(ns.vder(pu, pv));
    if su.length() < 1e-12 || sv.length() < 1e-12 {
        return None;
    }
    // `Plane::new(origin, u, v)` has normal `(u − o) × (v − o)`: the patch's
    // own `S_u × S_v`.
    let plane = truck_modeling::Plane::new(origin.point(), (origin + su).point(), (origin + sv).point());
    (vector_to_vec3(plane.normal()).dot(n) > 0.0).then_some(Surface::Plane(plane))
}

/// The cone a NURBS patch is, as a surface of revolution — or `None` when it
/// is not one.
///
/// It is one when the patch is ruled between two circular arcs about a
/// single axis, with corresponding points at the same azimuth all along: then
/// the ruling line at the start sweeps, about that axis, through every other
/// ruling line, which is what a surface of revolution is. Any of those
/// failing — a boss's arc lofted to a corner's, an arc lofted to a line, a
/// spline — leaves a ruled surface that is not one, and the caller keeps the
/// patch. The samples are checked against a tolerance that scales with the
/// coordinates ([`CONE_EPS`]) and a comparison that a NaN fails.
///
/// The curve runs along `u` and the straight run along `v`, which is how
/// both `BSplineSurface::homotopy` and a translational sweep lay a patch out;
/// a sweep's revolved faces are laid out the other way and stay NURBS
/// ([`recognise_revolved`] says why).
fn cone_of(surface: &Surface) -> Option<Surface> {
    let Surface::NurbsSurface(ns) = surface else { return None };
    let ((u0, u1), (v0, v1)) = ns.range_tuple();
    let param = |u: f64, v: f64| (u0 + (u1 - u0) * u, v0 + (v1 - v0) * v);
    let at = |u: f64, v: f64| {
        let (pu, pv) = param(u, v);
        point_to_vec3(ns.subs(pu, pv))
    };
    const U: [f64; 9] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
    let a: Vec<Vec3> = U.iter().map(|&u| at(u, 0.0)).collect();
    let b: Vec<Vec3> = U.iter().map(|&u| at(u, 1.0)).collect();
    let eps = CONE_EPS * a.iter().chain(&b).map(|p| p.length()).fold(1.0, f64::max);
    let near = |p: Vec3, q: Vec3| (p - q).length() <= eps;
    // Ruled: between the two rails the patch is the straight line between them.
    for (i, &u) in U.iter().enumerate() {
        for v in [0.25, 0.5, 0.75] {
            if !near(at(u, v), a[i] + (b[i] - a[i]) * v) {
                return None;
            }
        }
    }
    // Both rails are arcs of circles about one axis.
    let (c0, r0, n0) = circle_through(a[0], a[4], a[8])?;
    let (c1, r1, n1) = circle_through(b[0], b[4], b[8])?;
    let on_circle = |p: Vec3, c: Vec3, r: f64, n: Vec3| {
        let d = p - c;
        (d.length() - r).abs() <= eps && d.dot(n).abs() <= eps
    };
    if !a.iter().all(|&p| on_circle(p, c0, r0, n0)) || !b.iter().all(|&p| on_circle(p, c1, r1, n1)) {
        return None;
    }
    let between = c1 - c0;
    if n0.cross(n1).length() > CONE_EPS || !near(between - n0 * between.dot(n0), Vec3::ZERO) {
        return None;
    }
    // Corresponding points sit at the same azimuth, so the rulings are radial.
    let radial = |p: Vec3| {
        let r = p - c0;
        let r = r - n0 * r.dot(n0);
        (r.length() > eps).then(|| r * (1.0 / r.length()))
    };
    for (&p, &q) in a.iter().zip(&b) {
        if (radial(p)? - radial(q)?).length() > CONE_EPS {
            return None;
        }
    }
    // The revolved surface's normal is `uder × vder`: the ruling direction
    // by the turning direction, which flips with the sign of the axis. Pick
    // the sign that agrees with the patch's own normal, so the face's
    // orientation flag means what it meant.
    let mid = at(0.5, 0.5);
    let (pu, pv) = param(0.5, 0.5);
    let want = vector_to_vec3(ns.normal(pu, pv));
    let ruling_dir = b[4] - a[4];
    let axis = if ruling_dir.cross(n0.cross(mid - c0)).dot(want) >= 0.0 { n0 } else { -n0 };
    // The seam goes on the side of the start away from the arc.
    let with_rotation = (a[1] - a[0]).dot(axis.cross(a[0] - c0)) > 0.0;
    let away = if with_rotation { -axis } else { axis };
    let ruling = Curve::Line(Line(seam_offset(a[0].point(), c0, away), seam_offset(b[0].point(), c0, away)));
    Some(Surface::RevolutedCurve(truck_modeling::Processor::new(RevolutedCurve::by_revolution(
        ruling,
        c0.point(),
        axis.vector(),
    ))))
}

/// The surface of revolution a NURBS patch is, with its own meridian as the
/// generating curve — or `None` when it is not one.
///
/// [`cone_of`]'s general case, and it exists for the same reason: truck meshes
/// a NURBS patch by halving **both** parameters whenever a cell fails its
/// flatness test, so the turn's curvature forces the meridian direction to be
/// subdivided with it, and a surface whose profile is a gentle bell is meshed
/// as though the profile were as curved as the circle. A `RevolutedCurve`
/// divides its two directions independently, which is the whole saving.
///
/// It is one when the patch turns about a single axis and only turns: every
/// row of constant `v` is an arc of a circle centred on that axis, all the
/// circles are coaxial, and corresponding points across the rows sit at the
/// same azimuth. A cone satisfies that too — this function would recognise
/// one — which is why [`with_cones_revolved`] asks [`cone_of`] first and keeps
/// that path, and its ruling line, exactly as it was.
///
/// The generator is the patch's own `u = u_min` isoparametric curve, which is
/// the first row of its control points over its v knot vector — the meridian,
/// exactly, in the same rational form. It is turned off the seam by
/// [`SEAM_OFFSET`] for the reason `seam_offset` gives.
///
/// The recognition is sampled (9 columns × 17 rows against [`CONE_EPS`]), like
/// [`cone_of`]'s: close enough that nothing a script draws on purpose sneaks
/// through, and three decades inside the tolerance truck will invert boundary
/// points on.
fn revolution_of(surface: &Surface) -> Option<Surface> {
    let Surface::NurbsSurface(ns) = surface else { return None };
    let ((u0, u1), (v0, v1)) = ns.range_tuple();
    let param = |u: f64, v: f64| (u0 + (u1 - u0) * u, v0 + (v1 - v0) * v);
    let at = |u: f64, v: f64| {
        let (pu, pv) = param(u, v);
        point_to_vec3(ns.subs(pu, pv))
    };
    const U: [f64; 9] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
    const ROWS: usize = 17;
    let rows: Vec<Vec<Vec3>> = (0..ROWS)
        .map(|j| {
            let v = j as f64 / (ROWS - 1) as f64;
            U.iter().map(|&u| at(u, v)).collect()
        })
        .collect();
    let eps = CONE_EPS * rows.iter().flatten().map(|p| p.length()).fold(1.0, f64::max);

    // A row that is one point is a pole: the profile reaches the axis there
    // (a sphere octant's). The axis comes from the first row that is
    // not one, and a pole row must lie on it.
    let is_pole = |row: &[Vec3]| row.iter().all(|p| (*p - row[0]).length() <= eps);
    let poles: Vec<bool> = rows.iter().map(|r| is_pole(r)).collect();
    let base = poles.iter().position(|p| !p)?;

    // Every row is an arc of a circle, and all of them share one axis.
    let (c0, _, n0) = circle_through(rows[base][0], rows[base][4], rows[base][8])?;
    let mut centres = Vec::with_capacity(ROWS);
    for (j, row) in rows.iter().enumerate() {
        if poles[j] {
            let d = row[0] - c0;
            if (d - n0 * d.dot(n0)).length() > eps {
                return None;
            }
            centres.push(row[0]);
            continue;
        }
        let (c, r, n) = circle_through(row[0], row[4], row[8])?;
        if n.cross(n0).length() > CONE_EPS {
            return None;
        }
        let between = c - c0;
        if (between - n0 * between.dot(n0)).length() > eps {
            return None;
        }
        for &p in row {
            let d = p - c;
            if (d.length() - r).abs() > eps || d.dot(n0).abs() > eps {
                return None;
            }
        }
        centres.push(c);
    }

    // The patch turns with `u` and with nothing else: corresponding points of
    // every row are at the same azimuth. Without this a patch could be a stack
    // of coaxial circles that slides round as it climbs — a twisted skin, not
    // a revolution.
    let radial = |p: Vec3, c: Vec3| {
        let r = p - c;
        let r = r - n0 * r.dot(n0);
        (r.length() > eps).then(|| r * (1.0 / r.length()))
    };
    for i in 0..U.len() {
        let want = radial(rows[base][i], centres[base])?;
        for (j, (row, c)) in rows.iter().zip(&centres).enumerate() {
            if poles[j] || j == base {
                continue;
            }
            if (radial(row[i], *c)? - want).length() > CONE_EPS {
                return None;
            }
        }
    }

    // The revolved surface's normal is `uder × vder`: the meridian's direction
    // by the turning direction, which flips with the sign of the axis. Pick the
    // sign that agrees with the patch's own normal, so the face's orientation
    // flag keeps its meaning. (`cone_of` says the same about its ruling.)
    let mid = at(0.5, 0.5);
    let (pu, pv) = param(0.5, 0.5);
    let want = vector_to_vec3(ns.normal(pu, pv));
    let meridian_dir = rows[ROWS - 1][4] - rows[0][4];
    let axis = if meridian_dir.cross(n0.cross(mid - c0)).dot(want) >= 0.0 { n0 } else { -n0 };
    let with_rotation = (rows[base][1] - rows[base][0]).dot(axis.cross(rows[base][0] - c0)) > 0.0;
    let away = if with_rotation { -axis } else { axis };

    // The meridian itself: the first row of control points over the v knots,
    // turned off the seam. The weights come along unchanged — turning a
    // homogeneous point about an axis turns its Euclidean point and leaves `w`
    // alone — so the generator is the same rational curve the patch has.
    let bsp = ns.non_rationalized();
    let meridian = bsp.control_points().first()?;
    let turned: Vec<truck_modeling::Vector4> = meridian
        .iter()
        .map(|p| {
            if p.w.abs() < 1e-12 {
                return *p;
            }
            let e = Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w);
            let q = point_to_vec3(seam_offset(e.point(), c0, away));
            truck_modeling::Vector4::new(q.x * p.w, q.y * p.w, q.z * p.w, p.w)
        })
        .collect();
    let curve = truck_modeling::BSplineCurve::try_new(bsp.knot_vecs().1.clone(), turned).ok()?;
    Some(Surface::RevolutedCurve(truck_modeling::Processor::new(RevolutedCurve::by_revolution(
        Curve::NurbsCurve(truck_modeling::NurbsCurve::new(curve)),
        c0.point(),
        axis.vector(),
    ))))
}

/// The circle through three points: centre, radius and the unit normal of
/// its plane, or `None` when they are collinear (or two coincide).
pub(crate) fn circle_through(p: Vec3, q: Vec3, r: Vec3) -> Option<(Vec3, f64, Vec3)> {
    let (a, b) = (p - r, q - r);
    let axb = a.cross(b);
    if !(axb.length() > 1e-9 * a.length() * b.length()) {
        return None;
    }
    let centre = r + (b * a.dot(a) - a * b.dot(b)).cross(axb) * (1.0 / (2.0 * axb.dot(axb)));
    Some((centre, (p - centre).length(), axb.normalized()))
}

/// How far, in radians, a revolved face's seam is turned away from the
/// vertex it would otherwise pass through. See [`seam_offset`].
const SEAM_OFFSET: f64 = 1e-4;

/// `p` turned by [`SEAM_OFFSET`] about the axis through `centre` along
/// `away`: where the ruling line of a revolved band or tip face is planted.
/// `away` is the face's axis of revolution signed so that a positive turn
/// leads away from the face's own arc.
///
/// truck inverts a point onto a `RevolutedCurve` by taking `acos` of the dot
/// product of the point's unit radial with the ruling line's, and `acos` is
/// ill-conditioned at ±1. A vertex *on* the ruling line has the ruling's own
/// radial, so the dot product is a unit vector's squared length — which
/// `normalize` leaves one ulp short of 1 for some radii — and
/// `acos(1 - 2e-16)` is 2e-8, not 0. At 60 mm that is a 1.3 µm rotation,
/// over truck's 1 µm `near`, so the point fails to invert and the whole face
/// is dropped from the mesh. Measured on the impeller's shroud: the ring at
/// r = 59.9 was dropped, the one at r = 60 happened to round the other way,
/// and nothing under 47 mm can reach the threshold. Every boundary vertex of
/// a band sits on a ruling line by construction, so the surface's own ruling
/// line is turned a hair off them instead: a tenth of a milliradian is a
/// hundred times the width of the ill-conditioned zone at a metre of radius,
/// and a hundredth of the shortest polyline step any export tolerance will
/// produce, so no vertex lands on the seam, or opposite it. The
/// turn leads away from the arc, so the face's parameter range stays on one
/// side of the seam.
fn seam_offset(p: Point3, centre: Vec3, away: Vec3) -> Point3 {
    let r = point_to_vec3(p) - centre;
    let (sin, cos) = fmath::sin_cos(SEAM_OFFSET);
    (centre + r * cos + away.cross(r) * sin + away * (away.dot(r) * (1.0 - cos))).point()
}

/// What the tip fan needs to know about one base segment: enough to give the
/// face an exact surface that is invertible at the apex.
enum SegGeom {
    /// A straight base: the face is a planar triangle.
    Line,
    /// A circular base about `centre`: the face is a cone about `axis`
    /// through the centre, and `axis` is signed so that rotating about it
    /// carries a base point *against* the way the face's wire traverses the
    /// base — which is what makes the revolved surface's normal point out.
    Arc { centre: Vec3, axis: Vec3 },
    /// A spline: no exact tip surface.
    Curved,
}

/// The tip-fan geometry of a planar loop, as it has always been computed:
/// the arc's centre through the plane, and the axis the plane's normal, signed
/// by which side the apex is on.
fn planar_geoms(profile: &Loop, plane: &Plane, apex_above: bool) -> Vec<SegGeom> {
    let axis = if apex_above { -plane.normal } else { plane.normal };
    profile
        .segments
        .iter()
        .map(|seg| match seg {
            Segment::Line { .. } => SegGeom::Line,
            Segment::Arc { a, b, via } => match arc_params(*a, *b, *via) {
                Some((c, _, _, _)) => SegGeom::Arc { centre: plane.to_world(c.x, c.y), axis },
                None => SegGeom::Curved,
            },
            Segment::Bezier { .. } => SegGeom::Curved,
        })
        .collect()
}

/// The tip-fan geometry of a curve section: an arc's circle in space, and the
/// axis its own normal, signed against the direction the face's wire runs
/// along it (from the segment's start when the apex is above, from its end
/// when below).
fn curve_geoms(curve: &Curve3, apex_above: bool) -> Vec<SegGeom> {
    curve
        .segments
        .iter()
        .map(|seg| match seg {
            Segment3::Line { .. } => SegGeom::Line,
            Segment3::Arc { a, b, via } => match arc3_params(*a, *b, *via) {
                Some(arc) => {
                    let n = arc.normal();
                    let (from, along) = if apex_above { (*a, arc.tangent(0.0)) } else { (*b, -arc.tangent(1.0)) };
                    let carries = n.cross(from - arc.centre);
                    let axis = if carries.dot(along) < 0.0 { n } else { -n };
                    SegGeom::Arc { centre: arc.centre, axis }
                }
                None => SegGeom::Curved,
            },
            Segment3::Bezier { .. } | Segment3::Helix { .. } => SegGeom::Curved,
        })
        .collect()
}

/// The fan of faces between a closed wire and a single point: a pyramid's
/// tip, or a cone's apex.
///
/// The obvious construction — the same `BSplineSurface::homotopy` every other
/// band uses, with a curve degenerate at the point — **does not work**. The
/// patch collapses to nothing along one parametric edge, truck's NURBS
/// `search_parameter` cannot invert it there, so the face's boundary never
/// closes in parameter space and the tessellator emits zero triangles: a solid
/// whose tip is missing and whose mesh is not watertight. (Measured: a 10 mm
/// square lofted to a point gave 2 triangles, all of them the base.)
///
/// So a tip face gets a surface that *is* invertible at the tip:
///
/// * a straight base segment and a point are coplanar, so the face is a real
///   `Plane` and the tip is an ordinary triangle;
/// * a circular base segment whose axis runs through the point sweeps a cone,
///   so the face is a `RevolutedCurve` — the same surface type truck's own
///   `builder::cone` produces, and its parameter search has a robust
///   nearest-point fallback that copes with the apex row.
///
/// Anything else (a spline segment, or an arc whose centre is off the tip's
/// axis) is refused with a diagnostic rather than silently built hollow.
///
/// `geoms` says which of those each base segment is — computed by the caller
/// from a planar loop ([`planar_geoms`]) or a curve in space
/// ([`curve_geoms`]), so the fan itself is the same for both. `apex_above`
/// says which side of the band the point is on, which decides the winding and
/// therefore which way every tip face faces.
fn apex_fan(wire: &Wire, geoms: &[SegGeom], apex: &Vertex, apex_above: bool) -> Result<Vec<Face>> {
    if wire.len() != geoms.len() {
        return Err(CadpoleError::kernel("loft tip: the wire and its loop disagree"));
    }
    let apex_pt = apex.point();
    let tip = point_to_vec3(apex_pt);
    let mut spokes: HashMap<truck_modeling::VertexID, truck_modeling::Edge> = HashMap::new();
    let mut spoke = |v: &Vertex| -> truck_modeling::Edge {
        spokes
            .entry(v.id())
            .or_insert_with(|| if apex_above { builder::line(v, apex) } else { builder::line(apex, v) })
            .clone()
    };
    let mut faces = Vec::with_capacity(wire.len());
    for (edge, geom) in wire.edge_iter().zip(geoms) {
        // `from` -> `to` is the direction the face's wire runs along the base.
        let (boundary, from, to) = if apex_above {
            (
                vec![edge.clone(), spoke(&edge.back()), spoke(&edge.front()).inverse()],
                edge.front().point(),
                edge.back().point(),
            )
        } else {
            (
                vec![spoke(&edge.back()), edge.inverse(), spoke(&edge.front()).inverse()],
                edge.back().point(),
                edge.front().point(),
            )
        };
        let surface = match geom {
            // Three points, one plane: the tip face is a triangle, and
            // `Plane::new(o, p, q)`'s normal is (p - o) x (q - o), which is the
            // outward one for this winding.
            SegGeom::Line => Surface::Plane(truck_modeling::Plane::new(from, to, apex_pt)),
            SegGeom::Arc { centre, axis } => {
                let (centre, axis) = (*centre, *axis);
                // The arc has to spin about the tip, or the band is not a cone.
                let off = (tip - centre) - axis * (tip - centre).dot(axis);
                if off.length() > 1e-6 {
                    return Err(CadpoleError::invalid(format!(
                        "loft to a point: the arc centred at ({:.4}, {:.4}, {:.4}) is {:.4} mm off \
                         the axis through the point, so the band between them is not a cone and \
                         has no exact surface. Keep a tip section concentric with its arcs, or \
                         end the loft on a small section instead of a point",
                        centre.x, centre.y, centre.z, off.length()
                    )));
                }
                // Off the base vertex by a hair, for the reason `seam_offset`
                // gives; the apex is on the axis and stays put.
                let spoke_curve = Curve::Line(Line(seam_offset(from, centre, axis), apex_pt));
                Surface::RevolutedCurve(truck_modeling::Processor::new(RevolutedCurve::by_revolution(
                    spoke_curve,
                    centre.point(),
                    axis.vector(),
                )))
            }
            SegGeom::Curved => {
                return Err(CadpoleError::invalid(
                    "loft to a point: a spline segment cannot be closed onto a tip exactly; end \
                     the loft on a small section instead of a point",
                ));
            }
        };
        faces.push(
            Face::try_new(vec![boundary.into()], surface)
                .map_err(|e| CadpoleError::kernel(format!("loft tip: {e}")))?,
        );
    }
    Ok(faces)
}

/// A truncated cone: radius `r0` at `at`, radius `r1` a height `h` above it,
/// axis +Z. `r1 == 0` makes a point.
///
/// This is a two-section [`loft`] of concentric circles, so the side is the
/// same exact rational surface a lofted corner gets and `r1 == 0` reuses the
/// same tip construction. Gridfinity needs it for the 0.2 mm × 45° lead-in
/// chamfer at the mouth of every magnet bore.
pub fn cone(at: Vec3, r0: f64, r1: f64, h: f64) -> Result<KSolid> {
    let plan = LoftPlan::new(&cone_sections(at, r0, r1, h)?, true, SectionMatch::Index)?;
    let mut solids = loft(&plan)?;
    if solids.len() != 1 {
        return Err(CadpoleError::kernel("cone: expected exactly one solid"));
    }
    Ok(solids.remove(0))
}

/// The two sections a cone is lofted through. Public because the namer has to
/// describe the very same loft the solid was built from.
pub fn cone_sections(at: Vec3, r0: f64, r1: f64, h: f64) -> Result<Vec<crate::loft::Section>> {
    if !(r0.is_finite() && r1.is_finite() && h.is_finite() && at.x.is_finite() && at.y.is_finite() && at.z.is_finite()) {
        return Err(CadpoleError::invalid("cone: radii, height and position must be finite"));
    }
    if h <= 0.0 {
        return Err(CadpoleError::invalid("cone: h must be positive"));
    }
    if r0 <= 0.0 {
        return Err(CadpoleError::invalid("cone: r0 must be positive (r1 may be 0 for a point)"));
    }
    if r1 < 0.0 {
        return Err(CadpoleError::invalid("cone: r1 must not be negative"));
    }
    let base = crate::loft::Section::Profile(
        Sketch::new(Plane { origin: Vec3::new(0.0, 0.0, at.z), ..Plane::XY })
            .with_loop(Loop::circle(P2::new(at.x, at.y), r0)),
    );
    let top = if r1 > 0.0 {
        crate::loft::Section::Profile(
            Sketch::new(Plane { origin: Vec3::new(0.0, 0.0, at.z + h), ..Plane::XY })
                .with_loop(Loop::circle(P2::new(at.x, at.y), r1)),
        )
    } else {
        crate::loft::Section::Point(Vec3::new(at.x, at.y, at.z + h))
    };
    Ok(vec![base, top])
}

/// Which kernel a boolean should use. `kernel=` on the script functions maps
/// straight onto this (docs/starlark-api.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KernelChoice {
    /// B-rep first, mesh kernel when the B-rep kernel fails or the pair is
    /// over budget. See `Context::boolean`.
    #[default]
    Auto,
    /// Exact B-rep only: a failure is a diagnostic, never a demotion to a mesh
    /// body. This is what a document that must export STEP asks for.
    Brep,
    /// Mesh kernel only: B-rep operands are tessellated at the document mesh
    /// tolerance first, and the result is a mesh body.
    Mesh,
}

impl KernelChoice {
    /// Every choice, in documentation order. `parse` reads this and so does
    /// the `kernel` vocabulary cadpole-script exports, so the two are one
    /// list; `all_lists_every_variant` fails to compile when a variant is
    /// missing here.
    pub const ALL: [KernelChoice; 3] = [KernelChoice::Auto, KernelChoice::Brep, KernelChoice::Mesh];

    pub fn name(self) -> &'static str {
        match self {
            KernelChoice::Auto => "auto",
            KernelChoice::Brep => "brep",
            KernelChoice::Mesh => "mesh",
        }
    }

    pub fn parse(s: &str) -> Result<KernelChoice> {
        match Self::ALL.into_iter().find(|k| k.name() == s) {
            Some(k) => Ok(k),
            None => {
                let names: Vec<String> = Self::ALL.iter().map(|k| format!("{:?}", k.name())).collect();
                let (last, rest) = names.split_last().expect("at least one kernel");
                Err(CadpoleError::invalid(format!("kernel must be {} or {last}, got {s:?}", rest.join(", "))))
            }
        }
    }
}

#[cfg(test)]
mod kernel_choice_tests {
    use super::KernelChoice;

    #[test]
    fn all_lists_every_variant_and_parse_reads_it() {
        for k in KernelChoice::ALL {
            match k {
                KernelChoice::Auto | KernelChoice::Brep | KernelChoice::Mesh => {}
            }
            assert_eq!(KernelChoice::parse(k.name()).unwrap(), k);
        }
        let e = KernelChoice::parse("cuda").unwrap_err().to_string();
        assert_eq!(e, r#"kernel must be "auto", "brep" or "mesh", got "cuda""#);
    }
}

/// Combined face count (both operands) above which `kernel="auto"` stops
/// calling truck and goes straight to the mesh kernel.
///
/// truck's boolean cost grows with the *accumulated* face count, not with the
/// size of the region the two bodies actually share: the six foot unions of
/// `examples/gridfinity_bin.star` cost 58, 118, 177, 235, 300 and 353 ms for
/// operand pairs of 321, 495, 669, 843, 1017 and 1191 faces,
/// while every other boolean in the same script finishes in under 25 ms on a
/// pair of at most 147 faces. 200 sits between the two, so the exact kernel
/// keeps all the work it is good at. The decision is taken *before* the call
/// because shapeops has no cancellation hook.
///
/// This is the whole of the routing that depends on size. It never depends
/// on wall-clock time: a timing rule would make the kernel a function of the
/// machine's load, so the same script would give a B-rep body on a quiet box
/// and a mesh body under load, and the browser build, where the clock reads
/// zero, would differ from native. Routing is a function of the document;
/// the wall clock's only say is the `Deadline`, which is a hard
/// diagnostic.
pub const BREP_FACE_BUDGET: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

impl BoolOp {
    pub fn kind_name(self) -> &'static str {
        match self {
            BoolOp::Union => "union",
            BoolOp::Subtract => "subtract",
            BoolOp::Intersect => "intersect",
        }
    }
}

/// Opt-in kernel tracing: `CADPOLE_TRACE=1` prints one line per kernel operation
/// to stderr with its operand sizes, tolerance and wall time. Used by
/// `cadpole-bench` and when investigating slow scripts; zero cost when off.
/// What the kernel decided, for the user.
///
/// The kernel makes choices a script never asked about and cannot see: a
/// boolean the exact engine refused ran on the robust engine and lost its face
/// provenance; the cleanup of a boolean's result stopped early; a tessellation
/// closed only at a tolerance other than the one requested. As
/// `CADPOLE_TRACE` lines on stderr those are invisible to anyone who has
/// not built the kernel with instrumentation. This is the channel that
/// carries them instead: a kernel
/// operation [`push`]es a sentence saying what happened *and what it means for
/// the user*, and [`Context`](crate::Context) [`drain`]s the sentences after
/// each feature it evaluates and attaches them to that feature as info
/// diagnostics, with the feature's span, exactly as it already reports "the
/// B-rep kernel could not union ...; retrying on the mesh kernel".
///
/// Thread-local, deliberately. An evaluation runs on one thread, so the
/// notes a feature's kernel calls push are the ones its drain collects, and
/// two evaluations on two threads (the server) never see each other's. No
/// clock, no counter, nothing that could make the same document evaluate
/// differently: a note is a consequence of a decision, never an input to one.
///
/// A note carries no feature id or span. The kernel does not know which
/// feature it is working for, and does not need to: attribution is the
/// drainer's job, which keeps the kernel free of the feature tree.
pub mod notes {
    use std::cell::RefCell;

    thread_local! {
        static NOTES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    /// Record one note for the feature being evaluated on this thread.
    pub fn push(note: impl Into<String>) {
        let note = note.into();
        NOTES.with(|n| n.borrow_mut().push(note));
    }

    /// Take every note pushed on this thread since the last drain, in order.
    pub fn drain() -> Vec<String> {
        NOTES.with(|n| std::mem::take(&mut *n.borrow_mut()))
    }

    /// True when nothing has been pushed since the last drain.
    pub fn is_empty() -> bool {
        NOTES.with(|n| n.borrow().is_empty())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn notes_come_back_in_order_and_once() {
            drain();
            assert!(is_empty());
            push("first");
            push(String::from("second"));
            assert!(!is_empty());
            assert_eq!(drain(), vec!["first", "second"]);
            assert!(drain().is_empty(), "a drain empties the sink");
        }

        #[test]
        fn notes_are_per_thread() {
            drain();
            push("mine");
            let theirs = std::thread::spawn(|| {
                push("theirs");
                drain()
            })
            .join()
            .unwrap();
            assert_eq!(theirs, vec!["theirs"]);
            assert_eq!(drain(), vec!["mine"], "another thread's notes never arrive here");
        }
    }
}

pub mod trace {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use crate::time::Stopwatch;

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static INIT: std::sync::Once = std::sync::Once::new();
    static TOTAL_US: AtomicU64 = AtomicU64::new(0);
    static COUNT: AtomicU64 = AtomicU64::new(0);

    pub fn enabled() -> bool {
        INIT.call_once(|| {
            let on = std::env::var("CADPOLE_TRACE").map(|v| v != "0" && !v.is_empty()).unwrap_or(false);
            ENABLED.store(on, Ordering::Relaxed);
        });
        ENABLED.load(Ordering::Relaxed)
    }

    /// Force tracing on/off (for tests and `cadpole-bench`).
    pub fn set_enabled(on: bool) {
        INIT.call_once(|| {});
        ENABLED.store(on, Ordering::Relaxed);
    }

    pub fn reset() {
        TOTAL_US.store(0, Ordering::Relaxed);
        COUNT.store(0, Ordering::Relaxed);
    }

    /// (total microseconds, operation count) since the last reset.
    pub fn totals() -> (u64, u64) {
        (TOTAL_US.load(Ordering::Relaxed), COUNT.load(Ordering::Relaxed))
    }

    pub struct Span {
        what: String,
        start: Stopwatch,
    }

    impl Span {
        pub fn start(what: impl Into<String>) -> Span {
            Span { what: what.into(), start: Stopwatch::start() }
        }
    }

    impl Drop for Span {
        fn drop(&mut self) {
            let us = self.start.elapsed_us();
            TOTAL_US.fetch_add(us, Ordering::Relaxed);
            COUNT.fetch_add(1, Ordering::Relaxed);
            if enabled() {
                eprintln!("kernel {:<58} {:>9.2} ms", self.what, us as f64 / 1000.0);
            }
        }
    }
}

/// Run a kernel operation, converting a panic inside truck into an error so
/// scripts get a diagnostic instead of a crash. The panic message is kept.
pub fn guarded<T>(what: &str, f: impl FnOnce() -> T + std::panic::UnwindSafe) -> Result<T> {
    // Quiet for the duration: the message is returned, not printed.
    let r = quietly(|| std::panic::catch_unwind(f));
    r.map_err(|e| {
        let msg = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown panic".to_owned());
        CadpoleError::kernel(format!("{what}: kernel failure: {msg}"))
    })
}

thread_local! {
    /// How many [`quietly`] calls this thread is inside.
    static QUIET: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// How many [`quietly`] sections are open on any thread. truck runs parts of
/// a boolean and of its tessellation on rayon's global pool, whose workers
/// cannot be wrapped; a panic on one of them while a section is open is the
/// section's, caught and reported on the calling thread.
static OPEN_QUIET: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// True on a thread inside [`quietly`], so a caller that fans work out to
/// threads of its own can carry the quiet onto them.
pub fn is_quiet() -> bool {
    QUIET.with(|q| q.get()) > 0
}

/// Run `f` with panics on *this thread* kept off stderr, for a caller that
/// catches them and reports their message itself ([`guarded`], and
/// `cadpole_import::bounded::run` on its parse thread).
///
/// One process-wide hook, installed once, that stays silent for a thread
/// inside `quietly` and hands every other panic to the hook that was there
/// before. The previous way — `take_hook`, `set_hook(silent)`,
/// `catch_unwind`, `set_hook(prev)` — swapped a process-global from several
/// threads at once: a sweep's threads saved each other's silent hooks, so a
/// caught panic leaked to stderr when another thread restored the default
/// first, and a real panic anywhere went unprinted while any thread was
/// inside a kernel call, or for good if the last restore was a silent hook.
///
/// Threads a quiet caller starts itself are quiet when it wraps their work
/// in `quietly` too ([`is_quiet`]); a rayon pool worker is quiet while any
/// section is open, since its work is always some caller's.
pub fn quietly<T>(f: impl FnOnce() -> T) -> T {
    use std::sync::atomic::Ordering;
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let pool_worker = || OPEN_QUIET.load(Ordering::SeqCst) > 0 && rayon_core::current_thread_index().is_some();
            if QUIET.with(|q| q.get()) == 0 && !pool_worker() {
                prev(info);
            }
        }));
    });
    /// Leaves the quiet section on every path out, unwinding included.
    struct Leave;
    impl Drop for Leave {
        fn drop(&mut self) {
            QUIET.with(|q| q.set(q.get().saturating_sub(1)));
            OPEN_QUIET.fetch_sub(1, Ordering::SeqCst);
        }
    }
    OPEN_QUIET.fetch_add(1, Ordering::SeqCst);
    QUIET.with(|q| q.set(q.get() + 1));
    let _leave = Leave;
    f()
}

/// Number of B-rep faces in a solid. This is the cost driver for
/// [`boolean`] and therefore the input to the routing budget.
pub fn face_count(s: &KSolid) -> usize {
    s.face_iter().count()
}

// ------------------------------------------------------- the contact pre-pass

/// How two solids sit relative to each other, decided *before* any boolean is
/// attempted: the cheap pre-pass.
///
/// `truck_shapeops` implements *transversal* booleans: it intersects the two
/// boundaries, cuts every face along the intersection curves and keeps the
/// pieces on the wanted side. Hand it a pair whose boundaries never meet and
/// there is nothing to cut, so every face falls into its "unknown" bucket and
/// is classified by a ray cast that is wrong for the inverted operand a
/// subtraction passes it — which is how `a − b` with `b` strictly inside `a`
/// came back as the cavity turned inside out. So the disjoint cases
/// are answered here and never reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// The two boundaries come within `tol` of each other somewhere: they
    /// cross, touch, or are coincident. The only case `truck_shapeops` sees.
    Meeting,
    /// `b` is strictly inside `a`'s material, boundaries more than `tol` apart.
    BInsideA,
    /// `a` is strictly inside `b`'s material.
    AInsideB,
    /// Neither is inside the other and the boundaries never come within `tol`.
    Separate,
}

/// Classify a pair of solids.
///
/// **Tolerance.** Both operands are tessellated at `tol` and the test is made
/// on the triangles, so it is exact only to the chordal sag of that
/// tessellation: two curved bodies whose true surfaces pass within `tol` of
/// each other are reported as [`Disposition::Meeting`] even if they are
/// strictly separate, and a pair reported separate is separate by at least
/// `tol` minus that sag. The bias is deliberate — [`Disposition::Meeting`]
/// is the conservative answer, because it is the one that goes on to
/// `truck_shapeops`, which is where a doubtful pair belongs.
pub fn disposition(a: &KSolid, b: &KSolid, tol: f64) -> Result<Disposition> {
    Ok(match sides(a, b, tol)? {
        None => Disposition::Meeting,
        Some(s) => s.disposition(),
    })
}

/// Where every shell of each operand lies relative to the other operand's
/// material, for a pair whose boundaries do not meet; `None` when they do.
///
/// One probe per *operand* is only sound for a single shell. A body with a
/// cavity, or one an earlier subtract cut into two lumps, has shells on
/// different sides of the other body: a block whose skin sits in a hollow
/// cube's wall while it covers the cube's cavity, or a box that swallows one
/// lump of a bar and clears the other. So each shell gets its own probe —
/// the centroid of its first triangle, more than `tol` from the other
/// boundary, so its side is the side of the whole shell — and the winding
/// number of the other operand's whole soup, voids included, says which.
pub(crate) struct Sides {
    /// Per shell of `a`: inside `b`'s material?
    a_in_b: Vec<bool>,
    /// Per shell of `b`: inside `a`'s material?
    b_in_a: Vec<bool>,
    /// Signed volume of each shell of `a`: positive for a lump, negative for
    /// a void.
    a_volume: Vec<f64>,
    b_volume: Vec<f64>,
}

impl Sides {
    fn disposition(&self) -> Disposition {
        let all = |v: &[bool]| v.iter().all(|&x| x);
        let any = |v: &[bool]| v.iter().any(|&x| x);
        let (a_any, b_any) = (any(&self.a_in_b), any(&self.b_in_a));
        if all(&self.b_in_a) && !a_any {
            Disposition::BInsideA
        } else if all(&self.a_in_b) && !b_any {
            Disposition::AInsideB
        } else if !a_any && !b_any {
            Disposition::Separate
        } else {
            // Mixed: some shells inside, some not (only possible with more
            // than one shell), or mutually enclosing (impossible for two
            // closed surfaces that do not meet, so degenerate). Neither is a
            // whole-operand answer; `Meeting` is the conservative one.
            Disposition::Meeting
        }
    }

    fn mixed(&self) -> bool {
        self.disposition() == Disposition::Meeting
    }
}

pub(crate) fn sides(a: &KSolid, b: &KSolid, tol: f64) -> Result<Option<Sides>> {
    let tol = if tol > 0.0 { tol } else { DEFAULT_BOOLEAN_TOLERANCE };
    let _span = trace::Span::start(format!("classify a={}f b={}f tol={tol}", face_count(a), face_count(b)));
    let (ta, sa) = shell_soup(a, tol)?;
    let (tb, sb) = shell_soup(b, tol)?;
    let (Some(ba), Some(bb)) = (soup_bbox(&ta), soup_bbox(&tb)) else {
        return Err(CadpoleError::kernel("a boolean operand has no surface to work with"));
    };
    // Cheap rejection first: boxes more than `tol` apart on any axis cannot
    // have surfaces within `tol` of each other.
    let near = boxes_within(ba, bb, tol);
    if near && boundaries_meet(&ta, &tb, tol) {
        return Ok(None);
    }
    // The boundaries are more than `tol` apart everywhere, so each *shell* is
    // wholly on one side of the other operand: one probe per shell settles it.
    let probe = |tris: &[[Vec3; 3]], range: &std::ops::Range<usize>| centroid(&tris[range.start]);
    let volume = |tris: &[[Vec3; 3]], range: &std::ops::Range<usize>| {
        tris[range.clone()].iter().map(|t| t[0].dot(t[1].cross(t[2]))).sum::<f64>() / 6.0
    };
    Ok(Some(Sides {
        a_in_b: sa.iter().map(|r| encloses(&tb, probe(&ta, r))).collect(),
        b_in_a: sb.iter().map(|r| encloses(&ta, probe(&tb, r))).collect(),
        a_volume: sa.iter().map(|r| volume(&ta, r)).collect(),
        b_volume: sb.iter().map(|r| volume(&tb, r)).collect(),
    }))
}

/// [`soup`], with the range of triangles each shell contributed, in
/// `boundaries()` order. Every shell has at least one triangle.
fn shell_soup(s: &KSolid, tol: f64) -> Result<(Vec<[Vec3; 3]>, Vec<std::ops::Range<usize>>)> {
    let meshed = tessellate(s, tol)?;
    let faces_per_shell: Vec<usize> = s.boundaries().iter().map(|sh| sh.len()).collect();
    if faces_per_shell.iter().sum::<usize>() != meshed.faces.len() {
        return Err(CadpoleError::kernel(format!(
            "a boolean operand meshed to {} faces but has {}, so its shells cannot be told apart",
            meshed.faces.len(),
            faces_per_shell.iter().sum::<usize>()
        )));
    }
    let mut tris = Vec::with_capacity(meshed.triangle_count());
    let mut ranges = Vec::with_capacity(faces_per_shell.len());
    let mut face = 0;
    for n in faces_per_shell {
        let start = tris.len();
        for f in face..face + n {
            for t in 0..meshed.faces[f].triangles.len() {
                let [p, q, r] = meshed.triangle_points(f, t);
                tris.push([point_to_vec3(p), point_to_vec3(q), point_to_vec3(r)]);
            }
        }
        face += n;
        if tris.len() == start {
            return Err(CadpoleError::kernel("a boolean operand has a shell with no surface to work with"));
        }
        ranges.push(start..tris.len());
    }
    Ok((tris, ranges))
}

/// Outward-oriented triangles of a solid's whole boundary: the outer shell
/// plus any voids, whose faces are inverted and so point into the void. That
/// orientation is what makes [`encloses`] read 1 in the material, 0 outside
/// *and* 0 inside a void.
pub(crate) fn soup(s: &KSolid, tol: f64) -> Result<Vec<[Vec3; 3]>> {
    let meshed = tessellate(s, tol)?;
    Ok(meshed.triangles().map(|t| [point_to_vec3(t[0]), point_to_vec3(t[1]), point_to_vec3(t[2])]).collect())
}

pub(crate) fn soup_bbox(t: &[[Vec3; 3]]) -> Option<(Vec3, Vec3)> {
    let mut lo = t.first()?[0];
    let mut hi = lo;
    for tri in t {
        for p in tri {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    Some((lo, hi))
}

fn centroid(t: &[Vec3; 3]) -> Vec3 {
    (t[0] + t[1] + t[2]) * (1.0 / 3.0)
}

/// Volume enclosed by a solid's whole boundary, from the same outward-oriented
/// triangles [`disposition`] classifies with: the divergence theorem, one
/// tetrahedron per triangle. A void's triangles face into it, so they subtract,
/// which is what makes this the volume of the *material*.
pub(crate) fn soup_volume(s: &KSolid, tol: f64) -> Result<f64> {
    Ok(soup(s, tol)?.iter().map(|t| t[0].dot(t[1].cross(t[2]))).sum::<f64>() / 6.0)
}

/// Do two boxes come within `tol` of each other on every axis? Its negation is
/// [`Disposition::Separate`] decided on boxes alone, which is the half of the
/// classification that costs nothing — and all
/// [`crate::mesh_kernel::boolean_many`] needs of a fan of tools.
pub(crate) fn boxes_within((alo, ahi): (Vec3, Vec3), (blo, bhi): (Vec3, Vec3), tol: f64) -> bool {
    let gap = |a0: f64, a1: f64, b0: f64, b1: f64| (b0 - a1).max(a0 - b1);
    gap(alo.x, ahi.x, blo.x, bhi.x) <= tol
        && gap(alo.y, ahi.y, blo.y, bhi.y) <= tol
        && gap(alo.z, ahi.z, blo.z, bhi.z) <= tol
}

/// "they are 4 mm apart on Y", when the boxes say so: the short form of the
/// gap a contact diagnostic names.
fn gap_phrase(a: &KSolid, b: &KSolid, tol: f64) -> String {
    let (Ok(ta), Ok(tb)) = (soup(a, tol), soup(b, tol)) else { return String::new() };
    let (Some((alo, ahi)), Some((blo, bhi))) = (soup_bbox(&ta), soup_bbox(&tb)) else {
        return String::new();
    };
    let gap = |a0: f64, a1: f64, b0: f64, b1: f64| (b0 - a1).max(a0 - b1);
    let axes = [
        ("X", gap(alo.x, ahi.x, blo.x, bhi.x)),
        ("Y", gap(alo.y, ahi.y, blo.y, bhi.y)),
        ("Z", gap(alo.z, ahi.z, blo.z, bhi.z)),
    ];
    match axes.iter().filter(|(_, g)| *g > 0.0).max_by(|x, y| x.1.total_cmp(&y.1)) {
        Some((axis, g)) => format!(" (their bounding boxes are {g:.3} mm apart on {axis})"),
        None => String::new(),
    }
}

/// Do the two boundaries come within `tol` of each other anywhere?
///
/// Sound in the direction that matters: the answer is `true` whenever any two
/// triangles are within `tol`, because two triangles that close have bounding
/// boxes that close, and this tests exactly that (the renderer's BVH does the
/// broad phase). It can also say `true` for boxes that are near while the
/// triangles inside them are not — a pair of slivers crossing at a distance —
/// and that costs nothing but a trip through `truck_shapeops`, which is where
/// such a pair went before this function existed.
fn boundaries_meet(a: &[[Vec3; 3]], b: &[[Vec3; 3]], tol: f64) -> bool {
    use crate::render::bvh::{Bvh, Tri};
    // The test is symmetric: index the smaller soup and query with the larger.
    let (a, b) = if a.len() < b.len() { (b, a) } else { (a, b) };
    let f32v = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
    let sub = |p: Vec3, q: Vec3| f32v(p - q);
    // Degenerate triangles are dropped by the BVH. They cannot hide contact:
    // their vertices belong to the neighbouring triangles that do get indexed.
    let tris: Vec<Tri> = b
        .iter()
        .enumerate()
        .map(|(i, t)| Tri { p0: f32v(t[0]), e1: sub(t[1], t[0]), e2: sub(t[2], t[0]), item: 0, tri: i as u32 })
        .collect();
    let bvh = Bvh::from_triangles(tris);
    // The BVH is f32: pad by an ulp or two at this model's scale on top of the
    // real tolerance, so rounding can only widen the test.
    let scale = soup_bbox(b).map(|(lo, hi)| (hi - lo).length()).unwrap_or(1.0);
    let pad = tol as f32 + (scale as f32) * 1e-6 + f32::EPSILON;
    let mut near = Vec::new();
    for t in a {
        let lo = t[0].min(t[1]).min(t[2]);
        let hi = t[0].max(t[1]).max(t[2]);
        let lo = [lo.x as f32 - pad, lo.y as f32 - pad, lo.z as f32 - pad];
        let hi = [hi.x as f32 + pad, hi.y as f32 + pad, hi.z as f32 + pad];
        bvh.overlapping(lo, hi, &mut near);
        if !near.is_empty() {
            return true;
        }
    }
    false
}

/// Is `p` inside the material bounded by this outward-oriented soup?
///
/// The generalised winding number (van Oosterom & Strackee's solid angle),
/// summed over the triangles: 1 in the material, 0 outside, and 0 again inside
/// a void, because a void's triangles face the other way. A ray cast would
/// need a tie-break for the ray that grazes an edge; a solid angle has none.
/// The caller has already shown `p` is not on the surface.
pub(crate) fn encloses(tris: &[[Vec3; 3]], p: Vec3) -> bool {
    let mut sum = 0.0;
    for t in tris {
        let (a, b, c) = (t[0] - p, t[1] - p, t[2] - p);
        let (la, lb, lc) = (a.length(), b.length(), c.length());
        if la == 0.0 || lb == 0.0 || lc == 0.0 {
            continue;
        }
        let num = a.dot(b.cross(c));
        let den = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
        sum += fmath::atan2(num, den);
    }
    // sum is 2π·winding; inside is ±2π, outside 0.
    (sum / (2.0 * std::f64::consts::PI)).abs() > 0.5
}

// ------------------------------------------------------------ canonical order

/// Reorder a solid's shells and faces by their geometry.
///
/// **Why.** `truck_shapeops::{and,or}` assemble their result with
/// `Shell::connected_components` (truck-topology 0.6.0, `src/shell.rs`), whose
/// `create_one_component` seeds each component from
/// `adjacency.keys().next()` — an `FxHashMap` whose key is `&Face`, and
/// `Hash for Face` (`src/face.rs:1046`) hashes `Arc::as_ptr(&self.surface)`.
/// The hasher is deterministic; the *addresses* are not, so the same boolean
/// over the same operands hands back the same faces in a different order on
/// every call, and the tessellation that follows inherits it. The same
/// address-ordered component walk also feeds
/// `FacesClassification::integrate_by_component`, which decides a whole
/// component's fate from `boundary[0][0]`, so the instability is not only
/// cosmetic. It is truck's, not ours, and this is the boundary where
/// we take it back: faces are sorted on a key made of nothing but their
/// geometry, and shells on their extent, biggest first — which also restores
/// truck's own "outer shell first" convention that `connected_components` does
/// not keep.
pub fn canonical(s: KSolid) -> KSolid {
    let mut shells = s.into_boundaries();
    for sh in &mut shells {
        sh.sort_by_cached_key(face_key);
    }
    shells.sort_by_cached_key(shell_key);
    // Only the order changed, so every shell is still closed and manifold.
    Solid::new_unchecked(shells)
}

/// An ordering key built from a face's geometry alone. Lexicographic over the
/// numbers, then the surface kind.
#[derive(PartialEq)]
struct GeomKey {
    nums: Vec<f64>,
    kind: &'static str,
}

impl Eq for GeomKey {}

impl Ord for GeomKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        for (a, b) in self.nums.iter().zip(&other.nums) {
            let o = a.total_cmp(b);
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        }
        self.nums.len().cmp(&other.nums.len()).then_with(|| self.kind.cmp(other.kind))
    }
}

impl PartialOrd for GeomKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Key of one face: its vertex box, then each boundary loop as the cycle of
/// its vertices rotated to start at the lexicographically smallest one, the
/// loops themselves sorted.
///
/// The rotation is what keeps the key independent of where truck happened to
/// start the wire. Traversal *direction* is kept, which is what separates two
/// faces that share a vertex cycle — the two halves of a cylinder — since in a
/// closed shell they must walk their shared edges opposite ways.
fn face_key(f: &truck_modeling::Face) -> GeomKey {
    let mut loops: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    for wire in f.boundaries() {
        let pts: Vec<[f64; 3]> = wire
            .vertex_iter()
            .map(|v| {
                let p = point_to_vec3(v.point());
                lo = lo.min(p);
                hi = hi.max(p);
                [p.x, p.y, p.z]
            })
            .collect();
        if pts.is_empty() {
            continue;
        }
        let start = (0..pts.len()).min_by(|&i, &j| cmp3(&pts[i], &pts[j])).unwrap_or(0);
        let mut cycle = pts[start..].to_vec();
        cycle.extend_from_slice(&pts[..start]);
        loops.push(cycle);
    }
    loops.sort_by(|a, b| {
        a.iter().zip(b).map(|(x, y)| cmp3(x, y)).find(|o| o.is_ne()).unwrap_or_else(|| a.len().cmp(&b.len()))
    });
    let mut nums = vec![lo.x, lo.y, lo.z, hi.x, hi.y, hi.z, loops.len() as f64];
    for l in &loops {
        nums.push(l.len() as f64);
        nums.extend(l.iter().flatten().copied());
    }
    GeomKey { nums, kind: surface_kind(&f.surface()) }
}

/// Key of one shell: the largest extent first (the outer shell of a solid with
/// voids), then the first face's key so ties are still decided by geometry.
fn shell_key(sh: &truck_modeling::Shell) -> GeomKey {
    let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    for f in sh.iter() {
        for wire in f.boundaries() {
            for v in wire.vertex_iter() {
                let p = point_to_vec3(v.point());
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
    }
    let d = hi - lo;
    let mut nums = vec![-(d.x * d.y * d.z), -d.length()];
    if let Some(f) = sh.first() {
        nums.extend(face_key(f).nums);
    }
    GeomKey { nums, kind: sh.first().map(|f| surface_kind(&f.surface())).unwrap_or("") }
}

fn cmp3(a: &[f64; 3], b: &[f64; 3]) -> std::cmp::Ordering {
    a[0].total_cmp(&b[0]).then_with(|| a[1].total_cmp(&b[1])).then_with(|| a[2].total_cmp(&b[2]))
}

/// Boolean operation between two solids.
///
/// The pair is classified twice before `truck_shapeops` sees it. First
/// [`disposition`]: every disjoint case — one body wholly inside the other, or
/// the two wholly apart — is answered here from the operands themselves.
/// Then, for a pair whose boundaries do meet,
/// [`crate::contact::plan`]: a pair that meets *transversally* is what
/// `shapeops` is written for and goes straight to it, and a pair with coplanar
/// faces is answered by one of the three contact operations —
/// [`crate::contact::glue`], a provably harmless over-extension of the tool, or
/// a diagnostic.
pub fn boolean(op: BoolOp, a: &KSolid, b: &KSolid, tol: f64) -> Result<KSolid> {
    let _span = trace::Span::start(format!(
        "{} a={}f b={}f tol={tol}",
        op.kind_name(),
        a.face_iter().count(),
        b.face_iter().count()
    ));
    resolve(op, a, b, tol, true)
}

/// `glue` on its own: what the Starlark `glue()` asks for, and what
/// [`boolean`] routes a touching union to. A pair that is not a clean
/// face-to-face touch is a diagnostic rather than a silent fallback, which is
/// the point of asking for it by name.
pub fn glue(a: &KSolid, b: &KSolid, tol: f64) -> Result<KSolid> {
    let _span = trace::Span::start(format!(
        "glue a={}f b={}f tol={tol}",
        a.face_iter().count(),
        b.face_iter().count()
    ));
    match disposition(a, b, tol)? {
        Disposition::Meeting => match crate::contact::plan(BoolOp::Union, a, b, tol)? {
            crate::contact::Plan::Glue(pairs) => crate::contact::glue(a, b, &pairs, tol),
            crate::contact::Plan::Transversal => Err(CadpoleError::kernel(
                "glue joins two bodies that touch on coincident faces, and these two do not \
                 share a face: they cross, or they only meet along an edge. Use union(), which \
                 is the kernel boolean for a pair that crosses.",
            )),
            crate::contact::Plan::Refuse(why) => Err(CadpoleError::kernel(why)),
            crate::contact::Plan::ExtendTool(_) | crate::contact::Plan::Touching => {
                unreachable!("a union plans neither an extension nor a touching cut")
            }
        },
        other => Err(CadpoleError::kernel(format!(
            "glue joins two bodies that touch on coincident faces, and these two do not touch \
             at all ({}). Use union(), which handles bodies that are apart or nested.",
            match other {
                Disposition::BInsideA => "the second is wholly inside the first",
                Disposition::AInsideB => "the first is wholly inside the second",
                _ => "their boundaries never come together",
            }
        ))),
    }
}

/// One boolean, after routing. `may_extend` is false on the second pass over a
/// tool that has already been grown, so the over-extension cannot loop.
fn resolve(op: BoolOp, a: &KSolid, b: &KSolid, tol: f64, may_extend: bool) -> Result<KSolid> {
    let Some(sides) = sides(a, b, tol)? else {
        return meeting(op, a, b, tol, may_extend);
    };
    if sides.mixed() {
        // Shells of one operand on both sides of the other: a cavity covered
        // without the boundaries meeting, or one lump of two swallowed. Two
        // single shells cannot get here except degenerately; that pair goes to
        // `shapeops`, as it always did.
        if a.boundaries().len() == 1 && b.boundaries().len() == 1 {
            return meeting(op, a, b, tol, may_extend);
        }
        return by_shells(op, a, b, &sides);
    }
    match sides.disposition() {
        Disposition::Meeting => unreachable!("a mixed classification was answered above"),
        Disposition::BInsideA => match op {
            // `a` keeps its own shells and gains `b`'s, inverted: a void.
            BoolOp::Subtract => by_shells(op, a, b, &sides),
            BoolOp::Intersect => Ok(b.clone()),
            BoolOp::Union => Ok(a.clone()),
        },
        Disposition::AInsideB => match op {
            BoolOp::Subtract => Err(CadpoleError::kernel(
                "subtract leaves nothing: the first body lies entirely inside the second, and \
                 Cadpole has no empty body. Check the operand order, or the sizes.",
            )),
            BoolOp::Intersect => Ok(a.clone()),
            BoolOp::Union => Ok(b.clone()),
        },
        Disposition::Separate => match op {
            BoolOp::Subtract => Ok(a.clone()),
            BoolOp::Intersect => Err(CadpoleError::kernel(format!(
                "intersect of two bodies that never touch is empty, and Cadpole has no empty \
                 body{}. Move them so they overlap, or drop the intersect.",
                gap_phrase(a, b, tol)
            ))),
            BoolOp::Union => Err(CadpoleError::kernel(format!(
                "union of two bodies that never touch{}: the exact kernel cannot hold two \
                 separate lumps in one solid (a B-rep solid is one outer shell plus its voids). \
                 Overlap them, join them with a third body, keep them as two bodies, or pass \
                 kernel=\"mesh\" to get one mesh body with two lumps.",
                gap_phrase(a, b, tol)
            ))),
        },
    }
}

/// A boolean of two bodies whose boundaries never meet, answered shell by
/// shell. With no crossing, the result's boundary is a selection of the
/// operands' own shells:
///
/// * union: the shells of each that are not inside the other's material;
/// * intersect: the shells of each that are inside the other's material;
/// * subtract: `a`'s shells not inside `b`, plus `b`'s shells inside `a`,
///   inverted.
///
/// Exact, since no face is cut. Two outcomes are refused rather than built:
/// a union that leaves more than one lump (the same rule as two bodies that
/// never touch), and a subtract that turns a void of `b` into a lump floating
/// inside a new void of `a`.
fn by_shells(op: BoolOp, a: &KSolid, b: &KSolid, sides: &Sides) -> Result<KSolid> {
    let mut shells = Vec::new();
    let mut lumps = 0;
    let (mut b_void_kept, mut b_lump_kept) = (false, false);
    let keep_a = |inside: bool| match op {
        BoolOp::Union | BoolOp::Subtract => !inside,
        BoolOp::Intersect => inside,
    };
    let keep_b = |inside: bool| match op {
        BoolOp::Union => !inside,
        BoolOp::Intersect | BoolOp::Subtract => inside,
    };
    for (i, sh) in a.boundaries().iter().enumerate() {
        if keep_a(sides.a_in_b[i]) {
            lumps += usize::from(sides.a_volume[i] > 0.0);
            shells.push(sh.clone());
        }
    }
    for (i, sh) in b.boundaries().iter().enumerate() {
        if keep_b(sides.b_in_a[i]) {
            let mut sh = sh.clone();
            if op == BoolOp::Subtract {
                sh.iter_mut().for_each(|f| {
                    f.invert();
                });
                // A void of `b` turns into a lump, a lump of `b` into a void.
                if sides.b_volume[i] < 0.0 {
                    b_void_kept = true;
                    lumps += 1;
                } else {
                    b_lump_kept = true;
                }
            } else {
                lumps += usize::from(sides.b_volume[i] > 0.0);
            }
            shells.push(sh);
        }
    }
    // `b`'s void, kept and inverted, is a lump; when `b`'s outer skin is kept
    // too it is a new void around that lump, so the lump floats in it.
    if b_void_kept && b_lump_kept {
        return Err(CadpoleError::kernel(
            "subtracting a body that itself has a cavity would leave an island of material \
             floating inside the void, which one B-rep solid cannot hold; pass kernel=\"mesh\" \
             for this one",
        ));
    }
    if shells.is_empty() || lumps == 0 {
        return Err(CadpoleError::kernel(format!(
            "{} leaves nothing: no material of either body survives it, and Cadpole has no empty \
             body. Check the operand order, or the sizes.",
            op.kind_name()
        )));
    }
    if op == BoolOp::Union && lumps > 1 {
        return Err(CadpoleError::kernel(format!(
            "union leaves {lumps} separate lumps, since the bodies' boundaries never meet: the \
             exact kernel cannot hold separate lumps in one solid (a B-rep solid is one outer \
             shell plus its voids). Overlap them, keep them as separate bodies, or pass \
             kernel=\"mesh\" to get one mesh body with several lumps."
        )));
    }
    // Every shell was closed, connected and manifold before; inverting one
    // does not change that. `try_new` so a surprise is a diagnostic.
    Solid::try_new(shells)
        .map(canonical)
        .map_err(|e| CadpoleError::kernel(format!("{} could not assemble its shells: {e}", op.kind_name())))
}

/// The boundaries meet. *How* they meet chooses the operation.
fn meeting(op: BoolOp, a: &KSolid, b: &KSolid, tol: f64, may_extend: bool) -> Result<KSolid> {
    use crate::contact::Plan;
    // Before anything else: one `truck` solid is one outer shell plus its
    // voids, and every operation below — `shapeops`, and `glue`'s stitch —
    // treats a shell as a lump. A body that already has a cavity is beyond all
    // of them, and saying so here keeps that the first thing the script hears
    // rather than whichever contact happened to be found first.
    if a.boundaries().len() > 1 || b.boundaries().len() > 1 {
        return Err(CadpoleError::kernel(format!(
            "{} of a body that already has a cavity is beyond the exact kernel: its boolean \
             treats every shell after the first as a separate lump. Pass kernel=\"mesh\" for \
             this one, or make the cavity last.",
            op.kind_name()
        )));
    }
    match crate::contact::plan(op, a, b, tol)? {
        Plan::Transversal => shapeops(op, a, b, tol),
        Plan::Glue(pairs) => crate::contact::glue(a, b, &pairs, tol),
        // The tool rests against the body without entering it.
        Plan::Touching => match op {
            BoolOp::Subtract => Ok(a.clone()),
            BoolOp::Intersect => Err(CadpoleError::kernel(
                "intersect of two bodies that only touch is a surface, not a solid, and Cadpole has \
                 no empty body. Overlap them by at least 0.5 mm, or drop the intersect.",
            )),
            BoolOp::Union => unreachable!("a union never plans a touching cut"),
        },
        Plan::ExtendTool(exts) if may_extend => {
            let mut tool = b.clone();
            for e in &exts {
                tool = crate::contact::extend(&tool, e, tol);
            }
            // The grown tool is a different solid, so it earns the same
            // pre-pass as any other operand: a cutter grown past a body it was
            // flush with may now contain it outright, and that answer comes
            // from `disposition`, not from `shapeops`.
            resolve(op, a, &tool, tol, false)
        }
        Plan::ExtendTool(_) => Err(CadpoleError::kernel(format!(
            "{}: the tool is still coplanar with the body after being grown clear of it once, \
             which means the two share more than one plane. Stand the tool proud of the body by \
             at least 0.5 mm, or pass kernel=\"mesh\" for this one",
            op.kind_name()
        ))),
        Plan::Refuse(why) => Err(CadpoleError::kernel(why)),
    }
}

/// The boundaries meet: hand the pair to `truck_shapeops`, at `tol` and,
/// when that fails, at finer tolerances.
///
/// Why a ladder. truck-shapeops' transversal intersection fails in narrow,
/// tolerance-dependent bands — a centred r = 3.3 hole through a 9 mm plate,
/// a cup of radius 36.25 — that are not contacts at all: no faces are
/// coplanar and none graze, and the same pair builds at a nearby tolerance
/// (measured: every such case reported builds at one of 0.05, 0.02 or 0.01).
/// The failure is inside the crate; until it is fixed there
/// a pair that fails at the requested tolerance is tried at 0.4x and 0.2x of
/// it. A finer tolerance is never a less accurate answer, only a slower one.
/// Every result is held to the same checks: a closed shell that meshes. A
/// graze that `contact::explain` can name is a real contact, not noise, and
/// is reported at once, after one attempt.
///
/// A finer rung is not free: truck at 0.04 mm is a good deal slower than at
/// 0.1, and a chain of cuts that each needed it ran many times longer than
/// the mesh fallback it replaced (32 small holes in a plate: 0.4 s before
/// the ladder, 15 s with it). So the finer rungs are used once per
/// evaluation: after one pair has been built at a finer
/// rung, or has failed every rung, later pairs get one attempt at the
/// requested tolerance, as before the ladder, and one truck cannot do is
/// refused (under `kernel="auto"`, sent to the mesh kernel). Outside an
/// evaluation ([`ladder::scope`]) the rungs are always tried.
fn shapeops(op: BoolOp, a: &KSolid, b: &KSolid, tol: f64) -> Result<KSolid> {
    let mut first: Option<CadpoleError> = None;
    let mut tried = Vec::new();
    // Rounded to a micrometre so the rungs read as written (0.04, not
    // 0.04000000000000001) in the notes and the error.
    let rung = |f: f64| (tol * f * 1e6).round() / 1e6;
    let rungs = [tol, rung(0.4), rung(0.2)];
    let climb = ladder::may_climb(tol);
    for i in 0..if climb { rungs.len() } else { 1 } {
        let t = rungs[i];
        match shapeops_at(op, a, b, t) {
            Ok(Some(s)) => {
                if i > 0 {
                    ladder::spend(tol);
                }
                if t != tol {
                    notes::push(format!(
                        "the exact kernel could not {} at the boolean tolerance {tol} mm (a \
                         numerical failure inside it, not a contact) and built it at {t} mm instead",
                        op.kind_name()
                    ));
                }
                return Ok(s);
            }
            Ok(None) => {
                if first.is_none() {
                    if let Some(why) = crate::contact::explain(op, a, b, tol) {
                        return Err(CadpoleError::kernel(why));
                    }
                    first = Some(CadpoleError::kernel(format!(
                        "{} failed: the boundaries meet, but not transversally. No two faces are \
                         coplanar and none of them graze, so this is a numerical failure inside \
                         the kernel rather than a contact it cannot express",
                        op.kind_name()
                    )));
                }
            }
            Err(e) => {
                if first.is_none() {
                    // A real contact fails as an error too (a panic, an open
                    // shell): name it, without the tolerance advice, whose
                    // fix it is not.
                    if let Some(why) = crate::contact::explain(op, a, b, tol) {
                        return Err(CadpoleError::kernel(format!("{}. {why}", kernel_message(&e).trim_end_matches('.'))));
                    }
                    first = Some(e);
                }
            }
        }
        tried.push(t);
    }
    if climb {
        ladder::spend(tol);
    }
    let e = first.expect("every rung of the ladder failed");
    Err(CadpoleError::kernel(format!(
        "{} (tried at {} mm). Try a different --boolean-tolerance, overlap the bodies more, \
         or pass kernel=\"mesh\" for this one",
        kernel_message(&e),
        tried.iter().map(|t| format!("{t}")).collect::<Vec<_>>().join(", ")
    )))
}

/// What the tolerance ladder in [`shapeops`] has learned in this
/// evaluation: per thread, as an evaluation runs on one thread, and cleared
/// at the start of each one.
pub mod ladder {
    use std::cell::Cell;

    thread_local! {
        /// `None` outside an evaluation (no limit); inside one, the boolean
        /// tolerance at which it has used up its finer rungs, if it has.
        static SPENT: Cell<Option<Option<f64>>> = const { Cell::new(None) };
        #[cfg(test)]
        pub(super) static ATTEMPTS: Cell<usize> = const { Cell::new(0) };
    }

    /// One evaluation's use of the finer rungs, for as long as this lives.
    pub struct Scope(Option<Option<f64>>);

    impl Drop for Scope {
        fn drop(&mut self) {
            SPENT.with(|s| s.set(self.0));
        }
    }

    /// Start an evaluation's budget on this thread; the guard restores what
    /// was there before.
    pub fn scope() -> Scope {
        Scope(SPENT.with(|s| s.replace(Some(None))))
    }

    /// Whether a pair at `tol` may still try the finer rungs.
    pub(super) fn may_climb(tol: f64) -> bool {
        SPENT.with(|s| s.get().flatten() != Some(tol))
    }

    /// The finer rungs at `tol` have been used: a pair built on one, or
    /// failed them all. Outside an evaluation there is nothing to spend.
    pub(super) fn spend(tol: f64) {
        SPENT.with(|s| {
            if s.get().is_some() {
                s.set(Some(Some(tol)));
            }
        });
    }
}

/// The sentence inside a kernel error, without the "kernel operation
/// failed:" its `Display` adds, so it can be extended.
fn kernel_message(e: &CadpoleError) -> String {
    let s = e.to_string();
    s.strip_prefix("kernel operation failed: ").map(str::to_owned).unwrap_or(s)
}

/// One attempt at `tol`. `Ok(None)` is truck's own "could not", which the
/// caller may retry or explain.
fn shapeops_at(op: BoolOp, a: &KSolid, b: &KSolid, tol: f64) -> Result<Option<KSolid>> {
    #[cfg(test)]
    ladder::ATTEMPTS.with(|n| n.set(n.get() + 1));
    // `and`/`or` walk `boundaries()` shell by shell and fold each one into the
    // running result as though it were another outer lump, so a solid with a
    // void — which is what a fully enclosed subtraction now makes — comes back
    // as `None` or as a shell that is not closed. Measured, not guessed:
    // subtracting a crossing post from a hollow 20 mm cube returns `None` for
    // `and` and panics inside `Solid::new` for `or`. `meeting` refuses that
    // pair before it gets here, which is why there is no check in this
    // function.
    let res = guarded(op.kind_name(), std::panic::AssertUnwindSafe(|| match op {
        BoolOp::Union => truck_shapeops::or(a, b, tol),
        BoolOp::Intersect => truck_shapeops::and(a, b, tol),
        BoolOp::Subtract => {
            let mut nb = b.clone();
            nb.not();
            truck_shapeops::and(a, &nb, tol)
        }
    }))?;
    let Some(solid) = res else {
        // The pre-pass found no coplanar faces and the kernel failed anyway;
        // the caller asks `contact::explain` whether two surfaces graze
        // and otherwise retries.
        return Ok(None);
    };
    // truck can return a solid whose shell is not closed/oriented; catch it
    // here rather than crashing later in tessellation.
    let ok = guarded(op.kind_name(), std::panic::AssertUnwindSafe(|| {
        !solid.boundaries().is_empty()
            && solid.boundaries().iter().all(|sh| sh.shell_condition() == truck_modeling::ShellCondition::Closed)
    }))?;
    if !ok {
        return Err(CadpoleError::kernel(format!(
            "{} produced an open or inconsistently oriented shell (coincident faces are a known kernel limitation; try overlapping the bodies slightly)",
            op.kind_name()
        )));
    }
    // Closed is not enough: truck can return a closed shell with an edge
    // whose geometry is wrong — the hole loop of a spot face's floor whose
    // samples zigzag across the circle, so the loop crosses itself in the
    // face's parameter domain. Returned as is, the boolean "succeeds" and the
    // document dies later, in tessellation, with no script location. So the
    // result is meshed here, at the boolean tolerance and at the default
    // display tolerance, and a face that cannot be meshed fails the boolean
    // that made it.
    let solid = canonical(solid);
    let mut checked = vec![tol];
    if DEFAULT_TOLERANCE != tol {
        checked.push(DEFAULT_TOLERANCE);
    }
    for t in checked {
        if let Err(e) = tessellate(&solid, t) {
            return Err(CadpoleError::kernel(format!(
                "{} produced a solid with a face that cannot be meshed (at {t} mm: {}). The \
                 boundaries cross, but the kernel traced an intersection edge wrongly; this is a \
                 numerical failure inside the kernel",
                op.kind_name(),
                kernel_message(&e)
            )));
        }
    }
    Ok(Some(solid))
}

/// `s` under `t`.
///
/// Not `builder::transformed`, because of one edge kind. truck's
/// `IntersectionCurve::transform_by` (truck-geometry 0.5,
/// `decorators/intersection_curve.rs`) ends with `self.tol *=
/// trans.norm_l2()`, and `Matrix4::norm_l2` (matext4cgmath 0.1) takes the
/// eigenvalues of MᵀM through `solve_quartic`, whose Newton polish has no
/// iteration cap. For a translation MᵀM has a double eigenvalue at 1, where
/// the derivative vanishes, and for ordinary offsets — a 43 mm move, or
/// (0, 6, 28.97) — the loop never meets its stop rule: one native call the
/// evaluation deadline cannot interrupt, spinning forever. The same line was
/// also wrong when it did return: the norm of the *homogeneous* matrix grows
/// with the translation column, so every move coarsened the curve's tolerance
/// by about the distance moved.
///
/// The tolerance is a length, so it scales by how much the map stretches
/// lengths: the operator norm of the linear 3x3 part, which is exactly 1 for a
/// rigid motion. [`linear_stretch`] computes it in closed form. Every other
/// curve, every surface and the intersection curve's own surfaces and leader
/// are transformed by truck as before; none of those paths touches `norm_l2`.
pub fn transformed(s: &KSolid, t: &Transform) -> KSolid {
    let mat = t.0;
    let stretch = linear_stretch(&mat);
    s.mapped(
        &move |p: &Point3| truck_modeling::Transform::transform_point(&mat, *p),
        &move |c: &Curve| match c {
            Curve::IntersectionCurve(ic) => {
                let mut ic = ic.clone();
                let ed = ic.editor();
                ed.surface0.transform_by(mat);
                ed.surface1.transform_by(mat);
                ed.leader.transform_by(mat);
                *ed.tol *= stretch;
                Curve::IntersectionCurve(ic)
            }
            other => other.transformed(mat),
        },
        &move |surface: &Surface| surface.transformed(mat),
    )
}

/// The largest factor by which `m`'s linear part stretches a length: the
/// spectral norm of its upper-left 3x3, √λmax(AᵀA). Closed form (the
/// trigonometric solution of a symmetric 3x3 eigenproblem), so it always
/// returns; 1 to rounding for any rotation, whatever the translation.
fn linear_stretch(m: &truck_modeling::Matrix4) -> f64 {
    // cgmath is column-major: m.x, m.y, m.z are the images of the axes.
    let cols = [
        Vec3::new(m.x.x, m.x.y, m.x.z),
        Vec3::new(m.y.x, m.y.y, m.y.z),
        Vec3::new(m.z.x, m.z.y, m.z.z),
    ];
    // B = AᵀA, symmetric positive semi-definite.
    let b = |i: usize, j: usize| cols[i].dot(cols[j]);
    let (b00, b11, b22, b01, b02, b12) = (b(0, 0), b(1, 1), b(2, 2), b(0, 1), b(0, 2), b(1, 2));
    let off = b01 * b01 + b02 * b02 + b12 * b12;
    let q = (b00 + b11 + b22) / 3.0;
    let lambda_max = if off == 0.0 {
        b00.max(b11).max(b22)
    } else {
        let (d0, d1, d2) = (b00 - q, b11 - q, b22 - q);
        let p2 = d0 * d0 + d1 * d1 + d2 * d2 + 2.0 * off;
        let p = fmath::sqrt(p2 / 6.0);
        // det((B - qI) / p) / 2, clamped against rounding into acos's domain.
        let (c00, c11, c22) = ((b00 - q) / p, (b11 - q) / p, (b22 - q) / p);
        let (c01, c02, c12) = (b01 / p, b02 / p, b12 / p);
        let det = c00 * (c11 * c22 - c12 * c12) - c01 * (c01 * c22 - c12 * c02) + c02 * (c01 * c12 - c11 * c02);
        let r = (det / 2.0).clamp(-1.0, 1.0);
        q + 2.0 * p * fmath::cos(fmath::acos(r) / 3.0)
    };
    fmath::sqrt(lambda_max.max(0.0))
}

/// Tessellate a solid with the given tolerance (mm).
/// Triangulate every face, and refuse to hand back a solid with a face
/// missing or a hole along a seam. [`tessellate_reporting`] also says which
/// tolerance the mesh was made at.
pub fn tessellate(s: &KSolid, tol: f64) -> Result<MeshedSolid> {
    tessellate_reporting(s, tol).map(|t| t.meshed)
}

/// What a tessellation is held to.
///
/// `tolerance` is the chordal sag in mm every edge polyline and every face
/// grid is within. `max_normal_degrees` is the second bound on
/// a spline patch's grid: the most the surface normal may turn across
/// one cell, so that the vertex normals a renderer interpolates across a
/// triangle can follow the surface — a bound the chordal one does not imply,
/// because a twisted ruled strip 1 mm wide sags well under the tolerance over
/// a 40 mm ruling while its normal turns 20°. Planes and the cone bands
/// `with_cones_revolved` re-expresses never see it. Non-positive disables it.
/// The default is [`crate::ksurface::DEFAULT_MAX_NORMAL_DEGREES`], which says
/// what it costs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TessellationOptions {
    pub tolerance: f64,
    pub max_normal_degrees: f64,
}

impl TessellationOptions {
    /// The tolerance asked for and the default normal bound.
    pub fn new(tolerance: f64) -> Self {
        TessellationOptions { tolerance, max_normal_degrees: DEFAULT_MAX_NORMAL_DEGREES }
    }

    fn bounds(&self) -> DivisionBounds {
        DivisionBounds::from_degrees(self.max_normal_degrees)
    }
}

/// [`tessellate`] with every bound spelled out.
pub fn tessellate_with(s: &KSolid, options: &TessellationOptions) -> Result<MeshedSolid> {
    tessellate_reporting_with(s, options).map(|t| t.meshed)
}

/// What [`tessellate_reporting`] made, and at what tolerance.
pub struct Tessellation {
    pub meshed: MeshedSolid,
    /// The tolerance the mesh was triangulated at: always the one asked for
    /// (see [`tessellate_reporting`]).
    pub tolerance: f64,
    /// How far the triangles sit off the exact surfaces, measured
    /// ([`chord_error`]). `None` only when no face had a surface to measure
    /// against.
    pub chord: Option<ChordError>,
}

/// The largest measured distance from a mesh triangle to the exact surface
/// it stands for, and where it is.
///
/// Measured, not bounded: for every triangle, its centroid and its three
/// edge midpoints — the four places a chord sags most — are each asked how
/// far they sit from the surface. The exact surface evaluated at the same
/// parametric point bounds that distance from above (any surface point
/// does), and is the answer itself wherever the parametrisation is
/// uniform — on a cylinder the edge midpoint's bound *is* the sagitta.
/// Where the bound could raise the body's maximum it is refined to the true
/// distance by Newton's nearest point from that parameter
/// ([`crate::ksurface::nearest_parameter`]), which is what a twisted spline
/// patch needs: its parametric midpoint is displaced along the surface as
/// well as off it. A plane contributes zero and is skipped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChordError {
    /// The largest distance found, in mm.
    pub max: f64,
    /// Index of the face it was found on, in the solid's face order (the
    /// same order [`crate::tessellate`] names faces in).
    pub face: usize,
    /// The mesh point — a triangle's centroid or an edge's midpoint — that
    /// sits farthest from the exact surface.
    pub at: Vec3,
}

/// The chord error of a tessellation: the meshed faces walked in step with
/// the surfaces they were triangulated from (both are in the solid's face
/// order, as [`with_cones_revolved`] guarantees), every triangle sampled at
/// its centroid and edge midpoints. truck evaluates a vertex's position as
/// `surface.subs(uv)`, so those `uv`s are the parameter space the exact
/// surface answers in.
///
/// Four surface evaluations per triangle, and a Newton search only for the
/// samples whose bound exceeds the largest error so far: on the compressor
/// wheel's 40 k triangles that is not measurable beside the triangulation.
pub fn chord_error(s: &TessellationSolid, meshed: &MeshedSolid) -> Option<ChordError> {
    const SAMPLES: [[f64; 3]; 4] =
        [[1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0], [0.5, 0.5, 0.0], [0.0, 0.5, 0.5], [0.5, 0.0, 0.5]];
    let mut best: Option<ChordError> = None;
    for (fi, (exact, face)) in s.face_iter().zip(meshed.faces.iter()).enumerate() {
        let surface = exact.surface();
        // A plane's triangles lie in it: nothing to measure.
        if matches!(surface.surface(), Surface::Plane(_)) {
            continue;
        }
        for tri in &face.triangles {
            let vs = [&face.vertices[tri[0] as usize], &face.vertices[tri[1] as usize], &face.vertices[tri[2] as usize]];
            let p = [
                meshed.positions[vs[0].id as usize],
                meshed.positions[vs[1].id as usize],
                meshed.positions[vs[2].id as usize],
            ];
            for [w0, w1, w2] in SAMPLES {
                let u = vs[0].uv[0] * w0 + vs[1].uv[0] * w1 + vs[2].uv[0] * w2;
                let v = vs[0].uv[1] * w0 + vs[1].uv[1] * w1 + vs[2].uv[1] * w2;
                let m = Point3::from_vec(p[0].to_vec() * w0 + p[1].to_vec() * w1 + p[2].to_vec() * w2);
                let bound = (surface.subs(u, v) - m).magnitude();
                if !bound.is_finite() || best.is_some_and(|b| bound <= b.max) {
                    continue;
                }
                let d = crate::ksurface::nearest_parameter(surface.surface(), m, SPHint2D::Parameter(u, v), 16)
                    .map(|(u2, v2)| (surface.subs(u2, v2) - m).magnitude())
                    .filter(|d| d.is_finite())
                    .map_or(bound, |d| d.min(bound));
                if best.map_or(true, |b| d > b.max) {
                    best = Some(ChordError { max: d, face: fi, at: point_to_vec3(m) });
                }
            }
        }
    }
    best
}

/// [`tessellate`], reporting the tolerance used.
///
/// The mesh is made by [`crate::brepmesh`]: every edge polylined
/// once, every face triangulated in its own parameter domain against its
/// edges' shared samples, so the surface is closed by construction and
/// never welded closed. There is no retry at another tolerance: with the
/// boundary shared by id there is nothing for a retry to change. A face
/// that cannot be
/// triangulated is a defect of the face's parameter image (a boundary that
/// crosses itself, a sample that does not lie on the surface), and this
/// refuses with the face, what the tessellator saw it as, where it is, and
/// the reason, rather than a mesh with a hole in it. A shell the kernel
/// handed over *open* is refused too ([`open_shell`]): every face of it
/// triangulates, and the hole is the shell's. `Tessellation::tolerance` is
/// therefore always the tolerance asked for.
pub fn tessellate_reporting(s: &KSolid, tol: f64) -> Result<Tessellation> {
    tessellate_reporting_with(s, &TessellationOptions::new(tol))
}

/// [`tessellate_reporting`] with every bound spelled out.
pub fn tessellate_reporting_with(s: &KSolid, options: &TessellationOptions) -> Result<Tessellation> {
    let tol = if options.tolerance > 0.0 { options.tolerance } else { DEFAULT_TOLERANCE };
    let _span = trace::Span::start(format!("tessellate {}f tol={tol}", s.face_iter().count()));
    // What is meshed: cone patches as strips, spline patches on Cadpole's
    // division, the rest as it is.
    let bounds = options.bounds();
    let s = &{
        let _span = trace::Span::start("tessellate: surfaces");
        with_cones_revolved(s, bounds)
    };
    let meshed = guarded("tessellate", std::panic::AssertUnwindSafe(|| crate::brepmesh::tessellate(s, tol, bounds)))?
        .map_err(|e| CadpoleError::kernel(format!("tessellation: {e}")))?;
    if let Some(hole) = open_shell(&meshed) {
        return Err(CadpoleError::kernel(format!("tessellation: {hole}")));
    }
    let chord = {
        let _span = trace::Span::start("tessellate: chord error");
        chord_error(s, &meshed)
    };
    Ok(Tessellation { meshed, tolerance: tol, chord })
}

/// Whether a tessellation has a hole that a weld does not close, and if so
/// what to say about it: how many edges, on which faces.
///
/// Every face is triangulated against its edges' shared samples, so a
/// closed shell meshes closed on the vertex ids alone and this is one pass
/// over the triangles ([`crate::brepmesh::MeshedSolid::unmatched_edge_count`]).
/// A shell the kernel hands over *open* — an exact boolean that came back
/// with a gap along an intersection (a hip roof on a trapezoid at
/// 12:12, cut on the exact kernel: seven edges along a hip) — meshes face
/// by face all the same, gap included, because nothing in a face's own
/// triangulation can see that its neighbour is missing. The ops that mesh
/// their own result to validate it (the chamfer, the fillet)
/// rely on this refusal. An edge the shell carries twice at one place (two
/// topological edges, one position) is open on the ids and closed by a
/// weld, so only a hole the weld does not close is named.
fn open_shell(meshed: &crate::brepmesh::MeshedSolid) -> Option<String> {
    if meshed.unmatched_edge_count() == 0 {
        return None;
    }
    let mut mesh = crate::tessellate::Mesh::default();
    mesh.positions = meshed.positions.iter().flat_map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect();
    mesh.normals = vec![0.0; mesh.positions.len()];
    for (fi, face) in meshed.faces.iter().enumerate() {
        for tri in &face.triangles {
            mesh.indices.extend(tri.iter().map(|&k| face.vertices[k as usize].id));
            mesh.face_ids.push(fi as u32);
        }
    }
    let welded = mesh.welded(crate::mesh_kernel::WELD_EPS);
    let mut edges: HashMap<(u32, u32), (i32, u32)> = HashMap::new();
    for (t, &face) in welded.indices.chunks_exact(3).zip(&welded.face_ids) {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let key = if a < b { (a, b) } else { (b, a) };
            edges.entry(key).or_insert((0, face)).0 += if a < b { 1 } else { -1 };
        }
    }
    // Sorted, so the words are the same on every run and on both targets:
    // the map's own order is neither.
    let mut open: Vec<(u32, (u32, u32))> = edges.iter().filter(|(_, (count, _))| *count != 0).map(|(&key, &(_, face))| (face, key)).collect();
    if open.is_empty() {
        return None;
    }
    open.sort_unstable();
    let mut per_face: Vec<(u32, usize)> = Vec::new();
    for &(face, _) in &open {
        match per_face.last_mut() {
            Some((f, n)) if *f == face => *n += 1,
            _ => per_face.push((face, 1)),
        }
    }
    let mut faces: Vec<String> = per_face
        .iter()
        .take(4)
        .map(|&(f, n)| format!("face {f} ({}, {n})", meshed.faces[f as usize].kind))
        .collect();
    if per_face.len() > 4 {
        faces.push(format!("{} more", per_face.len() - 4));
    }
    let (_, (a, b)) = open[0];
    let at = |i: u32| {
        let i = i as usize * 3;
        [welded.positions[i], welded.positions[i + 1], welded.positions[i + 2]]
    };
    let (pa, pb) = (at(a), at(b));
    let n = open.len();
    Some(format!(
        "the shell is open: {n} edge{} of its mesh {} on one face only ({}; the first at ({:.3}, {:.3}, {:.3})), \
         and no weld within {} mm closes {}. Every face triangulated, so the hole is in the shell the kernel \
         handed over, not in its mesh, and a mesh with a hole in it is not returned",
        if n == 1 { "" } else { "s" },
        if n == 1 { "lies" } else { "lie" },
        faces.join(", "),
        (pa[0] + pb[0]) / 2.0,
        (pa[1] + pb[1]) / 2.0,
        (pa[2] + pb[2]) / 2.0,
        crate::mesh_kernel::WELD_EPS,
        if n == 1 { "it" } else { "them" },
    ))
}

/// The faces whose triangles carry unmatched edges in a tessellation at
/// `tol`, each with what the tessellator saw the face as and how many of
/// its edges are unmatched — judged on the vertex ids, no weld. Empty for a
/// closed mesh, and empty when the tessellation is refused outright. A
/// probe for tests.
#[cfg(test)]
pub(crate) fn open_edge_faces(s: &KSolid, tol: f64) -> Vec<(usize, &'static str, usize)> {
    let bounds = DivisionBounds::from_degrees(DEFAULT_MAX_NORMAL_DEGREES);
    let ts = with_cones_revolved(s, bounds);
    let Ok(meshed) = crate::brepmesh::tessellate(&ts, tol, bounds) else { return Vec::new() };
    let (_, per_face) = meshed.unmatched_edges();
    per_face.iter().enumerate().filter(|(_, n)| **n > 0).map(|(f, &n)| (f, meshed.faces[f].kind, n)).collect()
}

pub fn surface_kind(s: &Surface) -> &'static str {
    match s {
        Surface::Plane(_) => "plane",
        Surface::BSplineSurface(_) => "bspline",
        Surface::NurbsSurface(_) => "nurbs",
        Surface::RevolutedCurve(_) => "revolved",
    }
}

pub fn curve_kind(c: &Curve) -> &'static str {
    match c {
        Curve::Line(_) => "line",
        Curve::BSplineCurve(_) => "bspline",
        Curve::NurbsCurve(_) => "nurbs",
        Curve::IntersectionCurve(_) => "intersection",
    }
}

pub fn point_to_vec3(p: Point3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

pub fn vector_to_vec3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_sketch(w: f64, h: f64) -> Sketch {
        Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, h))
    }

    #[test]
    fn extrude_rect_makes_six_faces() {
        let solids = extrude(&box_sketch(10.0, 20.0), 5.0).unwrap();
        assert_eq!(solids.len(), 1);
        assert_eq!(solids[0].face_iter().count(), 6);
    }

    #[test]
    fn extrude_with_hole_makes_extra_faces() {
        let s = box_sketch(10.0, 10.0).with_loop(Loop::circle(P2::new(0.0, 0.0), 2.0));
        let solids = extrude(&s, 5.0).unwrap();
        assert_eq!(solids.len(), 1);
        // 6 box faces + 2 half-cylinder faces.
        assert_eq!(solids[0].face_iter().count(), 8);
        let mesh = tessellate(&solids[0], 0.05).unwrap();
        assert_eq!(mesh.faces.len(), 8);
    }

    #[test]
    fn subtract_removes_material() {
        let a = extrude(&box_sketch(10.0, 10.0), 10.0).unwrap().remove(0);
        let b = extrude(
            &Sketch::new(Plane::XY.offset(-1.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 4.0, 4.0)),
            12.0,
        )
        .unwrap()
        .remove(0);
        let r = boolean(BoolOp::Subtract, &a, &b, 0.05).unwrap();
        assert_eq!(r.face_iter().count(), 10);
    }

    #[test]
    fn spline_loop_extrudes_to_a_solid() {
        let pts = [P2::new(10.0, 0.0), P2::new(0.0, 8.0), P2::new(-10.0, 0.0), P2::new(0.0, -8.0)];
        let s = Sketch::new(Plane::XY).with_loop(Loop::spline(&pts, 0.5).unwrap());
        let solids = extrude(&s, 5.0).unwrap();
        assert_eq!(solids.len(), 1);
        assert_eq!(solids[0].face_iter().count(), 6);
        let (mesh, _) = crate::tessellate::tessellate(&solids[0], 0.05).unwrap();
        assert!(mesh.triangle_count() > 12);
    }

    #[test]
    fn coplanar_boolean_is_an_error_not_a_panic() {
        // Two boxes sharing their z=0 face: truck either fails or yields an
        // open shell; either way we must get an Err, never a panic.
        let a = extrude(&box_sketch(10.0, 10.0), 10.0).unwrap().remove(0);
        let b = extrude(&box_sketch(4.0, 4.0), 5.0).unwrap().remove(0);
        for op in [BoolOp::Union, BoolOp::Subtract, BoolOp::Intersect] {
            match boolean(op, &a, &b, 0.05) {
                Ok(s) => assert!(tessellate(&s, 0.05).is_ok(), "{op:?} produced an untessellatable solid"),
                Err(e) => assert!(!e.to_string().is_empty()),
            }
        }
    }

    // ------------------------------------------------------------------ loft

    use crate::loft::{LoftPlan, Section, SectionMatch};

    fn section(z: f64, l: Loop) -> Section {
        Section::Profile(Sketch::new(Plane::XY.offset(z)).with_loop(l))
    }

    fn square(z: f64, side: f64) -> Section {
        section(z, Loop::rect(P2::new(0.0, 0.0), side, side))
    }

    fn built(sections: &[Section]) -> KSolid {
        let plan = LoftPlan::new(sections, true, SectionMatch::Index).expect("a valid plan");
        let mut solids = loft(&plan).expect("a solid");
        assert_eq!(solids.len(), 1);
        solids.remove(0)
    }

    /// Volume and watertightness of a lofted solid, from the same mesh the
    /// exporter and the print checks would see.
    fn measure(s: &KSolid) -> (f64, bool, usize) {
        let (mesh, topo) = crate::tessellate::tessellate(s, 0.01).expect("tessellation");
        let welded = mesh.welded(1e-4);
        (welded.volume().abs(), welded.is_watertight(), topo.faces.len())
    }

    #[test]
    fn a_loft_of_two_identical_sections_is_a_prism() {
        let s = built(&[square(0.0, 10.0), square(4.0, 10.0)]);
        let (v, tight, faces) = measure(&s);
        assert!((v - 400.0).abs() < 1e-6, "volume {v}, expected 400");
        assert!(tight, "a loft must be watertight");
        assert_eq!(faces, 6, "4 sides + 2 caps");
    }

    #[test]
    fn a_loft_of_a_square_to_a_point_is_a_pyramid() {
        let s = built(&[square(0.0, 10.0), Section::Point(Vec3::new(0.0, 0.0, 9.0))]);
        let (v, tight, faces) = measure(&s);
        // base area x height / 3
        assert!((v - 300.0).abs() < 1e-6, "volume {v}, expected 300");
        assert!(tight, "a pyramid must be watertight");
        assert_eq!(faces, 5, "4 triangles + 1 cap");
    }

    #[test]
    fn a_loft_between_two_squares_is_a_frustum_of_the_textbook_volume() {
        for (a, b, h) in [(10.0, 4.0, 6.0), (4.0, 10.0, 6.0), (10.0, 0.2, 3.0)] {
            let s = built(&[square(0.0, a), square(h, b)]);
            let (v, tight, _) = measure(&s);
            // h/3 (A0 + A1 + sqrt(A0 A1))
            let want = h / 3.0 * (a * a + b * b + a * b);
            assert!((v - want).abs() < 1e-6 * want.max(1.0), "{a}->{b}: volume {v}, expected {want}");
            assert!(tight);
        }
    }

    #[test]
    fn reversing_the_sections_builds_the_same_solid_and_mirroring_them_mirrors_it() {
        let up = built(&[square(0.0, 10.0), square(6.0, 4.0)]);
        // Same sections, listed the other way round: the same solid.
        let down = built(&[square(6.0, 4.0), square(0.0, 10.0)]);
        let (v0, t0, f0) = measure(&up);
        let (v1, t1, f1) = measure(&down);
        assert!((v0 - v1).abs() < 1e-9, "{v0} vs {v1}");
        assert_eq!(f0, f1);
        assert!(t0 && t1);

        // Sections swapped between the two heights: the z-mirror of the first.
        let mirrored = built(&[square(0.0, 4.0), square(6.0, 10.0)]);
        let (v2, _, _) = measure(&mirrored);
        assert!((v0 - v2).abs() < 1e-9, "a mirror has the same volume: {v0} vs {v2}");
        let (lo, hi) = crate::tessellate::tessellate(&mirrored, 0.01).unwrap().0.bbox().unwrap();
        assert!((lo.z - 0.0).abs() < 1e-9 && (hi.z - 6.0).abs() < 1e-9);
        // The wide end moved from the bottom to the top.
        let wide_at = |s: &KSolid, z: f64| {
            let (mesh, _) = crate::tessellate::tessellate(s, 0.01).unwrap();
            mesh.positions
                .chunks_exact(3)
                .filter(|p| (p[2] as f64 - z).abs() < 1e-6)
                .map(|p| p[0].abs().max(p[1].abs()) as f64)
                .fold(0.0f64, f64::max)
        };
        assert!((wide_at(&up, 0.0) - 5.0).abs() < 1e-6);
        assert!((wide_at(&mirrored, 6.0) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn a_loft_with_a_hole_keeps_the_hole_and_stays_watertight() {
        let ring = |z: f64, outer: f64, inner: f64| {
            Section::Profile(
                Sketch::new(Plane::XY.offset(z))
                    .with_loop(Loop::rect(P2::new(0.0, 0.0), outer, outer))
                    .with_loop(Loop::rect(P2::new(0.0, 0.0), inner, inner)),
            )
        };
        let s = built(&[ring(0.0, 20.0, 10.0), ring(5.0, 12.0, 6.0)]);
        let (v, tight, faces) = measure(&s);
        let want = 5.0 / 3.0 * (400.0 + 144.0 + (400.0f64 * 144.0).sqrt())
            - 5.0 / 3.0 * (100.0 + 36.0 + (100.0f64 * 36.0).sqrt());
        assert!((v - want).abs() < 1e-6 * want, "volume {v}, expected {want}");
        assert!(tight, "a lofted tube must be watertight");
        assert_eq!(faces, 10, "4 outer + 4 inner sides + 2 caps");
    }

    // ------------------------------------------------------- the Gridfinity foot

    /// `(z, inset)` of the reference foot.
    const FOOT: [(f64, f64); 4] = [(-5.0, 3.2), (-4.2, 2.4), (-2.4, 2.4), (0.0, 0.0)];

    fn foot_solid() -> KSolid {
        let sections: Vec<Section> = FOOT
            .iter()
            .map(|(z, inset)| {
                section(
                    *z,
                    Loop::rounded_rect(P2::new(0.0, 0.0), 42.0 - 2.0 * inset, 42.0 - 2.0 * inset, 4.0 - inset),
                )
            })
            .collect();
        built(&sections)
    }

    /// The reference inset at a height inside the foot.
    fn foot_inset(z: f64) -> f64 {
        if z >= -2.4 {
            -z
        } else if z >= -4.2 {
            2.4
        } else {
            2.4 + (-4.2 - z)
        }
    }

    /// Distance from `(x, y)` to the boundary of the axis-aligned square of
    /// half-extent `b`, positive outside. Every section of the foot is the set
    /// of points at distance `4 - inset` from the **same** square of
    /// half-extent 17, because `21 - inset` minus `4 - inset` is 17 whatever
    /// the inset is: that identity is the whole geometry of the foot.
    fn dist_to_square(x: f64, y: f64, b: f64) -> f64 {
        let (dx, dy) = ((x.abs() - b).max(0.0), (y.abs() - b).max(0.0));
        (dx * dx + dy * dy).sqrt()
    }

    #[test]
    fn the_gridfinity_foot_is_exact_everywhere_not_stepped() {
        let s = foot_solid();
        let (mesh, topo) = crate::tessellate::tessellate(&s, 0.01).unwrap();
        assert!(mesh.welded(1e-4).is_watertight(), "the foot must be watertight");
        // 3 bands x 8 segments + 2 caps.
        assert_eq!(topo.faces.len(), 26);

        let mut checked = 0;
        let mut worst: f64 = 0.0;
        for p in mesh.positions.chunks_exact(3) {
            let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
            // Skip the two caps; everything else is on a lateral band.
            if z > -1e-6 || z < -5.0 + 1e-6 {
                continue;
            }
            let want = 4.0 - foot_inset(z);
            let got = dist_to_square(x, y, 17.0);
            worst = worst.max((got - want).abs());
            checked += 1;
        }
        // Every lateral vertex sits on one of the two interior rims, since a
        // cone band is a strip and a flat flank a boundary-only polygon: the
        // arc points of two rounded rectangles, no interior grid.
        assert!(checked > 60, "only {checked} lateral vertices");
        // A stepped foot is wrong by up to a whole step (0.6 mm for the 4-step
        // approximation the example used). The loft is wrong by the width of an
        // f32: `Mesh` stores positions as f32, and 21 mm * 2^-24 is 1.3e-6 mm,
        // which is what this bound is. The surface itself is exact.
        assert!(worst < 5e-6, "worst deviation from the true profile: {worst} mm");

        // Between the rims the mesh has nothing to sample, so the exact
        // geometry is asked directly: every straight edge that climbs is a
        // ruling line of a band, and a point partway up it must be exactly
        // where the piecewise-linear profile says — no f32 in the way here.
        // `edge_iter` walks each face's boundary, so a shared edge comes
        // round once per face; count it once.
        let mut seen = std::collections::HashSet::new();
        let mut rungs = 0;
        let mut worst_rung: f64 = 0.0;
        for e in s.edge_iter() {
            if !seen.insert(e.id()) {
                continue;
            }
            let Curve::Line(line) = e.curve() else { continue };
            let (a, b) = (point_to_vec3(line.0), point_to_vec3(line.1));
            if (a.z - b.z).abs() < 1e-9 {
                continue;
            }
            for t in [0.25, 0.5, 0.75] {
                let p = a + (b - a) * t;
                let want = 4.0 - foot_inset(p.z);
                worst_rung = worst_rung.max((dist_to_square(p.x, p.y, 17.0) - want).abs());
            }
            rungs += 1;
        }
        assert_eq!(rungs, 24, "8 ruling lines per band, three bands");
        assert!(worst_rung < 1e-9, "a ruling line leaves the true profile by {worst_rung} mm");
    }

    #[test]
    fn the_foot_is_bit_identical_across_evaluations_and_stable_under_a_nudge() {
        let a = crate::tessellate::tessellate(&foot_solid(), 0.01).unwrap();
        let b = crate::tessellate::tessellate(&foot_solid(), 0.01).unwrap();
        assert_eq!(a.0.positions, b.0.positions, "the same script twice is the same solid");
        assert_eq!(a.0.indices, b.0.indices);
        let names_a: Vec<&str> = a.1.faces.iter().map(|f| f.name.as_str()).collect();
        let names_b: Vec<&str> = b.1.faces.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names_a, names_b);

        // A parameter nudge must move the surface, not re-pair the sections:
        // the correspondence is by construction order, so the face count and
        // the volume move smoothly.
        let nudged: Vec<Section> = FOOT
            .iter()
            .map(|(z, inset)| {
                let inset = inset + 0.01;
                section(
                    *z,
                    Loop::rounded_rect(P2::new(0.0, 0.0), 42.0 - 2.0 * inset, 42.0 - 2.0 * inset, 4.0 - inset),
                )
            })
            .collect();
        let n = built(&nudged);
        let (v0, _, f0) = measure(&foot_solid());
        let (v1, _, f1) = measure(&n);
        assert_eq!(f0, f1);
        assert!((v0 - v1).abs() / v0 < 0.01, "volume jumped: {v0} -> {v1}");
    }

    // ------------------------------------- the bossed rim's matched loft

    /// The bossed ring of a rim shell: a square ring of half-width `h`,
    /// corner radius `r`, that detours in a U around one R-`rb` magnet boss in
    /// the middle of each wall, the boss sitting `d` in from the wall. This is
    /// the ∩-ring at the top of the brim's cavity, reduced
    /// to one cell: 6 segments a quarter, 24 in all.
    fn brim_ring(h: f64, r: f64, rb: f64, d: f64) -> Loop {
        let k = std::f64::consts::FRAC_1_SQRT_2;
        let rot = |p: P2, q: usize| match q % 4 {
            0 => p,
            1 => P2::new(-p.y, p.x),
            2 => P2::new(-p.x, -p.y),
            _ => P2::new(p.y, -p.x),
        };
        let quarter = [
            Segment::Line { a: P2::new(-h + r, -h), b: P2::new(-rb, -h) },
            Segment::Line { a: P2::new(-rb, -h), b: P2::new(-rb, -h + d) },
            Segment::Arc {
                a: P2::new(-rb, -h + d),
                b: P2::new(rb, -h + d),
                via: P2::new(0.0, -h + d + rb),
            },
            Segment::Line { a: P2::new(rb, -h + d), b: P2::new(rb, -h) },
            Segment::Line { a: P2::new(rb, -h), b: P2::new(h - r, -h) },
            Segment::Arc {
                a: P2::new(h - r, -h),
                b: P2::new(h, -h + r),
                via: P2::new(h - r + r * k, -h + r - r * k),
            },
        ];
        Loop {
            segments: (0..4)
                .flat_map(|q| {
                    quarter.iter().map(move |s| match s {
                        Segment::Line { a, b } => Segment::Line { a: rot(*a, q), b: rot(*b, q) },
                        Segment::Arc { a, b, via } => {
                            Segment::Arc { a: rot(*a, q), b: rot(*b, q), via: rot(*via, q) }
                        }
                        other => other.clone(),
                    })
                })
                .collect(),
        }
    }

    /// The 15 mm rim-shell loft: the plain body-wall ring at inset 1.2 up to
    /// the inset-2.6 ring that detours round four R4.025 bosses whose apex
    /// plane is 11.775 in from the wall.
    fn brim_sections() -> Vec<Section> {
        vec![
            section(0.0, Loop::rounded_rect(P2::new(0.0, 0.0), 39.6, 39.6, 6.3)),
            section(15.0, brim_ring(18.4, 4.9, 4.025, 5.15)),
        ]
    }

    fn brim_solid() -> KSolid {
        let plan =
            LoftPlan::new(&brim_sections(), true, SectionMatch::ArcLength).expect("a valid plan");
        let mut solids = loft(&plan).expect("a solid");
        assert_eq!(solids.len(), 1);
        solids.remove(0)
    }

    /// The acceptance case for the arc-length section matcher:
    /// 8 segments lofted to 24, and the result is a real solid.
    ///
    /// The face count is the point. Matching splits the two rings into 28
    /// pieces each, so the band is **28 exact faces** — planar ramps where two
    /// lines meet, a true cone at each plain corner, a ruled scoop at each
    /// boss. The discretised pairing this replaces (the
    /// fallback: both wires sampled to 360 points and paired index to index)
    /// would be 360 flat chord strips for the same band, and every arc in both
    /// profiles would be gone.
    #[test]
    fn the_base_brim_loft_is_a_solid_of_exact_faces_not_chords() {
        let s = brim_solid();
        let (v, tight, faces) = measure(&s);
        assert!(tight, "the rim-shell loft must be watertight");
        assert!(v > 0.0, "volume {v}");
        // Between a 39.6 ring and a smaller detoured one over 15 mm: bounded by
        // the two prisms it lies between.
        assert!(v > 15.0 * 200.0 && v < 15.0 * 39.6 * 39.6, "volume {v} mm^3 is not plausible");
        assert_eq!(faces, 30, "28 matched bands + 2 caps");

        // What a discretised pairing would have cost, measured the same way.
        let chords = 360;
        assert!(
            faces < chords / 10,
            "{faces} exact faces against {} for a {chords}-point discretised pairing",
            chords + 2
        );

        // Not one of those faces is a chord: every point of the split plain
        // ring is still on the plain ring, and every point of the split
        // detoured ring is still on its own arcs.
        let plan = LoftPlan::new(&brim_sections(), true, SectionMatch::ArcLength).unwrap();
        let ring = &plan.sections[0].loops()[0];
        let mut worst: f64 = 0.0;
        for seg in &ring.segments {
            let len = seg.length();
            for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let p = seg.point_at_length(len * t);
                let (dx, dy) = ((p.x.abs() - 13.5).max(0.0), (p.y.abs() - 13.5).max(0.0));
                worst = worst.max(((dx * dx + dy * dy).sqrt() - 6.3).abs());
            }
        }
        // A 360-point discretisation of the same ring is out by its sagitta,
        // r(1 - cos(half the step)) — about 6e-4 mm here. This is 1e-9.
        assert!(worst < 1e-9, "the matched ring left the original by {worst} mm");
    }

    /// Two evaluations of the matched loft agree bit for bit, and a parameter
    /// nudge moves the surface instead of re-pairing the rings.
    #[test]
    fn the_base_brim_loft_is_bit_identical_across_evaluations() {
        let a = crate::tessellate::tessellate(&brim_solid(), 0.01).unwrap();
        let b = crate::tessellate::tessellate(&brim_solid(), 0.01).unwrap();
        assert_eq!(a.0.positions, b.0.positions, "the same sections twice is the same solid");
        assert_eq!(a.0.indices, b.0.indices);
        let names_a: Vec<&str> = a.1.faces.iter().map(|f| f.name.as_str()).collect();
        let names_b: Vec<&str> = b.1.faces.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names_a, names_b);

        let (v0, _, f0) = measure(&brim_solid());
        for nudge in [0.05, 0.1, 0.2] {
            let sections = vec![
                section(0.0, Loop::rounded_rect(P2::new(0.0, 0.0), 39.6, 39.6, 6.3)),
                section(15.0, brim_ring(18.4, 4.9, 4.025 + nudge, 5.15)),
            ];
            let plan = LoftPlan::new(&sections, true, SectionMatch::ArcLength).unwrap();
            let mut solids = loft(&plan).unwrap();
            let (v1, tight, f1) = measure(&solids.remove(0));
            assert!(tight, "still watertight with the boss {nudge} mm bigger");
            assert_eq!(f0, f1, "the face count moved, so the sections were re-paired");
            assert!((v0 - v1).abs() / v0 < 0.05, "volume jumped: {v0} -> {v1}");
        }
    }

    // ------------------------------------------------------------------ cone

    #[test]
    fn a_cone_to_a_point_has_the_textbook_volume() {
        let s = cone(Vec3::ZERO, 5.0, 0.0, 9.0).unwrap();
        let (mesh, topo) = crate::tessellate::tessellate(&s, 0.002).unwrap();
        let v = mesh.welded(1e-4).volume().abs();
        let want = std::f64::consts::PI * 25.0 * 9.0 / 3.0;
        assert!((v - want).abs() < 0.01 * want, "volume {v}, expected {want}");
        assert!(mesh.welded(1e-4).is_watertight());
        // A circle is two arcs, so the wall is two faces; plus the base.
        assert_eq!(topo.faces.len(), 3);
    }

    #[test]
    fn a_truncated_cone_has_the_textbook_volume() {
        // The Gridfinity magnet mouth: 0.2 x 45 degrees at the top of a 6.15 bore.
        let s = cone(Vec3::ZERO, 3.275, 3.075, 0.2).unwrap();
        let (mesh, topo) = crate::tessellate::tessellate(&s, 0.002).unwrap();
        let v = mesh.welded(1e-4).volume().abs();
        let (r0, r1, h) = (3.275f64, 3.075f64, 0.2f64);
        let want = std::f64::consts::PI * h / 3.0 * (r0 * r0 + r1 * r1 + r0 * r1);
        assert!((v - want).abs() < 0.01 * want, "volume {v}, expected {want}");
        assert!(mesh.welded(1e-4).is_watertight());
        assert_eq!(topo.faces.len(), 4, "2 wall faces + 2 caps");
    }

    #[test]
    fn a_cone_subtracts_from_a_cylinder() {
        // The case the magnet socket actually needs: the lead-in chamfer is cut
        // out of solid material at the mouth of the bore.
        let block = extrude(
            &Sketch::new(Plane::XY.offset(-1.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0)),
            10.0,
        )
        .unwrap()
        .remove(0);
        let tool = cone(Vec3::new(0.0, 0.0, 0.0), 3.475, 3.075, 0.4).unwrap();
        let r = boolean(BoolOp::Subtract, &block, &tool, DEFAULT_BOOLEAN_TOLERANCE).unwrap();
        let (mesh, _) = crate::tessellate::tessellate(&r, 0.05).unwrap();
        assert!(mesh.welded(1e-4).is_watertight(), "the cut block must stay watertight");
        assert!(mesh.welded(1e-4).volume().abs() < 20.0 * 20.0 * 10.0, "material was removed");
    }

    #[test]
    fn bad_cone_arguments_are_diagnostics() {
        assert!(cone(Vec3::ZERO, 5.0, 0.0, 0.0).unwrap_err().to_string().contains("h must be positive"));
        assert!(cone(Vec3::ZERO, 0.0, 1.0, 3.0).unwrap_err().to_string().contains("r0 must be positive"));
        assert!(cone(Vec3::ZERO, 5.0, -1.0, 3.0).unwrap_err().to_string().contains("must not be negative"));
    }

    // -------------------------------------------------- the skinned loft

    /// The loft the way it was built before skinning landed: every band a
    /// `builder::try_wire_homotopy` between the two adjacent sections' wires.
    ///
    /// It lives here, and only here, so that "a ruled loft still builds the
    /// same solid" can be *checked* rather than asserted in a comment. If the
    /// skinner ever stops reducing to the homotopy at degree 1, this is the
    /// test that says so.
    fn homotopy_loft(plan: &LoftPlan) -> Result<Vec<KSolid>> {
        let order = plan.ascending();
        let Layout::Stacked { normal: up, .. } = &plan.layout else { panic!("a stacked plan") };
        let up = *up;
        let mut out = Vec::with_capacity(plan.regions.len());
        for (outer, holes) in &plan.regions {
            let mut rungs: Vec<Rung<'_>> = Vec::with_capacity(order.len());
            for &si in &order {
                match &plan.sections[si] {
                    PlannedSection::Point(p) => rungs.push(Rung::Apex(builder::vertex(p.point()))),
                    PlannedSection::Profile { plane, loops, .. } => {
                        let mut ws = vec![loop_to_wire(&loops[*outer], plane)?];
                        let mut ls = vec![&loops[*outer]];
                        for h in holes {
                            ws.push(loop_to_wire(&loops[*h], plane)?);
                            ls.push(&loops[*h]);
                        }
                        rungs.push(Rung::Wires { wires: ws, loops: ls, plane: *plane });
                    }
                    PlannedSection::Curve(_) => panic!("a stacked plan has no curve sections"),
                }
            }
            let mut shell = Shell::new();
            if let Rung::Wires { wires, .. } = &rungs[0] {
                shell.push(cap(wires, -up)?);
            }
            for k in 0..rungs.len() - 1 {
                match (&rungs[k], &rungs[k + 1]) {
                    (Rung::Wires { wires: a, .. }, Rung::Wires { wires: b, .. }) => {
                        for (wa, wb) in a.iter().zip(b) {
                            let band = builder::try_wire_homotopy(wa, wb)
                                .map_err(|e| CadpoleError::kernel(format!("band {k}: {e}")))?;
                            shell.extend(band.into_iter());
                        }
                    }
                    (Rung::Wires { wires, loops, plane }, Rung::Apex(v)) => {
                        shell.extend(apex_fan(&wires[0], &planar_geoms(loops[0], plane, true), v, true)?)
                    }
                    (Rung::Apex(v), Rung::Wires { wires, loops, plane }) => {
                        shell.extend(apex_fan(&wires[0], &planar_geoms(loops[0], plane, false), v, false)?)
                    }
                    (Rung::Apex(_), Rung::Apex(_)) => {
                        return Err(CadpoleError::invalid("two point sections in a row"))
                    }
                    (Rung::Curve { .. }, _) | (_, Rung::Curve { .. }) => unreachable!("a stacked plan"),
                }
            }
            if let Rung::Wires { wires, .. } = &rungs[rungs.len() - 1] {
                shell.push(cap(wires, up)?);
            }
            out.push(Solid::try_new(vec![shell]).map_err(|e| CadpoleError::kernel(e.to_string()))?);
        }
        Ok(out)
    }

    /// Everything about a lofted solid a change to the kernel could move: its
    /// B-rep vertices, the mesh it tessellates to, how many faces it has, what
    /// they are called, and its volume. Compared with `==`, so a difference of
    /// one ulp anywhere is a failure.
    #[derive(PartialEq, Debug)]
    struct Fingerprint {
        vertices: Vec<[u64; 3]>,
        faces: usize,
        names: Vec<String>,
        volume: f64,
        positions: Vec<f32>,
        indices: Vec<u32>,
    }

    fn fingerprint(s: &KSolid, plan: &LoftPlan) -> Fingerprint {
        let (mesh, mut topo) = crate::tessellate::tessellate(s, 0.01).expect("tessellation");
        let probes = topo.probes(&mesh);
        let generator = crate::naming::Generator::loft(plan, "loft");
        topo.apply_names(crate::naming::name_faces(&generator, "loft_1", &probes));
        let mut vertices: Vec<[u64; 3]> = s
            .vertex_iter()
            .map(|v| {
                let p = v.point();
                [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
            })
            .collect();
        vertices.sort_unstable();
        vertices.dedup();
        Fingerprint {
            vertices,
            faces: topo.faces.len(),
            names: topo.faces.iter().map(|f| f.name.clone()).collect(),
            volume: mesh.welded(1e-4).volume(),
            positions: mesh.positions.clone(),
            indices: mesh.indices.clone(),
        }
    }

    /// The sections of every ruled loft that has a test in this file, plus a
    /// couple more, so the equivalence is checked on lines, arcs, holes, tips,
    /// several bands and the arc-length matcher.
    fn ruled_shapes() -> Vec<(&'static str, Vec<Section>, SectionMatch)> {
        let circle = |z: f64, r: f64| section(z, Loop::circle(P2::new(0.0, 0.0), r));
        let ring = |z: f64, o: f64, i: f64| {
            Section::Profile(
                Sketch::new(Plane::XY.offset(z))
                    .with_loop(Loop::circle(P2::new(0.0, 0.0), o))
                    .with_loop(Loop::circle(P2::new(0.0, 0.0), i)),
            )
        };
        let foot: Vec<Section> = FOOT
            .iter()
            .map(|(z, inset)| {
                section(
                    *z,
                    Loop::rounded_rect(
                        P2::new(0.0, 0.0),
                        42.0 - 2.0 * inset,
                        42.0 - 2.0 * inset,
                        4.0 - inset,
                    ),
                )
            })
            .collect();
        vec![
            ("a prism", vec![square(0.0, 10.0), square(4.0, 10.0)], SectionMatch::Index),
            ("a frustum", vec![square(0.0, 10.0), square(4.0, 6.0)], SectionMatch::Index),
            (
                "a pyramid",
                vec![square(0.0, 10.0), Section::Point(Vec3::new(0.0, 0.0, 9.0))],
                SectionMatch::Index,
            ),
            (
                "a cone to a point",
                vec![circle(0.0, 5.0), Section::Point(Vec3::new(0.0, 0.0, 9.0))],
                SectionMatch::Index,
            ),
            ("a truncated cone", vec![circle(0.0, 3.275), circle(0.2, 3.075)], SectionMatch::Index),
            (
                "a three-circle stack",
                vec![circle(0.0, 10.0), circle(4.0, 14.0), circle(9.0, 6.0)],
                SectionMatch::Index,
            ),
            (
                "an annular band",
                vec![ring(0.0, 60.0, 50.0), ring(2.0, 57.0, 48.0)],
                SectionMatch::Index,
            ),
            ("the Gridfinity foot", foot, SectionMatch::Index),
            ("the bossed rim", brim_sections(), SectionMatch::ArcLength),
        ]
    }

    /// **The gate.** A ruled loft is the skinner at degree 1, and the solid it
    /// produces is the one `try_wire_homotopy` produced — vertex for vertex,
    /// triangle for triangle, name for name, and to the last bit of the volume.
    ///
    /// This is not a coincidence to be re-derived every time someone reads the
    /// code. With two sections there are no interior v-knots to average, so the
    /// v knot vector is `[0,0,1,1]`; the degree-1 basis functions at `v = 0`
    /// and `v = 1` are the rows of the identity; the interpolation therefore
    /// returns the section control points untouched, over the knot vector the
    /// compatibility step produced, which is the one `syncro_knots` would have
    /// produced. That is `BSplineSurface::homotopy`, term for term. What this
    /// test guards is that it *stays* that, including the exact-cone property
    /// `with_cones_revolved` reads off the patch and Gridfinity's foot depends
    /// on.
    #[test]
    fn a_ruled_loft_is_the_skinner_at_degree_one_and_builds_the_very_same_solid() {
        for (what, sections, matching) in ruled_shapes() {
            let plan = LoftPlan::new(&sections, true, matching).expect("a valid plan");
            let now = loft(&plan).expect("the skinner at degree 1");
            let before = homotopy_loft(&plan).expect("the homotopy");
            assert_eq!(now.len(), before.len(), "{what}: a different number of solids");
            for (a, b) in now.iter().zip(&before) {
                let (fa, fb) = (fingerprint(a, &plan), fingerprint(b, &plan));
                assert_eq!(fa.faces, fb.faces, "{what}: face count");
                assert_eq!(fa.names, fb.names, "{what}: face names");
                assert_eq!(fa.vertices, fb.vertices, "{what}: B-rep vertex positions");
                assert_eq!(fa.positions, fb.positions, "{what}: mesh vertex positions");
                assert_eq!(fa.indices, fb.indices, "{what}: mesh triangles");
                assert_eq!(fa.volume, fb.volume, "{what}: volume");
            }
        }
    }

    /// A stack of concentric circles whose radius follows a bell, like the hub
    /// of `examples/compressor_wheel.star`: the shape that made a ruled loft's
    /// bands visible and the reason `ruled=False` was wanted.
    fn bell(n: usize) -> Vec<Section> {
        (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                let z = 36.0 * t;
                let r = 8.0 + 22.0 * (1.0 - fmath::powf(1.0 - t, 2.2));
                section(z, Loop::circle(P2::new(0.0, 0.0), r))
            })
            .collect()
    }

    /// The compatible control nets of a loft's columns, and the v direction
    /// over them: what `side_faces` builds, reached directly so a test can look
    /// at the surface rather than at the solid around it.
    #[allow(clippy::type_complexity)]
    fn columns(
        sections: &[Section],
        max_v_degree: usize,
    ) -> (crate::skin::VDirection, Vec<crate::skin::Compatible>, Vec<Vec<Curve>>) {
        let plan = LoftPlan::new(sections, false, SectionMatch::Index).expect("a valid plan");
        let wires: Vec<Wire> = plan
            .ascending()
            .iter()
            .map(|&si| match &plan.sections[si] {
                PlannedSection::Profile { plane, loops, .. } => {
                    loop_to_wire(&loops[0], plane).unwrap()
                }
                PlannedSection::Point(_) | PlannedSection::Curve(_) => {
                    panic!("this helper wants profile sections")
                }
            })
            .collect();
        let edges: Vec<Vec<truck_modeling::Edge>> =
            wires.iter().map(|w| w.edge_iter().cloned().collect()).collect();
        let mut nets = Vec::new();
        let mut curves = Vec::new();
        for e in 0..edges[0].len() {
            let cs: Vec<Curve> = edges.iter().map(|es| es[e].oriented_curve()).collect();
            let lifted = cs.iter().cloned().map(Curve::lift_up).collect();
            nets.push(crate::skin::make_compatible(lifted).unwrap());
            curves.push(cs);
        }
        let v = crate::skin::VDirection::new(&nets, max_v_degree).expect("a v direction");
        (v, nets, curves)
    }

    /// **Interpolation** — the defining property. The surface passes through every input section, at that
    /// section's own parameter, to a nanometre.
    ///
    /// The sections here are Cadpole circles, so they are *rational* quadratics
    /// whose weights went through degree elevation and knot refinement with the
    /// rest of the control point. Had that been done on the Euclidean points
    /// and the weights separately, the arcs would have stopped being arcs and
    /// this would fail long before 1e-9.
    #[test]
    fn a_skinned_surface_passes_through_every_section() {
        let sections = bell(7);
        let (v, nets, curves) = columns(&sections, crate::skin::MAX_V_DEGREE);
        assert_eq!(v.degree(), 3, "seven sections, so the cubic is available");
        let mut worst: f64 = 0.0;
        for (net, section_curves) in nets.iter().zip(&curves) {
            let s = truck_modeling::NurbsSurface::new(v.skin(net).expect("a surface"));
            for (k, c) in section_curves.iter().enumerate() {
                for i in 0..=24 {
                    let u = i as f64 / 24.0;
                    let on_surface = point_to_vec3(s.subs(u, v.params()[k]));
                    let on_section = point_to_vec3(c.subs(u));
                    worst = worst.max((on_surface - on_section).length());
                }
            }
        }
        assert!(worst < 1e-9, "the surface leaves its sections by {worst} mm");
    }

    /// **Rational sections survive.** A skin through circles of differing radii
    /// still passes through exact circles: at every section parameter the
    /// surface's points are on the circle of that section's radius, to a
    /// nanometre, all the way round.
    ///
    /// This is the property that keeps a lofted solid of revolution a solid of
    /// revolution. Sections with identical weights — which equal-angular-span
    /// arcs have — interpolate to a surface with those same weights, so the
    /// homogeneous interpolation is the Euclidean one and the arcs come through
    /// untouched.
    #[test]
    fn skinning_through_circles_still_passes_through_circles() {
        let radii = [10.0, 18.0, 12.0, 25.0, 9.0];
        let sections: Vec<Section> = radii
            .iter()
            .enumerate()
            .map(|(i, r)| section(i as f64 * 4.0, Loop::circle(P2::new(0.0, 0.0), *r)))
            .collect();
        let (v, nets, _) = columns(&sections, crate::skin::MAX_V_DEGREE);
        let mut worst: f64 = 0.0;
        for net in &nets {
            let s = truck_modeling::NurbsSurface::new(v.skin(net).expect("a surface"));
            for (k, r) in radii.iter().enumerate() {
                for i in 0..=24 {
                    let p = point_to_vec3(s.subs(i as f64 / 24.0, v.params()[k]));
                    worst = worst.max(((p.x * p.x + p.y * p.y).sqrt() - r).abs());
                    worst = worst.max((p.z - k as f64 * 4.0).abs());
                }
            }
        }
        assert!(worst < 1e-9, "a section of the skin is off its circle by {worst} mm");
    }

    /// **Continuity across a section plane** — the kink being fixed.
    ///
    /// The ruled loft meets a section plane with two different surfaces and the
    /// normal jumps by a fixed angle; the skin runs one surface through it and
    /// the normal's two one-sided limits are the same vector. Both are measured
    /// on the same twelve-circle bell profile, so the number for the ruled case
    /// is the size of the defect.
    ///
    /// "The normal at `v_k − h` is close to the normal at `v_k + h`" is not
    /// enough on its own: on a curved surface those two differ by the normal's
    /// own rate of turn times `2h` whether or not there is a crease. What says
    /// *continuous* is that the difference goes to zero **with** `h`, so the
    /// step is shrunk by ten twice and the angle has to shrink with it. A
    /// crease does not shrink: it is the same angle at every `h`.
    #[test]
    fn a_skinned_loft_has_no_crease_at_a_section_plane_and_a_ruled_one_does() {
        let sections = bell(12);
        let (v, nets, _) = columns(&sections, crate::skin::MAX_V_DEGREE);
        let smooth = truck_modeling::NurbsSurface::new(v.skin(&nets[0]).expect("a surface"));
        let angle = |a: Vec3, b: Vec3| fmath::asin(a.cross(b).length()).to_degrees();
        let across = |h: f64| {
            let mut worst: f64 = 0.0;
            for k in 1..sections.len() - 1 {
                let vk = v.params()[k];
                for u in [0.2, 0.5, 0.8] {
                    let a = vector_to_vec3(smooth.normal(u, vk - h)).normalized();
                    let b = vector_to_vec3(smooth.normal(u, vk + h)).normalized();
                    worst = worst.max(angle(a, b));
                }
            }
            worst
        };
        let (coarse, mid, fine) = (across(1e-4), across(1e-5), across(1e-6));
        assert!(
            mid < coarse / 5.0 && fine < mid / 5.0,
            "the skin's normal is not converging across a section plane: {coarse}, {mid}, {fine} \
             degrees at h = 1e-4, 1e-5, 1e-6"
        );
        assert!(fine < 1e-3, "the skin's normal turns {fine} degrees across a section plane");

        // The same measurement on the ruled loft: band k-1 at its top against
        // band k at its bottom, which are the two faces that meet on section k.
        // There is no h to shrink — the two surfaces simply disagree.
        let mut bands = Vec::new();
        for pair in sections.windows(2) {
            let (vb, nb, _) = columns(pair, 1);
            bands.push(truck_modeling::NurbsSurface::new(vb.skin(&nb[0]).expect("a surface")));
        }
        let mut worst_ruled: f64 = 0.0;
        for k in 1..sections.len() - 1 {
            for u in [0.2, 0.5, 0.8] {
                let a = vector_to_vec3(bands[k - 1].normal(u, 1.0)).normalized();
                let b = vector_to_vec3(bands[k].normal(u, 0.0)).normalized();
                worst_ruled = worst_ruled.max(angle(a, b));
            }
        }
        assert!(
            worst_ruled > 1.0,
            "the ruled loft was supposed to have a crease to fix, and its worst is \
             {worst_ruled} degrees"
        );
    }

    /// Two sections have nothing to be smooth between, so `ruled=False` is the
    /// ruled loft — the same solid, not merely a similar one. `q = min(3, K)`
    /// is what says so.
    #[test]
    fn a_two_section_smooth_loft_is_the_ruled_loft() {
        let circle = |z: f64, r: f64| section(z, Loop::circle(P2::new(0.0, 0.0), r));
        for sections in [
            vec![square(0.0, 10.0), square(4.0, 6.0)],
            vec![circle(0.0, 5.0), circle(3.0, 8.0)],
        ] {
            let ruled = LoftPlan::new(&sections, true, SectionMatch::Index).unwrap();
            let smooth = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
            let a = loft(&ruled).unwrap().remove(0);
            let b = loft(&smooth).unwrap().remove(0);
            let (fa, fb) = (fingerprint(&a, &ruled), fingerprint(&b, &smooth));
            assert_eq!(fa.faces, fb.faces);
            assert_eq!(fa.names, fb.names);
            assert_eq!(fa.vertices, fb.vertices);
            assert_eq!(fa.positions, fb.positions);
            assert_eq!(fa.volume, fb.volume);
        }
    }

    /// Three sections give a quadratic in v: one face per column rather than
    /// one per band, and still a watertight solid.
    #[test]
    fn three_sections_skin_to_one_face_a_column_at_degree_two() {
        let sections = bell(3);
        let plan = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
        let s = loft(&plan).unwrap().remove(0);
        let (mesh, topo) = crate::tessellate::tessellate(&s, 0.01).unwrap();
        assert!(mesh.welded(1e-4).is_watertight(), "a skinned loft must be watertight");
        // A circle is two arcs: two side faces for the whole stack, not four.
        assert_eq!(topo.faces.len(), 4, "2 sides + 2 caps");
        let (v, _, _) = columns(&sections, crate::skin::MAX_V_DEGREE);
        assert_eq!(v.degree(), 2, "three sections clamp the cubic to a quadratic");
    }

    /// A point section is a fan, not a section: the skin runs through the
    /// genuine sections and the tip keeps the exact cone (or plane) faces
    /// `apex_fan` builds, so a smooth loft can still end on a point.
    #[test]
    fn a_smooth_loft_may_still_end_on_a_point() {
        let mut sections = bell(5);
        sections.push(Section::Point(Vec3::new(0.0, 0.0, 48.0)));
        let plan = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
        let s = loft(&plan).unwrap().remove(0);
        let (mesh, topo) = crate::tessellate::tessellate(&s, 0.01).unwrap();
        assert!(mesh.welded(1e-4).is_watertight(), "a skinned loft to a tip must be watertight");
        // 2 skinned sides + 2 tip faces + 1 cap. The tip is still a fan.
        assert_eq!(topo.faces.len(), 5);
        assert!(mesh.welded(1e-4).volume().abs() > 0.0);
    }

    /// A skinned loft with a hole keeps the hole, and both its loops are single
    /// faces through every section.
    #[test]
    fn a_skinned_loft_with_a_hole_keeps_the_hole() {
        let ring = |z: f64, o: f64, i: f64| {
            Section::Profile(
                Sketch::new(Plane::XY.offset(z))
                    .with_loop(Loop::circle(P2::new(0.0, 0.0), o))
                    .with_loop(Loop::circle(P2::new(0.0, 0.0), i)),
            )
        };
        let sections = vec![ring(0.0, 20.0, 10.0), ring(4.0, 26.0, 16.0), ring(9.0, 18.0, 9.0)];
        let plan = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
        let s = loft(&plan).unwrap().remove(0);
        let (mesh, topo) = crate::tessellate::tessellate(&s, 0.01).unwrap();
        assert!(mesh.welded(1e-4).is_watertight(), "a skinned tube must be watertight");
        assert_eq!(topo.faces.len(), 6, "2 outer + 2 inner sides + 2 caps");
        assert!(mesh.welded(1e-4).volume().abs() > 0.0);
    }

    /// A skinned loft through concentric circles is a surface of revolution,
    /// [`revolution_of`] says so, and the meridian it hands back is the patch.
    ///
    /// This is [`cone_of`]'s reason over again. One patch spanning a whole bell
    /// is not a cone, so truck grids it and halves the meridian direction every
    /// time the circle's curvature fails a cell — 21,550 triangles for the
    /// compressor wheel's hub, against 5,852 for the 36-band ruled loft it was
    /// meant to replace. Recognised, the same hub is 4,564. What the assertions
    /// below check is the recognition itself: that the revolved surface is the
    /// patch to [`CONE_EPS`], that a cone still goes down [`cone_of`]'s path,
    /// and that a patch which is *not* a revolution is refused.
    #[test]
    fn a_skinned_stack_of_circles_is_recognised_as_a_surface_of_revolution() {
        let sections = bell(6);
        let (v, nets, _) = columns(&sections, crate::skin::MAX_V_DEGREE);
        let patch = Surface::NurbsSurface(truck_modeling::NurbsSurface::new(
            v.skin(&nets[0]).expect("a surface"),
        ));
        assert!(cone_of(&patch).is_none(), "a bell is not a cone");
        let Some(revolved) = revolution_of(&patch) else {
            panic!("a skin through concentric circles is a surface of revolution");
        };
        assert!(matches!(revolved, Surface::RevolutedCurve(_)));

        // The replacement is the same surface: every point of the patch is on
        // it, and the two agree on which way is out.
        let ((u0, u1), (v0, v1)) = match &patch {
            Surface::NurbsSurface(ns) => ns.range_tuple(),
            _ => unreachable!(),
        };
        let mut worst: f64 = 0.0;
        for i in 0..=12 {
            for j in 0..=12 {
                let (u, w) = (i as f64 / 12.0, j as f64 / 12.0);
                let p = point_to_vec3(patch.subs(u0 + (u1 - u0) * u, v0 + (v1 - v0) * w));
                let (pu, pw) = revolved
                    .search_nearest_parameter(p.point(), None, 100)
                    .expect("the revolved surface can invert a point of the patch");
                worst = worst.max((point_to_vec3(revolved.subs(pu, pw)) - p).length());
            }
        }
        assert!(worst < 1e-6, "the revolved meridian leaves the patch by {worst} mm");
        let n_patch = vector_to_vec3(patch.normal(
            u0 + (u1 - u0) * 0.5,
            v0 + (v1 - v0) * 0.5,
        ))
        .normalized();
        let mid = point_to_vec3(patch.subs(u0 + (u1 - u0) * 0.5, v0 + (v1 - v0) * 0.5));
        let (pu, pw) = revolved.search_nearest_parameter(mid.point(), None, 100).unwrap();
        let n_rev = vector_to_vec3(revolved.normal(pu, pw)).normalized();
        assert!(n_patch.dot(n_rev) > 0.99, "the revolved surface faces the other way");

        // A band between arcs about *different* axes turns as it climbs, so it
        // is not a revolution and keeps its patch.
        let off = vec![
            section(0.0, Loop::circle(P2::new(0.0, 0.0), 10.0)),
            section(3.0, Loop::circle(P2::new(3.0, 0.0), 9.0)),
            section(6.0, Loop::circle(P2::new(6.0, 0.0), 8.0)),
        ];
        let (vo, no, _) = columns(&off, crate::skin::MAX_V_DEGREE);
        let skew = Surface::NurbsSurface(truck_modeling::NurbsSurface::new(
            vo.skin(&no[0]).expect("a surface"),
        ));
        assert!(revolution_of(&skew).is_none(), "off-axis circles are not a revolution");

        // And the whole solid still meshes, at a size that says the patch was
        // not gridded. Measured: 1,692 triangles here, 9,132 before the
        // recognition.
        let plan = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
        let s = loft(&plan).unwrap().remove(0);
        let (mesh, topo) = crate::tessellate::tessellate(&s, DEFAULT_TOLERANCE).unwrap();
        assert!(mesh.welded(1e-4).is_watertight());
        let tris: u32 = topo.faces.iter().map(|f| f.tri_count).sum();
        assert!(tris < 3000, "{tris} triangles: the bell was gridded, not revolved");
    }

    /// The face names of a skinned loft carry no band suffix, because there are
    /// no bands: the whole column is one face, so it is the plain `side[l.s]` a
    /// two-section loft gets, and the caps are still `start` and `end`.
    #[test]
    fn a_skinned_loft_names_a_column_once() {
        let sections = bell(6);
        let plan = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
        let s = loft(&plan).unwrap().remove(0);
        let f = fingerprint(&s, &plan);
        let mut names = f.names.clone();
        names.sort();
        assert_eq!(
            names,
            vec!["loft_1/end", "loft_1/side[0.0]", "loft_1/side[0.1]", "loft_1/start"]
        );

        // The ruled loft through the same sections still carries the band.
        let ruled = LoftPlan::new(&sections, true, SectionMatch::Index).unwrap();
        let r = loft(&ruled).unwrap().remove(0);
        let rf = fingerprint(&r, &ruled);
        assert!(
            rf.names.iter().any(|n| n == "loft_1/side[0.0]#3"),
            "a ruled loft's bands are still named: {:?}",
            rf.names
        );
    }

    /// Every face must survive tessellation, or no mesh is returned.
    ///
    /// truck's own face triangulator drops a face it cannot triangulate and
    /// says nothing, so the caller gets a mesh with a hole in it: on the vase
    /// example, a 120 degree panel of its outer wall at every tolerance from
    /// 0.02 to 0.08. The edge-first tessellator must not; the test
    /// judges the ids with no weld.
    ///
    /// This builds the same shape: two Bezier walls revolved about Z, an
    /// outer one and an inner one making the wall thickness.
    #[test]
    fn no_face_is_ever_dropped_from_a_tessellation() {
        use std::f64::consts::TAU;
        let wall = 2.4;
        let outer = |x: f64, y: f64| P2::new(x, y);
        let profile = Loop {
            segments: vec![
                Segment::Line { a: outer(3.0, 0.0), b: outer(30.0, 0.0) },
                Segment::Bezier {
                    a: outer(30.0, 0.0),
                    b: outer(34.0, 120.0),
                    ctrl: vec![outer(48.0, 66.0), outer(26.0, 105.6)],
                },
                Segment::Line { a: outer(34.0, 120.0), b: outer(34.0 - wall, 120.0) },
                Segment::Bezier {
                    a: outer(34.0 - wall, 120.0),
                    b: outer(31.0 - wall, 4.0),
                    ctrl: vec![outer(26.0 - wall, 105.6), outer(48.0 - wall, 66.0)],
                },
                Segment::Line { a: outer(31.0 - wall, 4.0), b: outer(3.0, 4.0) },
                Segment::Line { a: outer(3.0, 4.0), b: outer(3.0, 0.0) },
            ],
        };
        let sketch = Sketch::new(Plane::XZ).with_loop(profile);
        let solid = &revolve(&sketch, Vec3::ZERO, Vec3::Z, TAU).unwrap()[0];

        // Across the band where a triangle could drop, and outside it.
        let mut volumes = Vec::new();
        for tol in [0.2, 0.1, 0.08, 0.05, 0.03, 0.02, 0.015, 0.01] {
            let meshed = tessellate(solid, tol).unwrap_or_else(|e| panic!("tolerance {tol}: {e}"));
            assert_eq!(meshed.faces.len(), solid.face_iter().count(), "tolerance {tol} dropped a face");
            assert_eq!(meshed.unmatched_edges().0, 0, "tolerance {tol} is open by id");
            let (mesh, _) = crate::tessellate::tessellate(solid, tol).unwrap();
            assert!(mesh.welded(1e-4).is_watertight(), "tolerance {tol} is not watertight");
            volumes.push((tol, mesh.volume()));
        }
        // A hole does not merely open the surface, it makes the volume wrong,
        // so agreement across tolerances is the real assertion here.
        let (_, first) = volumes[0];
        assert!(first > 0.0, "{volumes:?}");
        for (tol, v) in &volumes {
            assert!((v - first).abs() / first < 0.05, "tolerance {tol} gives {v}, others give {first}");
        }
    }

    /// A boolean result tessellates to a *closed* surface, at every tolerance
    /// the export range names — not merely to one with all its faces present.
    ///
    /// `examples/showcase.star`'s bracket body: a 70×34×7 plate with an r9
    /// boss standing 1 mm into its top. Nothing exotic, and at every
    /// tolerance from 0.01 down to 0.005 — the range used for export —
    /// a face-by-face tessellation can leave one or two triangle-sized holes
    /// on the plate-top/boss intersection curve while every face still has a
    /// polygon: an STL with six unbalanced edges, which the next mesh boolean
    /// refuses.
    ///
    /// The volume is asserted across tolerances for the same reason the vase
    /// test does it: a hole does not just open the surface, it makes every
    /// number computed over it wrong.
    #[test]
    fn a_boolean_result_tessellates_closed_at_export_tolerances() {
        let plate = extrude(
            &Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(14.0, 110.0), 70.0, 34.0)),
            7.0,
        )
        .unwrap()
        .remove(0);
        let boss = extrude(
            &Sketch::new(Plane::XY.offset(6.0)).with_loop(Loop::circle(P2::new(14.0, 110.0), 9.0)),
            12.0,
        )
        .unwrap()
        .remove(0);
        let body = boolean(BoolOp::Union, &plate, &boss, 0.1).expect("plate ∪ boss");

        let mut volumes = Vec::new();
        for tol in [0.02, 0.015, 0.01, 0.008, 0.005] {
            let t = tessellate_reporting(&body, tol).unwrap_or_else(|e| panic!("tolerance {tol}: {e}"));
            assert_eq!(t.meshed.faces.len(), body.face_iter().count(), "tolerance {tol} dropped a face");
            assert_eq!(t.meshed.unmatched_edges().0, 0, "tolerance {tol} is open by id");
            let (mesh, _) = crate::tessellate::tessellate(&body, tol).unwrap();
            let welded = mesh.welded(crate::mesh_kernel::WELD_EPS);
            assert!(welded.is_watertight(), "tolerance {tol} left the surface open");
            // The mesh is one the mesh kernel will accept.
            assert!(
                crate::mesh_kernel::to_manifold(&mesh, 0, tol).is_ok(),
                "tolerance {tol}: the mesh kernel refused the tessellation"
            );
            volumes.push((tol, t.tolerance, mesh.volume()));
        }
        let want = 70.0 * 34.0 * 7.0 + std::f64::consts::PI * 81.0 * 11.0;
        for (asked, used, v) in &volumes {
            assert!(
                (v - want).abs() / want < 0.01,
                "tolerance {asked} (tessellated at {used}) gives {v}, want about {want}"
            );
        }
    }

    #[test]
    fn revolve_circle_makes_torus_like_solid() {
        let s = Sketch::new(Plane::XZ).with_loop(Loop::circle(P2::new(10.0, 0.0), 2.0));
        let solids = revolve(&s, Vec3::ZERO, Vec3::Z, std::f64::consts::TAU).unwrap();
        assert_eq!(solids.len(), 1);
        assert!(solids[0].face_iter().count() >= 2);
    }

    /// A revolved solid must enclose positive volume however the profile was
    /// drawn and whichever way it is turned.
    ///
    /// `extrude` flips a profile that faces against the sweep, and `revolve`
    /// must too: otherwise a revolved body -- the vase, the planter, the
    /// showcase -- comes out with every normal pointing inward, a negative
    /// volume that `cadpole check` and a slicer both read as a void.
    ///
    /// Pappus gives the expected answer in closed form: a torus of tube radius
    /// r swept about a circle of radius R is 2 pi^2 R r^2, so this checks the
    /// size as well as the sign. The tolerance is loose because what is
    /// measured is a mesh: at the 0.05 mm chord error this tessellates at, the
    /// facets cut the corners of a 2 mm tube and come in a couple of per cent
    /// under the true volume. A tighter bound here would be a test of the
    /// tessellator, which is not what this is for.
    #[test]
    fn a_revolved_solid_is_never_inside_out() {
        use std::f64::consts::{PI, TAU};
        let torus = 2.0 * PI * PI * 10.0 * 2.0 * 2.0;
        for (plane, axis) in [(Plane::XZ, Vec3::Z), (Plane::YZ, Vec3::Z)] {
            for angle in [TAU, -TAU] {
                let s = Sketch::new(plane).with_loop(Loop::circle(P2::new(10.0, 0.0), 2.0));
                let solid = &revolve(&s, Vec3::ZERO, axis, angle).unwrap()[0];
                let v = volume_of(solid);
                assert!(v > 0.0, "{plane:?} at {angle} rad is inside out: {v} mm3");
                assert!((v - torus).abs() / torus < 0.05, "{plane:?} at {angle} rad: {v} vs {torus}");
                assert!(is_watertight(solid), "{plane:?} at {angle} rad is not watertight");
            }
        }
        // A half turn is half the solid, and still the right way out.
        let s = Sketch::new(Plane::XZ).with_loop(Loop::circle(P2::new(10.0, 0.0), 2.0));
        let half = volume_of(&revolve(&s, Vec3::ZERO, Vec3::Z, PI).unwrap()[0]);
        assert!((half - torus / 2.0).abs() / torus < 0.05, "half turn: {half} vs {}", torus / 2.0);
    }

    // ---- the contact pre-pass ------------------------------------

    /// An axis-aligned box with its minimum corner at `at`.
    fn cube(size: f64, at: (f64, f64, f64)) -> KSolid {
        let s = Sketch::new(Plane::XY.offset(at.2))
            .with_loop(Loop::rect(P2::new(at.0 + size / 2.0, at.1 + size / 2.0), size, size));
        extrude(&s, size).unwrap().remove(0)
    }

    fn volume_of(s: &KSolid) -> f64 {
        crate::tessellate::tessellate(s, 0.05).unwrap().0.volume()
    }

    fn is_watertight(s: &KSolid) -> bool {
        crate::tessellate::tessellate(s, 0.05).unwrap().0.welded(1e-4).is_watertight()
    }

    /// A shell handed over open — one face gone, as an exact boolean that
    /// comes back with a gap in it leaves one — still triangulates face by
    /// face, since nothing in a face's own triangulation sees that its
    /// neighbour is missing. The driver refuses the mesh, naming the open
    /// edges, rather than return a surface with a hole in it.
    #[test]
    fn an_open_shell_is_refused_rather_than_meshed_with_its_hole() {
        let closed = cube(10.0, (0.0, 0.0, 0.0));
        let t = tessellate_reporting(&closed, 0.05).unwrap();
        assert_eq!(t.meshed.unmatched_edge_count(), 0);
        let mut shell = closed.into_boundaries().remove(0);
        assert!(shell.pop().is_some(), "a cube has six faces");
        let open = Solid::new_unchecked(vec![shell]);
        let e = tessellate_reporting(&open, 0.05).map(|_| ()).unwrap_err().to_string();
        assert!(e.contains("the shell is open: 4 edges of its mesh lie on one face only"), "{e}");
        assert!(e.contains("a mesh with a hole in it is not returned"), "{e}");
    }

    #[test]
    fn a_fully_enclosed_pair_is_classified_without_the_kernel() {
        let outer = cube(20.0, (0.0, 0.0, 0.0));
        let inner = cube(18.8, (0.6, 0.6, 0.6));
        assert_eq!(disposition(&outer, &inner, 0.1).unwrap(), Disposition::BInsideA);
        assert_eq!(disposition(&inner, &outer, 0.1).unwrap(), Disposition::AInsideB);
        let far = cube(5.0, (100.0, 0.0, 0.0));
        assert_eq!(disposition(&outer, &far, 0.1).unwrap(), Disposition::Separate);
        // A cutter that crosses the wall is the only case shapeops sees.
        let post = cube(6.0, (7.0, 7.0, -3.0));
        assert_eq!(disposition(&outer, &post, 0.1).unwrap(), Disposition::Meeting);
    }

    #[test]
    fn subtracting_an_enclosed_solid_leaves_a_solid_with_a_void() {
        // A hollow shell: 20 mm cube, 0.6 mm walls.
        // truck answered this with the cavity turned inside out.
        let outer = cube(20.0, (0.0, 0.0, 0.0));
        let inner = cube(18.8, (0.6, 0.6, 0.6));
        let shell = boolean(BoolOp::Subtract, &outer, &inner, 0.1).unwrap();
        assert_eq!(shell.boundaries().len(), 2, "an outer shell plus one void");
        let want = fmath::powi(20.0f64, 3) - fmath::powi(18.8f64, 3);
        let got = volume_of(&shell);
        // Mesh positions are f32, so the volume matches to about seven digits.
        assert!((got - want).abs() < want * 1e-6, "volume {got}, want {want}: a void subtracts");
        assert!(is_watertight(&shell), "both shells are closed, so the mesh is too");
        assert_eq!(shell.face_iter().count(), 12);
    }

    #[test]
    fn an_enclosed_pair_answers_union_and_intersect_from_the_operands() {
        let outer = cube(20.0, (0.0, 0.0, 0.0));
        let inner = cube(10.0, (5.0, 5.0, 5.0));
        let u = boolean(BoolOp::Union, &outer, &inner, 0.1).unwrap();
        assert!((volume_of(&u) - 8000.0).abs() < 1e-6, "union of a solid pair is the outer body");
        let i = boolean(BoolOp::Intersect, &outer, &inner, 0.1).unwrap();
        assert!((volume_of(&i) - 1000.0).abs() < 1e-6, "intersect is the inner body");
        // Mirrored: the enclosing body second.
        let u = boolean(BoolOp::Union, &inner, &outer, 0.1).unwrap();
        assert!((volume_of(&u) - 8000.0).abs() < 1e-6);
        let i = boolean(BoolOp::Intersect, &inner, &outer, 0.1).unwrap();
        assert!((volume_of(&i) - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn subtracting_something_bigger_than_the_body_says_what_happened() {
        let outer = cube(20.0, (0.0, 0.0, 0.0));
        let inner = cube(10.0, (5.0, 5.0, 5.0));
        let e = boolean(BoolOp::Subtract, &inner, &outer, 0.1).unwrap_err().to_string();
        assert!(e.contains("leaves nothing"), "{e}");
        assert!(e.contains("operand order"), "the message has to say what to do: {e}");
    }

    #[test]
    fn bodies_that_never_touch_get_an_answer_or_a_reason() {
        let a = cube(20.0, (0.0, 0.0, 0.0));
        let b = cube(5.0, (0.0, 40.0, 0.0));
        // Subtracting something that is not there removes nothing.
        let r = boolean(BoolOp::Subtract, &a, &b, 0.1).unwrap();
        assert_eq!(r.boundaries().len(), 1);
        assert!((volume_of(&r) - 8000.0).abs() < 1e-6);
        // Intersect is empty, and Cadpole has no empty body.
        let e = boolean(BoolOp::Intersect, &a, &b, 0.1).unwrap_err().to_string();
        assert!(e.contains("never touch") && e.contains("empty"), "{e}");
        assert!(e.contains("20.000 mm apart on Y"), "the diagnostic names the gap: {e}");
        // Union of two lumps is the kernel limitation it is.
        let e = boolean(BoolOp::Union, &a, &b, 0.1).unwrap_err().to_string();
        assert!(e.contains("two separate lumps"), "{e}");
        assert!(e.contains(r#"kernel="mesh""#), "the message has to say what to do: {e}");
    }

    #[test]
    fn a_body_inside_a_void_is_not_inside_the_body() {
        // The winding number is summed over every shell, so the cavity of a
        // hollow shell reads as outside its material.
        let shell = boolean(
            BoolOp::Subtract,
            &cube(20.0, (0.0, 0.0, 0.0)),
            &cube(16.0, (2.0, 2.0, 2.0)),
            0.1,
        )
        .unwrap();
        let pea = cube(4.0, (8.0, 8.0, 8.0));
        assert_eq!(disposition(&shell, &pea, 0.1).unwrap(), Disposition::Separate);
        let r = boolean(BoolOp::Subtract, &shell, &pea, 0.1).unwrap();
        assert!((volume_of(&r) - volume_of(&shell)).abs() < 1e-6, "nothing to remove");
        // And subtracting the shell itself, cavity and all, is refused rather
        // than silently losing the island of material the cavity would leave.
        let e = boolean(BoolOp::Subtract, &cube(30.0, (-5.0, -5.0, -5.0)), &shell, 0.1)
            .unwrap_err()
            .to_string();
        assert!(e.contains("island of material"), "{e}");
    }

    #[test]
    fn cutting_a_body_that_already_has_a_cavity_says_what_to_do() {
        // truck's boolean folds shell after shell into one lump, so a solid
        // with a void is beyond it. That is a diagnostic, not a wrong answer.
        let shell = boolean(
            BoolOp::Subtract,
            &cube(20.0, (0.0, 0.0, 0.0)),
            &cube(16.0, (2.0, 2.0, 2.0)),
            0.1,
        )
        .unwrap();
        let post = cube(4.0, (8.0, -2.0, 8.0));
        assert_eq!(disposition(&shell, &post, 0.1).unwrap(), Disposition::Meeting);
        for op in [BoolOp::Subtract, BoolOp::Union, BoolOp::Intersect] {
            let e = boolean(op, &shell, &post, 0.1).unwrap_err().to_string();
            assert!(e.contains("already has a cavity"), "{op:?}: {e}");
            assert!(e.contains(r#"kernel="mesh""#), "{op:?}: {e}");
        }
    }

    // ---- run-to-run stability -------------------------------------

    /// Everything a caller can observe about a tessellated boolean, as bytes.
    fn signature(s: &KSolid) -> String {
        let (mesh, topo) = crate::tessellate::tessellate(s, 0.05).unwrap();
        let mut out = String::new();
        for p in &mesh.positions {
            out.push_str(&format!("{:08x} ", p.to_bits()));
        }
        for i in mesh.indices.iter().chain(&mesh.face_ids) {
            out.push_str(&format!("{i} "));
        }
        for f in &topo.faces {
            out.push_str(&format!("|{}:{:?}", f.name, f.aliases));
        }
        for e in &topo.edges {
            out.push_str(&format!("|{}", e.name));
        }
        out
    }

    #[test]
    fn the_same_boolean_tessellates_identically_twenty_times() {
        let a = cube(20.0, (0.0, 0.0, 0.0));
        // A post through two walls: a boolean shapeops really has to compute.
        let b = cube(6.0, (7.0, 7.0, -3.0));
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..20 {
            seen.insert(signature(&boolean(BoolOp::Subtract, &a, &b, 0.1).unwrap()));
        }
        assert_eq!(seen.len(), 1, "{} distinct tessellations of one boolean", seen.len());
    }

    #[test]
    fn canonical_order_is_the_geometry_not_the_call_order() {
        // Two ways to reach the same solid: the face order must match.
        let a = cube(20.0, (0.0, 0.0, 0.0));
        let b = cube(6.0, (7.0, 7.0, -3.0));
        let c = cube(6.0, (7.0, 7.0, 17.0));
        let cut = |x: &KSolid, y: &KSolid| boolean(BoolOp::Subtract, x, y, 0.1).unwrap();
        let first = cut(&cut(&a, &b), &c);
        let second = cut(&cut(&a, &c), &b);
        assert_eq!(signature(&first), signature(&second));
    }

    /// The example's foot: the reference profile plus `OVERLAP` mm of
    /// full-pitch prism above the top face, which is what gives two
    /// neighbouring feet a shared *face* instead of a shared line.
    fn foot_with_band() -> KSolid {
        let profile: [(f64, f64); 5] =
            [(-5.0, 3.2), (-4.2, 2.4), (-2.4, 2.4), (0.0, 0.0), (0.5, 0.0)];
        let sections: Vec<Section> = profile
            .iter()
            .map(|(z, inset)| {
                section(
                    *z,
                    Loop::rounded_rect(
                        P2::new(0.0, 0.0),
                        42.0 - 2.0 * inset,
                        42.0 - 2.0 * inset,
                        4.0 - inset,
                    ),
                )
            })
            .collect();
        built(&sections)
    }

    // ---- the three contact operations ------------------------------
    //
    // Three reproductions. Each
    // one is a shape the Gridfinity bin actually builds, reduced to the
    // smallest solid that still has the contact in it.

    /// A rectangular block with its minimum corner at `at`.
    fn brick(size: (f64, f64, f64), at: (f64, f64, f64)) -> KSolid {
        let s = Sketch::new(Plane::XY.offset(at.2)).with_loop(Loop::rect(
            P2::new(at.0 + size.0 / 2.0, at.1 + size.1 / 2.0),
            size.0,
            size.1,
        ));
        extrude(&s, size.2).unwrap().remove(0)
    }

    /// `examples/gridfinity_bin.star`'s **last** operation, in miniature: the
    /// one contact case that is a diagnostic rather than an operation.
    ///
    /// The bin trims the welded body to the body footprint with an intersect
    /// against a prism whose side faces lie exactly on the body's own — the
    /// perimeter feet stand 0.25 mm proud of it and that is what the trim takes
    /// off. Here: a 20 mm block with a tab 2 mm proud of its `x = 0` face,
    /// trimmed back by a prism whose `x = 0` face *is* that face.
    ///
    /// **Why this is refused, and must be.** The body *ends* on that plane and
    /// also *crosses* it, which is a "partial coincidence" — a
    /// hard error. Growing the tool outward, the over-cut of operation 2,
    /// would leave the tab standing proud by the size of the growth (measured:
    /// 8006.4 mm³ instead of 8000). Shrinking it would cut the block. There is
    /// no offset of the tool that answers this, because the result's face on
    /// that plane is the body's own face *merged with* the newly cut tab
    /// faces, and merging two coplanar faces is a boolean in the plane, which
    /// the exact kernel does not have. So the answer is the diagnostic, and
    /// what it has to do is say all of that.
    #[test]
    fn a_trim_whose_tool_sides_lie_on_the_bodys_own_is_refused_and_says_why() {
        let body = cube(20.0, (0.0, 0.0, 0.0));
        let tab = brick((4.0, 4.0, 4.0), (-2.0, 8.0, 3.0));
        // The weld itself is transversal: the tab crosses the wall.
        let welded = boolean(BoolOp::Union, &body, &tab, 0.1).unwrap();
        assert!(volume_of(&welded) > 8000.0, "the tab is proud of the block");

        let footprint = brick((20.0, 20.0, 30.0), (0.0, 0.0, -5.0));
        let e = boolean(BoolOp::Intersect, &welded, &footprint, 0.1).unwrap_err().to_string();
        // It names the face pair and both boxes ...
        assert!(e.contains("the first body's face #"), "{e}");
        assert!(e.contains("the second body's face #"), "{e}");
        assert!(e.contains("lie in the same plane"), "{e}");
        // ... the overlap on each axis ...
        assert!(e.contains("they overlap by X "), "{e}");
        assert!(e.contains("Y ") && e.contains("Z "), "{e}");
        // ... why the over-cut is not available here ...
        assert!(e.contains("partial coincidence"), "{e}");
        assert!(e.contains("ends on that plane and crosses it"), "{e}");
        // ... and what to do instead.
        assert!(e.contains("0.5 mm"), "the fix has to be a number: {e}");
        assert!(e.contains(r#"kernel="mesh""#), "{e}");

        // And the ordering the diagnostic recommends does work: cut the tab
        // back *before* it is welded on, while the cut is still transversal.
        let cut = boolean(BoolOp::Intersect, &tab, &footprint, 0.1).unwrap();
        assert!((volume_of(&cut) - 32.0).abs() < 1e-6, "half the tab: {}", volume_of(&cut));
    }

    /// The bin's feet, touching each other along their top edges.
    ///
    /// Two Gridfinity feet one pitch apart meet at `z = 0` along a *line*, and
    /// the union of two solids that share only an edge is not a 2-manifold. So
    /// the example carries each foot `OVERLAP` mm further up as a full-pitch
    /// prism band, which turns that line into a **face**: the two feet then
    /// share real area, and the shared faces are identical and face each other.
    /// That is `glue`'s case — a union with no `shapeops` in it.
    #[test]
    fn two_feet_that_share_a_whole_face_fuse_into_one_solid() {
        let a = foot_with_band();
        let b = transformed(&a, &Transform::translation(Vec3::new(42.0, 0.0, 0.0)));
        let both = boolean(BoolOp::Union, &a, &b, 0.1).unwrap();
        assert_eq!(both.boundaries().len(), 1, "one shell, not two lumps");
        let want = volume_of(&a) + volume_of(&b);
        assert!(
            (volume_of(&both) - want).abs() < want * 1e-6,
            "the fuse adds no material and loses none: {} vs {want}",
            volume_of(&both)
        );
        assert!(is_watertight(&both));
        // The two shared faces are gone: every face of the result is a face of
        // one of the operands, minus the pair that became interior.
        assert_eq!(both.face_iter().count(), a.face_iter().count() * 2 - 2);
    }

    /// A cutter flush with a body face: the pocket whose floor is the body's
    /// own bottom, and the through-slot flush at both ends. This is the
    /// `depth + 0.02` a script would otherwise write by hand, and the one contact case
    /// where moving the *tool* changes nothing at all.
    #[test]
    fn a_cutter_flush_with_a_body_face_still_cuts() {
        let body = brick((20.0, 20.0, 10.0), (0.0, 0.0, 0.0));

        // (a) a blind pocket whose top face is the body's top face.
        let pocket = brick((5.0, 5.0, 6.0), (5.0, 5.0, 4.0));
        let r = boolean(BoolOp::Subtract, &body, &pocket, 0.1).unwrap();
        assert!(
            (volume_of(&r) - (4000.0 - 150.0)).abs() < 1e-3,
            "a 5x5x6 pocket: {}",
            volume_of(&r)
        );
        assert!(is_watertight(&r));

        // (b) a through slot, flush at both ends.
        let slot = brick((5.0, 5.0, 10.0), (5.0, 5.0, 0.0));
        let r = boolean(BoolOp::Subtract, &body, &slot, 0.1).unwrap();
        assert!(
            (volume_of(&r) - (4000.0 - 250.0)).abs() < 1e-3,
            "a 5x5 through slot: {}",
            volume_of(&r)
        );
        assert!(is_watertight(&r));

        // (c) a cutter whose *side* face is the body's side face: a notch in
        // the corner, cut by a tool that is flush on two sides at once.
        let notch = brick((5.0, 5.0, 5.0), (0.0, 5.0, 2.0));
        let r = boolean(BoolOp::Subtract, &body, &notch, 0.1).unwrap();
        assert!(
            (volume_of(&r) - (4000.0 - 125.0)).abs() < 1e-3,
            "a flush-sided notch: {}",
            volume_of(&r)
        );
        assert!(is_watertight(&r));
    }

    /// Boundary segments of a meshed face: the vertex count of its polygon.
    fn rim_of(f: &crate::tessellate::FaceInfo, topo: &crate::tessellate::Topology) -> usize {
        f.edge_ids.iter().map(|&e| topo.edges[e as usize].polyline.len() - 1).sum()
    }

    /// A cone band tessellates as a strip — two triangles per rim segment and
    /// no vertex inside — whatever its height, and so does an extruded arc.
    ///
    /// Before `with_cones_revolved` the same bands were 7,552 triangles a
    /// half-face at the display tolerance, 1 mm tall or 17: truck's NURBS
    /// tessellator halves the height direction along with the arc's, so the
    /// count never followed the geometry. The strip is the tight bound — a
    /// face with `n` boundary vertices and none inside has exactly `n - 2`
    /// triangles — and every curved face is held to it, including the hole's
    /// band of an annular section, whose loop runs the other way round. The
    /// planar caps were boundary-only already (truck's `Plane` has no interior
    /// division) and are held to that: `n - 2` for a disc, `n` for a ring.
    #[test]
    fn a_cone_band_tessellates_as_a_strip_whatever_its_height() {
        let circle = |z: f64, r: f64| section(z, Loop::circle(P2::new(0.0, 0.0), r));
        let annulus = |z: f64, ro: f64, ri: f64| {
            Section::Profile(
                Sketch::new(Plane::XY.offset(z))
                    .with_loop(Loop::circle(P2::new(0.0, 0.0), ro))
                    .with_loop(Loop::circle(P2::new(0.0, 0.0), ri)),
            )
        };
        let disc = extrude(&Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 60.0)), 1.0)
            .unwrap()
            .remove(0);
        let bodies = [
            ("thin band", built(&[circle(0.0, 60.0), circle(1.0, 57.0)])),
            ("tall cone", built(&[circle(0.0, 60.0), circle(17.0, 26.0)])),
            ("lofted cylinder", built(&[circle(0.0, 60.0), circle(5.0, 60.0)])),
            ("annular", built(&[annulus(0.0, 60.0, 50.0), annulus(2.0, 57.0, 48.0)])),
            ("extruded disc", disc),
        ];
        for (what, s) in &bodies {
            let (mesh, topo) = crate::tessellate::tessellate(s, DEFAULT_TOLERANCE).unwrap();
            assert!(mesh.welded(1e-4).is_watertight(), "{what}: watertight");
            assert_eq!(topo.faces.iter().filter(|f| f.surface_kind == "plane").count(), 2, "{what}: two caps");
            for f in &topo.faces {
                let rim = rim_of(f, &topo);
                if f.surface_kind == "plane" {
                    assert!(f.tri_count as usize <= rim, "{what}: a cap with {rim} boundary vertices has no interior");
                } else {
                    assert_eq!(f.tri_count as usize, rim - 2, "{what}: a band with {rim} boundary vertices is a strip");
                }
            }
        }
    }

    /// The chord error of a cylinder is the chord rule's sagitta:
    /// measured on its side, under the tolerance and no less than a fifth of
    /// it (`ksurface`'s cylinder test allows the division up to twice the
    /// chord rule's count), at every tolerance a document uses, and where it
    /// is found is a point off the surface by that much.
    #[test]
    fn the_chord_error_of_a_cylinder_is_its_sagitta_under_the_tolerance() {
        let r = 10.0;
        let cylinder = extrude(&Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), r)), 10.0)
            .unwrap()
            .remove(0);
        for tol in [0.2, 0.05, 0.01] {
            let options = TessellationOptions::new(tol);
            let ((_, topo), used, chord) = crate::tessellate::tessellate_reporting_with(&cylinder, &options).unwrap();
            assert_eq!(used, tol);
            let chord = chord.expect("measured");
            assert!(chord.max <= tol, "tol {tol}: chord error {}", chord.max);
            assert!(chord.max >= tol / 5.0, "tol {tol}: chord error {} is no sagitta", chord.max);
            assert_ne!(topo.faces[chord.face].surface_kind, "plane", "tol {tol}: on the side");
            // The point is a chord midpoint: inside the cylinder by the error.
            let radial = (chord.at.x * chord.at.x + chord.at.y * chord.at.y).sqrt();
            assert!((r - radial - chord.max).abs() < 1e-6, "tol {tol}: at radius {radial}, error {}", chord.max);
        }
    }

    /// A twisted ruled loft — a square lofted to the same square turned 30°
    /// — divides its rulings by the normal bound: each ruling's
    /// polyline has more than its two ends, every side face is more than a
    /// pair of triangles, the surface normal turns by no more than the bound
    /// across any triangle's ruling edge, and the mesh still closes at the
    /// tolerance asked for. Without the bound the rulings are two points
    /// each and the sides are strips.
    #[test]
    fn a_twisted_loft_divides_its_rulings_by_the_normal_bound() {
        let (sn, cs) = fmath::sin_cos(30.0f64.to_radians());
        let turned: Vec<P2> = [(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)]
            .iter()
            .map(|&(x, y): &(f64, f64)| P2::new(x * cs - y * sn, x * sn + y * cs))
            .collect();
        let s = built(&[square(0.0, 20.0), section(40.0, Loop::polygon(&turned))]);
        let options = TessellationOptions::new(DEFAULT_TOLERANCE);
        let ((mesh, topo), used, chord) = crate::tessellate::tessellate_reporting_with(&s, &options).unwrap();
        assert_eq!(used, DEFAULT_TOLERANCE, "at the requested tolerance");
        assert!(mesh.welded(1e-4).is_watertight());
        // The chord error is measured on the twisted bands. It is
        // *not* under the tolerance here — 0.090 mm at 0.05, by both
        // directions of measurement: the bound holds the bilinear cell and
        // the two flat triangles a twisted cell becomes sag more — which is
        // why it is measured and reported rather than assumed. The number
        // is pinned loosely so a division that fixes it moves this test.
        let chord = chord.expect("a twisted band has a surface to measure against");
        assert!(topo.faces[chord.face].surface_kind != "plane", "on a band, not a cap");
        assert!(chord.max > DEFAULT_TOLERANCE && chord.max < 0.12, "chord error {} on face {}", chord.max, chord.face);
        let rulings: Vec<_> = topo.edges.iter().filter(|e| (e.end.z - e.start.z).abs() > 39.0).collect();
        assert_eq!(rulings.len(), 4);
        let want = (30.0f64 / DEFAULT_MAX_NORMAL_DEGREES).ceil() as usize + 1;
        for e in &rulings {
            assert!(e.polyline.len() >= want, "a ruling of {} points, {want} wanted", e.polyline.len());
        }
        for f in topo.faces.iter().filter(|f| f.surface_kind != "plane") {
            assert!(f.tri_count > 2, "a twisted side is not a strip: {} triangles", f.tri_count);
        }
        let strips = crate::tessellate::tessellate_with(&s, &TessellationOptions { max_normal_degrees: 0.0, ..options }).unwrap().1;
        for e in strips.edges.iter().filter(|e| (e.end.z - e.start.z).abs() > 39.0) {
            assert_eq!(e.polyline.len(), 2, "without the normal bound a ruling is its two ends");
        }
    }

    /// The compressor wheel's bore and counterbore, united: two of the
    /// samples of the circle where they meet sit a hair off the bore's
    /// cylinder, and truck's exact inversion refuses them. The nearest point
    /// stands in for a point that close (`ksurface`), so the mesh is made at
    /// the tolerance asked for.
    #[test]
    fn a_bore_with_a_counterbore_tessellates_at_the_requested_tolerance() {
        let bore = extrude(&Sketch::new(Plane::XY.offset(-6.0)).with_loop(Loop::circle(P2::new(0.0, 0.0), 5.0)), 55.0)
            .unwrap()
            .remove(0);
        let counterbore = extrude(&Sketch::new(Plane::XY.offset(41.0)).with_loop(Loop::circle(P2::new(0.0, 0.0), 8.0)), 4.0)
            .unwrap()
            .remove(0);
        let cuts = boolean(BoolOp::Union, &bore, &counterbore, 0.05).unwrap();
        assert_eq!(cuts.face_iter().count(), 10);
        let (mesh, used) = crate::tessellate::tessellate_reporting(&cuts, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(used, DEFAULT_TOLERANCE, "at the requested tolerance");
        assert!(mesh.0.welded(1e-4).is_watertight());
        // The exact 4810 mm³ less the chordal facets of a 22-gon in a 5 mm circle.
        let exact = std::f64::consts::PI * 25.0 * 55.0 + std::f64::consts::PI * (64.0 - 25.0) * 4.0;
        let volume = mesh.0.welded(1e-4).volume();
        assert!(volume < exact && volume > 0.98 * exact, "{volume} against {exact}");
    }

    /// A skinned two-loop loft like the wing's
    /// section 2 — an outline and its inward offset in every section,
    /// `ruled = false` — is re-tessellated where a rigid move lands it.
    /// With the boundary shared by id, twenty sub-millimetre offsets each
    /// mesh closed by id at the plate tolerance, in the time a
    /// whole-millimetre move takes.
    #[test]
    fn a_skinned_two_loop_loft_meshes_closed_wherever_a_rigid_move_lands_it() {
        let section = |z: f64, w: f64, h: f64| {
            Section::Profile(
                Sketch::new(Plane::XY.offset(z))
                    .with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), w, h, 3.0))
                    .with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), w - 4.0, h - 4.0, 1.5)),
            )
        };
        let sections = [section(0.0, 60.0, 14.0), section(30.0, 56.0, 13.0), section(60.0, 50.0, 12.0)];
        let plan = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
        let s = loft(&plan).unwrap().remove(0);
        let tol = 0.01;
        let time = |s: &KSolid| {
            let w = crate::time::Stopwatch::start();
            let meshed = tessellate(s, tol).unwrap();
            (meshed, w.elapsed_ms())
        };
        let (whole, _) = time(&transformed(&s, &Transform::translation(Vec3::new(1.0, 0.0, 0.0))));
        assert_eq!(whole.unmatched_edges().0, 0, "open by id at a whole millimetre");
        let mut seed = 0x9E37_79B9u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 10_000) as f64 / 10_000.0
        };
        let mut times = Vec::new();
        for k in 0..20 {
            let d = Vec3::new(next(), next(), next() * 3.0);
            let (meshed, t) = time(&transformed(&s, &Transform::translation(d)));
            assert_eq!(meshed.unmatched_edges().0, 0, "offset {k} {d:?}: open by id");
            assert_eq!(meshed.faces.len(), s.face_iter().count(), "offset {k} {d:?}: a face missing");
            times.push((t, k, d));
        }
        // The defect this guards against was a factor of a hundred on one
        // offset among its neighbours, so it is judged against the run's
        // own median rather than a clock read under a parallel test load.
        let mut sorted: Vec<f64> = times.iter().map(|t| t.0).collect();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2].max(20.0);
        for (t, k, d) in &times {
            assert!(*t < 8.0 * median, "offset {k} {d:?}: {t:.0} ms against a median of {median:.0} ms");
        }
    }

    /// A skin through sixteen stations — fifteen bands in one NURBS face
    /// per column, more than the sixteen-interval faces a sweep is cut into,
    /// where truck evaluates every basis function of the knot
    /// vector per point — meshes closed by id at the default tolerance and
    /// is a solid.
    #[test]
    fn a_skin_through_sixteen_stations_meshes_closed() {
        let sections: Vec<Section> = (0..16)
            .map(|i| {
                let z = 4.0 * i as f64;
                let r = 10.0 + 3.0 * fmath::sin(z / 12.0);
                section(z, Loop::circle(P2::new(0.0, 0.0), r))
            })
            .collect();
        let plan = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
        let s = loft(&plan).unwrap().remove(0);
        let meshed = tessellate(&s, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(meshed.unmatched_edges().0, 0, "open by id");
        assert_eq!(meshed.faces.len(), s.face_iter().count());
        let (mesh, _) = crate::tessellate::tessellate(&s, DEFAULT_TOLERANCE).unwrap();
        let report = crate::solid::validate(&mesh);
        assert!(report.is_solid(), "{}", report.summary());
    }

    /// An oblique bore through a block — the wing's knuckle bores, the
    /// case that read a pin's radial clearance as 0 — meshes closed by id
    /// at 0.05 and 0.01, is a solid, and every chord is within the
    /// tolerance of the bore. The hole's walls are strips whose rims bow
    /// away from them in (u, v); a Delaunay triangulation in millimetres
    /// fanned chords across the bow, 170° across the bore, and one in the
    /// division's own steps makes the ladder.
    #[test]
    fn an_oblique_bore_meshes_as_a_ladder_within_the_tolerance() {
        let hd = Vec3::new(0.2588, 0.9659, 0.0).normalized();
        let o = Vec3::new(9.3, 0.0, 9.3);
        let block = extrude(&Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(10.0, 10.0), 20.0, 20.0)), 20.0).unwrap().remove(0);
        let hole_plane = Plane::new(o - hd * 5.0, hd, Vec3::Z).unwrap();
        let hole = extrude(&Sketch::new(hole_plane).with_loop(Loop::circle(P2::new(0.0, 0.0), 1.85)), 32.0).unwrap().remove(0);
        let bored = boolean(BoolOp::Subtract, &block, &hole, 0.1).unwrap();
        // The hole crosses the 20 mm block obliquely: 20 / cos 15° long.
        let want = 8000.0 - std::f64::consts::PI * 1.85 * 1.85 * 20.0 / hd.y;
        for tol in [0.05, 0.01] {
            let t = tessellate_reporting(&bored, tol).unwrap_or_else(|e| panic!("at {tol}: {e}"));
            assert_eq!(t.meshed.unmatched_edges().0, 0, "at {tol}: open by id");
            let chord = t.chord.expect("a bore to measure against").max;
            assert!(chord <= tol, "at {tol}: chord error {chord}");
            let (mesh, _) = crate::tessellate::tessellate(&bored, tol).unwrap();
            let report = crate::solid::validate(&mesh);
            assert!(report.is_solid(), "at {tol}: {}", report.summary());
            let v = mesh.volume();
            assert!((v - want).abs() < 0.002 * want, "at {tol}: volume {v}, closed form {want}");
        }
    }

    /// The seam of a revolved band is off its vertices, so the ring the
    /// impeller's shroud starts on — r = 59.9, the radius at which truck's
    /// `acos` rounds a point on the seam 1.3 µm off the surface — meshes.
    /// See `seam_offset`.
    #[test]
    fn a_band_at_a_radius_that_trips_trucks_seam_inversion_still_meshes() {
        let annulus = |z: f64, ro: f64, ri: f64| {
            Section::Profile(
                Sketch::new(Plane::XY.offset(z))
                    .with_loop(Loop::circle(P2::new(0.0, 0.0), ro))
                    .with_loop(Loop::circle(P2::new(0.0, 0.0), ri)),
            )
        };
        let s = built(&[annulus(9.15, 60.0, 59.9), annulus(9.93723201751709, 60.0, 55.4276959918437)]);
        let (mesh, topo) = crate::tessellate::tessellate(&s, DEFAULT_TOLERANCE).unwrap();
        assert!(mesh.welded(1e-4).is_watertight());
        for f in topo.faces.iter().filter(|f| f.surface_kind != "plane") {
            assert_eq!(f.tri_count as usize, rim_of(f, &topo) - 2, "a strip, not a dropped face or a grid");
        }
    }

    /// A band that is not a cone keeps its patch — neither `cone_of` nor
    /// `revolution_of` takes it — and a section's arc lofted to an arc about
    /// a different axis still makes a solid.
    ///
    /// The band's sag across a ruling is within the tolerance and its normal
    /// turns under the bound along one, so it is a strip like a cone's.
    #[test]
    fn a_band_between_arcs_about_different_axes_is_still_ruled_and_solid() {
        let off = section(6.0, Loop::circle(P2::new(3.0, 0.0), 8.0));
        let s = built(&[section(0.0, Loop::circle(P2::new(0.0, 0.0), 10.0)), off]);
        let (mesh, topo) = crate::tessellate::tessellate(&s, DEFAULT_TOLERANCE).unwrap();
        assert!(mesh.welded(1e-4).is_watertight());
        let bands: Vec<_> = s.face_iter().map(|f| f.surface()).filter(|sf| !matches!(sf, Surface::Plane(_))).collect();
        assert_eq!(bands.len(), 2, "two half-bands");
        for band in &bands {
            assert!(cone_of(band).is_none() && revolution_of(band).is_none(), "off-axis arcs are not a cone");
        }
        assert_eq!(topo.faces.iter().filter(|f| f.surface_kind == "plane").count(), 2);
    }

    // ------------------------------------------------ sections in space

    use crate::curve::Curve as C3;
    use crate::loft::Cap;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn built_caps(sections: &[Section], caps: [Cap; 2]) -> KSolid {
        let plan = LoftPlan::with_caps(sections, true, SectionMatch::Index, caps).expect("a valid plan");
        let mut solids = loft(&plan).expect("a solid");
        assert_eq!(solids.len(), 1);
        solids.remove(0)
    }

    /// Volume, watertightness and face count from the mesh the exporter would
    /// see, plus the strict solid verdict: closed, manifold, not inside out.
    fn measure3(s: &KSolid) -> (f64, bool, usize) {
        let (mesh, topo) = crate::tessellate::tessellate(s, 0.02).expect("tessellation");
        let welded = mesh.welded(1e-4);
        let report = crate::solid::validate(&mesh);
        assert!(report.is_solid(), "not a solid: {}", report.summary());
        (welded.volume(), welded.is_watertight(), topo.faces.len())
    }

    /// A blade-like closed section in space: `n` stations along a chord of
    /// `chord` mm, a thickness that tapers from `t` at the trailing edge, a
    /// round nose, and a square trailing edge — drawn from the *middle* of the
    /// trailing edge, so the seam and the nose are the ribbon's two ends.
    /// Turned by `twist` degrees about Z, lifted to `z`, and warped out of
    /// its plane by `warp` (0 keeps it planar).
    fn blade_section(n: usize, chord: f64, t: f64, z: f64, twist: f64, warp: f64) -> C3 {
        let (s, c) = fmath::sin_cos(twist.to_radians());
        let place = |x: f64, y: f64| v(x * c - y * s, x * s + y * c, z + warp * fmath::sin(x / chord * std::f64::consts::PI));
        let h = |x: f64| t / 2.0 * (1.0 - 0.4 * (x / chord) * (x / chord));
        let xs: Vec<f64> = (0..n).map(|i| chord * i as f64 / (n - 1) as f64).collect();
        let press: Vec<Vec3> = xs.iter().map(|&x| place(x, h(x))).collect();
        let suct: Vec<Vec3> = xs.iter().map(|&x| place(x, -h(x))).collect();
        let te = place(chord, 0.0);
        let nose = place(-h(0.0), 0.0);
        C3::new(te)
            .line_to(press[n - 1])
            .unwrap()
            .spline_through(&press[..n - 1].iter().rev().copied().collect::<Vec<_>>(), 0.5)
            .unwrap()
            .arc_to(suct[0], nose)
            .unwrap()
            .spline_through(&suct[1..], 0.5)
            .unwrap()
            .line_to(te)
            .unwrap()
            .close()
            .unwrap()
    }

    /// Two closed splines in space, same
    /// segment count, lofted into one solid whose tip and root lie in no plane
    /// and are closed by the ruled cap. The face count is arithmetic on the
    /// construction: `2n + 1` segments drawn, so `2n + 1` bands (the cap's
    /// half-way cut through the nose is a vertex of the capped wire only, not
    /// a new band); each ribbon `n - 1` ruled pieces plus one flat nose piece
    /// (the seam's two half-lines are straight through it and need no face).
    #[test]
    fn a_loft_between_two_closed_3d_splines_is_a_solid_with_the_expected_faces() {
        let n = 6;
        let root = blade_section(n, 30.0, 2.0, 0.0, 0.0, 0.8);
        let tip = blade_section(n, 26.0, 1.4, 20.0, 25.0, -0.6);
        assert!(root.plane().is_none() && tip.plane().is_none(), "the sections must be genuinely 3D");
        let s = built_caps(&[Section::Curve(root), Section::Curve(tip)], [Cap::Ruled, Cap::Ruled]);
        let (vol, tight, faces) = measure3(&s);
        assert!(tight);
        assert!(vol > 0.0, "volume {vol}");
        // About chord x thickness x height, thinned: a sanity bound, not a formula.
        assert!(vol > 0.5 * 26.0 * 1.4 * 20.0 && vol < 30.0 * 2.0 * 20.0, "volume {vol}");
        assert_eq!(faces, (2 * n + 1) + 2 * n, "bands + two ribbons of n pieces each");
    }

    /// Two *planar but non-parallel* sections — refused as sketches — loft once
    /// they are passed as curves: a square on XY to the same square on a
    /// plane tilted 30 degrees, closed by planes.
    #[test]
    fn two_planar_non_parallel_sections_loft_as_curves() {
        let a = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let tilted = Plane::new(v(0.0, 0.0, 12.0), v(0.0, 0.5, 1.0), Vec3::X).unwrap();
        let b = Sketch::new(tilted).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        // As sketches this is still the parallel-sections refusal...
        let e = LoftPlan::new(&[Section::Profile(a.clone()), Section::Profile(b.clone())], true, SectionMatch::Index)
            .unwrap_err()
            .to_string();
        assert!(e.contains("not parallel") && e.contains("curves"), "{e}");
        // ...and as curves it is a solid.
        let s = built_caps(
            &[Section::Curve(C3::from_loop(&a.loops[0], &a.plane)), Section::Curve(C3::from_loop(&b.loops[0], &b.plane))],
            [Cap::Plane, Cap::Plane],
        );
        let (vol, tight, faces) = measure3(&s);
        assert!(tight);
        assert_eq!(faces, 6, "4 ruled sides + 2 planar caps");
        // The top is tilted, so the volume is between the shortest and the
        // tallest prism the section could make.
        let (lo, hi) = (12.0 - 5.0 * fmath::sin(fmath::atan(0.5f64)) * 1.2, 12.0 + 5.0 * fmath::sin(fmath::atan(0.5f64)) * 1.2);
        assert!(vol > 100.0 * lo * 0.9 && vol < 100.0 * hi * 1.1, "volume {vol}");
    }

    /// The orientation rule: the same sections listed the other way
    /// round, or every section drawn the other way round, still give an
    /// outward-facing solid of the same positive volume — because which way
    /// is "out" is read from the signed volume, not guessed from "up".
    #[test]
    fn reversing_the_list_or_the_curves_still_gives_an_outward_solid() {
        let n = 6;
        let root = blade_section(n, 30.0, 2.0, 0.0, 0.0, 0.8);
        let tip = blade_section(n, 26.0, 1.4, 20.0, 25.0, -0.6);
        let forward = built_caps(&[Section::Curve(root.clone()), Section::Curve(tip.clone())], [Cap::Ruled, Cap::Ruled]);
        let backward = built_caps(&[Section::Curve(tip.clone()), Section::Curve(root.clone())], [Cap::Ruled, Cap::Ruled]);
        let reversed = built_caps(&[Section::Curve(root.reversed()), Section::Curve(tip.reversed())], [Cap::Ruled, Cap::Ruled]);
        let (v0, t0, f0) = measure3(&forward);
        let (v1, t1, f1) = measure3(&backward);
        let (v2, t2, f2) = measure3(&reversed);
        assert!(t0 && t1 && t2);
        assert!(v0 > 0.0 && v1 > 0.0 && v2 > 0.0, "{v0} {v1} {v2}");
        assert_eq!(f0, f1);
        assert_eq!(f0, f2);
        // The same surfaces parametrised the other way round are chorded
        // differently by the tessellator, and on a 2 mm blade a chord error
        // is a percent of the thickness: the volumes agree only as the
        // tolerance shrinks (measured: 1.4% apart at 0.05, 0.23% at 0.02,
        // 0.07% at 0.005), which is what identical geometry looks like.
        let fine = |s: &KSolid| crate::tessellate::tessellate(s, 0.005).unwrap().0.welded(1e-4).volume();
        let (v0, v1, v2) = (fine(&forward), fine(&backward), fine(&reversed));
        assert!((v0 - v1).abs() < 0.005 * v0, "{v0} vs {v1}");
        assert!((v0 - v2).abs() < 0.005 * v0, "{v0} vs {v2}");
        // Same for planar caps on planar-but-twisted sections.
        let a = blade_section(n, 30.0, 2.0, 0.0, 0.0, 0.0);
        let b = blade_section(n, 26.0, 1.4, 20.0, 25.0, 0.0);
        let fwd = built_caps(&[Section::Curve(a.clone()), Section::Curve(b.clone())], [Cap::Plane, Cap::Plane]);
        let bwd = built_caps(&[Section::Curve(b), Section::Curve(a)], [Cap::Plane, Cap::Plane]);
        // The two are the same surfaces parametrised the other way round, so
        // the tessellator chords them differently: equal to the tessellation
        // tolerance, which on a 2 mm blade is a percent of the thickness.
        let (p0, _, _) = measure3(&fwd);
        let (p1, _, _) = measure3(&bwd);
        assert!(p0 > 0.0 && (p0 - p1).abs() < 0.01 * p0, "{p0} vs {p1}");
    }

    /// A sketch section and a curve section in one loft: the sketch is
    /// lifted onto its plane as drawn and pairs with the curve by index.
    #[test]
    fn a_sketch_section_and_a_curve_section_mix() {
        let base = Sketch::new(Plane::XY).with_loop(Loop::rounded_rect(P2::new(0.0, 0.0), 20.0, 12.0, 3.0));
        // The same outline drawn as a curve, smaller, higher and twisted, in
        // the same construction order (the loop starts on its bottom edge).
        let top = C3::from_loop(&Loop::rounded_rect(P2::new(0.0, 0.0), 14.0, 8.0, 2.0), &Plane::XY);
        let (s, c) = fmath::sin_cos(20f64.to_radians());
        let spun = |p: Vec3| v(p.x * c - p.y * s, p.x * s + p.y * c, 15.0 + 0.4 * p.x);
        let top = C3 {
            start: spun(top.start),
            closed: true,
            segments: top
                .segments
                .iter()
                .map(|seg| match seg {
                    Segment3::Line { a, b } => Segment3::Line { a: spun(*a), b: spun(*b) },
                    Segment3::Arc { a, b, via } => Segment3::Arc { a: spun(*a), b: spun(*b), via: spun(*via) },
                    Segment3::Bezier { a, b, ctrl } => {
                        Segment3::Bezier { a: spun(*a), b: spun(*b), ctrl: ctrl.iter().map(|c| spun(*c)).collect() }
                    }
                    Segment3::Helix { .. } => unreachable!("a rounded rectangle has no helix"),
                })
                .collect(),
        };
        assert!(top.plane().is_some(), "an affine image of a planar loop is planar");
        let s = built_caps(&[Section::Profile(base), Section::Curve(top)], [Cap::Plane, Cap::Plane]);
        let (vol, tight, faces) = measure3(&s);
        assert!(tight && vol > 0.0);
        assert_eq!(faces, 8 + 2, "8 bands (4 lines, 4 corner arcs) + 2 caps");
    }

    /// A curve section can still end in a point: straight base segments make
    /// planar triangles, and a twisted quad under a point is six faces once
    /// its own non-planar base is ruled shut.
    #[test]
    fn a_curve_section_still_lofts_to_a_point() {
        let base = C3::new(v(-5.0, -5.0, 0.0))
            .line_to(v(5.0, -5.0, 1.0))
            .unwrap()
            .line_to(v(5.0, 5.0, 0.0))
            .unwrap()
            .line_to(v(-5.0, 5.0, 1.0))
            .unwrap()
            .close()
            .unwrap();
        assert!(base.plane().is_none());
        let s = built_caps(&[Section::Curve(base), Section::Point(v(0.0, 0.0, 12.0))], [Cap::Ruled, Cap::Plane]);
        let (vol, tight, faces) = measure3(&s);
        assert!(tight && vol > 0.0);
        assert_eq!(faces, 4 + 2, "four triangles to the tip, two closing the twisted base");
        // A circular base about the tip's axis is a cone; off it, a refusal.
        let ring = C3::from_loop(&Loop::circle(P2::new(0.0, 0.0), 5.0), &Plane::XY.offset(2.0));
        let cone = built_caps(&[Section::Curve(ring), Section::Point(v(0.0, 0.0, 11.0))], [Cap::Plane, Cap::Plane]);
        let (vol, _, faces) = measure3(&cone);
        let want = std::f64::consts::PI * 25.0 * 9.0 / 3.0;
        assert!((vol - want).abs() < 0.01 * want, "volume {vol}, expected {want}");
        assert_eq!(faces, 3);
        let off = C3::from_loop(&Loop::circle(P2::new(3.0, 0.0), 5.0), &Plane::XY);
        let plan = LoftPlan::with_caps(
            &[Section::Curve(off), Section::Point(v(0.0, 0.0, 9.0))],
            true,
            SectionMatch::Index,
            [Cap::Plane, Cap::Plane],
        )
        .unwrap();
        let e = loft(&plan).unwrap_err().to_string();
        assert!(e.contains("off the axis"), "{e}");
    }

    /// `ruled=False` through curve sections is the same skin: three
    /// twisted blade sections in space skin to one face per column, the
    /// surface passes through the middle section, and the ruled caps close
    /// the ends exactly as they do on a ruled loft.
    #[test]
    fn a_smooth_loft_through_curve_sections_is_one_face_per_column() {
        let n = 6;
        let root = blade_section(n, 30.0, 2.0, 0.0, 0.0, 0.8);
        let mid = blade_section(n, 28.0, 1.7, 10.0, 12.0, 0.1);
        let tip = blade_section(n, 26.0, 1.4, 20.0, 25.0, -0.6);
        let sections = [Section::Curve(root.clone()), Section::Curve(mid.clone()), Section::Curve(tip.clone())];
        let plan = LoftPlan::with_caps(&sections, false, SectionMatch::Index, [Cap::Ruled, Cap::Ruled]).unwrap();
        let smooth = loft(&plan).unwrap().remove(0);
        let (vol, tight, faces) = measure3(&smooth);
        assert!(tight && vol > 0.0, "volume {vol}");
        // One side face per segment of the (cut) sections, not one per band.
        let segs = plan.sections[0].curve().unwrap().segments.len();
        assert_eq!(faces, segs + 2 * n, "{segs} columns + two ribbons of n pieces");
        let plan = LoftPlan::with_caps(&sections, true, SectionMatch::Index, [Cap::Ruled, Cap::Ruled]).unwrap();
        let ruled = loft(&plan).unwrap().remove(0);
        let (rv, _, rf) = measure3(&ruled);
        assert_eq!(rf, 2 * segs + 2 * n, "the ruled loft has one face per band");
        assert!((vol - rv).abs() < 0.05 * rv, "smooth {vol} vs ruled {rv}: the same sections, nearly the same solid");
        // The skin passes through the middle section: its vertices lie on the
        // solid's surface, to the tessellation tolerance.
        let (mesh, _) = crate::tessellate::tessellate(&smooth, 0.02).unwrap();
        for p in plan.sections[1].curve().unwrap().vertices() {
            // To the mesh's *surface*, not its vertices: the column edges
            // are one cubic span through three stations, so the middle one
            // is at t = 0.5 — which truck's halving always sampled and a
            // bound-driven division need not.
            let mut best = f64::MAX;
            for t in 0..mesh.triangle_count() {
                if let Some([a, b, c]) = mesh.triangle(t) {
                    best = best.min(crate::naming::point_triangle_distance(p, a, b, c));
                }
            }
            assert!(best < 0.05, "middle-section vertex {p:?} is {best} mm from the skinned surface");
        }
    }

    /// `match="arclen"` carries over to sections in space: an octagon pairs
    /// with a twelve-gon on a tilted plane by cutting both at each other's
    /// fractions — 8 + 12 − 4 shared = 16 pieces — and the loft builds.
    #[test]
    fn arc_length_matching_works_on_curve_sections() {
        let ring = |n: usize, r: f64| -> Vec<P2> {
            (0..n)
                .map(|i| {
                    let t = std::f64::consts::TAU * i as f64 / n as f64;
                    P2::new(r * fmath::cos(t), r * fmath::sin(t))
                })
                .collect()
        };
        let octagon = C3::from_loop(&Loop::polygon(&ring(8, 10.0)), &Plane::XY);
        let tilted = Plane::new(v(0.0, 0.0, 8.0), v(0.2, 0.0, 1.0), Vec3::X).unwrap();
        let dodecagon = C3::from_loop(&Loop::polygon(&ring(12, 6.0)), &tilted);
        let e = LoftPlan::new(&[Section::Curve(octagon.clone()), Section::Curve(dodecagon.clone())], true, SectionMatch::Index)
            .unwrap_err()
            .to_string();
        assert!(e.contains("match=\"arclen\""), "{e}");
        let plan =
            LoftPlan::new(&[Section::Curve(octagon), Section::Curve(dodecagon)], true, SectionMatch::ArcLength).unwrap();
        let counts: Vec<usize> = plan.sections.iter().map(|s| s.curve().unwrap().segments.len()).collect();
        assert_eq!(counts, vec![16, 16]);
        let mut solids = loft(&plan).unwrap();
        let (vol, tight, faces) = measure3(&solids.remove(0));
        assert!(tight && vol > 0.0);
        assert_eq!(faces, 16 + 2);
    }

}

/// Regressions for the boolean and rigid-move defects fixed together: each
/// test failed on the code before its fix, for the reason its name gives.
#[cfg(test)]
mod regression_tests {
    use super::*;

    fn cylinder(r: f64, h: f64, at: Vec3) -> KSolid {
        let sketch = Sketch::new(Plane::XY.offset(at.z)).with_loop(Loop::circle(P2::new(at.x, at.y), r));
        extrude(&sketch, h).unwrap().remove(0)
    }

    /// Two crossing cylinders joined exactly: the seam is a truck
    /// `IntersectionCurve`, the edge kind every rigid-move hang needed.
    fn crossed_cylinders() -> KSolid {
        let a = cylinder(10.0, 30.0, Vec3::ZERO);
        let b = cylinder(5.0, 40.0, Vec3::new(0.0, 0.0, -20.0));
        let b = transformed(&b, &Transform::rotation(Vec3::ZERO, Vec3::X, 90f64.to_radians()));
        let b = transformed(&b, &Transform::translation(Vec3::new(0.0, 0.0, 15.0)));
        boolean(BoolOp::Union, &a, &b, 0.1).unwrap()
    }

    fn intersection_tolerances(s: &KSolid) -> Vec<f64> {
        s.edge_iter()
            .filter_map(|e| match e.curve() {
                Curve::IntersectionCurve(ic) => Some(ic.tolerance()),
                _ => None,
            })
            .collect()
    }

    /// Run `f` on a thread and give up after `secs`: a hang is a failure, not
    /// a test run that never ends.
    fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()
    }

    /// A rigid move does not change lengths, so an intersection curve's
    /// tolerance must come through unchanged. truck scaled it by the operator
    /// norm of the whole 4x4 homogeneous matrix, translation column included:
    /// a 43 mm move made the tolerance about 43 times coarser.
    #[test]
    fn a_rigid_move_keeps_the_intersection_curve_tolerance() {
        let j = crossed_cylinders();
        let before = intersection_tolerances(&j);
        assert!(!before.is_empty(), "the union should have intersection-curve edges");
        let moved = within(60, move || {
            let m = transformed(&j, &Transform::translation(Vec3::new(100.0, 0.0, 0.0)));
            intersection_tolerances(&m)
        })
        .expect("a translation by 100 mm did not finish");
        assert_eq!(before.len(), moved.len());
        for (b, m) in before.iter().zip(&moved) {
            assert!((b - m).abs() <= 1e-12 * b.abs().max(1.0), "tolerance {b} became {m} under a rigid move");
        }
    }

    /// The offsets that never returned: truck's Matrix4 `norm_l2` finds the
    /// eigenvalues of MᵀM with a Newton polish that has no iteration cap, and
    /// for a translation the double root at 1 can keep it spinning forever.
    #[test]
    fn translating_an_exact_boolean_result_finishes_for_every_offset() {
        let j = crossed_cylinders();
        for tx in [43.0, 46.0, 121.0] {
            let j = j.clone();
            let done = within(60, move || face_count(&transformed(&j, &Transform::translation(Vec3::new(tx, 0.0, 0.0)))));
            assert!(done.is_some(), "translate by ({tx}, 0, 0) did not finish within 60 s");
        }
        let drilled = {
            let body = cylinder(20.0, 20.0, Vec3::ZERO);
            let h = cylinder(3.0, 60.0, Vec3::new(0.0, 0.0, -30.0));
            let h = transformed(&h, &Transform::rotation(Vec3::ZERO, Vec3::X, 90f64.to_radians()));
            let h = transformed(&h, &Transform::translation(Vec3::new(0.0, 0.0, 10.0)));
            boolean(BoolOp::Subtract, &body, &h, 0.1).unwrap()
        };
        let done = within(60, move || {
            face_count(&transformed(&drilled, &Transform::translation(Vec3::new(0.0, 6.0, 28.97193145751953))))
        });
        assert!(done.is_some(), "translate by (0, 6, 28.97193145751953) did not finish within 60 s");
    }

    /// Scaling still scales the tolerance, by the linear part's largest
    /// stretch and nothing else.
    #[test]
    fn a_scale_scales_the_intersection_curve_tolerance_by_its_largest_stretch() {
        let j = crossed_cylinders();
        let before = intersection_tolerances(&j);
        let t = Transform(
            Matrix4::from_translation(Vector3::new(7.0, -3.0, 2.0)) * Matrix4::from_nonuniform_scale(2.0, 0.5, 3.0),
        );
        let after = intersection_tolerances(&transformed(&j, &t));
        for (b, a) in before.iter().zip(&after) {
            assert!((a - 3.0 * b).abs() <= 1e-9 * b, "tolerance {b} became {a}, want {}", 3.0 * b);
        }
    }
    /// An axis-aligned box from `lo` to `hi`.
    fn block(lo: (f64, f64, f64), hi: (f64, f64, f64)) -> KSolid {
        let (w, d, h) = (hi.0 - lo.0, hi.1 - lo.1, hi.2 - lo.2);
        let s = Sketch::new(Plane::XY.offset(lo.2))
            .with_loop(Loop::rect(P2::new(lo.0 + w / 2.0, lo.1 + d / 2.0), w, d));
        extrude(&s, h).unwrap().remove(0)
    }

    fn volume(s: &KSolid) -> f64 {
        crate::tessellate::tessellate(s, 0.05).unwrap().0.volume()
    }

    fn assert_volume(what: &str, r: Result<KSolid>, want: f64) {
        let s = r.unwrap_or_else(|e| panic!("{what}: {e}"));
        let got = volume(&s);
        assert!((got - want).abs() < 1e-6 * want.max(1.0), "{what}: volume {got}, want {want}");
    }

    /// A 10 mm cube with a 6 mm cavity, and an 8 mm block whose faces sit in
    /// the cube's walls: the boundaries never meet, but the block covers the
    /// cavity. One probe on the block's first triangle said "inside the
    /// cube's material" and the answer was a, b, or a with b as a void.
    #[test]
    fn a_body_covering_a_cavity_gets_the_true_boolean() {
        let hollow = boolean(BoolOp::Subtract, &block((0.0, 0.0, 0.0), (10.0, 10.0, 10.0)), &block((2.0, 2.0, 2.0), (8.0, 8.0, 8.0)), 0.1)
            .unwrap();
        assert_eq!(hollow.boundaries().len(), 2);
        let cover = block((1.0, 1.0, 1.0), (9.0, 9.0, 9.0));
        assert_volume("union", boolean(BoolOp::Union, &hollow, &cover, 0.1), 1000.0);
        assert_volume("subtract", boolean(BoolOp::Subtract, &hollow, &cover, 0.1), 1000.0 - 512.0);
        assert_volume("intersect", boolean(BoolOp::Intersect, &hollow, &cover, 0.1), 512.0 - 216.0);
        // And the other way round.
        assert_volume("cover - hollow", boolean(BoolOp::Subtract, &cover, &hollow, 0.1), 216.0);
    }

    /// A bar cut into two lumps by an exact subtract, against a box that
    /// swallows one lump and clears the other. Which lump came first decided
    /// the answer; now every shell is classified.
    #[test]
    fn a_two_lump_body_is_classified_lump_by_lump() {
        let bar = boolean(BoolOp::Subtract, &block((0.0, 0.0, 0.0), (60.0, 10.0, 10.0)), &block((10.0, -1.0, -1.0), (50.0, 11.0, 11.0)), 0.1)
            .unwrap();
        assert_eq!(bar.boundaries().len(), 2);
        assert_volume("bar", Ok(bar.clone()), 2000.0);
        for (which, bx) in [
            ("second", block((40.0, -10.0, -10.0), (70.0, 20.0, 20.0))),
            ("first", block((-10.0, -10.0, -10.0), (20.0, 20.0, 20.0))),
        ] {
            assert_volume(&format!("bar - box over the {which} lump"), boolean(BoolOp::Subtract, &bar, &bx, 0.1), 1000.0);
            assert_volume(&format!("box - bar, {which} lump"), boolean(BoolOp::Subtract, &bx, &bar, 0.1), 26000.0);
            assert_volume(&format!("bar & box, {which} lump"), boolean(BoolOp::Intersect, &bar, &bx, 0.1), 1000.0);
            // Box plus the far lump is two lumps: refused, the way a union of
            // two bodies that never touch is, so auto goes to the mesh kernel.
            let e = boolean(BoolOp::Union, &bar, &bx, 0.1).map(|s| volume(&s));
            assert!(matches!(&e, Err(m) if m.to_string().contains("2 separate lumps")), "union, {which} lump: {e:?}");
        }
    }

    /// A spot face (r = 5 pocket) and a coaxial r = 1.5 hole through its floor,
    /// at boolean tolerance 0.1. truck returned a closed shell whose hole
    /// loop on the pocket floor zigzags across the circle, the subtract
    /// reported success, and the document died later in tessellation with no
    /// script location. A boolean's answer must be a solid that meshes, or an
    /// error at the boolean.
    #[test]
    fn a_boolean_never_returns_a_face_that_cannot_be_meshed() {
        let blk = block((-15.0, -12.0, 0.0), (15.0, 12.0, 15.0));
        let faced = boolean(BoolOp::Subtract, &blk, &cylinder(5.0, 5.0, Vec3::new(0.0, 0.0, 13.0)), 0.1).unwrap();
        for ro in [1.3, 1.5] {
            match boolean(BoolOp::Subtract, &faced, &cylinder(ro, 6.0, Vec3::new(0.0, 0.0, 9.5)), 0.1) {
                Ok(s) => {
                    for tol in [0.1, DEFAULT_TOLERANCE] {
                        if let Err(e) = tessellate(&s, tol) {
                            panic!("hole r={ro}: the subtract succeeded but its result does not mesh at {tol}: {e}");
                        }
                    }
                }
                Err(e) => assert!(e.to_string().contains("subtract"), "hole r={ro}: {e}"),
            }
        }
    }

    /// Plain transversal cuts truck-shapeops fails at one tolerance and
    /// builds at a nearby one: a centred r = 3.3 hole through a 9 mm plate,
    /// and two coaxial cups. All three failed at 0.1 with "the boundaries
    /// meet, but not transversally" or with an untraceable rim edge.
    #[test]
    fn plain_transversal_cuts_build_exactly() {
        let pi = std::f64::consts::PI;
        let plate = block((-15.0, -15.0, 0.0), (15.0, 15.0, 9.0));
        let hole = cylinder(3.3, 11.0, Vec3::new(0.0, 0.0, -1.0));
        let r = boolean(BoolOp::Subtract, &plate, &hole, 0.1);
        let want = 30.0 * 30.0 * 9.0 - pi * 3.3 * 3.3 * 9.0;
        let s = r.unwrap_or_else(|e| panic!("drilled plate: {e}"));
        assert_eq!(s.face_iter().count(), 8);
        let got = volume(&s);
        assert!((got - want).abs() < 0.01 * want, "drilled plate: volume {got}, want {want}");
        for (ro, ri) in [(36.25, 33.05), (44.5, 41.3)] {
            let cup = boolean(
                BoolOp::Subtract,
                &cylinder(ro, 96.0, Vec3::ZERO),
                &cylinder(ri, 96.0, Vec3::new(0.0, 0.0, 5.0)),
                0.1,
            )
            .unwrap_or_else(|e| panic!("cup r={ro}: {e}"));
            let want = pi * ro * ro * 96.0 - pi * ri * ri * 91.0;
            let got = volume(&cup);
            assert!((got - want).abs() < 0.01 * want, "cup r={ro}: volume {got}, want {want}");
        }
    }

    fn attempts<T>(f: impl FnOnce() -> T) -> (T, usize) {
        ladder::ATTEMPTS.with(|n| n.set(0));
        let r = f();
        (r, ladder::ATTEMPTS.with(|n| n.get()))
    }

    /// The finer rungs are tried once per evaluation: the centred r = 3.3
    /// hole that fails at 0.1 builds at a finer rung, and the same cut again
    /// gets one attempt and is refused (under `kernel="auto"` it goes to the
    /// mesh kernel), instead of paying truck's slower finer booleans for a
    /// whole chain of cuts.
    #[test]
    fn the_finer_rungs_are_tried_once_per_evaluation() {
        let scope = ladder::scope();
        let plate = block((-15.0, -15.0, 0.0), (15.0, 15.0, 9.0));
        let hole = cylinder(3.3, 11.0, Vec3::new(0.0, 0.0, -1.0));
        let (first, n) = attempts(|| boolean(BoolOp::Subtract, &plate, &hole, 0.1));
        first.expect("the first cut builds on a finer rung");
        assert!(n >= 2, "the premise: 0.1 fails for this cut ({n} attempt)");
        let (second, n) = attempts(|| boolean(BoolOp::Subtract, &plate, &hole, 0.1));
        assert_eq!(n, 1, "one attempt once the finer rungs are spent");
        assert!(second.is_err());
        drop(scope);
        let _scope = ladder::scope();
        let (again, _) = attempts(|| boolean(BoolOp::Subtract, &plate, &hole, 0.1));
        again.expect("a new evaluation has its finer rungs back");
    }

    /// A pair every rung fails spends them too, so later pairs get one
    /// attempt each; a pair that builds first time is untouched.
    #[test]
    fn after_a_pair_fails_every_rung_later_pairs_get_one_attempt() {
        let _scope = ladder::scope();
        ladder::spend(0.1);
        let plate = block((-15.0, -15.0, 0.0), (15.0, 15.0, 9.0));
        let hole = cylinder(2.0, 11.0, Vec3::new(5.0, 5.0, -1.0));
        let (r, n) = attempts(|| boolean(BoolOp::Subtract, &plate, &hole, 0.1));
        r.expect("an ordinary hole still builds");
        assert_eq!(n, 1);
    }

    /// A cylinder tangent to a wall from inside is a contact, not a numerical
    /// failure. truck fails it with an error (not `None`), and the ladder used
    /// to retry it at two finer tolerances and put the tolerance advice in
    /// front of the named contact. One attempt, and the contact is the answer.
    #[test]
    fn a_graze_that_fails_as_an_error_is_named_after_one_attempt() {

        let wall = block((0.0, 0.0, 0.0), (20.0, 20.0, 10.0));
        let post = cylinder(5.0, 20.0, Vec3::new(5.0, 10.0, -5.0));
        let (r, n) = attempts(|| boolean(BoolOp::Subtract, &wall, &post, 0.1));
        match r {
            Ok(s) => {
                // If truck builds it, it is a closed solid: nothing to name.
                assert!(volume(&s) > 0.0);
                assert_eq!(n, 1);
            }
            Err(e) => {
                let e = e.to_string();
                assert!(n <= 1, "{n} attempts: {e}");
                assert!(!e.contains("--boolean-tolerance"), "{e}");
            }
        }

    }

    /// The race in `guarded`: several threads swapping the
    /// process-wide panic hook. Run in a child process, because what is
    /// being tested is what reaches stderr: eight threads panic inside
    /// `guarded` over and over while two panic outside it. Not one guarded
    /// panic may be printed, and every unguarded one must be.
    #[test]
    fn concurrent_guarded_panics_stay_quiet_and_others_still_print() {
        const CHILD: &str = "CADPOLE_HOOK_RACE_CHILD";
        const LOUD: usize = 2 * 40;
        if std::env::var_os(CHILD).is_some() {
            let quiet: Vec<_> = (0..8)
                .map(|i| {
                    std::thread::spawn(move || {
                        for n in 0..400 {
                            let r = guarded("race", move || -> u8 { panic!("guarded-panic-{i}-{n}") });
                            assert!(r.is_err());
                        }
                    })
                })
                .collect();
            let loud: Vec<_> = (0..2)
                .map(|_| {
                    std::thread::spawn(|| {
                        for _ in 0..LOUD / 2 {
                            let _ = std::panic::catch_unwind(|| panic!("unguarded-panic"));
                            std::thread::yield_now();
                        }
                    })
                })
                .collect();
            for t in quiet.into_iter().chain(loud) {
                t.join().unwrap();
            }
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "kernel::regression_tests::concurrent_guarded_panics_stay_quiet_and_others_still_print", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "the child failed: {err}");
        let leaked = err.matches("guarded-panic-").count();
        let printed = err.matches("unguarded-panic").count();
        assert_eq!(leaked, 0, "{leaked} panics caught by `guarded` reached stderr");
        assert_eq!(printed, LOUD, "{} of {LOUD} unguarded panics were swallowed", LOUD - printed);
    }

    /// A quiet caller's work that panics on another thread: a scoped thread
    /// the caller starts (as brepmesh does per face) and a rayon pool worker
    /// (as truck's tessellation and intersection do). Neither panic may reach
    /// stderr; a pool panic with no section open still does.
    #[test]
    fn panics_on_a_quiet_callers_worker_threads_stay_quiet() {
        const CHILD: &str = "CADPOLE_QUIET_WORKER_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let r = guarded("scoped", || {
                let quiet = is_quiet();
                std::thread::scope(|s| {
                    let h = s.spawn(move || {
                        let work = || -> u8 { panic!("scoped-worker-panic") };
                        if quiet { quietly(work) } else { work() }
                    });
                    assert!(h.join().is_err());
                });
            });
            assert!(r.is_ok());
            let r = guarded("pool", || {
                let (tx, rx) = std::sync::mpsc::channel();
                rayon_core::spawn(move || {
                    let r = std::panic::catch_unwind(|| -> u8 { panic!("pool-worker-panic") });
                    tx.send(r.is_err()).unwrap();
                });
                assert!(rx.recv().unwrap());
            });
            assert!(r.is_ok());
            let (tx, rx) = std::sync::mpsc::channel();
            rayon_core::spawn(move || {
                let r = std::panic::catch_unwind(|| -> u8 { panic!("loud-pool-panic") });
                tx.send(r.is_err()).unwrap();
            });
            assert!(rx.recv().unwrap());
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "kernel::regression_tests::panics_on_a_quiet_callers_worker_threads_stay_quiet", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "the child failed: {err}");
        assert_eq!(err.matches("scoped-worker-panic").count(), 0, "{err}");
        assert_eq!(err.matches("pool-worker-panic").count(), 0, "{err}");
        assert_eq!(err.matches("loud-pool-panic").count(), 1, "a panic outside any quiet section still prints: {err}");
    }

    /// A profile with material on both sides of its axis sweeps through
    /// itself. truck builds the overlapping solid anyway and tessellation
    /// failed on it later ("passes through boundary vertex 11", "does not
    /// close"), saying nothing about the axis. Touching the axis is fine.
    #[test]
    fn revolving_a_profile_across_its_axis_says_so() {
        let tau = std::f64::consts::TAU;
        for c in [0.0, 4.0] {
            let sk = Sketch::new(Plane::XZ).with_loop(Loop::rect(P2::new(c, 5.0), 10.0, 10.0));
            let e = revolve(&sk, Vec3::ZERO, Vec3::Z, tau).map(|_| ()).unwrap_err().to_string();
            assert!(e.contains("crosses the axis"), "centre x={c}: {e}");
        }
        // The same about an axis off the origin, along the sketch's x.
        let sk = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 20.0), 10.0, 10.0));
        let e = revolve(&sk, Vec3::new(0.0, 22.0, 0.0), Vec3::X, tau).map(|_| ()).unwrap_err().to_string();
        assert!(e.contains("crosses the axis"), "{e}");
        // A crossing far under a micrometre, and its reach printed so it
        // does not read as zero.
        for (d, shown) in [(1e-6, "1.0e-6"), (1e-8, "1.0e-8")] {
            let sk = Sketch::new(Plane::XZ).with_loop(Loop::rect(P2::new(5.0 - d, 5.0), 10.0, 10.0));
            let e = revolve(&sk, Vec3::ZERO, Vec3::Z, tau).map(|_| ()).unwrap_err().to_string();
            assert!(e.contains("crosses the axis") && e.contains(shown), "crossing {d}: {e}");
        }
        // A quarter turn is refused too, and the reason holds at any angle.
        let sk = Sketch::new(Plane::XZ).with_loop(Loop::rect(P2::new(0.0, 5.0), 10.0, 10.0));
        let e = revolve(&sk, Vec3::ZERO, Vec3::Z, tau / 4.0).map(|_| ()).unwrap_err().to_string();
        assert!(e.contains("crosses the axis") && e.contains("cannot be swept into one solid"), "{e}");
        assert!(!e.contains("pass through itself"), "{e}");
        // Touching the axis, or clear of it: built.
        for c in [5.0, 12.0] {
            let sk = Sketch::new(Plane::XZ).with_loop(Loop::rect(P2::new(c, 5.0), 10.0, 10.0));
            let s = revolve(&sk, Vec3::ZERO, Vec3::Z, tau).unwrap_or_else(|e| panic!("centre x={c}: {e}"));
            assert!(volume(&s[0]) > 0.0);
        }
    }

}
