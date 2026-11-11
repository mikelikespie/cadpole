//! Views of a solid, as drawing IR: orthographic projection with hidden-line
//! removal, and sections by a plane.
//!
//! ## How hidden lines are computed
//!
//! Three steps, and each one is chosen to keep the answer honest on curved
//! surfaces rather than only on boxes.
//!
//! 1. **Which edges could be drawn.** Two sources, and they do not overlap.
//!    *Creases* come from the topology: an edge whose `dihedral_degrees` is at
//!    or above the crease threshold is a real corner of the solid, it is
//!    already a named entity, and it is already a polyline. *Silhouettes* come
//!    from the mesh: a mesh edge whose two triangles face opposite ways
//!    relative to the eye is on the outline even though the surface is smooth
//!    there — that is the seam of a cylinder as you turn it, and no topology
//!    edge exists for it. Silhouette candidates are taken only where the two
//!    triangles are *not* a crease, so a box contributes its twelve topology
//!    edges and nothing else.
//! 2. **Where they are.** Orthographic projection through a [`ViewBasis`]:
//!    screen x and y from the right/up vectors, plus a depth along the vector
//!    toward the eye. No perspective — a drawing must be measurable.
//! 3. **Which parts are in front.** Each candidate polyline is cut into short
//!    steps, and each step's midpoint is tested against every triangle whose
//!    screen bounding box could contain it (a uniform screen grid keeps that
//!    to a handful). A triangle covering the point with greater depth hides it.
//!    Runs of the same answer are merged back into polylines, and the boundary
//!    between a visible and a hidden run is refined by bisection so an edge
//!    that disappears behind a boss breaks where the boss actually is.
//!
//! Depth comparisons use a tolerance scaled to the model, because the edge
//! being tested lies exactly on the faces that meet there; without it every
//! edge would hide itself.
//!
//! ## Sections
//!
//! [`section_loops`] slices the triangles, chains the resulting segments into
//! closed loops and hands them back in the cut plane's own 2D coordinates.
//! Nesting is then the ordinary sketch rule (outer loop, holes), so a cut
//! through a tube gives an annulus whose area is the material area — which is
//! exactly what a section is for.

use std::collections::HashMap;

use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::sketch::{Loop, Path, Segment, Sketch, P2};
use cadpole_core::tessellate::{Mesh, Topology};

use crate::ir::{layer, Dash, Drawing, HatchItem, Item, Pattern, Stroke, Style};

/// What to do with edges the solid hides from this eye.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hidden {
    /// Dashed, on the hidden layer. The drafting default.
    #[default]
    Dashed,
    /// Left out entirely: a cleaner picture, a poorer drawing.
    Omit,
    /// Drawn like the visible ones, which is a wireframe.
    Solid,
}

impl Hidden {
    pub fn parse(s: &str) -> Option<Hidden> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "dashed" | "dash" | "on" => Hidden::Dashed,
            "omit" | "off" | "none" | "hide" => Hidden::Omit,
            "solid" | "wireframe" => Hidden::Solid,
            _ => return None,
        })
    }
}

/// An orthographic frame: where right, up and the eye are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewBasis {
    pub right: Vec3,
    pub up: Vec3,
    /// From the model toward the eye. Greater depth is nearer the viewer.
    pub toward_eye: Vec3,
}

impl ViewBasis {
    /// The named views of docs/views.md. Unknown names give the isometric,
    /// which is the useful default for a single picture.
    pub fn preset(name: &str) -> ViewBasis {
        // Matches `cadpole_core::render::Camera::preset`: z is up, "front" looks
        // along +y from -y.
        let (eye, up) = match name.trim().to_ascii_lowercase().as_str() {
            "front" => (Vec3::new(0.0, -1.0, 0.0), Vec3::Z),
            "back" => (Vec3::new(0.0, 1.0, 0.0), Vec3::Z),
            "top" | "plan" => (Vec3::new(0.0, 0.0, 1.0), Vec3::Y),
            "bottom" => (Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, -1.0, 0.0)),
            "left" => (Vec3::new(-1.0, 0.0, 0.0), Vec3::Z),
            "right" => (Vec3::new(1.0, 0.0, 0.0), Vec3::Z),
            "iso-back" => (Vec3::new(-1.0, 1.0, 0.8).normalized(), Vec3::Z),
            _ => (Vec3::new(1.0, -1.0, 0.8).normalized(), Vec3::Z),
        };
        ViewBasis::looking_from(eye, up)
    }

    /// A basis looking from `eye_dir` (a direction, not a point) with `up`
    /// pointing up on the page.
    pub fn looking_from(eye_dir: Vec3, up: Vec3) -> ViewBasis {
        let f = eye_dir.normalized();
        let mut right = up.cross(f);
        if right.length() < 1e-9 {
            right = Vec3::X.cross(f);
        }
        if right.length() < 1e-9 {
            right = Vec3::Y.cross(f);
        }
        let right = right.normalized();
        let up = f.cross(right).normalized();
        ViewBasis {
            right,
            up,
            toward_eye: f,
        }
    }

    /// Screen coordinates and depth. Depth increases toward the eye.
    pub fn project(&self, p: Vec3) -> (P2, f64) {
        (
            P2::new(p.dot(self.right), p.dot(self.up)),
            p.dot(self.toward_eye),
        )
    }
}

/// How much work the hidden-line pass does.
#[derive(Clone, Debug)]
pub struct ProjectOptions {
    /// An edge at or above this dihedral angle is drawn as a corner of the
    /// solid. 15° keeps a cylinder's tangent seams out of the picture.
    pub crease_degrees: f64,
    pub hidden: Hidden,
    /// Longest step the visibility test takes along an edge, as a fraction of
    /// the model's diagonal. Smaller finds shorter hidden stretches.
    pub step_fraction: f64,
    /// Bisection rounds at each visible/hidden boundary.
    pub refine: usize,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        ProjectOptions {
            crease_degrees: 15.0,
            hidden: Hidden::Dashed,
            step_fraction: 0.01,
            refine: 8,
        }
    }
}

/// The result of projecting a solid: polylines in screen coordinates.
#[derive(Clone, Debug, Default)]
pub struct Projection {
    pub visible: Vec<Vec<P2>>,
    pub hidden: Vec<Vec<P2>>,
    /// Which body each polyline of `visible` came from, as an index into
    /// the [`BodyRanges`] the projection was given; all zero without one.
    pub visible_body: Vec<usize>,
    /// The same for `hidden`.
    pub hidden_body: Vec<usize>,
}

/// Where each body's share of a combined mesh starts, so a plan can
/// put the walls, the site and the neighbours on layers of their own while
/// hidden-line removal still runs across all of them. `vertices[i]` and
/// `edges[i]` are the first vertex and the first topology edge of body `i`;
/// both ascend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyRanges {
    pub vertices: Vec<usize>,
    pub edges: Vec<usize>,
}

impl BodyRanges {
    fn body_of(starts: &[usize], index: usize) -> usize {
        match starts.binary_search(&index) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        }
    }
    pub fn body_of_vertex(&self, v: usize) -> usize {
        Self::body_of(&self.vertices, v)
    }
    pub fn body_of_edge(&self, e: usize) -> usize {
        Self::body_of(&self.edges, e)
    }
}

