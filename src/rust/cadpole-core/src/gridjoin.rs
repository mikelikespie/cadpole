//! The exact union of axis-aligned boxes: what `union(..., weld=)`
//! runs for the boxes of a model built out of them -- a house drawn as a
//! thousand walls, slabs, posts and treads -- so that a story's several
//! hundred touching, overlapping and nearly touching boxes come out as one
//! closed, consistently oriented, embedded surface, every time.
//!
//! **Why not the mesh kernel.** Manifold unions what it is given exactly,
//! and two boxes that meet only along an edge or at a corner *are* a
//! surface pinched to nothing there, which the exit gate refuses.
//! A box-built model is full of such contacts: a wall that stops where
//! another starts, one corner diagonal to the next. Growing every box
//! until the contacts overlap does not reliably join them, can return a
//! surface that is watertight but not a solid, and thickens every wall by
//! twice the growth.
//!
//! **What this does instead.** Every box is cut into the cells of the grid
//! its own faces span, a cell is solid when a box covers it, and the
//! boundary of the solid cells is the answer: exact, because a union of
//! boxes is itself a union of grid cells.
//!
//! 1. **Weld.** On each axis, coordinates closer than `weld` are one grid
//!    line (single linkage), placed at the coordinate most box faces use.
//!    Two boxes 0.01 apart then touch, a sliver thinner than the weld
//!    vanishes, and a coordinate spelled two ways by float arithmetic is
//!    one coordinate.
//! 2. **Sweep.** The grid is swept one z slab at a time with two slabs of
//!    owners in memory -- the first box in operand order that covers each
//!    cell -- so a story of eighteen million cells costs two slices of a
//!    few hundred kilobytes, not the whole volume. A face of the result is a
//!    cell face with a solid cell on one side only, and it is named after
//!    the owning box's side, which lies on exactly that plane.
//! 3. **Bridge.** Two solid cells diagonal across an edge with the other two
//!    empty, or two solid cells meeting at one corner with the other six
//!    empty (Latecki's two critical configurations, and their complements),
//!    are the only places the boundary of a union of cells fails to be a
//!    2-manifold. Each is joined by a bridge -- a square prism `weld` wide
//!    along the edge, a cube `weld` wide at the corner -- which is what a
//!    printer does with parts that touch anyway; then the sweep runs again,
//!    until none is left.
//! 4. **Mesh.** The cell faces on one plane facing one way under one name
//!    are merged into regions, every region's boundary is split at every
//!    corner of every region (so no edge of one face ends in the middle of
//!    another's -- no T-junction), and each region is triangulated by the
//!    exact constrained Delaunay triangulation the tessellator uses
//!    ([`crate::cdt`]). Every edge is then checked to have exactly two
//!    triangles wound against each other: the result is closed and
//!    oriented by construction, and this says so rather than assumes it.

use std::collections::HashMap;

use crate::cdt::{Cdt, Snap};
use crate::geom::Vec3;
use crate::tessellate::{Mesh, Topology};
use crate::{CadpoleError, Result};

/// The six sides of a box in [`GridBox::sides`] order: `2 * axis` is the low
/// side, `2 * axis + 1` the high one.
pub const SIDES: [&str; 6] = ["-x", "+x", "-y", "+y", "-z", "+z"];

/// One axis-aligned box, with the provenance id each of its six sides gives
/// the faces of the result it supplies (`-x, +x, -y, +y, -z, +z`).
#[derive(Clone, Debug, PartialEq)]
pub struct GridBox {
    pub lo: [f64; 3],
    pub hi: [f64; 3],
    pub sides: [u32; 6],
}

/// What the union did, for the diagnostic that says so.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridReport {
    /// Boxes given.
    pub boxes: usize,
    /// Boxes thinner than the weld in some direction, which the weld closed
    /// to nothing; the index of the first one.
    pub slivers: usize,
    pub first_sliver: Option<usize>,
    /// Distinct coordinates the weld moved onto a neighbour.
    pub snapped: usize,
    /// Edge contacts (runs of grid edges) and corner contacts bridged.
    pub bridged_edges: usize,
    pub bridged_corners: usize,
    /// Where the first bridge went, for the diagnostic.
    pub first_bridge: Option<Vec3>,
    /// Grid lines per axis in the last round.
    pub grid: [usize; 3],
    /// Sweeps run: 1 when nothing needed a bridge.
    pub rounds: usize,
    pub triangles: usize,
}

/// The union as an index mesh: welded positions, outward triangles, and the
/// provenance id of each triangle (a box side's, or the bridge's).
#[derive(Clone, Debug)]
pub struct GridMesh {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub provenance: Vec<u32>,
    pub report: GridReport,
    /// The grid lines on each axis once welded, before any bridge: the
    /// planes the boxes' faces lie on. `union(weld=)` moves the operands
    /// that are not boxes onto them where they come within the weld
    /// ([`snap_to_lines`]).
    pub lines: [Vec<f64>; 3],
    /// The boxes once welded, before any bridge: what the union is solid
    /// over, for a caller asking whether a point lies in it
    /// ([`snap_body`]'s contacts).
    pub welded: Vec<([f64; 3], [f64; 3])>,
}

/// The largest slice of the grid one z slab may have: two of them are held,
/// four bytes a cell. 2^24 cells is a 4096 x 4096 plan, 64 MB a slice.
pub const MAX_SLAB_CELLS: usize = 1 << 24;
/// The most cells one union may sweep. The sweep is linear in it; a story
/// of a few hundred boxes is tens of millions.
pub const MAX_CELLS: u64 = 1 << 31;
/// Bridging rounds before the union gives up and says where.
const MAX_ROUNDS: usize = 6;

const NONE: u32 = u32::MAX;

/// Whether a body's tessellation is an axis-aligned box, and if so its two
/// corners and, for each side in [`SIDES`] order, the index in `topo` of the
/// first face lying on it. A box here is a mesh whose every triangle faces
/// along an axis, whose every vertex lies on its bounding box's boundary,
/// and whose volume is its bounding box's -- which together leave nothing
/// but the box. A rotated box, a box with a hole, an L-shaped extrusion are
/// not, and are unioned the usual way.
pub fn axis_box(mesh: &Mesh, topo: &Topology) -> Option<([f64; 3], [f64; 3], [u32; 6])> {
    let (l, h) = mesh.bbox()?;
    let (lo, hi) = ([l.x, l.y, l.z], [h.x, h.y, h.z]);
    if (0..3).any(|a| !(hi[a] > lo[a])) {
        return None;
    }
    let ext = (0..3).map(|a| hi[a] - lo[a]).fold(0.0, f64::max);
    let eps = ext * 1e-6;
    for p in mesh.positions.chunks_exact(3) {
        let p = [p[0] as f64, p[1] as f64, p[2] as f64];
        if !(0..3).any(|a| (p[a] - lo[a]).abs() <= eps || (p[a] - hi[a]).abs() <= eps) {
            return None;
        }
    }
    for t in 0..mesh.triangle_count() {
        let tri = mesh.triangle(t)?;
        let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
        let len = n.length();
        if !(len > 0.0) {
            continue;
        }
        let c = [n.x.abs() / len, n.y.abs() / len, n.z.abs() / len];
        if c.iter().filter(|x| **x > 1.0 - 1e-6).count() != 1 {
            return None;
        }
    }
    let volume = (hi[0] - lo[0]) * (hi[1] - lo[1]) * (hi[2] - lo[2]);
    if (mesh.volume() - volume).abs() > 1e-5 * volume {
        return None;
    }
    let mut sides = [u32::MAX; 6];
    for (i, f) in topo.faces.iter().enumerate() {
        let c = [f.normal.x, f.normal.y, f.normal.z];
        let a = (0..3).max_by(|&x, &y| c[x].abs().total_cmp(&c[y].abs()))?;
        let s = 2 * a + usize::from(c[a] > 0.0);
        if sides[s] == u32::MAX {
            sides[s] = i as u32;
        }
    }
    if sides.contains(&u32::MAX) {
        return None;
    }
    Some((lo, hi, sides))
}

/// The exact union of `boxes` with coordinates welded within `weld`, edge
/// and corner contacts bridged `weld` wide, and the bridges' faces given the
/// provenance id `bridge`. `weld` must be positive.
pub fn union_boxes(boxes: &[GridBox], weld: f64, bridge: u32) -> Result<GridMesh> {
    let _span = crate::kernel::trace::Span::start(format!("grid join of {} boxes, weld {weld}", boxes.len()));
    if !(weld > 0.0) || !weld.is_finite() {
        return Err(CadpoleError::invalid(format!("join: the weld must be a positive length, got {weld}")));
    }
    let mut report = GridReport { boxes: boxes.len(), ..Default::default() };
    for (k, b) in boxes.iter().enumerate() {
        for a in 0..3 {
            if !(b.lo[a].is_finite() && b.hi[a].is_finite()) || b.hi[a] < b.lo[a] {
                return Err(CadpoleError::invalid(format!("join: box {k} is not a box: {:?}..{:?}", b.lo, b.hi)));
            }
        }
    }
    // 1. The weld: every coordinate onto its line, once. Everything after
    // works on exact coordinates, so the bridges' own offsets (half a weld)
    // are never welded away.
    let mut snapped: Vec<GridBox> = Vec::with_capacity(boxes.len());
    let mut lines: [Vec<f64>; 3] = Default::default();
    let mut maps: [(Vec<f64>, Vec<u32>); 3] = Default::default();
    for a in 0..3 {
        let mut values: Vec<f64> = boxes.iter().flat_map(|b| [b.lo[a], b.hi[a]]).collect();
        let (l, raw, idx, moved) = snap_axis(&mut values, weld);
        report.snapped += moved;
        lines[a] = l;
        maps[a] = (raw, idx);
    }
    let line_of = |a: usize, v: f64| -> usize {
        let (raw, idx) = &maps[a];
        let k = raw.partition_point(|x| *x < v);
        idx[k.min(raw.len() - 1)] as usize
    };
    for (k, b) in boxes.iter().enumerate() {
        let mut lo = [0.0; 3];
        let mut hi = [0.0; 3];
        let mut thin = false;
        for a in 0..3 {
            let (i0, i1) = (line_of(a, b.lo[a]), line_of(a, b.hi[a]));
            thin |= i0 >= i1;
            lo[a] = lines[a][i0];
            hi[a] = lines[a][i1];
        }
        if thin {
            report.slivers += 1;
            report.first_sliver.get_or_insert(k);
            continue;
        }
        snapped.push(GridBox { lo, hi, sides: b.sides });
    }
    if snapped.is_empty() {
        return Err(CadpoleError::invalid(format!(
            "join: every one of the {} boxes is thinner than the weld of {} in some direction, so nothing is left",
            boxes.len(),
            weld
        )));
    }

    let welded: Vec<([f64; 3], [f64; 3])> = snapped.iter().map(|b| (b.lo, b.hi)).collect();
    // 2-3. Sweep, bridge, sweep again until nothing needs a bridge.
    let w = weld / 2.0;
    for round in 0..MAX_ROUNDS {
        report.rounds = round + 1;
        let grid = Grid::new(&snapped)?;
        report.grid = [grid.lines[0].len(), grid.lines[1].len(), grid.lines[2].len()];
        let mut faces: Vec<FaceRec> = Vec::new();
        let mut defects = Defects::default();
        grid.sweep(&mut faces, &mut defects)?;
        if defects.is_empty() {
            let (positions, triangles, provenance) = mesh(&grid, faces)?;
            report.triangles = triangles.len();
            return Ok(GridMesh { positions, triangles, provenance, report, lines, welded });
        }
        let (edges, corners) = grid.bridges(&defects, w, bridge, &mut snapped);
        report.bridged_edges += edges;
        report.bridged_corners += corners;
        if report.first_bridge.is_none() {
            report.first_bridge = defects.first_point(&grid);
        }
    }
    let at = report.first_bridge.map(|p| format!(" near ({:.4}, {:.4}, {:.4})", p.x, p.y, p.z)).unwrap_or_default();
    Err(CadpoleError::kernel(format!(
        "join: contacts along an edge or at a corner{at} were still pinched after {MAX_ROUNDS} rounds of bridging; \
         a larger weld closes them, or overlap the boxes there"
    )))
}

/// What [`snap_to_lines`] moved.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnapReport {
    /// Vertex coordinates moved onto a line.
    pub moved: usize,
    /// The furthest any moved, in model units.
    pub furthest: f64,
}

/// `mesh` with every vertex coordinate that lies within `weld` of a grid
/// line on its axis moved onto the nearest such line, or `None`
/// when none was out of place: the weld of [`union_boxes`] carried to an
/// operand that is not a box, so a corbel, a wedge or a sloped slab whose
/// faces rest on the boxes' planes a float rounding away -- or anything up
/// to the weld -- meets them exactly and the mesh boolean sees a tie, not a
/// sliver. The positions are `f32`, and a line is written as the `f32`
/// [`union_boxes`]'s own mesh carries, so the two coincide exactly.
pub fn snap_to_lines(mesh: &Mesh, lines: &[Vec<f64>; 3], weld: f64) -> Option<(Mesh, SnapReport)> {
    let mut out = mesh.clone();
    let mut report = SnapReport::default();
    for (i, p) in out.positions.iter_mut().enumerate() {
        let l = &lines[i % 3];
        if l.is_empty() {
            continue;
        }
        let v = *p as f64;
        let k = l.partition_point(|x| *x < v);
        let nearest = [k.checked_sub(1).map(|j| l[j]), l.get(k).copied()]
            .into_iter()
            .flatten()
            .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()));
        if let Some(n) = nearest {
            let d = (n - v).abs();
            if d <= weld && n as f32 != *p {
                *p = n as f32;
                report.moved += 1;
                report.furthest = report.furthest.max(d);
            }
        }
    }
    (report.moved > 0).then_some((out, report))
}

/// What [`snap_body`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapped {
    /// Vertex coordinates moved onto a line within the weld, and the
    /// furthest any moved; zero when none was, or when that was declined.
    pub snap: SnapReport,
    /// Moving them would have left the body not a solid (a feature thinner
    /// than the weld collapses): left where it was.
    pub declined: bool,
}

/// The weld carried to a mesh body that is not a box: every vertex
/// coordinate within `weld` of a grid line moves onto it
/// ([`snap_to_lines`]), in place, kept only when the body is still a closed,
/// embedded solid afterwards -- so a face modelled on a box's plane is on it
/// exactly, not a float rounding off.
///
/// The faces whose vertices moved get their recorded surfaces refitted: a
/// face recognised as a plane gets the plane its triangles now lie on, any
/// other forgets its recognised class, and the centroid, normal and area are
/// its triangles' again. The mesh boolean's shared-plane snap reads
/// a face's plane from its class, and a stale one put the vertices back off
/// the plane they had just been moved onto -- a 10 nm shear that left a fold.
pub fn snap_body(body: &mut (Mesh, Topology), lines: &[Vec<f64>; 3], weld: f64) -> Snapped {
    let mut out = Snapped::default();
    let Some((m, r)) = snap_to_lines(&body.0, lines, weld) else { return out };
    let opts = crate::solid::SolidOptions { pair_limit: crate::mesh_kernel::UNION_PAIR_LIMIT, ..Default::default() };
    if !crate::solid::validate_with(&m, &opts).is_solid() {
        out.declined = true;
        return out;
    }
    refit_moved_faces(&body.0, &m, &mut body.1);
    body.0 = m;
    out.snap = r;
    out
}

/// A mesh body's faces that rest flat against `boxes` pushed `depth` into
/// them, in place, kept only when the body is still a solid; the
/// number of faces pushed. A face counts when it lies on one of `lines`'
/// planes and box material lies behind all of it: a probe `depth` along its
/// normal from each triangle's centroid and from points just inside each of
/// its corners lands inside a box. The union with the boxes is then an
/// overlap there, which the mesh boolean resolves, and not an exact tie,
/// which it sometimes resolves as a fold -- a 40-degree corbel under a lip
/// came back with the wall's face left whole under it at some depths and
/// not others. The volume pushed is inside the boxes. `union(weld=)` tries
/// this only when the union without it fails.
pub fn push_body(body: &mut (Mesh, Topology), lines: &[Vec<f64>; 3], depth: f64, boxes: &[([f64; 3], [f64; 3])]) -> usize {
    let Some((m, n)) = push_contacts(&body.0, &body.1, lines, depth, boxes) else { return 0 };
    let opts = crate::solid::SolidOptions { pair_limit: crate::mesh_kernel::UNION_PAIR_LIMIT, ..Default::default() };
    if !crate::solid::validate_with(&m, &opts).is_solid() {
        return 0;
    }
    refit_moved_faces(&body.0, &m, &mut body.1);
    body.0 = m;
    n
}