/// A half-space `(p - origin) · normal >= 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HalfSpace {
    pub origin: Vec3,
    pub normal: Vec3,
}

impl HalfSpace {
    pub fn new(origin: Vec3, normal: Vec3) -> HalfSpace {
        HalfSpace {
            origin,
            normal: normal.normalized(),
        }
    }
    /// The same boundary, the other side kept.
    pub fn flipped(&self) -> HalfSpace {
        HalfSpace {
            origin: self.origin,
            normal: -self.normal,
        }
    }
    fn signed(&self, p: Vec3) -> f64 {
        (p - self.origin).dot(self.normal)
    }
}

/// The part of `mesh` inside `hs`: whole triangles kept, crossing ones cut
/// along the boundary, the rest dropped. Normals are interpolated so the
/// result is a mesh like any other. What a plan projects is the model below
/// the cut and what a section projects is the model beyond it, and both are
/// this.
pub fn clip_mesh(mesh: &Mesh, hs: &HalfSpace) -> Mesh {
    let has_normals = mesh.normals.len() == mesh.positions.len();
    let pos = |i: u32| {
        let j = i as usize * 3;
        Vec3::new(
            mesh.positions[j] as f64,
            mesh.positions[j + 1] as f64,
            mesh.positions[j + 2] as f64,
        )
    };
    let nrm = |i: u32| {
        let j = i as usize * 3;
        if has_normals {
            Vec3::new(
                mesh.normals[j] as f64,
                mesh.normals[j + 1] as f64,
                mesh.normals[j + 2] as f64,
            )
        } else {
            Vec3::Z
        }
    };
    let mut out = Mesh::default();
    let mut push = |p: Vec3, n: Vec3| -> u32 {
        out.positions
            .extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
        out.normals
            .extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
        (out.positions.len() / 3 - 1) as u32
    };
    // A vertex on the boundary counts as inside, so a body resting on the
    // cut plane keeps its face there rather than losing it to rounding.
    let eps = 1e-9;
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut faces: Vec<u32> = Vec::new();
    for (ti, t) in mesh.indices.chunks_exact(3).enumerate() {
        let p = [pos(t[0]), pos(t[1]), pos(t[2])];
        let n = [nrm(t[0]), nrm(t[1]), nrm(t[2])];
        let d = [hs.signed(p[0]), hs.signed(p[1]), hs.signed(p[2])];
        let inside: Vec<usize> = (0..3).filter(|k| d[*k] >= -eps).collect();
        let face = mesh.face_ids.get(ti).copied().unwrap_or(0);
        match inside.len() {
            0 => {}
            3 => {
                let ids = [push(p[0], n[0]), push(p[1], n[1]), push(p[2], n[2])];
                tris.push(ids);
                faces.push(face);
            }
            1 => {
                // One corner kept: a smaller triangle with two new points on
                // the boundary.
                let a = inside[0];
                let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                let (pb, nb) = cut(p[a], p[b], n[a], n[b], d[a], d[b]);
                let (pc, nc) = cut(p[a], p[c], n[a], n[c], d[a], d[c]);
                let ids = [push(p[a], n[a]), push(pb, nb), push(pc, nc)];
                tris.push(ids);
                faces.push(face);
            }
            _ => {
                // Two corners kept: a quad, as two triangles that keep the
                // winding.
                let out_k = (0..3).find(|k| d[*k] < -eps).unwrap_or(0);
                let (a, b) = ((out_k + 1) % 3, (out_k + 2) % 3);
                let (pab, nab) = cut(p[a], p[out_k], n[a], n[out_k], d[a], d[out_k]);
                let (pbo, nbo) = cut(p[b], p[out_k], n[b], n[out_k], d[b], d[out_k]);
                let ia = push(p[a], n[a]);
                let ib = push(p[b], n[b]);
                let ibo = push(pbo, nbo);
                let iab = push(pab, nab);
                tris.push([ia, ib, ibo]);
                tris.push([ia, ibo, iab]);
                faces.push(face);
                faces.push(face);
            }
        }
    }
    for t in tris {
        out.indices.extend_from_slice(&t);
    }
    out.face_ids = faces;
    out
}

/// The point where the segment `a`–`b` crosses the boundary, with its
/// normal, given the signed distances of its ends.
fn cut(a: Vec3, b: Vec3, na: Vec3, nb: Vec3, da: f64, db: f64) -> (Vec3, Vec3) {
    let t = if (da - db).abs() < 1e-18 {
        0.5
    } else {
        (da / (da - db)).clamp(0.0, 1.0)
    };
    (a + (b - a) * t, (na + (nb - na) * t).normalized())
}

/// The edges of `topo` cut to `hs`: each polyline split where it leaves
/// the half-space, every kept piece an edge with the source's dihedral, so
/// [`project`] draws the creases of a cut body exactly as far as its mesh
/// goes. Faces and vertices are passed through, because the projection
/// reads neither.
pub fn clip_topology(topo: &Topology, hs: &HalfSpace) -> Topology {
    let mut out = Topology {
        faces: topo.faces.clone(),
        edges: Vec::new(),
        vertices: topo.vertices.clone(),
    };
    for e in &topo.edges {
        let poly: Vec<Vec3> = if e.polyline.len() >= 2 {
            e.polyline.clone()
        } else {
            vec![e.start, e.end]
        };
        let mut piece: Vec<Vec3> = Vec::new();
        let flush = |piece: &mut Vec<Vec3>, out: &mut Topology| {
            if piece.len() >= 2 {
                let mut ne = e.clone();
                ne.start = piece[0];
                ne.end = *piece.last().unwrap();
                ne.polyline = std::mem::take(piece);
                ne.length = ne
                    .polyline
                    .windows(2)
                    .map(|w| (w[1] - w[0]).length())
                    .sum();
                out.edges.push(ne);
            }
            piece.clear();
        };
        for w in poly.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (da, db) = (hs.signed(a), hs.signed(b));
            let eps = 1e-9;
            match (da >= -eps, db >= -eps) {
                (true, true) => {
                    if piece.is_empty() {
                        piece.push(a);
                    }
                    piece.push(b);
                }
                (true, false) => {
                    if piece.is_empty() {
                        piece.push(a);
                    }
                    piece.push(cut(a, b, Vec3::Z, Vec3::Z, da, db).0);
                    flush(&mut piece, &mut out);
                }
                (false, true) => {
                    flush(&mut piece, &mut out);
                    piece.push(cut(a, b, Vec3::Z, Vec3::Z, da, db).0);
                    piece.push(b);
                }
                (false, false) => flush(&mut piece, &mut out),
            }
        }
        flush(&mut piece, &mut out);
    }
    out
}