/// [`push_body`]'s work: the faces of `mesh` that lie flat on a grid line's
/// plane with box material behind all of them, every vertex at their
/// positions moved `depth` along the face's normal (one move per axis for a
/// vertex on two such faces). `None` when no face does.
fn push_contacts(mesh: &Mesh, topo: &Topology, lines: &[Vec<f64>; 3], depth: f64, boxes: &[([f64; 3], [f64; 3])]) -> Option<(Mesh, usize)> {
    if boxes.is_empty() || !(depth > 0.0) {
        return None;
    }
    let pos = |i: u32| {
        let k = 3 * i as usize;
        [mesh.positions[k], mesh.positions[k + 1], mesh.positions[k + 2]]
    };
    let inside = |p: [f64; 3]| boxes.iter().any(|(lo, hi)| (0..3).all(|a| p[a] > lo[a] && p[a] < hi[a]));
    let on_line = |a: usize, v: f32| lines[a].iter().any(|l| *l as f32 == v);
    // Moves keyed by exact position, so every copy of a vertex (one per
    // face it is on) moves together and no crack opens.
    let mut moves: HashMap<[u32; 3], [f64; 3]> = HashMap::new();
    let mut pushed = 0;
    for f in &topo.faces {
        let tris = f.tri_start as usize..(f.tri_start + f.tri_count) as usize;
        if tris.is_empty() {
            continue;
        }
        // Flat on one axis plane: every vertex shares the coordinate.
        let first = pos(mesh.indices[3 * tris.start]);
        let Some(a) = (0..3).find(|a| tris.clone().all(|t| (0..3).all(|c| pos(mesh.indices[3 * t + c])[*a] == first[*a])))
        else {
            continue;
        };
        if !on_line(a, first[a]) {
            continue;
        }
        let mut sign = 0.0;
        let mut ok = true;
        for t in tris.clone() {
            let p = [0, 1, 2].map(|c| pos(mesh.indices[3 * t + c]).map(|x| x as f64));
            let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            if n[a] == 0.0 {
                continue;
            }
            let s = n[a].signum();
            if sign == 0.0 {
                sign = s;
            } else if s != sign {
                ok = false;
                break;
            }
            let c = [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0, (p[0][2] + p[1][2] + p[2][2]) / 3.0];
            // The centroid and a point just inside each corner: a face only
            // partly backed by a box is left alone.
            let probes = [c, p[0], p[1], p[2]].map(|v| {
                let mut q = [0.0; 3];
                for k in 0..3 {
                    q[k] = v[k] + (c[k] - v[k]) * 1e-3;
                }
                q[a] += s * depth;
                q
            });
            if !probes.iter().all(|q| inside(*q)) {
                ok = false;
                break;
            }
        }
        if !ok || sign == 0.0 {
            continue;
        }
        pushed += 1;
        for t in tris {
            for c in 0..3 {
                let v = pos(mesh.indices[3 * t + c]);
                let m = moves.entry(v.map(f32::to_bits)).or_insert([0.0; 3]);
                m[a] = sign * depth;
            }
        }
    }
    if pushed == 0 {
        return None;
    }
    let mut out = mesh.clone();
    for p in out.positions.chunks_exact_mut(3) {
        if let Some(m) = moves.get(&[p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]) {
            for a in 0..3 {
                p[a] = (p[a] as f64 + m[a]) as f32;
            }
        }
    }
    Some((out, pushed))
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn refit_moved_faces(before: &Mesh, after: &Mesh, topo: &mut Topology) {
    use crate::surface_class::SurfaceClass;
    let at = |m: &Mesh, i: u32| {
        let k = 3 * i as usize;
        Vec3::new(m.positions[k] as f64, m.positions[k + 1] as f64, m.positions[k + 2] as f64)
    };
    for f in topo.faces.iter_mut() {
        let tris = f.tri_start as usize..(f.tri_start + f.tri_count) as usize;
        let moved = tris.clone().any(|t| {
            (0..3).any(|c| {
                let i = after.indices[3 * t + c];
                at(before, i) != at(after, i)
            })
        });
        if !moved {
            continue;
        }
        let (mut vector, mut area, mut centroid) = (Vec3::ZERO, 0.0, Vec3::ZERO);
        for t in tris {
            let p = [0, 1, 2].map(|c| at(after, after.indices[3 * t + c]));
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            let a = n.length() * 0.5;
            vector = vector + n * 0.5;
            area += a;
            centroid = centroid + (p[0] + p[1] + p[2]) * (a / 3.0);
        }
        if area <= 0.0 || vector.length() <= 0.0 {
            f.surface = SurfaceClass::Other;
            continue;
        }
        let n = vector.normalized();
        let c = centroid * (1.0 / area);
        f.surface = match &f.surface {
            SurfaceClass::Plane { normal, .. } => {
                SurfaceClass::Plane { origin: c, normal: if n.dot(*normal) < 0.0 { -n } else { n } }
            }
            _ => SurfaceClass::Other,
        };
        f.centroid = c;
        f.normal = n;
        f.area = area;
    }
}

/// The nearest grid line to any vertex of `mesh` that lies beyond `weld`
/// but within `reach` of it: `(axis, line, distance)` -- a face that nearly
/// rests on the boxes' plane and that the weld did not reach. What
/// a refused `union(weld=)` names as the thing to change.
pub fn nearest_miss(mesh: &Mesh, lines: &[Vec<f64>; 3], weld: f64, reach: f64) -> Option<(usize, f64, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, p) in mesh.positions.iter().enumerate() {
        let a = i % 3;
        let l = &lines[a];
        let v = *p as f64;
        let k = l.partition_point(|x| *x < v);
        for n in [k.checked_sub(1).map(|j| l[j]), l.get(k).copied()].into_iter().flatten() {
            let d = (n - v).abs();
            if d > weld && d <= reach && best.is_none_or(|(_, _, b)| d < b) {
                best = Some((a, n, d));
            }
        }
    }
    best
}

/// The axis-aligned boxes a closed, outward-wound mesh whose every face is
/// perpendicular to an axis -- an orthogonal polyhedron, which is what
/// [`union_boxes`] makes -- is the union of, cell by cell on the grid its
/// own vertices span: what lets two such solids that share faces be
/// joined exactly by [`union_boxes`] instead of a mesh boolean that has to
/// guess at their ties. `None` when the mesh is not one: a triangle off the
/// axis planes, or a surface that does not enclose its cells once (open,
/// wound inward, or through itself). [`orthogonal_core`] is the same for a
/// mesh that is orthogonal only in part.
pub fn orthogonal_boxes(mesh: &Mesh, eps: f64) -> Option<Vec<([f64; 3], [f64; 3])>> {
    orthogonal_core(mesh, eps).filter(|c| c.complete).map(|c| c.boxes)
}

/// What [`orthogonal_core`] found.
#[derive(Clone, Debug, PartialEq)]
pub struct OrthogonalCore {
    /// Boxes wholly inside the solid, not overlapping each other.
    pub boxes: Vec<([f64; 3], [f64; 3])>,
    /// Whether they are the whole solid: every face was perpendicular to an
    /// axis. Otherwise they are the part of it no slanted face reaches.
    pub complete: bool,
}

/// The boxes of a closed, outward-wound mesh's solid cells on the grid its
/// own vertices span, and whether they are all of it.
///
/// Each vertex coordinate is put on a line of its axis: values within `eps`
/// of the value before them on one line, so a coordinate spelled two ways,
/// or a needle a cleanup left a few micrometres wide, is one line. A column
/// of cells over each plan cell is solid between a face below it and a face
/// above it: a face perpendicular to z covers a cell when the winding number
/// of its triangles' edges about the cell's centre is non-zero, counted
/// along a ray towards +x -- with exact integer comparisons on the grid for
/// an edge parallel to y (a cell centre is never on a grid line); an edge
/// across a face is shared by two of its triangles in opposite directions,
/// computed once from its lower end so the two cancel exactly, and one that
/// borders a slanted triangle only crosses cells taken out of the count
/// below. Faces that face down open a column, faces that face up close it.
/// A triangle on no axis plane -- a corbel's slope, a slanted sliver --
/// takes the columns its plan box touches out of the count, since it may
/// cross them anywhere: the cells of every other column are exact, and are
/// the core. The solid cells are
/// returned as boxes: runs along x, merged across y and then across z where
/// they line up. `None` when the mesh is empty or its counted columns do
/// not enclose their cells once (open, wound inward, or through itself).
pub fn orthogonal_core(mesh: &Mesh, eps: f64) -> Option<OrthogonalCore> {
    let n = mesh.vertex_count();
    if mesh.triangle_count() == 0 || n == 0 {
        return None;
    }
    // Lines per axis, and every vertex's index on each: values sorted, a
    // value within `eps` of the one before it on the same line.
    let mut lines: [Vec<f64>; 3] = Default::default();
    let mut index = vec![[0u32; 3]; n];
    for a in 0..3 {
        let mut order: Vec<(f64, u32)> = (0..n).map(|v| (mesh.positions[3 * v + a] as f64, v as u32)).collect();
        order.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut l: Vec<f64> = Vec::new();
        let mut last = f64::NEG_INFINITY;
        for (x, v) in order {
            if !x.is_finite() {
                return None;
            }
            if l.is_empty() || x - last > eps {
                l.push(x);
            }
            last = x;
            index[v as usize][a] = (l.len() - 1) as u32;
        }
        lines[a] = l;
    }
    let [nx, ny, nz] = [lines[0].len(), lines[1].len(), lines[2].len()];
    if nx < 2 || ny < 2 || nz < 2 {
        return None;
    }
    let (cx, cy) = (nx - 1, ny - 1);
    if (cx as u64) * (cy as u64) > MAX_SLAB_CELLS as u64 || (cx as u64) * (cy as u64) * ((nz - 1) as u64) > MAX_CELLS {
        return None;
    }
    // The edges parallel to y of the faces perpendicular to z, by plane:
    // (x line, y from, y to, +1 going +y / -1 going -y); and the columns a
    // slanted triangle reaches.
    let mut planes: Vec<Vec<(u32, u32, u32, i32)>> = vec![Vec::new(); nz];
    let mut diagonals: Vec<Vec<([u32; 3], [u32; 3], i32)>> = vec![Vec::new(); nz];
    let mut bad = vec![false; cx * cy];
    let mut complete = true;
    for t in mesh.indices.chunks_exact(3) {
        let p = [index[t[0] as usize], index[t[1] as usize], index[t[2] as usize]];
        let flat = |a: usize| p[0][a] == p[1][a] && p[1][a] == p[2][a];
        let on = (0..3).filter(|a| flat(*a)).count();
        if on == 0 {
            complete = false;
            let lo = |a: usize| p.iter().map(|q| q[a]).min().unwrap_or(0);
            let hi = |a: usize| p.iter().map(|q| q[a]).max().unwrap_or(0);
            // The cells whose plan its plan box reaches into.
            for j in lo(1)..hi(1) {
                for i in lo(0)..hi(0) {
                    bad[j as usize * cx + i as usize] = true;
                }
            }
            continue;
        }
        if on > 1 || !flat(2) {
            continue; // collapsed to a line, or a face across x or y
        }
        for k in 0..3 {
            let (u, v) = (p[k], p[(k + 1) % 3]);
            if u[1] == v[1] {
                continue; // along x: never crosses a ray at a cell's centre
            }
            let d = if u[1] < v[1] { 1 } else { -1 };
            let (a, b) = if (u[1], u[0]) < (v[1], v[0]) { (u, v) } else { (v, u) };
            if a[0] == b[0] {
                planes[p[0][2] as usize].push((a[0], a[1], b[1], d));
            } else {
                diagonals[p[0][2] as usize].push((a, b, d));
            }
        }
    }
    // The centres of the cells along x, for a diagonal's crossings.
    let centres: Vec<f64> = (0..cx).map(|i| 0.5 * (lines[0][i] + lines[0][i + 1])).collect();
    // Sweep up the planes: a cell's winding over its plane's faces, and
    // the column's state above the plane.
    let mut solid = vec![0i32; cx * cy];
    let mut row = vec![0i32; cx + 1];
    let mut open: HashMap<(u32, u32, u32, u32), u32> = HashMap::new();
    let mut out: Vec<([f64; 3], [f64; 3])> = Vec::new();
    let mut by_row: Vec<Vec<(u32, i32)>> = vec![Vec::new(); cy];
    for (k, edges) in planes.iter().enumerate() {
        for r in by_row.iter_mut() {
            r.clear();
        }
        for &(x, lo, hi, d) in edges {
            for j in lo..hi {
                by_row[j as usize].push((x, d));
            }
        }
        // A diagonal counts for the cells whose centre is left of where it
        // crosses the row's centre line.
        for &(a, b, d) in &diagonals[k] {
            let (ax, ay) = (lines[0][a[0] as usize], lines[1][a[1] as usize]);
            let (bx, by) = (lines[0][b[0] as usize], lines[1][b[1] as usize]);
            for j in a[1]..b[1] {
                let y = 0.5 * (lines[1][j as usize] + lines[1][j as usize + 1]);
                let x = ax + (y - ay) * (bx - ax) / (by - ay);
                let cut = centres.partition_point(|c| *c < x) as u32;
                by_row[j as usize].push((cut, d));
            }
        }
        for (j, crossings) in by_row.iter().enumerate() {
            if crossings.is_empty() {
                continue;
            }
            row.fill(0);
            // An edge at x line X counts for every cell left of it.
            for &(x, d) in crossings {
                row[0] += d;
                row[x as usize] -= d;
            }
            let mut w = 0;
            for i in 0..cx {
                w += row[i];
                // A face up (+z, counter-clockwise from above) winds +1 and
                // closes the column; a face down winds -1 and opens it.
                solid[j * cx + i] -= w;
            }
        }
        if (0..cx * cy).any(|c| !bad[c] && solid[c] != 0 && solid[c] != 1) {
            return None;
        }
        if k + 1 == nz {
            if (0..cx * cy).any(|c| !bad[c] && solid[c] != 0) {
                return None; // a column left open at the top
            }
            break;
        }
        // The slab k..k+1 as rectangles: runs along x, merged down y.
        let mut rects: Vec<(u32, u32, u32, u32)> = Vec::new();
        let mut prev: Vec<(u32, u32, usize)> = Vec::new(); // (i0, i1, rect) of the row below
        let on = |c: usize| !bad[c] && solid[c] == 1;
        for j in 0..cy {
            let mut cur: Vec<(u32, u32, usize)> = Vec::new();
            let mut i = 0;
            while i < cx {
                if !on(j * cx + i) {
                    i += 1;
                    continue;
                }
                let s = i;
                while i < cx && on(j * cx + i) {
                    i += 1;
                }
                let (i0, i1) = (s as u32, i as u32);
                match prev.iter().find(|(a, b, _)| *a == i0 && *b == i1) {
                    Some(&(_, _, r)) => {
                        rects[r].3 = j as u32 + 1;
                        cur.push((i0, i1, r));
                    }
                    None => {
                        rects.push((i0, i1, j as u32, j as u32 + 1));
                        cur.push((i0, i1, rects.len() - 1));
                    }
                }
            }
            prev = cur;
        }
        // Across z: a rectangle that goes on from the slab below keeps its
        // box open; the rest close.
        let mut next: HashMap<(u32, u32, u32, u32), u32> = HashMap::with_capacity(rects.len());
        for r in rects {
            let start = open.remove(&r).unwrap_or(k as u32);
            next.insert(r, start);
        }
        for (r, start) in open.drain() {
            out.push(cell_box(&lines, r, start, k as u32));
        }
        open = next;
    }
    for (r, start) in open.drain() {
        out.push(cell_box(&lines, r, start, (nz - 1) as u32));
    }
    if out.is_empty() && complete {
        return None;
    }
    out.sort_by(|a, b| a.0[2].total_cmp(&b.0[2]).then(a.0[1].total_cmp(&b.0[1])).then(a.0[0].total_cmp(&b.0[0])));
    Some(OrthogonalCore { boxes: out, complete })
}

/// The box over plan cells `i0..i1` x `j0..j1` from z line `k0` to `k1`.
fn cell_box(lines: &[Vec<f64>; 3], (i0, i1, j0, j1): (u32, u32, u32, u32), k0: u32, k1: u32) -> ([f64; 3], [f64; 3]) {
    (
        [lines[0][i0 as usize], lines[1][j0 as usize], lines[2][k0 as usize]],
        [lines[0][i1 as usize], lines[1][j1 as usize], lines[2][k1 as usize]],
    )
}

/// One axis's coordinates welded: sorted, every value within `weld` of the
/// previous distinct value joins its line (single linkage), and the line
/// sits at the value the most faces use, the smallest on a tie. Returns the
/// lines, the distinct input values in order, the line of each, and how
/// many distinct values moved.
fn snap_axis(values: &mut [f64], weld: f64) -> (Vec<f64>, Vec<f64>, Vec<u32>, usize) {
    values.sort_by(|a, b| a.total_cmp(b));
    let mut distinct: Vec<(f64, usize)> = Vec::new();
    for &v in values.iter() {
        match distinct.last_mut() {
            Some((d, n)) if *d == v => *n += 1,
            _ => distinct.push((v, 1)),
        }
    }
    let mut lines = Vec::new();
    let mut idx = vec![0u32; distinct.len()];
    let mut moved = 0;
    let mut start = 0;
    while start < distinct.len() {
        let mut end = start + 1;
        while end < distinct.len() && distinct[end].0 - distinct[end - 1].0 <= weld {
            end += 1;
        }
        // The value most faces use, the smallest on a tie (`max_by_key`
        // keeps the last maximum, so walk from the right).
        let rep = distinct[start..end].iter().rev().max_by_key(|(_, n)| *n).map(|(v, _)| *v).unwrap_or(distinct[start].0);
        for (k, d) in distinct[start..end].iter().enumerate() {
            idx[start + k] = lines.len() as u32;
            if d.0 != rep {
                moved += 1;
            }
        }
        lines.push(rep);
        start = end;
    }
    (lines, distinct.into_iter().map(|(v, _)| v).collect(), idx, moved)
}

/// A box on the grid: its cell range per axis and its sides' provenance.
#[derive(Clone, Copy, Debug)]
struct IBox {
    r: [[u32; 2]; 3],
    sides: [u32; 6],
}

/// One cell face of the result: on the plane `plane` across `axis`, facing
/// `+axis` (`orient` 1) or `-axis` (-1), at cell `(u, v)` of that plane,
/// where `u` runs along the next axis and `v` the one after
/// (`(axis + 1) % 3`, `(axis + 2) % 3`), named by `prov`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FaceRec {
    axis: u8,
    plane: u32,
    orient: i8,
    prov: u32,
    v: u32,
    u: u32,
}

/// The places the solid cells are pinched: grid edges (`axis`, the grid
/// point the unit segment starts at) and grid points.
#[derive(Default)]
struct Defects {
    edges: Vec<(u8, [u32; 3])>,
    corners: Vec<[u32; 3]>,
}

impl Defects {
    fn is_empty(&self) -> bool {
        self.edges.is_empty() && self.corners.is_empty()
    }

    fn first_point(&self, grid: &Grid) -> Option<Vec3> {
        let p = self.edges.first().map(|e| e.1).or_else(|| self.corners.first().copied())?;
        Some(grid.point(p))
    }
}