/// Append `mesh` and `topo` to a combined pair, recording where they start
/// in `ranges`. The inverse of drawing bodies one at a time: hidden-line
/// removal has to see them together.
pub fn append_body(
    mesh: &mut Mesh,
    topo: &mut Topology,
    ranges: &mut BodyRanges,
    m: &Mesh,
    t: &Topology,
) {
    ranges.vertices.push(mesh.vertex_count());
    ranges.edges.push(topo.edges.len());
    let base = mesh.vertex_count() as u32;
    let face_base = topo.faces.len() as u32;
    mesh.positions.extend_from_slice(&m.positions);
    if m.normals.len() == m.positions.len() {
        mesh.normals.extend_from_slice(&m.normals);
    } else {
        mesh.normals.extend(std::iter::repeat_n(0.0f32, m.positions.len()));
    }
    mesh.indices.extend(m.indices.iter().map(|i| i + base));
    mesh.face_ids.extend(m.face_ids.iter().map(|f| f + face_base));
    topo.faces.extend(t.faces.iter().cloned());
    topo.edges.extend(t.edges.iter().cloned());
    topo.vertices.extend(t.vertices.iter().cloned());
}

impl Projection {
    pub fn bounds(&self) -> Option<(P2, P2)> {
        let mut lo = P2::new(f64::MAX, f64::MAX);
        let mut hi = P2::new(f64::MIN, f64::MIN);
        let mut any = false;
        for l in self.visible.iter().chain(&self.hidden) {
            for p in l {
                lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
                any = true;
            }
        }
        if any {
            Some((lo, hi))
        } else {
            None
        }
    }
}

/// Project `mesh` through `basis`, splitting every candidate edge into its
/// visible and hidden runs.
pub fn project(
    mesh: &Mesh,
    topo: &Topology,
    basis: &ViewBasis,
    opts: &ProjectOptions,
) -> Projection {
    project_tagged(mesh, topo, basis, opts, &BodyRanges::default())
}

/// [`project`], with every polyline tagged by the body it came from.
pub fn project_tagged(
    mesh: &Mesh,
    topo: &Topology,
    basis: &ViewBasis,
    opts: &ProjectOptions,
    ranges: &BodyRanges,
) -> Projection {
    project_occluded(mesh, topo, basis, opts, ranges, &Mesh::default())
}

/// [`project_tagged`], with `occluders` hiding lines as the mesh does but
/// contributing no lines of their own: a section's cut face, which is solid
/// material on the drawing and has no edges but the cut outline.
pub fn project_occluded(
    mesh: &Mesh,
    topo: &Topology,
    basis: &ViewBasis,
    opts: &ProjectOptions,
    ranges: &BodyRanges,
    occluders: &Mesh,
) -> Projection {
    let mut out = Projection::default();
    if mesh.indices.is_empty() {
        return out;
    }
    let diag = mesh
        .bbox()
        .map(|(lo, hi)| (hi - lo).length())
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(1.0);

    let tris = if occluders.indices.is_empty() {
        ScreenTris::build(mesh, basis, diag)
    } else {
        let mut both = mesh.clone();
        let base = both.vertex_count() as u32;
        both.positions.extend_from_slice(&occluders.positions);
        both.normals.extend(std::iter::repeat_n(0.0f32, occluders.positions.len()));
        both.indices.extend(occluders.indices.iter().map(|i| i + base));
        both.face_ids.extend(std::iter::repeat_n(u32::MAX, occluders.triangle_count()));
        ScreenTris::build(&both, basis, diag)
    };
    let candidates = candidate_edges(mesh, topo, basis, opts.crease_degrees, ranges);
    let step = (diag * opts.step_fraction).max(diag * 1e-4);
    let depth_eps = diag * 1e-4;
    // The visibility walk chops every edge into short steps; a straight edge
    // comes back out as a hundred collinear points. Collapsing them is not an
    // optimisation — a drawing with a hundred vertices per line is a drawing
    // nobody can edit — and at this tolerance it is lossless: a genuinely
    // curved silhouette keeps every point it needs.
    let simplify_eps = diag * 1e-7;

    // (polyline, visible, body): sorted at the end, body and all, so the
    // written file stays a function of the model.
    let mut runs: Vec<(Vec<P2>, bool, usize)> = Vec::new();
    for (poly, body) in candidates {
        let mut run: Vec<P2> = Vec::new();
        let mut run_visible = true;
        let mut started = false;
        for w in poly.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = (b - a).length();
            let n = ((len / step).ceil() as usize).clamp(1, 512);
            // The parameter of the last sample tested on this segment: the
            // visibility changed somewhere between it and the next one, which
            // may be in the first half of this step or the second half of the
            // previous one.
            let mut last_mid: Option<f64> = None;
            for i in 0..n {
                let t0 = i as f64 / n as f64;
                let t1 = (i + 1) as f64 / n as f64;
                let tm = (t0 + t1) / 2.0;
                let p0 = lerp(a, b, t0);
                let p1 = lerp(a, b, t1);
                let vis = tris.visible(basis, lerp(a, b, tm), depth_eps);
                if !started {
                    run.push(basis.project(p0).0);
                    run_visible = vis;
                    started = true;
                } else if vis != run_visible {
                    // Find where it actually changes rather than snapping to
                    // the sample grid: between the two samples that disagree,
                    // not just within this step, or a boundary in the second
                    // half of the previous step lands at this step's start.
                    let ta = last_mid.unwrap_or(t0);
                    let cut = bisect(
                        &tris,
                        basis,
                        lerp(a, b, ta),
                        lerp(a, b, tm),
                        run_visible,
                        depth_eps,
                        opts.refine,
                    );
                    let ab = b - a;
                    let tc = (cut - a).dot(ab) / ab.dot(ab);
                    if tc < t0 && run.len() > 1 {
                        // The run already reaches p0, past the change: back
                        // it up to the cut.
                        run.pop();
                    }
                    run.push(basis.project(cut).0);
                    emit(
                        &mut runs,
                        std::mem::take(&mut run),
                        run_visible,
                        body,
                        opts.hidden,
                        simplify_eps,
                    );
                    run.push(basis.project(cut).0);
                    run_visible = vis;
                }
                last_mid = Some(tm);
                run.push(basis.project(p1).0);
            }
        }
        if started {
            emit(&mut runs, run, run_visible, body, opts.hidden, simplify_eps);
        }
    }
    // Candidate edges are already canonical, but the visible/hidden split can
    // interleave them; sorting makes the written file a function of the model.
    runs.sort_by(|a, b| {
        cmp_points(&a.0, &b.0, |p| (p.x, p.y, 0.0)).then(a.2.cmp(&b.2))
    });
    // Each 3D edge was classified on its own, but lines are drawn in 2D, where
    // edges coincide: a box's back edges project exactly onto its front ones,
    // and two corners of a post behind a wall project onto one line. Ink is
    // laid once: a stretch already drawn visible is not drawn again, nor
    // drawn hidden on top of itself, and a hidden stretch already drawn hidden
    // is not drawn twice (two dash patterns out of phase read as a solid
    // line). Visible lines go first so they take precedence; the sort above
    // makes which duplicate survives a function of the geometry.
    let cover_tol = diag * 1e-5;
    let mut drawn = Coverage::new(diag / 256.0, cover_tol);
    for pass_visible in [true, false] {
        for (pts, visible, body) in &runs {
            if *visible != pass_visible {
                continue;
            }
            let pieces = drawn.uncovered(pts);
            drawn.add(pts);
            for piece in pieces {
                let piece = simplify(&dedupe(piece), simplify_eps);
                if piece.len() < 2 || polyline_length(&piece) <= cover_tol * 10.0 {
                    continue;
                }
                if *visible {
                    out.visible.push(piece);
                    out.visible_body.push(*body);
                } else {
                    out.hidden.push(piece);
                    out.hidden_body.push(*body);
                }
            }
        }
    }
    out
}