struct Grid {
    lines: [Vec<f64>; 3],
    boxes: Vec<IBox>,
}

impl Grid {
    fn new(boxes: &[GridBox]) -> Result<Grid> {
        let mut lines: [Vec<f64>; 3] = Default::default();
        for (a, l) in lines.iter_mut().enumerate() {
            let mut v: Vec<f64> = boxes.iter().flat_map(|b| [b.lo[a], b.hi[a]]).collect();
            v.sort_by(|x, y| x.total_cmp(y));
            v.dedup();
            *l = v;
        }
        let n = [lines[0].len() - 1, lines[1].len() - 1, lines[2].len() - 1];
        if n[0] * n[1] > MAX_SLAB_CELLS {
            return Err(CadpoleError::invalid(format!(
                "join: the boxes span a plan grid of {} x {} cells, more than the {} one slice may hold; join a story \
                 or a band at a time",
                n[0], n[1], MAX_SLAB_CELLS
            )));
        }
        if (n[0] as u64) * (n[1] as u64) * (n[2] as u64) > MAX_CELLS {
            return Err(CadpoleError::invalid(format!(
                "join: the boxes span {} x {} x {} grid cells, more than {MAX_CELLS}; join a story or a band at a time",
                n[0], n[1], n[2]
            )));
        }
        let index = |a: usize, v: f64| lines[a].partition_point(|x| *x < v) as u32;
        let ib = boxes
            .iter()
            .map(|b| IBox {
                r: [
                    [index(0, b.lo[0]), index(0, b.hi[0])],
                    [index(1, b.lo[1]), index(1, b.hi[1])],
                    [index(2, b.lo[2]), index(2, b.hi[2])],
                ],
                sides: b.sides,
            })
            .collect();
        Ok(Grid { lines, boxes: ib })
    }

    fn n(&self) -> [usize; 3] {
        [self.lines[0].len() - 1, self.lines[1].len() - 1, self.lines[2].len() - 1]
    }

    fn point(&self, p: [u32; 3]) -> Vec3 {
        Vec3::new(self.lines[0][p[0] as usize], self.lines[1][p[1] as usize], self.lines[2][p[2] as usize])
    }

    /// One z slab at a time: the owners of slab `k` (the first box in
    /// operand order covering each cell) beside those of slab `k - 1`; the
    /// cell faces between a solid and an empty cell; and the pinches.
    fn sweep(&self, faces: &mut Vec<FaceRec>, defects: &mut Defects) -> Result<()> {
        let [nx, ny, nz] = self.n();
        let mut prev = vec![NONE; nx * ny];
        let mut cur = vec![NONE; nx * ny];
        let mut starts: Vec<u32> = (0..self.boxes.len() as u32).collect();
        starts.sort_by_key(|&b| (self.boxes[b as usize].r[2][0], b));
        let mut next_start = 0;
        let mut active: Vec<u32> = Vec::new();
        for k in 0..=nz {
            if k < nz {
                active.retain(|&b| self.boxes[b as usize].r[2][1] as usize > k);
                while next_start < starts.len() && self.boxes[starts[next_start] as usize].r[2][0] as usize == k {
                    let b = starts[next_start];
                    let at = active.partition_point(|&x| x < b);
                    active.insert(at, b);
                    next_start += 1;
                }
                cur.fill(NONE);
                for &b in &active {
                    let r = self.boxes[b as usize].r;
                    for j in r[1][0] as usize..r[1][1] as usize {
                        let row = &mut cur[j * nx + r[0][0] as usize..j * nx + r[0][1] as usize];
                        for c in row.iter_mut() {
                            if *c == NONE {
                                *c = b;
                            }
                        }
                    }
                }
            } else {
                cur.fill(NONE);
            }
            let side = |b: u32, s: usize| self.boxes[b as usize].sides[s];
            // Faces across z between slab k - 1 (`prev`) and slab k.
            for j in 0..ny {
                for i in 0..nx {
                    let (below, above) = (prev[j * nx + i], cur[j * nx + i]);
                    if (below != NONE) != (above != NONE) {
                        let (orient, prov) = if below != NONE { (1, side(below, 5)) } else { (-1, side(above, 4)) };
                        faces.push(FaceRec { axis: 2, plane: k as u32, orient, prov, u: i as u32, v: j as u32 });
                    }
                }
            }
            if k < nz {
                // Faces across x and y inside slab k.
                for j in 0..ny {
                    for i in 0..=nx {
                        let l = if i > 0 { cur[j * nx + i - 1] } else { NONE };
                        let r = if i < nx { cur[j * nx + i] } else { NONE };
                        if (l != NONE) != (r != NONE) {
                            let (orient, prov) = if l != NONE { (1, side(l, 1)) } else { (-1, side(r, 0)) };
                            faces.push(FaceRec { axis: 0, plane: i as u32, orient, prov, u: j as u32, v: k as u32 });
                        }
                    }
                }
                for i in 0..nx {
                    for j in 0..=ny {
                        let d = if j > 0 { cur[(j - 1) * nx + i] } else { NONE };
                        let u = if j < ny { cur[j * nx + i] } else { NONE };
                        if (d != NONE) != (u != NONE) {
                            let (orient, prov) = if d != NONE { (1, side(d, 3)) } else { (-1, side(u, 2)) };
                            faces.push(FaceRec { axis: 1, plane: j as u32, orient, prov, u: k as u32, v: i as u32 });
                        }
                    }
                }
                // Pinched edges along z inside slab k.
                let s = |i: usize, j: usize| cur[j * nx + i] != NONE;
                for j in 1..ny {
                    for i in 1..nx {
                        if diagonal(s(i - 1, j - 1), s(i, j - 1), s(i - 1, j), s(i, j)) {
                            defects.edges.push((2, [i as u32, j as u32, k as u32]));
                        }
                    }
                }
            }
            if k > 0 && k < nz {
                let p = |i: usize, j: usize| prev[j * nx + i] != NONE;
                let c = |i: usize, j: usize| cur[j * nx + i] != NONE;
                // Pinched edges along x and along y on the plane z = k.
                for j in 1..ny {
                    for i in 0..nx {
                        if diagonal(p(i, j - 1), p(i, j), c(i, j - 1), c(i, j)) {
                            defects.edges.push((0, [i as u32, j as u32, k as u32]));
                        }
                    }
                }
                for j in 0..ny {
                    for i in 1..nx {
                        if diagonal(p(i - 1, j), p(i, j), c(i - 1, j), c(i, j)) {
                            defects.edges.push((1, [i as u32, j as u32, k as u32]));
                        }
                    }
                }
                // Pinched corners: two solid octants meeting at one point
                // with the other six empty, or the reverse.
                for j in 1..ny {
                    for i in 1..nx {
                        let mut bits = 0u8;
                        for (b, (x, y, z)) in OCTANTS.iter().enumerate() {
                            let (ii, jj) = (i - 1 + x, j - 1 + y);
                            if if *z == 0 { p(ii, jj) } else { c(ii, jj) } {
                                bits |= 1 << b;
                            }
                        }
                        if corner_pinch(bits) {
                            defects.corners.push([i as u32, j as u32, k as u32]);
                        }
                    }
                }
            }
            std::mem::swap(&mut prev, &mut cur);
        }
        Ok(())
    }

    /// A bridge box for every pinch: one square prism `2 w` across along
    /// each run of pinched edges, one cube `2 w` across at each pinched
    /// corner, appended to `boxes` with the provenance `bridge` on every
    /// side. Returns how many of each.
    fn bridges(&self, d: &Defects, w: f64, bridge: u32, boxes: &mut Vec<GridBox>) -> (usize, usize) {
        // Runs: consecutive unit segments along one grid line.
        let mut by_line: HashMap<(u8, [u32; 2]), Vec<u32>> = HashMap::new();
        for (axis, p) in &d.edges {
            let a = *axis as usize;
            let (o1, o2) = ((a + 1) % 3, (a + 2) % 3);
            by_line.entry((*axis, [p[o1], p[o2]])).or_default().push(p[a]);
        }
        let mut keys: Vec<_> = by_line.keys().copied().collect();
        keys.sort();
        let mut runs = 0;
        for key in keys {
            let mut s = by_line.remove(&key).unwrap_or_default();
            s.sort_unstable();
            s.dedup();
            let (axis, fixed) = key;
            let a = axis as usize;
            let (o1, o2) = ((a + 1) % 3, (a + 2) % 3);
            let mut k = 0;
            while k < s.len() {
                let mut e = k + 1;
                while e < s.len() && s[e] == s[e - 1] + 1 {
                    e += 1;
                }
                let mut lo = [0.0; 3];
                let mut hi = [0.0; 3];
                lo[a] = self.lines[a][s[k] as usize];
                hi[a] = self.lines[a][s[e - 1] as usize + 1];
                let (c1, c2) = (self.lines[o1][fixed[0] as usize], self.lines[o2][fixed[1] as usize]);
                lo[o1] = c1 - w;
                hi[o1] = c1 + w;
                lo[o2] = c2 - w;
                hi[o2] = c2 + w;
                boxes.push(GridBox { lo, hi, sides: [bridge; 6] });
                runs += 1;
                k = e;
            }
        }
        let mut corners: Vec<[u32; 3]> = d.corners.clone();
        corners.sort_unstable();
        corners.dedup();
        for p in &corners {
            let c = self.point(*p);
            boxes.push(GridBox {
                lo: [c.x - w, c.y - w, c.z - w],
                hi: [c.x + w, c.y + w, c.z + w],
                sides: [bridge; 6],
            });
        }
        (runs, corners.len())
    }
}