fn polyline_length(pts: &[P2]) -> f64 {
    pts.windows(2)
        .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
        .sum()
}

/// The 2D segments already inked, bucketed by the grid cells they pass
/// through, so "is this stretch already drawn?" touches a handful.
struct Coverage {
    segs: Vec<(P2, P2)>,
    cells: HashMap<(i64, i64), Vec<u32>>,
    cell: f64,
    tol: f64,
}

impl Coverage {
    fn new(cell: f64, tol: f64) -> Coverage {
        Coverage {
            segs: Vec::new(),
            cells: HashMap::new(),
            cell: cell.max(tol * 4.0).max(1e-12),
            tol,
        }
    }

    /// The cells a segment passes through, sampled at half a cell.
    fn cells_of(&self, a: P2, b: P2) -> Vec<(i64, i64)> {
        let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        let n = ((len / (self.cell * 0.5)).ceil() as usize).clamp(1, 1 << 16);
        let mut out: Vec<(i64, i64)> = (0..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                (
                    ((a.x + (b.x - a.x) * t) / self.cell).floor() as i64,
                    ((a.y + (b.y - a.y) * t) / self.cell).floor() as i64,
                )
            })
            .collect();
        out.dedup();
        out
    }

    fn add(&mut self, pts: &[P2]) {
        for w in pts.windows(2) {
            let id = self.segs.len() as u32;
            self.segs.push((w[0], w[1]));
            for c in self.cells_of(w[0], w[1]) {
                let list = self.cells.entry(c).or_default();
                if list.last() != Some(&id) {
                    list.push(id);
                }
            }
        }
    }

    /// The stretches of `pts` that no inked segment lies on, as polylines.
    fn uncovered(&self, pts: &[P2]) -> Vec<Vec<P2>> {
        let mut out: Vec<Vec<P2>> = Vec::new();
        let mut cur: Vec<P2> = Vec::new();
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let d = P2::new(b.x - a.x, b.y - a.y);
            let len2 = d.x * d.x + d.y * d.y;
            if len2 <= 0.0 {
                continue;
            }
            let len = len2.sqrt();
            // Parameter intervals of a-b that lie on an inked segment.
            let mut ids: Vec<u32> = Vec::new();
            for (cx, cy) in self.cells_of(a, b) {
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        if let Some(l) = self.cells.get(&(cx + dx, cy + dy)) {
                            ids.extend_from_slice(l);
                        }
                    }
                }
            }
            ids.sort_unstable();
            ids.dedup();
            let mut covered: Vec<(f64, f64)> = Vec::new();
            for id in ids {
                let (p, q) = self.segs[id as usize];
                // Both ends on the line through a-b: the inked segment is
                // collinear with this one.
                let off = |r: P2| ((r.x - a.x) * d.y - (r.y - a.y) * d.x).abs() / len;
                if off(p) > self.tol || off(q) > self.tol {
                    continue;
                }
                let t = |r: P2| ((r.x - a.x) * d.x + (r.y - a.y) * d.y) / len2;
                let (t0, t1) = (t(p).min(t(q)), t(p).max(t(q)));
                // Only a real overlap counts, then widened by the slack: a
                // collinear segment that merely meets this one end to end
                // would otherwise trim its last `tol` and open a gap at a
                // shared endpoint.
                let slack = self.tol / len;
                let (r0, r1) = (t0.max(0.0), t1.min(1.0));
                if r1 - r0 <= slack {
                    continue;
                }
                covered.push(((r0 - slack).max(0.0), (r1 + slack).min(1.0)));
            }
            covered.sort_by(|x, y| x.0.total_cmp(&y.0));
            let at = |t: f64| P2::new(a.x + d.x * t, a.y + d.y * t);
            let mut t = 0.0f64;
            let mut free: Vec<(f64, f64)> = Vec::new();
            for (c0, c1) in covered {
                if c0 > t {
                    free.push((t, c0));
                }
                t = t.max(c1);
            }
            if t < 1.0 {
                free.push((t, 1.0));
            }
            for (f0, f1) in free {
                let (p0, p1) = (at(f0), at(f1));
                // A free stretch that starts where the last one ended
                // continues it; anything else starts a new polyline.
                let joins = f0 == 0.0 && cur.last().is_some_and(|l| l.x == a.x && l.y == a.y);
                if !joins && !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                if cur.is_empty() {
                    cur.push(if f0 == 0.0 { a } else { p0 });
                }
                cur.push(if f1 == 1.0 { b } else { p1 });
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }
}

fn emit(
    out: &mut Vec<(Vec<P2>, bool, usize)>,
    pts: Vec<P2>,
    visible: bool,
    body: usize,
    hidden: Hidden,
    eps: f64,
) {
    let pts = simplify(&dedupe(pts), eps);
    if pts.len() < 2 {
        return;
    }
    if visible || hidden != Hidden::Omit {
        out.push((pts, visible, body));
    }
}

fn dedupe(pts: Vec<P2>) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::with_capacity(pts.len());
    for p in pts {
        match out.last() {
            Some(q) if (q.x - p.x).abs() < 1e-12 && (q.y - p.y).abs() < 1e-12 => {}
            _ => out.push(p),
        }
    }
    out
}

/// Douglas-Peucker: keep the points that carry the shape, drop the rest.
fn simplify(pts: &[P2], eps: f64) -> Vec<P2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (mut worst, mut at) = (0.0f64, a);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = point_segment_distance(*p, pts[a], pts[b]);
            if d > worst {
                worst = d;
                at = i;
            }
        }
        if worst > eps {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    pts.iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| *p)
        .collect()
}

fn point_segment_distance(p: P2, a: P2, b: P2) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-24 {
        return ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt();
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    ((p.x - (a.x + dx * t)).powi(2) + (p.y - (a.y + dy * t)).powi(2)).sqrt()
}

fn bisect(
    tris: &ScreenTris,
    basis: &ViewBasis,
    a: Vec3,
    b: Vec3,
    a_visible: bool,
    eps: f64,
    rounds: usize,
) -> Vec3 {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..rounds {
        let m = (lo + hi) / 2.0;
        let p = lerp(a, b, m);
        if tris.visible(basis, p, eps) == a_visible {
            lo = m;
        } else {
            hi = m;
        }
    }
    lerp(a, b, (lo + hi) / 2.0)
}

fn lerp(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    a + (b - a) * t
}