/// The octants around a grid point, bit `b` of a corner pattern: `(x, y,
/// z)` offsets of `b = x + 2y + 4z`.
const OCTANTS: [(usize, usize, usize); 8] =
    [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (1, 0, 1), (0, 1, 1), (1, 1, 1)];

/// Four cells around an edge, in the order (low, low), (high, low), (low,
/// high), (high, high) of the two axes across it: solid on one diagonal
/// only.
fn diagonal(a: bool, b: bool, c: bool, d: bool) -> bool {
    (a && d && !b && !c) || (b && c && !a && !d)
}

/// Two octants that share only the corner, one solid pair and six empty, or
/// one empty pair and six solid: octant `b` and `7 - b`.
fn corner_pinch(bits: u8) -> bool {
    let pair = |b: u8| (1u8 << b) | (1u8 << (7 - b));
    (0..4).any(|b| bits == pair(b) || bits == !pair(b))
}

/// The three grid indices of the point `(u, v)` on the plane `plane` across
/// `axis`.
fn grid_point(axis: usize, plane: u32, u: u32, v: u32) -> [u32; 3] {
    let mut p = [0u32; 3];
    p[axis] = plane;
    p[(axis + 1) % 3] = u;
    p[(axis + 2) % 3] = v;
    p
}

/// Unit boundary edges of one region, in the plane's `(u, v)` grid
/// coordinates: along `u` at row `v` from `u` to `u + 1`, or along `v` at
/// column `u` from `v` to `v + 1`; `dir` is the direction the boundary
/// runs with the region on its left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct UnitEdge {
    along_u: bool,
    /// The fixed coordinate (the row for an edge along `u`).
    line: u32,
    dir: i8,
    /// The varying coordinate where the unit segment starts.
    at: u32,
}

/// The boundary of the cells `cells` (sorted by `(v, u)`), counter-clockwise
/// round the region.
fn boundary(cells: &[(u32, u32)]) -> Vec<UnitEdge> {
    let has = |u: i64, v: i64| -> bool {
        u >= 0 && v >= 0 && cells.binary_search(&(v as u32, u as u32)).is_ok()
    };
    let mut out = Vec::new();
    for &(v, u) in cells {
        let (ui, vi) = (u as i64, v as i64);
        if !has(ui, vi - 1) {
            out.push(UnitEdge { along_u: true, line: v, dir: 1, at: u });
        }
        if !has(ui, vi + 1) {
            out.push(UnitEdge { along_u: true, line: v + 1, dir: -1, at: u });
        }
        if !has(ui - 1, vi) {
            out.push(UnitEdge { along_u: false, line: u, dir: -1, at: v });
        }
        if !has(ui + 1, vi) {
            out.push(UnitEdge { along_u: false, line: u + 1, dir: 1, at: v });
        }
    }
    out
}

/// One group of cell faces: a plane, a facing and a name.
struct Group {
    axis: usize,
    plane: u32,
    orient: i8,
    prov: u32,
    edges: Vec<UnitEdge>,
}