/// Every edge that could appear in the picture: topology creases plus mesh
/// silhouettes, each as a world-space polyline.
fn candidate_edges(
    mesh: &Mesh,
    topo: &Topology,
    basis: &ViewBasis,
    crease_degrees: f64,
    ranges: &BodyRanges,
) -> Vec<(Vec<Vec3>, usize)> {
    let mut out: Vec<(Vec<Vec3>, usize)> = Vec::new();
    for (i, e) in topo.edges.iter().enumerate() {
        // A negative dihedral is an open boundary: always a real edge.
        if e.dihedral_degrees < 0.0 || e.dihedral_degrees >= crease_degrees {
            let poly: Vec<Vec3> = if e.polyline.len() >= 2 {
                e.polyline.clone()
            } else {
                vec![e.start, e.end]
            };
            out.push((poly, ranges.body_of_edge(i)));
        }
    }
    out.extend(silhouettes(mesh, basis, crease_degrees, ranges));
    out
}

/// Mesh edges where the surface turns away from the eye without being a
/// crease: the outline of a curved face.
fn silhouettes(
    mesh: &Mesh,
    basis: &ViewBasis,
    crease_degrees: f64,
    ranges: &BodyRanges,
) -> Vec<(Vec<Vec3>, usize)> {
    // `origin` is the source triangle of every welded one; through it, one
    // source vertex per welded vertex, so a chain can be tagged with the
    // body its first segment came from.
    let (welded, origin) = mesh.welded_with_map(1e-4);
    let mut rep: Vec<u32> = vec![u32::MAX; welded.vertex_count()];
    for (k, t) in welded.indices.chunks_exact(3).enumerate() {
        let ot = origin[k] as usize * 3;
        let src = &mesh.indices[ot..ot + 3];
        for j in 0..3 {
            if rep[t[j] as usize] == u32::MAX {
                rep[t[j] as usize] = src[j];
            }
        }
    }
    let pos = |i: u32| {
        let j = i as usize * 3;
        Vec3::new(
            welded.positions[j] as f64,
            welded.positions[j + 1] as f64,
            welded.positions[j + 2] as f64,
        )
    };
    let mut normals: Vec<Vec3> = Vec::with_capacity(welded.triangle_count());
    for t in welded.indices.chunks_exact(3) {
        let (a, b, c) = (pos(t[0]), pos(t[1]), pos(t[2]));
        normals.push((b - a).cross(c - a).normalized());
    }
    let mut shared: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (ti, t) in welded.indices.chunks_exact(3).enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let key = if a < b { (a, b) } else { (b, a) };
            shared.entry(key).or_default().push(ti);
        }
    }
    let cos_crease = crease_degrees.to_radians().cos();
    let mut segs: Vec<(u32, u32)> = Vec::new();
    for (key, tris) in &shared {
        if tris.len() != 2 {
            continue;
        }
        let (n0, n1) = (normals[tris[0]], normals[tris[1]]);
        if !(n0.length() > 0.0 && n1.length() > 0.0) {
            continue;
        }
        // A crease is already a topology edge; only smooth edges are ours.
        if n0.dot(n1) < cos_crease {
            continue;
        }
        let (d0, d1) = (n0.dot(basis.toward_eye), n1.dot(basis.toward_eye));
        if (d0 > 0.0) != (d1 > 0.0) {
            segs.push(*key);
        }
    }
    segs.sort_unstable();
    let body_of = |i: u32| ranges.body_of_vertex(rep[i as usize] as usize);
    chain(&segs, &|i| pos(i), &body_of)
}

/// Chain shared-endpoint segments into the longest polylines we can, so a
/// cylinder's silhouette is one stroke and not two hundred.
///
/// The result is *canonical*: each chain is rotated and oriented so it starts
/// at its own lowest point and runs the smaller way round, and the chains are
/// then sorted. Without that the drawing depends on the order the triangles
/// happened to arrive in — which a mesh boolean is under no obligation to keep
/// stable from one process to the next — and two exports of one model differ.
/// With it, the drawing is a function of the geometry and nothing else.
fn chain(
    segs: &[(u32, u32)],
    pos: &dyn Fn(u32) -> Vec3,
    body_of: &dyn Fn(u32) -> usize,
) -> Vec<(Vec<Vec3>, usize)> {
    let mut adj: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        adj.entry(*a).or_default().push(i);
        adj.entry(*b).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out: Vec<(Vec<Vec3>, usize)> = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (a, b) = segs[start];
        let body = body_of(a);
        let mut ids = vec![a, b];
        // Walk forward, then backward.
        for forward in [true, false] {
            loop {
                let end = if forward {
                    *ids.last().unwrap()
                } else {
                    ids[0]
                };
                let next = adj.get(&end).and_then(|list| {
                    list.iter().copied().find(|i| {
                        !used[*i] && {
                            let (u, v) = segs[*i];
                            u == end || v == end
                        }
                    })
                });
                let Some(i) = next else { break };
                used[i] = true;
                let (u, v) = segs[i];
                let other = if u == end { v } else { u };
                if forward {
                    ids.push(other);
                } else {
                    ids.insert(0, other);
                }
                if other == ids[0] && forward {
                    break; // closed
                }
            }
        }
        out.push((
            canonical(ids.into_iter().map(pos).collect(), |p: Vec3| {
                (p.x, p.y, p.z)
            }),
            body,
        ));
    }
    out.sort_by(|a, b| cmp_points(&a.0, &b.0, |p: Vec3| (p.x, p.y, p.z)));
    out
}

/// Rotate a closed chain to start at its lowest point and run the smaller way
/// round; orient an open one so its lower end comes first. Either way the
/// answer stops depending on where the walk happened to start.
fn canonical<T: Copy>(mut pts: Vec<T>, key: impl Fn(T) -> (f64, f64, f64) + Copy) -> Vec<T> {
    if pts.len() < 3 {
        if pts.len() == 2 && before_key(key(pts[1]), key(pts[0])) {
            pts.reverse();
        }
        return pts;
    }
    let closed = key(pts[0]) == key(pts[pts.len() - 1]);
    if closed {
        pts.pop();
    }
    let n = pts.len();
    if !closed {
        // An open chain can be reversed but not rotated.
        if before_key(key(pts[n - 1]), key(pts[0])) {
            pts.reverse();
        }
        return pts;
    }
    let mut at = 0;
    for i in 1..n {
        if before_key(key(pts[i]), key(pts[at])) {
            at = i;
        }
    }
    let forward: Vec<T> = (0..n).map(|i| pts[(at + i) % n]).collect();
    let backward: Vec<T> = (0..n).map(|i| pts[(at + n - i) % n]).collect();
    let mut out = if cmp_points(&forward, &backward, key) == std::cmp::Ordering::Greater {
        backward
    } else {
        forward
    };
    out.push(out[0]);
    out
}

/// Lexicographic order on a point's coordinates.
fn before_key(a: (f64, f64, f64), b: (f64, f64, f64)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && (a.1 < b.1 || (a.1 == b.1 && a.2 < b.2)))
}

/// The same, spelled out, for call sites that have loose coordinates.
fn before(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> bool {
    before_key((ax, ay, az), (bx, by, bz))
}

/// Lexicographic order on two point sequences.
fn cmp_points<T: Copy>(a: &[T], b: &[T], key: impl Fn(T) -> (f64, f64, f64)) -> std::cmp::Ordering {
    for (x, y) in a.iter().zip(b) {
        let (kx, ky) = (key(*x), key(*y));
        if before_key(kx, ky) {
            return std::cmp::Ordering::Less;
        }
        if before_key(ky, kx) {
            return std::cmp::Ordering::Greater;
        }
    }
    a.len().cmp(&b.len())
}

/// Triangles in screen space, bucketed so a point query touches a handful.
struct ScreenTris {
    /// (screen a, b, c, depth a, b, c)
    tris: Vec<([P2; 3], [f64; 3])>,
    cells: HashMap<(i64, i64), Vec<u32>>,
    cell: f64,
    lo: P2,
}

impl ScreenTris {
    fn build(mesh: &Mesh, basis: &ViewBasis, diag: f64) -> ScreenTris {
        let pos = |i: u32| {
            let j = i as usize * 3;
            Vec3::new(
                mesh.positions[j] as f64,
                mesh.positions[j + 1] as f64,
                mesh.positions[j + 2] as f64,
            )
        };
        let mut tris = Vec::with_capacity(mesh.triangle_count());
        let mut lo = P2::new(f64::MAX, f64::MAX);
        let mut hi = P2::new(f64::MIN, f64::MIN);
        for t in mesh.indices.chunks_exact(3) {
            let mut s = [P2::new(0.0, 0.0); 3];
            let mut d = [0.0f64; 3];
            for k in 0..3 {
                let (p, z) = basis.project(pos(t[k]));
                s[k] = p;
                d[k] = z;
                lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
            tris.push((s, d));
        }
        // Aim for a few triangles per cell.
        let n = (tris.len() as f64).sqrt().max(1.0);
        let extent = (hi.x - lo.x).max(hi.y - lo.y).max(diag * 1e-6);
        let cell = (extent / n).max(extent * 1e-3);
        let mut cells: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
        for (i, (s, _)) in tris.iter().enumerate() {
            let (x0, x1) = (
                ((s[0].x.min(s[1].x).min(s[2].x) - lo.x) / cell).floor() as i64,
                ((s[0].x.max(s[1].x).max(s[2].x) - lo.x) / cell).floor() as i64,
            );
            let (y0, y1) = (
                ((s[0].y.min(s[1].y).min(s[2].y) - lo.y) / cell).floor() as i64,
                ((s[0].y.max(s[1].y).max(s[2].y) - lo.y) / cell).floor() as i64,
            );
            // A triangle spanning a huge number of cells is registered by its
            // bounding box anyway; the clamp keeps a degenerate one cheap.
            for gx in x0..=x1.min(x0 + 512) {
                for gy in y0..=y1.min(y0 + 512) {
                    cells.entry((gx, gy)).or_default().push(i as u32);
                }
            }
        }
        ScreenTris {
            tris,
            cells,
            cell,
            lo,
        }
    }

    /// True when nothing in the mesh covers `p` nearer to the eye.
    fn visible(&self, basis: &ViewBasis, p: Vec3, eps: f64) -> bool {
        let (s, z) = basis.project(p);
        let gx = ((s.x - self.lo.x) / self.cell).floor() as i64;
        let gy = ((s.y - self.lo.y) / self.cell).floor() as i64;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(ids) = self.cells.get(&(gx + dx, gy + dy)) else {
                    continue;
                };
                for id in ids {
                    let (tri, depth) = &self.tris[*id as usize];
                    if let Some(w) = barycentric(tri, s) {
                        let zt = depth[0] * w[0] + depth[1] * w[1] + depth[2] * w[2];
                        if zt > z + eps {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

/// Barycentric weights of `p` in `tri`, or `None` when it is outside.
fn barycentric(tri: &[P2; 3], p: P2) -> Option<[f64; 3]> {
    let (a, b, c) = (tri[0], tri[1], tri[2]);
    let d = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
    if d.abs() < 1e-18 {
        return None;
    }
    let w0 = ((b.y - c.y) * (p.x - c.x) + (c.x - b.x) * (p.y - c.y)) / d;
    let w1 = ((c.y - a.y) * (p.x - c.x) + (a.x - c.x) * (p.y - c.y)) / d;
    let w2 = 1.0 - w0 - w1;
    const T: f64 = -1e-9;
    if w0 < T || w1 < T || w2 < T {
        None
    } else {
        Some([w0, w1, w2])
    }
}

/// A projected view as a drawing, on the standard view layers.
pub fn to_drawing(
    mesh: &Mesh,
    topo: &Topology,
    basis: &ViewBasis,
    opts: &ProjectOptions,
) -> Drawing {
    to_drawing_occluded(mesh, topo, basis, opts, &Mesh::default())
}

/// [`to_drawing`], with `occluders` hiding lines as the mesh does (see
/// [`project_occluded`]).
pub fn to_drawing_occluded(
    mesh: &Mesh,
    topo: &Topology,
    basis: &ViewBasis,
    opts: &ProjectOptions,
    occluders: &Mesh,
) -> Drawing {
    let p = project_occluded(mesh, topo, basis, opts, &BodyRanges::default(), occluders);
    let mut d = Drawing::new();
    let vis = d.layer(layer::VISIBLE);
    let hid = d.layer(layer::HIDDEN);
    for poly in &p.visible {
        d.polyline(vis, poly, false);
    }
    if opts.hidden != Hidden::Omit {
        for poly in &p.hidden {
            let style = if opts.hidden == Hidden::Solid {
                Style::dashed(hid, Dash::Solid)
            } else {
                Style::on(hid)
            };
            if poly.len() >= 2 {
                let mut path = Path::new(poly[0]);
                for q in &poly[1..] {
                    path = path.line_to(*q);
                }
                d.push(Item::Stroke(Stroke {
                    style,
                    name: String::new(),
                    path,
                    closed: false,
                }));
            }
        }
    }
    d
}

/// The loops where `plane` cuts `mesh`, in the plane's own 2D coordinates.
///
/// Segments are chained by shared endpoints within `tolerance`; anything that
/// does not close is dropped, because an open chain is a tessellation artefact
/// rather than a boundary of material.
pub fn section_loops(mesh: &Mesh, plane: &Plane, tolerance: f64) -> Vec<Loop> {
    let pos = |i: u32| {
        let j = i as usize * 3;
        Vec3::new(
            mesh.positions[j] as f64,
            mesh.positions[j + 1] as f64,
            mesh.positions[j + 2] as f64,
        )
    };
    let n = plane.normal.normalized();
    let x = plane.x_dir.normalized();
    let y = plane.y_dir().normalized();
    let to2 = |p: Vec3| {
        let r = p - plane.origin;
        P2::new(r.dot(x), r.dot(y))
    };
    // A vertex that lands exactly on the cut plane is the hard case, and it is
    // the *common* one: extrude a tube 30 mm tall, cut it at 15, and the
    // tessellation's own row of vertices is sitting on the plane. Left alone,
    // the triangles above and the triangles below each contribute that row's
    // edges, every segment appears twice, and the chainer walks out along the
    // circle and straight back — an annulus becomes twenty ribbons of zero
    // area.
    //
    // So a distance too close to zero to trust is pushed to the positive side
    // (the "simulation of simplicity" trick): the plane is nudged by ten
    // nanometres rather than the topology being special-cased. Triangles below
    // the row still cross it, triangles above no longer do, and each edge is
    // emitted once.
    let eps = (tolerance * 1e-3).max(1e-9);
    let sd = |p: Vec3| {
        let v = (p - plane.origin).dot(n);
        if v.abs() < eps {
            eps
        } else {
            v
        }
    };

    let mut segs: Vec<(P2, P2)> = Vec::new();
    for t in mesh.indices.chunks_exact(3) {
        let p = [pos(t[0]), pos(t[1]), pos(t[2])];
        let d = [sd(p[0]), sd(p[1]), sd(p[2])];
        if d.iter().all(|v| *v > 0.0) || d.iter().all(|v| *v < 0.0) {
            continue;
        }
        let mut hits: Vec<P2> = Vec::new();
        for k in 0..3 {
            let (a, b) = (k, (k + 1) % 3);
            if (d[a] > 0.0) == (d[b] > 0.0) {
                continue;
            }
            // The two triangles sharing this edge must land on *bitwise* the
            // same crossing point, or the weld below has two representatives
            // to choose between and picks whichever it met first — which makes
            // the cut depend on triangle order. Interpolating from the
            // lexicographically lower vertex means both evaluate the identical
            // expression.
            let (pa, pb, da, db) = if before(p[a].x, p[a].y, p[a].z, p[b].x, p[b].y, p[b].z) {
                (p[a], p[b], d[a], d[b])
            } else {
                (p[b], p[a], d[b], d[a])
            };
            let t = da / (da - db);
            if t.is_finite() && (0.0..=1.0).contains(&t) {
                hits.push(to2(pa + (pb - pa) * t));
            }
        }
        if hits.len() == 2 {
            let (a, b) = (hits[0], hits[1]);
            if (a.x - b.x).abs() > 1e-12 || (a.y - b.y).abs() > 1e-12 {
                segs.push((a, b));
            }
        }
    }
    chain_2d(&segs, tolerance)
}

/// Chain 2D segments into closed loops.
///
/// Two triangles sharing an edge compute the same crossing point from the same
/// two vertices, but often with the edge in the opposite order, so the two
/// answers differ in the last few bits. Snapping to a grid would then split a
/// loop wherever a point happened to straddle a cell boundary — which is
/// exactly how an annulus turns into twenty-two fragments. So endpoints are
/// *welded*: each one either joins a point already seen within `tolerance` (the
/// nine neighbouring cells are searched, so a cell boundary cannot separate
/// two points that are close) or becomes a new one.
fn chain_2d(segs: &[(P2, P2)], tolerance: f64) -> Vec<Loop> {
    let cell = tolerance.max(1e-9);
    // Two arrays, and the split is the point. `reps` is what the spatial index
    // in `cells` refers to and it is **never** written again, so the index can
    // never go stale — a representative that moved by up to `tolerance` would
    // sit a whole cell away from where its id is filed, and the ±1 search
    // would start missing matches and silently dropping loops. `best` carries
    // the answer: the lowest point in each cluster, which is what makes the
    // coordinate a property of the geometry rather than of arrival order.
    let mut reps: Vec<P2> = Vec::new();
    let mut best: Vec<P2> = Vec::new();
    let mut cells: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
    // Two triangles sharing an edge compute *bitwise* the same crossing (see
    // the interpolation above), so the common case never reaches the spatial
    // search at all.
    let mut exact: HashMap<(u64, u64), u32> = HashMap::new();
    let mut weld = |p: P2| -> u32 {
        let bits = (p.x.to_bits(), p.y.to_bits());
        if let Some(id) = exact.get(&bits) {
            return *id;
        }
        let key = ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(ids) = cells.get(&(key.0 + dx, key.1 + dy)) {
                    for id in ids {
                        let q = reps[*id as usize];
                        if (q.x - p.x).abs() <= tolerance && (q.y - p.y).abs() <= tolerance {
                            let b = &mut best[*id as usize];
                            if before_key((p.x, p.y, 0.0), (b.x, b.y, 0.0)) {
                                *b = p;
                            }
                            exact.insert(bits, *id);
                            return *id;
                        }
                    }
                }
            }
        }
        reps.push(p);
        best.push(p);
        let id = (reps.len() - 1) as u32;
        cells.entry(key).or_default().push(id);
        exact.insert(bits, id);
        id
    };
    let ids: Vec<(u32, u32)> = segs
        .iter()
        .map(|(a, b)| (weld(*a), weld(*b)))
        .filter(|(a, b)| a != b)
        .collect();
    let points = best;

    let mut adj: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, (a, b)) in ids.iter().enumerate() {
        adj.entry(*a).or_default().push(i);
        adj.entry(*b).or_default().push(i);
    }
    let mut used = vec![false; ids.len()];
    let mut loops = Vec::new();
    for start in 0..ids.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (a, b) = ids[start];
        let mut chain = vec![a, b];
        let mut closed = false;
        loop {
            let end = *chain.last().unwrap();
            let next = adj.get(&end).and_then(|list| {
                list.iter().copied().find(|i| {
                    !used[*i] && {
                        let (u, v) = ids[*i];
                        u == end || v == end
                    }
                })
            });
            let Some(i) = next else { break };
            used[i] = true;
            let (u, v) = ids[i];
            let other = if u == end { v } else { u };
            if other == chain[0] {
                closed = true;
                break;
            }
            chain.push(other);
        }
        // An open chain is a tessellation artefact, not a boundary of
        // material: a section only means anything when it closes.
        if closed && chain.len() >= 3 {
            let mut pts: Vec<P2> = chain.iter().map(|i| points[*i as usize]).collect();
            // Canonical for the same reason the 3D chains are: the cut must
            // not depend on which triangle the slicer reached first. The
            // closing edge is implicit in a `Loop`, so it is spelled out for
            // `canonical` — which rotates a closed ring but can only reverse an
            // open chain — and dropped again afterwards.
            pts.push(pts[0]);
            let mut pts = canonical(pts, |p| (p.x, p.y, 0.0));
            pts.pop();
            loops.push(Loop::polygon(&pts));
        }
    }
    loops.sort_by(|a, b| {
        let (pa, pb) = (loop_points(a), loop_points(b));
        cmp_points(&pa, &pb, |p| (p.x, p.y, 0.0))
    });
    loops
}

/// The cut face as triangles on `plane`: the section's regions as a face,
/// meshed. Empty when the loops do not make a face.
pub fn section_cap(loops: &[Loop], plane: &Plane, tolerance: f64) -> Mesh {
    let sketch = loops.iter().fold(Sketch::new(*plane), |s, l| s.with_loop(l.clone()));
    let Ok(solids) = cadpole_core::kernel::extrude(&sketch, tolerance.max(1e-3) * 10.0) else { return Mesh::default() };
    let n = plane.normal.normalized();
    let mut cap = Mesh::default();
    for solid in &solids {
        let Ok((m, _)) = cadpole_core::tessellate::tessellate(solid, tolerance) else { continue };
        let on = |i: u32| {
            let j = i as usize * 3;
            let p = Vec3::new(m.positions[j] as f64, m.positions[j + 1] as f64, m.positions[j + 2] as f64);
            ((p - plane.origin).dot(n)).abs() <= 1e-6 * (1.0 + (p - plane.origin).length())
        };
        for t in m.indices.chunks_exact(3) {
            if t.iter().all(|&i| on(i)) {
                let base = cap.vertex_count() as u32;
                for &i in t {
                    let j = i as usize * 3;
                    cap.positions.extend_from_slice(&m.positions[j..j + 3]);
                }
                cap.indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
        }
    }
    cap
}

/// A section drawing: the cut face hatched and outlined, with the geometry
/// behind the cut drawn as a projected view when `behind` is set.
///
/// The view looks along `-plane.normal` (the eye is on the normal's side), so
/// what is behind the cut is the half-space `(p - origin) · normal <= 0`. Only
/// that part of the model is projected: the material on the eye's side of the
/// plane is what the cut removed, and drawing it would put cut-away geometry
/// over the section and into its dimensions.
pub fn section_drawing(
    mesh: &Mesh,
    topo: &Topology,
    plane: &Plane,
    tolerance: f64,
    pattern: Pattern,
    behind: Option<&ProjectOptions>,
) -> Drawing {
    let loops = section_loops(mesh, plane, tolerance);
    let mut d = if let Some(opts) = behind {
        let basis = ViewBasis::looking_from(plane.normal, plane.y_dir());
        let beyond = HalfSpace::new(plane.origin, -plane.normal);
        // The clipped mesh is open at the cut; the cut face is solid
        // material, and an edge behind it is hidden, not visible through it.
        let cap = section_cap(&loops, plane, tolerance);
        to_drawing_occluded(&clip_mesh(mesh, &beyond), &clip_topology(topo, &beyond), &basis, opts, &cap)
    } else {
        Drawing::new()
    };
    let cut = d.layer(layer::SECTION);
    let patt = d.layer(layer::SECTION_HATCH);
    let sketch = loops
        .iter()
        .fold(Sketch::new(*plane), |s, l| s.with_loop(l.clone()));
    if let Ok(regions) = sketch.regions() {
        for r in regions {
            d.push(Item::Hatch(HatchItem {
                style: Style::on(patt),
                name: String::new(),
                outer: r.outer.clone(),
                holes: r.holes.clone(),
                pattern: pattern.clone(),
                scale: 1.0,
            }));
            for l in std::iter::once(&r.outer).chain(&r.holes) {
                d.loop_stroke(cut, l);
            }
        }
    } else {
        d.notes.push(
            "section: the cut loops do not nest, so the cut face is outlined but not hatched"
                .to_owned(),
        );
        for l in &loops {
            d.loop_stroke(cut, l);
        }
    }
    d
}

/// Signed area of a polygon loop, positive counter-clockwise. Exposed because
/// "does the section have the right area" is the test that says the slicer and
/// the chainer agree.
pub fn loop_area(l: &Loop) -> f64 {
    l.signed_area()
}

/// Every segment of `l` as a line, for callers that need a polyline.
pub fn loop_points(l: &Loop) -> Vec<P2> {
    l.segments
        .iter()
        .map(|s| match s {
            Segment::Line { a, .. } | Segment::Arc { a, .. } | Segment::Bezier { a, .. } => *a,
        })
        .collect()
}

#[cfg(test)]
mod clip_tests {
    use super::*;
    use cadpole_core::geom::Plane;
    use cadpole_core::sketch::{Loop, Sketch};

    fn box_mesh(w: f64, d: f64, h: f64) -> (Mesh, Topology) {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
        let solids = cadpole_core::kernel::extrude(&s, h).expect("extrude");
        cadpole_core::tessellate::tessellate(&solids[0], 0.05).expect("tess")
    }

    #[test]
    fn a_half_space_clip_keeps_the_volume_below_the_cut() {
        let (mesh, topo) = box_mesh(40.0, 20.0, 10.0);
        // Keep z <= 4: the normal points down from a plane at z = 4.
        let hs = HalfSpace::new(Vec3::new(0.0, 0.0, 4.0), Vec3::new(0.0, 0.0, -1.0));
        let cut = clip_mesh(&mesh, &hs);
        assert!(cut.triangle_count() > 0);
        assert_eq!(cut.normals.len(), cut.positions.len(), "normals kept");
        for p in cut.positions.chunks_exact(3) {
            assert!(p[2] <= 4.0 + 1e-5, "a vertex above the cut at z = {}", p[2]);
        }
        // Only vertical crease edges cross the plane; they are cut to length.
        let ct = clip_topology(&topo, &hs);
        for e in &ct.edges {
            for p in &e.polyline {
                assert!(p.z <= 4.0 + 1e-6);
            }
        }
        let verticals: Vec<&cadpole_core::tessellate::EdgeInfo> =
            ct.edges.iter().filter(|e| (e.start.z - e.end.z).abs() > 1.0).collect();
        assert_eq!(verticals.len(), 4, "four uprights, each 4 mm long now");
        for e in &verticals {
            assert!((e.length - 4.0).abs() < 1e-6, "{}", e.length);
        }
        // And the top's four edges are gone.
        assert_eq!(ct.edges.len(), 8);
        // A projection of the clipped box from above shows a 40 x 20 outline.
        let p = project(&cut, &ct, &ViewBasis::preset("top"), &ProjectOptions::default());
        let (lo, hi) = p.bounds().unwrap();
        assert!((hi.x - lo.x - 40.0).abs() < 1e-6 && (hi.y - lo.y - 20.0).abs() < 1e-6);
    }

    #[test]
    fn projected_lines_are_tagged_with_their_body() {
        let (m1, t1) = box_mesh(10.0, 10.0, 10.0);
        let (mut m2, mut t2) = box_mesh(10.0, 10.0, 10.0);
        for p in m2.positions.chunks_exact_mut(3) {
            p[0] += 40.0;
        }
        for e in &mut t2.edges {
            e.start.x += 40.0;
            e.end.x += 40.0;
            for p in &mut e.polyline {
                p.x += 40.0;
            }
        }
        let mut mesh = Mesh::default();
        let mut topo = Topology::default();
        let mut ranges = BodyRanges::default();
        append_body(&mut mesh, &mut topo, &mut ranges, &m1, &t1);
        append_body(&mut mesh, &mut topo, &mut ranges, &m2, &t2);
        assert_eq!(ranges.vertices, vec![0, m1.vertex_count()]);
        let p = project_tagged(
            &mesh,
            &topo,
            &ViewBasis::preset("front"),
            &ProjectOptions::default(),
            &ranges,
        );
        assert_eq!(p.visible.len(), p.visible_body.len());
        assert!(
            p.visible_body.contains(&0) && p.visible_body.contains(&1),
            "{:?}",
            p.visible_body
        );
        for (poly, body) in p.visible.iter().zip(&p.visible_body) {
            for q in poly {
                if *body == 0 {
                    assert!(q.x <= 5.0 + 1e-6, "body 0 lines stay left: {q:?}");
                } else {
                    assert!(q.x >= 35.0 - 1e-6, "body 1 lines stay right: {q:?}");
                }
            }
        }
    }
}