/// Merge the cell faces into regions, split every region's boundary at
/// every corner of every region, triangulate each region, and check that
/// every edge has two triangles wound against each other.
fn mesh(grid: &Grid, mut faces: Vec<FaceRec>) -> Result<(Vec<Vec3>, Vec<[u32; 3]>, Vec<u32>)> {
    faces.sort_unstable();
    let mut groups: Vec<Group> = Vec::new();
    let mut corners: HashMap<(u8, [u32; 2]), Vec<u32>> = HashMap::new();
    let mut start = 0;
    while start < faces.len() {
        let f = faces[start];
        let mut end = start + 1;
        while end < faces.len()
            && (faces[end].axis, faces[end].plane, faces[end].orient, faces[end].prov) == (f.axis, f.plane, f.orient, f.prov)
        {
            end += 1;
        }
        let cells: Vec<(u32, u32)> = faces[start..end].iter().map(|r| (r.v, r.u)).collect();
        let edges = boundary(&cells);
        // A corner is a point the boundary turns at: a unit edge along u
        // and one along v both end there.
        let mut mask: HashMap<(u32, u32), u8> = HashMap::new();
        for e in &edges {
            let (p, q) = if e.along_u { ((e.at, e.line), (e.at + 1, e.line)) } else { ((e.line, e.at), (e.line, e.at + 1)) };
            let bit = if e.along_u { 1 } else { 2 };
            *mask.entry(p).or_default() |= bit;
            *mask.entry(q).or_default() |= bit;
        }
        let axis = f.axis as usize;
        for ((u, v), m) in mask {
            if m == 3 {
                let p = grid_point(axis, f.plane, u, v);
                for a in 0..3 {
                    let (o1, o2) = ((a + 1) % 3, (a + 2) % 3);
                    corners.entry((a as u8, [p[o1], p[o2]])).or_default().push(p[a]);
                }
            }
        }
        groups.push(Group { axis, plane: f.plane, orient: f.orient, prov: f.prov, edges });
        start = end;
    }
    drop(faces);
    for v in corners.values_mut() {
        v.sort_unstable();
        v.dedup();
    }

    let mut vertex_of: HashMap<[u32; 3], u32> = HashMap::new();
    let mut positions: Vec<Vec3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut provenance: Vec<u32> = Vec::new();
    for g in &groups {
        let (ua, va) = ((g.axis + 1) % 3, (g.axis + 2) % 3);
        // Runs: consecutive unit edges along one line in one direction.
        let mut edges = g.edges.clone();
        edges.sort_unstable();
        let mut segments: Vec<((u32, u32), (u32, u32))> = Vec::new();
        let mut k = 0;
        while k < edges.len() {
            let e = edges[k];
            let mut end = k + 1;
            while end < edges.len() && {
                let n = edges[end];
                (n.along_u, n.line, n.dir) == (e.along_u, e.line, e.dir) && n.at == edges[end - 1].at + 1
            } {
                end += 1;
            }
            let (a0, a1) = (e.at, edges[end - 1].at + 1);
            // The corners of every region on this line, inside the run.
            let (vary, fixed) = if e.along_u {
                (ua, grid_point(g.axis, g.plane, 0, e.line))
            } else {
                (va, grid_point(g.axis, g.plane, e.line, 0))
            };
            let (o1, o2) = ((vary + 1) % 3, (vary + 2) % 3);
            let mut cuts = vec![a0];
            if let Some(list) = corners.get(&(vary as u8, [fixed[o1], fixed[o2]])) {
                let from = list.partition_point(|x| *x <= a0);
                cuts.extend(list[from..].iter().copied().take_while(|x| *x < a1));
            }
            cuts.push(a1);
            for w in cuts.windows(2) {
                let pt = |c: u32| if e.along_u { (c, e.line) } else { (e.line, c) };
                segments.push((pt(w[0]), pt(w[1])));
            }
            k = end;
        }
        // The region's points, in the plane's real coordinates.
        let mut pts: Vec<(u32, u32)> = segments.iter().flat_map(|(p, q)| [*p, *q]).collect();
        pts.sort_unstable();
        pts.dedup();
        let coord = |(u, v): (u32, u32)| [grid.lines[ua][u as usize], grid.lines[va][v as usize]];
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in &pts {
            let c = coord(*p);
            for d in 0..2 {
                lo[d] = lo[d].min(c[d]);
                hi[d] = hi[d].max(c[d]);
            }
        }
        let snap = Snap::over(lo, hi);
        let mut cdt = Cdt::new(pts.len());
        let mut id: HashMap<(u32, u32), u32> = HashMap::with_capacity(pts.len());
        for p in &pts {
            let (v, fresh) = cdt.insert(snap.snap(coord(*p)));
            if !fresh {
                return Err(CadpoleError::kernel(format!(
                    "join: two grid points of one face snapped together on the plane {} = {}; the boxes are too far \
                     apart for their smallest feature",
                    ["x", "y", "z"][g.axis],
                    grid.lines[g.axis][g.plane as usize]
                )));
            }
            id.insert(*p, v);
        }
        for (p, q) in &segments {
            cdt.constrain(id[p], id[q]).map_err(|e| {
                CadpoleError::kernel(format!(
                    "join: a face on the plane {} = {} could not be triangulated: {e}",
                    ["x", "y", "z"][g.axis],
                    grid.lines[g.axis][g.plane as usize]
                ))
            })?;
        }
        let inside = cdt.classify();
        let mut back: HashMap<u32, (u32, u32)> = HashMap::with_capacity(pts.len());
        for (p, v) in &id {
            back.insert(*v, *p);
        }
        for (t, tri) in cdt.triangles().enumerate() {
            if !inside[t] || tri.iter().any(|v| Cdt::is_super(*v)) {
                continue;
            }
            let mut out = [0u32; 3];
            for (c, v) in tri.iter().enumerate() {
                let (u, w) = back[v];
                let gp = grid_point(g.axis, g.plane, u, w);
                out[c] = *vertex_of.entry(gp).or_insert_with(|| {
                    positions.push(grid.point(gp));
                    (positions.len() - 1) as u32
                });
            }
            // Counter-clockwise in (u, v) faces +axis, since u x v = axis.
            if g.orient < 0 {
                out.swap(1, 2);
            }
            triangles.push(out);
            provenance.push(g.prov);
        }
    }

    // Closed and oriented: every edge twice, once each way.
    let mut uses: HashMap<(u32, u32), i32> = HashMap::with_capacity(triangles.len() * 2);
    for t in &triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let (key, s) = if a < b { ((a, b), 1) } else { ((b, a), -1) };
            let e = uses.entry(key).or_insert(0);
            *e += s * 1000 + 1;
        }
    }
    for ((a, b), n) in &uses {
        // Two uses wound against each other sum to 2 (one +1001, one -999).
        if *n != 2 {
            let (pa, pb) = (positions[*a as usize], positions[*b as usize]);
            return Err(CadpoleError::kernel(format!(
                "join: the result is not closed along the edge ({:.4}, {:.4}, {:.4})..({:.4}, {:.4}, {:.4}); this is a \
                 Cadpole bug -- the boxes' coordinates would reproduce it",
                pa.x, pa.y, pa.z, pb.x, pb.y, pb.z
            )));
        }
    }
    Ok((positions, triangles, provenance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solid;

    fn b(lo: [f64; 3], hi: [f64; 3], id: u32) -> GridBox {
        GridBox { lo, hi, sides: [6 * id, 6 * id + 1, 6 * id + 2, 6 * id + 3, 6 * id + 4, 6 * id + 5] }
    }

    fn as_mesh(g: &GridMesh) -> Mesh {
        let mut m = Mesh::default();
        for p in &g.positions {
            m.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
            m.normals.extend_from_slice(&[0.0, 0.0, 1.0]);
        }
        for t in &g.triangles {
            m.indices.extend_from_slice(t);
        }
        m.face_ids = g.provenance.clone();
        m
    }

    /// Closed, oriented, embedded, and of the volume `want`.
    fn assert_solid(g: &GridMesh, want: f64) {
        let m = as_mesh(g);
        let r = solid::validate_with(&m, &solid::SolidOptions { pair_limit: usize::MAX, ..Default::default() });
        assert!(r.is_solid(), "{:?}", r.problems());
        assert!((r.volume - want).abs() <= 1e-6 * want.max(1.0), "volume {} want {want}", r.volume);
    }

    #[test]
    fn one_box_is_twelve_triangles_named_by_its_sides() {
        let g = union_boxes(&[b([0.0; 3], [10.0, 20.0, 30.0], 0)], 0.01, 99).unwrap();
        assert_eq!(g.triangles.len(), 12);
        assert_eq!(g.positions.len(), 8);
        assert_solid(&g, 6000.0);
        let mut sides: Vec<u32> = g.provenance.clone();
        sides.sort_unstable();
        sides.dedup();
        assert_eq!(sides, [0, 1, 2, 3, 4, 5]);
        assert_eq!(g.report.rounds, 1);
    }

    #[test]
    fn two_boxes_sharing_a_face_are_one_box_with_a_seam_by_name() {
        let g = union_boxes(&[b([0.0; 3], [10.0, 10.0, 10.0], 0), b([10.0, 0.0, 0.0], [20.0, 10.0, 10.0], 1)], 0.01, 99).unwrap();
        assert_solid(&g, 2000.0);
        // The shared face is inside; the four long sides are two faces each
        // (a seam where the names change), no T-junction anywhere.
        assert!(!g.provenance.contains(&1) && !g.provenance.contains(&6), "{:?}", g.provenance);
        assert_eq!(g.report.bridged_edges + g.report.bridged_corners, 0);
    }

    /// Two boxes touching along a vertical edge on a
    /// slab under both. The mesh kernel refuses it; here it is bridged.
    #[test]
    fn an_edge_contact_is_bridged_into_one_solid() {
        let boxes = [
            b([0.0, 0.0, 0.0], [10.0, 10.0, 10.0], 0),
            b([10.0, 10.0, 0.0], [20.0, 20.0, 10.0], 1),
            b([-5.0, -5.0, -1.0], [25.0, 25.0, 0.0], 2),
        ];
        let g = union_boxes(&boxes, 0.1, 99).unwrap();
        assert_eq!(g.report.bridged_edges, 1, "{:?}", g.report);
        // Two squares 0.05 x 0.05 of bridge along the 10-tall edge.
        assert_solid(&g, 2000.0 + 900.0 + 2.0 * 0.05 * 0.05 * 10.0);
        assert!(g.provenance.contains(&99));
        // Without the slab the two boxes are bridged just the same.
        let g = union_boxes(&boxes[..2], 0.1, 99).unwrap();
        assert_solid(&g, 2000.0 + 2.0 * 0.05 * 0.05 * 10.0);
    }

    #[test]
    fn a_corner_contact_is_bridged_by_a_cube() {
        let g = union_boxes(&[b([0.0; 3], [10.0; 3], 0), b([10.0; 3], [20.0; 3], 1)], 0.2, 99).unwrap();
        assert_eq!(g.report.bridged_corners, 1, "{:?}", g.report);
        // The cube 0.2 across at the corner adds its six octants not
        // already inside a box: 6 * 0.1^3.
        assert_solid(&g, 2000.0 + 6.0 * 0.001);
    }

    #[test]
    fn a_near_miss_is_welded_and_a_sliver_is_dropped() {
        // 0.004 apart: one face once welded at 0.01. A 0.003-thick pane is
        // a sliver.
        let boxes = [
            b([0.0; 3], [10.0, 10.0, 10.0], 0),
            b([10.004, 0.0, 0.0], [20.0, 10.0, 10.0], 1),
            b([3.0, 3.0, 10.0], [3.003, 6.0, 12.0], 2),
        ];
        let g = union_boxes(&boxes, 0.01, 99).unwrap();
        assert_eq!(g.report.slivers, 1);
        assert_eq!(g.report.first_sliver, Some(2));
        assert!(g.report.snapped >= 1);
        // The weld puts the shared plane at the value most faces use: 10
        // (the first box's +x, which ties with 10.004 -- the smaller wins).
        assert_solid(&g, 2000.0);
        assert_eq!(g.triangles.len(), 12 + 8, "one box with a seam on four sides: {}", g.triangles.len());
    }

    #[test]
    fn a_wall_split_at_openings_with_a_window_head_has_no_t_junction() {
        // A wall in three pieces -- two piers, a sill, a head -- on a slab;
        // every piece meets the next on a face, and the head's ends land in
        // the middle of the piers' faces.
        let boxes = [
            b([-2.0, -2.0, -1.0], [22.0, 3.0, 0.0], 0),
            b([0.0, 0.0, 0.0], [5.0, 1.0, 10.0], 1),
            b([15.0, 0.0, 0.0], [20.0, 1.0, 10.0], 2),
            b([5.0, 0.0, 0.0], [15.0, 1.0, 3.0], 3),
            b([5.0, 0.0, 8.0], [15.0, 1.0, 10.0], 4),
        ];
        let g = union_boxes(&boxes, 0.01, 99).unwrap();
        assert_solid(&g, 24.0 * 5.0 + 2.0 * 50.0 + 30.0 + 20.0);
    }

    #[test]
    fn a_closed_cavity_is_a_second_shell() {
        // A hollow box from six slabs: the cavity's walls face inward.
        let boxes = [
            b([0.0, 0.0, 0.0], [10.0, 10.0, 1.0], 0),
            b([0.0, 0.0, 9.0], [10.0, 10.0, 10.0], 1),
            b([0.0, 0.0, 1.0], [1.0, 10.0, 9.0], 2),
            b([9.0, 0.0, 1.0], [10.0, 10.0, 9.0], 3),
            b([1.0, 0.0, 1.0], [9.0, 1.0, 9.0], 4),
            b([1.0, 9.0, 1.0], [9.0, 10.0, 9.0], 5),
        ];
        let g = union_boxes(&boxes, 0.01, 99).unwrap();
        assert_solid(&g, 1000.0 - 512.0);
        let r = solid::validate(&as_mesh(&g));
        assert_eq!(r.components, 2);
    }

    fn volume_of(boxes: &[([f64; 3], [f64; 3])]) -> f64 {
        boxes.iter().map(|(lo, hi)| (hi[0] - lo[0]) * (hi[1] - lo[1]) * (hi[2] - lo[2])).sum()
    }

    fn as_grid_boxes(boxes: &[([f64; 3], [f64; 3])], id: u32) -> Vec<GridBox> {
        boxes.iter().map(|(lo, hi)| b(*lo, *hi, id)).collect()
    }

    /// A welded union is an orthogonal polyhedron, and it comes back
    /// apart into boxes that fill exactly its volume -- an L, a step, a
    /// hollow -- so two such solids that share faces join on the grid.
    #[test]
    fn an_orthogonal_solid_comes_apart_into_the_boxes_it_fills() {
        // An L of two boxes and a step on top of it.
        let parts = [
            b([0.0, 0.0, 0.0], [10.0, 2.0, 5.0], 0),
            b([0.0, 0.0, 0.0], [2.0, 8.0, 5.0], 1),
            b([0.0, 0.0, 5.0], [4.0, 4.0, 6.5], 2),
        ];
        let g = union_boxes(&parts, 0.01, 99).unwrap();
        let want = 100.0 + 80.0 - 20.0 + 24.0;
        assert_solid(&g, want);
        let boxes = orthogonal_boxes(&as_mesh(&g), 1e-9).expect("an orthogonal solid");
        assert!((volume_of(&boxes) - want).abs() < 1e-9, "{boxes:?}");
        // The boxes do not overlap: each cell once.
        let again = union_boxes(&as_grid_boxes(&boxes, 0), 0.01, 99).unwrap();
        assert_solid(&again, want);
        // A hollow box: the cavity is not filled.
        let hollow = [
            b([0.0, 0.0, 0.0], [10.0, 10.0, 1.0], 0),
            b([0.0, 0.0, 9.0], [10.0, 10.0, 10.0], 1),
            b([0.0, 0.0, 1.0], [1.0, 10.0, 9.0], 2),
            b([9.0, 0.0, 1.0], [10.0, 10.0, 9.0], 3),
            b([1.0, 0.0, 1.0], [9.0, 1.0, 9.0], 4),
            b([1.0, 9.0, 1.0], [9.0, 10.0, 9.0], 5),
        ];
        let g = union_boxes(&hollow, 0.01, 99).unwrap();
        let boxes = orthogonal_boxes(&as_mesh(&g), 1e-9).unwrap();
        assert!((volume_of(&boxes) - 488.0).abs() < 1e-9);
    }

    /// Two parts of one piece -- a slab and
    /// the walls standing on it, a frame overlapping a wall -- each a solid
    /// of its own, sharing faces. Apart into boxes and joined on the grid
    /// they are one solid of the union's volume.
    #[test]
    fn parts_that_share_faces_join_exactly_through_their_boxes() {
        let slab = union_boxes(&[b([0.0, 0.0, 0.0], [20.0, 12.0, 2.4], 0)], 0.01, 99).unwrap();
        let walls = union_boxes(
            &[
                b([0.0, 0.0, 2.4], [20.0, 1.6, 30.0], 0),
                b([0.0, 0.0, 2.4], [1.6, 12.0, 30.0], 1),
                // A partition that meets the slab's corner along an edge.
                b([20.0, 12.0, 0.0], [22.0, 14.0, 30.0], 2),
            ],
            0.01,
            99,
        )
        .unwrap();
        let frame = union_boxes(&[b([5.0, -0.3, 20.0], [12.0, 1.6, 22.0], 0)], 0.01, 99).unwrap();
        let mut all = Vec::new();
        for g in [&slab, &walls, &frame] {
            all.extend(as_grid_boxes(&orthogonal_boxes(&as_mesh(g), 1e-9).unwrap(), 0));
        }
        let fused = union_boxes(&all, 0.01, 99).unwrap();
        let walls_v = 20.0 * 1.6 * 27.6 + 1.6 * 10.4 * 27.6 + 2.0 * 2.0 * 30.0;
        let want = 20.0 * 12.0 * 2.4 + walls_v + 7.0 * 0.3 * 2.0;
        assert_eq!(fused.report.bridged_edges, 1, "the partition's edge contact is bridged: {:?}", fused.report);
        let m = as_mesh(&fused);
        let r = solid::validate_with(&m, &solid::SolidOptions { pair_limit: usize::MAX, ..Default::default() });
        assert!(r.is_solid(), "{:?}", r.problems());
        assert!((r.volume - want).abs() < 0.01, "volume {} want {want}", r.volume);
    }

    #[test]
    fn a_mesh_off_the_axis_planes_is_not_orthogonal() {
        // A box turned 30 degrees about z.
        let g = union_boxes(&[b([0.0; 3], [10.0, 5.0, 3.0], 0)], 0.01, 99).unwrap();
        let mut m = as_mesh(&g);
        let (c, s) = (crate::fmath::cos(30f64.to_radians()), crate::fmath::sin(30f64.to_radians()));
        for p in m.positions.chunks_exact_mut(3) {
            let (x, y) = (p[0] as f64, p[1] as f64);
            p[0] = (c * x - s * y) as f32;
            p[1] = (s * x + c * y) as f32;
        }
        assert!(orthogonal_boxes(&m, 1e-9).is_none());
        // Wound inward: the columns close before they open.
        let mut inward = as_mesh(&g);
        for t in inward.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
        assert!(orthogonal_boxes(&inward, 1e-9).is_none());
    }

    /// Random soups, joined, taken apart and joined again: the same solid.
    #[test]
    fn a_joined_soup_comes_apart_and_back_to_the_same_volume() {
        let mut seed = 0x2545f4914f6cdd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for case in 0..20 {
            let mut boxes = Vec::new();
            for k in 0..(10 + case * 5) {
                let mut lo = [0.0; 3];
                let mut hi = [0.0; 3];
                for a in 0..3 {
                    let s = (next() % 10) as f64;
                    lo[a] = s;
                    hi[a] = s + 1.0 + (next() % 4) as f64;
                }
                boxes.push(b(lo, hi, k as u32));
            }
            let g = union_boxes(&boxes, 0.01, 9999).unwrap();
            let m = as_mesh(&g);
            let v = solid::validate(&m).volume;
            let parts = orthogonal_boxes(&m, 1e-9).unwrap_or_else(|| panic!("case {case}: not taken apart"));
            assert!((volume_of(&parts) - v).abs() < 1e-6 * v.max(1.0), "case {case}: {} vs {v}", volume_of(&parts));
            // A weld finer than the bridges' half-width keeps them as they are.
            let again = union_boxes(&as_grid_boxes(&parts, 0), 1e-4, 9999).unwrap();
            assert_solid(&again, v);
        }
    }

    /// A non-box operand's vertices within the weld of a grid line go
    /// onto it, as the `f32` the grid's own mesh carries; the rest stay.
    #[test]
    fn snapping_moves_only_what_the_weld_reaches() {
        let lines = [vec![0.0, 10.0], vec![0.0, 1.6], vec![0.0, 20.0, 30.0]];
        let mut m = Mesh::default();
        // Three vertices: one 10 nm off x = 10, one 0.004 off y = 1.6, one
        // 0.3 from every line (left alone), one exactly on z = 20.
        m.positions = vec![10.00001, 0.7, 19.6425, 3.0, 1.604, 20.0, 9.7, 0.8, 25.0];
        let (s, r) = snap_to_lines(&m, &lines, 0.01).expect("something moved");
        assert_eq!(r.moved, 2);
        assert!((r.furthest - 0.004).abs() < 1e-6, "{r:?}");
        assert_eq!(s.positions[0], 10.0f32);
        assert_eq!(s.positions[4], 1.6f64 as f32);
        assert_eq!(&s.positions[6..9], &[9.7, 0.8, 25.0]);
        assert_eq!(s.positions[2], 19.6425, "0.36 from z = 20: beyond the weld");
        assert!(snap_to_lines(&s, &lines, 0.01).is_none(), "nothing left to move");
        let miss = nearest_miss(&m, &lines, 0.01, 0.5).unwrap();
        assert_eq!(miss.0, 0, "x = 9.7 is 0.3 from x = 10: {miss:?}");
        assert!((miss.2 - 0.3).abs() < 1e-6);
    }

    /// Hundreds of boxes in random contact -- touching, overlapping, along
    /// edges, at corners, a hair apart -- always come out a solid.
    #[test]
    fn random_box_soups_always_come_out_solid() {
        let mut seed = 0x9e3779b97f4a7c15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for case in 0..40 {
            let n = 20 + (case % 5) * 30;
            let mut boxes = Vec::new();
            for k in 0..n {
                // Coordinates on a coarse lattice, so faces, edges and
                // corners coincide often, plus a jitter below the weld.
                let mut lo = [0.0; 3];
                let mut hi = [0.0; 3];
                for a in 0..3 {
                    let s = (next() % 12) as f64;
                    let l = 1.0 + (next() % 4) as f64;
                    let j = ((next() % 100) as f64 - 50.0) * 1e-5;
                    lo[a] = s + j;
                    hi[a] = s + l;
                }
                boxes.push(b(lo, hi, k as u32));
            }
            let g = union_boxes(&boxes, 0.01, 9999).unwrap_or_else(|e| panic!("case {case}: {e}"));
            let m = as_mesh(&g);
            let r = solid::validate_with(&m, &solid::SolidOptions { pair_limit: usize::MAX, ..Default::default() });
            assert!(r.is_solid(), "case {case}: {:?} ({:?})", r.problems(), g.report);
        }
    }
}
