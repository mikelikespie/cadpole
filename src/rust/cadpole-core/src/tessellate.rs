//! Convert kernel solids into flat render meshes plus a named topology.

use std::collections::BTreeMap;
use std::sync::Arc;

use truck_meshalgo::prelude::*;
use truck_modeling::{Surface, Transformed};

use crate::geom::Vec3;
use crate::kernel::{self, KSolid};
use crate::naming::{self, Fingerprint, Kind, Named};
use crate::surface_class::{self, SurfaceClass};
use crate::fmath;

/// How far under [`Mesh::smooth_seams`]'s threshold a fold must be to shade
/// as one surface: an f32 normal's rounding, with room to spare, so a fold
/// of exactly the threshold stays a crease.
pub const SEAM_MARGIN_DEGREES: f64 = 0.05;

/// Flat triangle mesh, ready for the GPU or the software rasterizer.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    /// One topology face index per triangle.
    pub face_ids: Vec<u32>,
    /// One colour per vertex, sRGB-encoded 8-bit RGB triples: what an
    /// imported scan carries (`COLOR_0`, an OBJ's `v x y z r g b`).
    /// Empty on every mesh Cadpole makes itself. Valid only while it holds
    /// exactly three bytes per vertex ([`Mesh::vertex_colors`]); an
    /// operation that rebuilds the vertices without carrying it (a boolean,
    /// a cleanup) leaves it empty, and a reader that finds it the wrong
    /// length treats it as absent, never as a fault.
    pub colors: Vec<u8>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }
    /// The per-vertex colours, when the mesh carries a complete set: three
    /// sRGB-encoded bytes per vertex. `None` for every mesh without
    /// them, and for one whose colours no longer match its vertices.
    pub fn vertex_colors(&self) -> Option<&[u8]> {
        (!self.colors.is_empty() && self.colors.len() == self.positions.len()).then_some(self.colors.as_slice())
    }
    pub fn bbox(&self) -> Option<(Vec3, Vec3)> {
        if self.positions.is_empty() {
            return None;
        }
        let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        for p in self.positions.chunks_exact(3) {
            let v = Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        Some((lo, hi))
    }
    /// Signed volume (positive for outward-facing closed meshes), by the
    /// divergence theorem over triangles.
    pub fn volume(&self) -> f64 {
        let mut v = 0.0f64;
        for t in self.indices.chunks_exact(3) {
            let p = |i: u32| {
                let i = i as usize * 3;
                (self.positions[i] as f64, self.positions[i + 1] as f64, self.positions[i + 2] as f64)
            };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            v += a.0 * (b.1 * c.2 - b.2 * c.1) - a.1 * (b.0 * c.2 - b.2 * c.0) + a.2 * (b.0 * c.1 - b.1 * c.0);
        }
        v / 6.0
    }

    /// The three corners of triangle `t`, in world space.
    pub fn triangle(&self, t: usize) -> Option<[Vec3; 3]> {
        if (t + 1) * 3 > self.indices.len() {
            return None;
        }
        let tri = &self.indices[t * 3..t * 3 + 3];
        let p = |i: u32| {
            let i = i as usize * 3;
            Vec3::new(
                self.positions[i] as f64,
                self.positions[i + 1] as f64,
                self.positions[i + 2] as f64,
            )
        };
        Some([p(tri[0]), p(tri[1]), p(tri[2])])
    }

    /// Total surface area.
    pub fn area(&self) -> f64 {
        let mut s = 0.0f64;
        for t in self.indices.chunks_exact(3) {
            let p = |i: u32| {
                let i = i as usize * 3;
                Vec3::new(self.positions[i] as f64, self.positions[i + 1] as f64, self.positions[i + 2] as f64)
            };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            s += (b - a).cross(c - a).length() / 2.0;
        }
        s
    }

    /// The same mesh with vertices merged by position (within `eps`), so that
    /// faces share edges and manifoldness can be checked.
    pub fn welded(&self, eps: f32) -> Mesh {
        self.welded_with_map(eps).0
    }

    /// [`welded`](Self::welded), plus the index in `self` of every triangle
    /// the welded mesh kept, in order. Welding drops the triangles it
    /// collapses and keeps the rest in their original order, so a finding on
    /// the welded mesh — a crossing pair from `solid::validate`, say — maps
    /// back to the triangle the renderer draws through this table.
    pub fn welded_with_map(&self, eps: f32) -> (Mesh, Vec<u32>) {
        // Spatial hash with cells of size `eps`; a point merges with any
        // earlier point within `eps` found in its own or the 26 neighbouring
        // cells, so near-identical points never split across a cell boundary.
        let q = |v: f32| (v / eps).floor() as i64;
        let mut cells: std::collections::HashMap<(i64, i64, i64), Vec<u32>> = std::collections::HashMap::new();
        let mut remap = Vec::with_capacity(self.vertex_count());
        let mut out = Mesh::default();
        let eps2 = eps * eps;
        // A merged vertex keeps the colour of the first vertex at its
        // position, as it keeps that vertex's normal.
        let colors = self.vertex_colors();
        for (i, p) in self.positions.chunks_exact(3).enumerate() {
            let key = (q(p[0]), q(p[1]), q(p[2]));
            let mut found = None;
            'search: for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(ids) = cells.get(&(key.0 + dx, key.1 + dy, key.2 + dz)) {
                            for &id in ids {
                                let j = id as usize * 3;
                                let d = ((out.positions[j] - p[0]) * (out.positions[j] - p[0])) + ((out.positions[j + 1] - p[1]) * (out.positions[j + 1] - p[1])) + ((out.positions[j + 2] - p[2]) * (out.positions[j + 2] - p[2]));
                                if d <= eps2 {
                                    found = Some(id);
                                    break 'search;
                                }
                            }
                        }
                    }
                }
            }
            let id = match found {
                Some(id) => id,
                None => {
                    out.positions.extend_from_slice(p);
                    out.normals.extend_from_slice(&self.normals[i * 3..i * 3 + 3]);
                    if let Some(c) = colors {
                        out.colors.extend_from_slice(&c[i * 3..i * 3 + 3]);
                    }
                    let id = (out.positions.len() / 3 - 1) as u32;
                    cells.entry(key).or_default().push(id);
                    id
                }
            };
            remap.push(id);
        }
        let mut origin = Vec::with_capacity(self.triangle_count());
        for (t, tri) in self.indices.chunks_exact(3).enumerate() {
            let m = [remap[tri[0] as usize], remap[tri[1] as usize], remap[tri[2] as usize]];
            if m[0] == m[1] || m[1] == m[2] || m[0] == m[2] {
                continue; // degenerate after welding
            }
            out.indices.extend_from_slice(&m);
            out.face_ids.push(self.face_ids.get(t).map(|f| f.to_owned()).unwrap_or(0));
            origin.push(t as u32);
        }
        (out, origin)
    }

    /// Smooth the shading normals across seams that fold by less than
    /// `max_degrees` (by more than [`SEAM_MARGIN_DEGREES`] less), in place.
    ///
    /// Tessellation and the mesh kernel both give each topology face its own
    /// vertices and a normal that is the face's own. That is right at a box
    /// corner and wrong along a curve drawn as a polyline: an extruded
    /// 26-segment spiral is 26 flat faces, each reflecting its own slice of
    /// the environment, and the render shows every segment. Here a vertex
    /// also takes in the normals of any other face's vertices at the same
    /// position that are within `max_degrees` of its own, each weighted by
    /// the angle its triangles subtend there, so a shallow seam shades as
    /// one surface while a real edge stays a crease. A vertex with no such
    /// neighbour keeps its normal *exactly*: the normals are the surface's
    /// — truck's evaluation on a B-rep face, the source surface's after a
    /// mesh boolean — and re-deriving them from the triangle planes
    /// here would put the facet half-angle back into every curved face on
    /// the way to the renderer. Positions and indices
    /// are untouched: this is for the eye, not the kernel, and a mesh with
    /// no normals gets area-weighted ones first. Thirty degrees is the
    /// convention (Blender's auto-smooth) and what the line overlay calls a
    /// crease, so what is drawn as an edge is what shades as one.
    pub fn smooth_seams(&mut self, max_degrees: f64) {
        let nv = self.vertex_count();
        if nv == 0 || max_degrees <= 0.0 {
            return;
        }
        let pos = |i: usize| Vec3::new(self.positions[i * 3] as f64, self.positions[i * 3 + 1] as f64, self.positions[i * 3 + 2] as f64);
        // Area-weighted triangle normals, and each vertex's own triangles.
        let ntri = self.triangle_count();
        let mut tri_n = Vec::with_capacity(ntri);
        let mut own = vec![Vec3::ZERO; nv];
        for t in self.indices.chunks_exact(3) {
            let (a, b, c) = (pos(t[0] as usize), pos(t[1] as usize), pos(t[2] as usize));
            let n = (b - a).cross(c - a);
            tri_n.push(n);
            for &v in t {
                own[v as usize] = own[v as usize] + n;
            }
        }
        // Vertices that share a position, found the way `welded` finds them,
        // at the mesh kernel's weld tolerance: two faces of a B-rep body are
        // tessellated separately and agree along their shared edge only to
        // that, and matching them any tighter left every loft band of a hub
        // shaded flat, as terraces.
        let eps = self.seam_eps();
        let q = |v: f32| (v / eps).floor() as i64;
        let mut cells: std::collections::HashMap<(i64, i64, i64), Vec<u32>> = std::collections::HashMap::new();
        let mut class = Vec::with_capacity(nv);
        let mut class_of_first: Vec<u32> = Vec::new();
        for i in 0..nv {
            let p = &self.positions[i * 3..i * 3 + 3];
            let key = (q(p[0]), q(p[1]), q(p[2]));
            let mut found = None;
            'search: for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(ids) = cells.get(&(key.0 + dx, key.1 + dy, key.2 + dz)) {
                            for &id in ids {
                                let j = id as usize * 3;
                                let d = ((self.positions[j] - p[0]) * (self.positions[j] - p[0])) + ((self.positions[j + 1] - p[1]) * (self.positions[j + 1] - p[1])) + ((self.positions[j + 2] - p[2]) * (self.positions[j + 2] - p[2]));
                                if d <= eps * eps {
                                    found = Some(class_of_first[id as usize]);
                                    break 'search;
                                }
                            }
                        }
                    }
                }
            }
            let c = found.unwrap_or_else(|| {
                cells.entry(key).or_default().push(i as u32);
                class_of_first.len() as u32
            });
            class_of_first.push(c);
            class.push(c);
        }
        // Every corner that sits at a position, weighted by the angle the
        // triangle makes there (Thurmer & Wuthrich): a quad split into two
        // triangles then pulls no harder than one left whole, which area
        // weighting gets wrong, and tessellation splits faces as it likes.
        // What a corner contributes is its *vertex's* normal, not its
        // triangle's plane: on a flat face the two are the same, on a
        // curved one only the first is the surface's.
        let nclass = class.iter().copied().max().map(|c| c as usize + 1).unwrap_or(0);
        let mut corners: Vec<Vec<(u32, f64)>> = vec![Vec::new(); nclass];
        for (t, tri) in self.indices.chunks_exact(3).enumerate() {
            if tri_n[t].length() <= 0.0 {
                continue;
            }
            for k in 0..3 {
                let p = pos(tri[k] as usize);
                let a = pos(tri[(k + 1) % 3] as usize) - p;
                let b = pos(tri[(k + 2) % 3] as usize) - p;
                let angle = fmath::atan2(a.cross(b).length(), a.dot(b));
                corners[class[tri[k] as usize] as usize].push((tri[k], angle));
            }
        }
        let has_normals = self.normals.len() >= self.positions.len();
        // Each vertex's normal as given, or area-weighted from its own
        // triangles when the mesh has none (or a zero one).
        let given: Vec<Vec3> = (0..nv)
            .map(|i| {
                let g = if has_normals {
                    Vec3::new(self.normals[i * 3] as f64, self.normals[i * 3 + 1] as f64, self.normals[i * 3 + 2] as f64)
                } else {
                    Vec3::ZERO
                };
                if g.length() > 1e-12 { g.normalized() } else { own[i].normalized() }
            })
            .collect();
        // A fold of exactly `max_degrees` is a crease, as the line overlay
        // draws it: a 30 degree chamfer on a 30 degree threshold is a real
        // edge, and its two faces' normals, rounded to f32, land a hair
        // either side of the cosine. So a seam is a fold under the
        // threshold by more than that rounding.
        let cos_max = fmath::cos((max_degrees - SEAM_MARGIN_DEGREES).to_radians());
        let mut out = Vec::with_capacity(nv * 3);
        for i in 0..nv {
            let reference = given[i];
            let mut sum = Vec3::ZERO;
            for &(j, angle) in &corners[class[i] as usize] {
                let n = given[j as usize];
                if n.dot(reference) > cos_max {
                    sum = sum + n * angle;
                }
            }
            let n = if sum.length() > 1e-12 { sum.normalized() } else { reference };
            out.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
        }
        if crate::kernel::trace::enabled() {
            let moved = out
                .chunks_exact(3)
                .zip(self.normals.chunks_exact(3))
                .filter(|(a, b)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs() > 1e-3)
                .count();
            let _ = crate::kernel::trace::Span::start(format!(
                "smooth_seams: {nv} vertices in {nclass} positions (eps {eps}), {moved} normals changed"
            ));
        }
        self.normals = out;
    }

    /// How close two vertices must be to count as one position for
    /// [`Self::smooth_seams`]: a thousandth of the mesh's extent, floored at
    /// the weld tolerance. Two faces of a B-rep body are tessellated
    /// separately and agree along their shared edge only approximately; a
    /// fixed 1e-4 mm left every band of a lofted hub shaded flat.
    fn seam_eps(&self) -> f32 {
        let diag = self.bbox().map(|(lo, hi)| (hi - lo).length()).unwrap_or(1.0) as f32;
        (diag * 1e-5).max(crate::mesh_kernel::WELD_EPS)
    }

    /// True when every edge's directed uses cancel: one triangle each way is
    /// closed, orientable and consistently oriented. Weld first.
    ///
    /// It cannot see an internal wall. Two triangles each way on an edge —
    /// the seam two fused lumps still share — sums to zero exactly like a
    /// clean edge, and so does an inside-out surface. The question "is this
    /// a solid?" is [`crate::solid::validate`], which counts the uses; this
    /// is kept as the mesh kernel's cheap operand gate, and tightening it
    /// would change what `to_manifold` accepts.
    pub fn is_watertight(&self) -> bool {
        let mut edges: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();
        for t in self.indices.chunks_exact(3) {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                // Directed edge a->b counts +1, its reverse -1; a closed
                // consistently oriented mesh sums to 0 on every undirected edge
                // with exactly one of each.
                let key = if a < b { (a, b) } else { (b, a) };
                *edges.entry(key).or_insert(0) += if a < b { 1 } else { -1 };
            }
        }
        !edges.is_empty() && edges.values().all(|&c| c == 0)
    }

    /// True when every edge is shared by exactly two triangles, one
    /// traversing it each way: a closed, consistently oriented 2-manifold
    /// with no internal wall. Stricter than [`Mesh::is_watertight`], whose
    /// directed sum lets a double-sided sheet inside the solid through (two
    /// triangles each way still sum to zero). Weld first.
    pub fn is_closed_manifold(&self) -> bool {
        crate::mesh_clean::closed_manifold(self.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]))
    }

    pub fn to_proto(&self) -> cadpole_proto::v1::Mesh {
        cadpole_proto::v1::Mesh {
            positions: self.positions.clone(),
            normals: self.normals.clone(),
            indices: self.indices.clone(),
            face_ids: self.face_ids.clone(),
            colors: self.vertex_colors().map(<[u8]>::to_vec).unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FaceInfo {
    /// Generated name (the naming grammar in docs/design.md), e.g. `extrude_1/side[0.2]`.
    pub name: String,
    /// Other names that resolve to this face: the generic form of a
    /// primitive's face, the group name of a split piece, or the other
    /// sources of a face a boolean merged.
    pub aliases: Vec<String>,
    /// truck's own surface type: `plane`, `bspline`, `nurbs`, `revolved`
    /// (`kernel::surface_kind`), or `group` on a mesh body. Unchanged by
    /// recognition, because the namer and the fingerprints compare it.
    pub surface_kind: &'static str,
    /// The analytic surface this face was recognised as, with its
    /// parameters ([`crate::surface_class`]); `Other` for a free-form
    /// surface and for every mesh-body group.
    pub surface: SurfaceClass,
    /// The face's own surface, kept when the class is `Other`: a free-form
    /// patch has no closed-form normal, so a mesh boolean evaluates the
    /// exact normal of a vertex it makes by inverting the point onto this
    /// ([`crate::mesh_kernel`]). `None` for a recognised class, whose
    /// closed form is exact and cheaper to evaluate and to move, and for a
    /// mesh-body group that never had a surface. Shared, because a topology
    /// is cloned freely and a patch is a control net.
    pub geometry: Option<Arc<Surface>>,
    pub centroid: Vec3,
    pub normal: Vec3,
    pub area: f64,
    pub tri_start: u32,
    pub tri_count: u32,
    pub edges: Vec<String>,
    /// Indices into [`Topology::edges`], the durable form of `edges`.
    pub edge_ids: Vec<u32>,
    /// The surface's `v` direction at its parametric centre, unit: the
    /// ruling of a lofted or swept face (Cadpole's lofts run `v` between the
    /// sections) and the turning direction of a revolved one. Zero when
    /// unknown — a mesh body's group, a field's contour. What the `milled`
    /// texture lays its scallops along.
    pub v_dir: Vec3,
}

impl FaceInfo {
    /// What `Face.kind` reports: the recognised class when there is one,
    /// otherwise truck's own type — a closed vocabulary, in which
    /// the truck strings are the aliases a free-form surface falls back to.
    pub fn kind(&self) -> &'static str {
        if self.surface.is_recognised() { self.surface.kind_name() } else { self.surface_kind }
    }

    /// The recognised class alone: `plane`, `cylinder`, `cone`, `sphere`,
    /// `torus` or `other`.
    pub fn surface_class(&self) -> &'static str {
        self.surface.kind_name()
    }
}

#[derive(Clone, Debug, Default)]
pub struct EdgeInfo {
    /// Generated name: the canonically ordered pair of faces this edge bounds,
    /// e.g. `lid/end|side[0.2]`.
    pub name: String,
    pub curve_kind: &'static str,
    pub start: Vec3,
    pub end: Vec3,
    pub length: f64,
    pub polyline: Vec<Vec3>,
    pub faces: Vec<String>,
    /// Indices into [`Topology::faces`], the durable form of `faces`.
    pub face_ids: Vec<u32>,
    /// Angle between the surfaces meeting at this edge, in degrees: 0 is
    /// tangent (a smooth seam that should not be drawn as a crease), 90 is a
    /// box corner, 180 folds back on itself. Measured from the triangles that
    /// share the edge, so it is right on curved faces too. `-1` when the edge
    /// has no two adjacent triangles (an open boundary).
    pub dihedral_degrees: f64,
    /// True when the material is on the inside of the fold (a convex edge).
    pub convex: bool,
}

#[derive(Clone, Debug, Default)]
pub struct VertexInfo {
    /// Generated name: the faces that meet here, e.g. `b/end|side[0.0]|side[0.3]`.
    pub name: String,
    pub position: Vec3,
    pub faces: Vec<String>,
    /// Indices into [`Topology::faces`], the durable form of `faces`.
    pub face_ids: Vec<u32>,
}

impl Named for FaceInfo {
    fn name(&self) -> &str {
        &self.name
    }
    fn aliases(&self) -> &[String] {
        &self.aliases
    }
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            surface_kind: self.surface_kind.to_owned(),
            centroid: self.centroid,
            normal: self.normal,
            area: self.area,
            length: 0.0,
        }
    }
}

impl Named for EdgeInfo {
    fn name(&self) -> &str {
        &self.name
    }
    fn aliases(&self) -> &[String] {
        &[]
    }
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            surface_kind: self.curve_kind.to_owned(),
            centroid: (self.start + self.end) * 0.5,
            normal: Vec3::ZERO,
            area: 0.0,
            length: self.length,
        }
    }
}

impl Named for VertexInfo {
    fn name(&self) -> &str {
        &self.name
    }
    fn aliases(&self) -> &[String] {
        &[]
    }
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint { centroid: self.position, ..Default::default() }
    }
}

impl Mesh {
    /// This mesh moved by `t`. Used to reuse one tessellation across copies
    /// and patterns instead of re-tessellating identical geometry.
    pub fn transformed(&self, t: &crate::geom::Transform) -> Mesh {
        let mut out = self.clone();
        for p in out.positions.chunks_exact_mut(3) {
            let v = t.point(Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
            p[0] = v.x as f32;
            p[1] = v.y as f32;
            p[2] = v.z as f32;
        }
        for n in out.normals.chunks_exact_mut(3) {
            let v = t.direction(Vec3::new(n[0] as f64, n[1] as f64, n[2] as f64));
            n[0] = v.x as f32;
            n[1] = v.y as f32;
            n[2] = v.z as f32;
        }
        out
    }
}

#[derive(Clone, Debug, Default)]
pub struct Topology {
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
    pub vertices: Vec<VertexInfo>,
}

/// How many points per face the namer gets. Enough to tell which source
/// surface a face lies on, few enough that naming a 10 000-face body is not
/// the expensive part of evaluating it.
const FACE_SAMPLES: usize = 32;

/// Points spread over a face: triangle centroids, which lie on the face (to
/// within the chord error) and never on an edge it shares with another face.
pub fn face_samples(mesh: &Mesh, f: &FaceInfo, max: usize) -> Vec<Vec3> {
    let n = f.tri_count as usize;
    if n == 0 {
        return vec![f.centroid];
    }
    let step = n.div_ceil(max.max(1)).max(1);
    let mut out = Vec::with_capacity(n.div_ceil(step));
    for k in (0..n).step_by(step) {
        if let Some(tri) = mesh.triangle(f.tri_start as usize + k) {
            out.push((tri[0] + tri[1] + tri[2]) * (1.0 / 3.0));
        }
    }
    if out.is_empty() {
        out.push(f.centroid);
    }
    out
}

impl Topology {
    /// One probe per face, for [`crate::naming`]: geometry plus points on the
    /// face, and nothing that knows which kernel produced it.
    pub fn probes(&self, mesh: &Mesh) -> Vec<naming::FaceProbe> {
        self.faces
            .iter()
            .map(|f| naming::FaceProbe {
                surface_kind: f.surface_kind,
                centroid: f.centroid,
                normal: f.normal,
                area: f.area,
                samples: face_samples(mesh, f, FACE_SAMPLES),
            })
            .collect()
    }

    /// Give the faces the names the namer worked out, then rederive every edge
    /// and vertex name from them. Returns how many faces matched nothing.
    pub fn apply_names(&mut self, named: Vec<naming::NamedFace>) -> usize {
        let mut unmatched = 0;
        for (f, n) in self.faces.iter_mut().zip(named) {
            if n.role == naming::Role::Unmatched {
                unmatched += 1;
            }
            f.name = n.name;
            f.aliases = n.aliases;
        }
        self.split_duplicate_face_names();
        self.rederive_names();
        unmatched
    }

    /// Faces that came out with the same name are the pieces that name was
    /// split into — by a boolean, or by uniting a body with a copy of itself.
    /// They become `name#k` in [`naming::order_pieces`]' canonical order, and
    /// the unsuffixed name stays on as a group alias, so a stored reference to
    /// it resolves to the *set* rather than silently picking one.
    fn split_duplicate_face_names(&mut self) {
        let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, f) in self.faces.iter().enumerate() {
            by_name.entry(f.name.clone()).or_default().push(i);
        }
        for (name, members) in by_name {
            if members.len() < 2 {
                continue;
            }
            let pieces: Vec<(Vec3, Vec3, f64)> =
                members.iter().map(|&i| (self.faces[i].centroid, self.faces[i].normal, self.faces[i].area)).collect();
            for (k, &pos) in naming::order_pieces(&pieces).iter().enumerate() {
                let f = &mut self.faces[members[pos]];
                f.name = format!("{name}#{k}");
                if !f.aliases.contains(&name) {
                    f.aliases.push(name.clone());
                }
            }
        }
    }

    /// Rebuild every edge and vertex name from the face names, and refresh the
    /// name lists that cross-reference them. Idempotent.
    pub fn rederive_names(&mut self) {
        let faces: Vec<String> = self.faces.iter().map(|f| f.name.clone()).collect();
        let pick = |ids: &[u32]| -> Vec<String> {
            ids.iter().filter_map(|i| faces.get(*i as usize).cloned()).collect()
        };
        for (i, e) in self.edges.iter_mut().enumerate() {
            e.faces = pick(&e.face_ids);
            e.name = naming::edge_name(&e.faces);
            if e.name.is_empty() {
                e.name = Kind::Edge.index_name(i);
            }
        }
        for (i, v) in self.vertices.iter_mut().enumerate() {
            v.faces = pick(&v.face_ids);
            v.name = naming::join_names(&v.faces);
            if v.name.is_empty() {
                v.name = Kind::Vertex.index_name(i);
            }
        }
        // Several edges (or vertices) can share one face pair — a cylinder cut
        // by a plane gives two. They take an `@k` ordinal in a canonical order.
        let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in self.edges.iter().enumerate() {
            by_name.entry(e.name.clone()).or_default().push(i);
        }
        for (name, members) in by_name {
            if members.len() < 2 {
                continue;
            }
            let geom: Vec<naming::EdgeGeom> = members
                .iter()
                .map(|&i| {
                    let e = &self.edges[i];
                    naming::EdgeGeom {
                        start: e.start,
                        end: e.end,
                        length: e.length,
                        midpoint: naming::polyline_midpoint(&e.polyline, e.length),
                    }
                })
                .collect();
            for (k, &pos) in naming::order_edges(&geom).iter().enumerate() {
                self.edges[members[pos]].name = format!("{name}@{k}");
            }
        }
        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        let mut dup: BTreeMap<String, u32> = BTreeMap::new();
        for v in &self.vertices {
            *seen.entry(v.name.clone()).or_insert(0) += 1;
        }
        for v in &mut self.vertices {
            if seen.get(&v.name).copied().unwrap_or(0) > 1 {
                let k = dup.entry(v.name.clone()).or_insert(0);
                v.name = format!("{}@{k}", v.name);
                *k += 1;
            }
        }
        let edges: Vec<String> = self.edges.iter().map(|e| e.name.clone()).collect();
        for f in &mut self.faces {
            f.edges = f.edge_ids.iter().filter_map(|i| edges.get(*i as usize).cloned()).collect();
        }
    }

    /// Diagonal of the bounding box of every face centroid — the scale the
    /// geometric fallback's tolerances are relative to.
    pub fn diagonal(&self) -> f64 {
        let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        for f in &self.faces {
            lo = lo.min(f.centroid);
            hi = hi.max(f.centroid);
        }
        if self.faces.is_empty() { 0.0 } else { (hi - lo).length() }
    }

    /// This topology moved by `t`, keeping every name (a copy's faces are the
    /// same faces, which is what topological naming wants).
    pub fn transformed(&self, t: &crate::geom::Transform) -> Topology {
        let s = t.scale_factor();
        Topology {
            faces: self
                .faces
                .iter()
                .map(|f| FaceInfo {
                    centroid: t.point(f.centroid),
                    normal: t.direction(f.normal),
                    area: f.area * s * s,
                    surface: f.surface.transformed(t),
                    geometry: f.geometry.as_ref().map(|g| Arc::new(g.transformed(t.0))),
                    ..f.clone()
                })
                .collect(),
            edges: self
                .edges
                .iter()
                .map(|e| EdgeInfo {
                    start: t.point(e.start),
                    end: t.point(e.end),
                    length: e.length * s,
                    polyline: e.polyline.iter().map(|p| t.point(*p)).collect(),
                    ..e.clone()
                })
                .collect(),
            vertices: self
                .vertices
                .iter()
                .map(|v| VertexInfo { position: t.point(v.position), ..v.clone() })
                .collect(),
        }
    }

    pub fn to_proto(&self) -> cadpole_proto::v1::Topology {
        use cadpole_proto::v1 as pb;
        pb::Topology {
            faces: self
                .faces
                .iter()
                .enumerate()
                .map(|(i, f)| pb::TopoFace {
                    name: f.name.clone(),
                    aliases: f.aliases.clone(),
                    index: i as u32,
                    surface_kind: f.surface_kind.to_owned(),
                    centroid: Some(f.centroid.to_proto()),
                    normal: Some(f.normal.to_proto()),
                    area: f.area,
                    edges: f.edges.clone(),
                    tri_start: f.tri_start,
                    tri_count: f.tri_count,
                })
                .collect(),
            edges: self
                .edges
                .iter()
                .enumerate()
                .map(|(i, e)| pb::TopoEdge {
                    name: e.name.clone(),
                    index: i as u32,
                    curve_kind: e.curve_kind.to_owned(),
                    start: Some(e.start.to_proto()),
                    end: Some(e.end.to_proto()),
                    length: e.length,
                    faces: e.faces.clone(),
                    polyline: e.polyline.iter().map(|p| p.to_proto()).collect(),
                    dihedral_degrees: e.dihedral_degrees,
                    convex: e.convex,
                })
                .collect(),
            vertices: self
                .vertices
                .iter()
                .enumerate()
                .map(|(i, v)| pb::TopoVertex {
                    name: v.name.clone(),
                    index: i as u32,
                    faces: v.faces.clone(),
                    position: Some(v.position.to_proto()),
                })
                .collect(),
        }
    }
}

/// Angle in degrees between the two triangles sharing each mesh edge, keyed
/// by the welded vertex pair, together with convexity. Built once per body.
struct DihedralMap {
    /// (lo, hi) welded vertex ids -> (degrees, convex)
    edges: std::collections::HashMap<(u32, u32), (f64, bool)>,
    /// Spatial hash of welded vertex positions for polyline lookup.
    cells: std::collections::HashMap<(i64, i64, i64), Vec<u32>>,
    positions: Vec<Vec3>,
    eps: f64,
}

impl DihedralMap {
    fn build(mesh: &Mesh, eps: f64) -> DihedralMap {
        let welded = mesh.welded(eps as f32);
        let positions: Vec<Vec3> = welded
            .positions
            .chunks_exact(3)
            .map(|p| Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64))
            .collect();
        let mut cells: std::collections::HashMap<(i64, i64, i64), Vec<u32>> = Default::default();
        let key = |v: Vec3, eps: f64| ((v.x / eps).floor() as i64, (v.y / eps).floor() as i64, (v.z / eps).floor() as i64);
        for (i, p) in positions.iter().enumerate() {
            cells.entry(key(*p, eps)).or_default().push(i as u32);
        }
        // Triangle normals by undirected edge.
        let mut per_edge: std::collections::HashMap<(u32, u32), Vec<(Vec3, u32)>> = Default::default();
        for tri in welded.indices.chunks_exact(3) {
            let (a, b, c) = (positions[tri[0] as usize], positions[tri[1] as usize], positions[tri[2] as usize]);
            let n = (b - a).cross(c - a);
            if n.length() <= 0.0 {
                continue;
            }
            let n = n.normalized();
            for k in 0..3 {
                let (u, v) = (tri[k], tri[(k + 1) % 3]);
                let opposite = tri[(k + 2) % 3];
                let e = if u < v { (u, v) } else { (v, u) };
                per_edge.entry(e).or_default().push((n, opposite));
            }
        }
        let mut edges = std::collections::HashMap::with_capacity(per_edge.len());
        for (e, tris) in per_edge {
            if tris.len() != 2 {
                continue;
            }
            let (n0, opp0) = tris[0];
            let (n1, _) = tris[1];
            let cos = n0.dot(n1).clamp(-1.0, 1.0);
            let angle = fmath::acos(cos).to_degrees();
            // Convex when the far vertex of the first triangle sits behind the
            // second triangle's plane (on the material side of an outward
            // normal), and a fold of 0° counts as convex. No test on the
            // edge direction: `along` runs from the lower to the higher welded
            // id, not with the first triangle's winding, so its sign is a coin
            // toss.
            let d = positions[opp0 as usize] - positions[e.0 as usize];
            let convex = d.dot(n1) <= 0.0;
            edges.insert(e, (angle, convex));
        }
        DihedralMap { edges, cells, positions, eps }
    }

    fn vertex_at(&self, p: Vec3) -> Option<u32> {
        let k = ((p.x / self.eps).floor() as i64, (p.y / self.eps).floor() as i64, (p.z / self.eps).floor() as i64);
        let mut best: Option<(u32, f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(ids) = self.cells.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                        for &id in ids {
                            let d = (self.positions[id as usize] - p).length();
                            if d <= self.eps && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                                best = Some((id, d));
                            }
                        }
                    }
                }
            }
        }
        best.map(|(id, _)| id)
    }

    /// Representative dihedral for a polyline: the median over its segments,
    /// which ignores the odd degenerate segment at a seam.
    fn for_polyline(&self, poly: &[Vec3]) -> (f64, bool) {
        let mut angles: Vec<f64> = Vec::new();
        let mut convex_votes = 0i32;
        for w in poly.windows(2) {
            let (Some(a), Some(b)) = (self.vertex_at(w[0]), self.vertex_at(w[1])) else { continue };
            let e = if a < b { (a, b) } else { (b, a) };
            if let Some(&(deg, convex)) = self.edges.get(&e) {
                angles.push(deg);
                convex_votes += if convex { 1 } else { -1 };
            }
        }
        if angles.is_empty() {
            return (-1.0, true);
        }
        angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (angles[angles.len() / 2], convex_votes >= 0)
    }
}

/// [`FaceInfo::v_dir`]: the unit `∂S/∂v` at the middle of the surface's
/// parameter domain, or zero when the domain is unbounded or the
/// derivative vanishes there (a degenerate corner).
fn v_direction(s: &truck_modeling::Surface) -> Vec3 {
    use truck_modeling::ParametricSurface;
    let (ur, vr) = s.try_range_tuple();
    let (Some((u0, u1)), Some((v0, v1))) = (ur, vr) else { return Vec3::ZERO };
    if !(u0.is_finite() && u1.is_finite() && v0.is_finite() && v1.is_finite()) {
        return Vec3::ZERO;
    }
    let d = s.vder(0.5 * (u0 + u1), 0.5 * (v0 + v1));
    let d = Vec3::new(d.x, d.y, d.z);
    if d.x.is_finite() && d.y.is_finite() && d.z.is_finite() && d.length() > 1e-9 { d.normalized() } else { Vec3::ZERO }
}

/// Tessellate a solid into a mesh and derive its named topology.
pub fn tessellate(solid: &KSolid, tol: f64) -> crate::Result<(Mesh, Topology)> {
    tessellate_reporting(solid, tol).map(|(mb, _)| mb)
}

/// [`tessellate`] with every bound spelled out ([`kernel::TessellationOptions`]).
pub fn tessellate_with(solid: &KSolid, options: &kernel::TessellationOptions) -> crate::Result<(Mesh, Topology)> {
    tessellate_reporting_with(solid, options).map(|(mb, _, _)| mb)
}

/// [`tessellate`], plus the tolerance the mesh was made at
/// ([`kernel::Tessellation::tolerance`]).
pub fn tessellate_reporting(solid: &KSolid, tol: f64) -> crate::Result<((Mesh, Topology), f64)> {
    tessellate_reporting_with(solid, &kernel::TessellationOptions::new(tol)).map(|(mb, t, _)| (mb, t))
}

/// [`tessellate_reporting`] with every bound spelled out, plus the chord
/// error the kernel measured on the mesh ([`kernel::ChordError`]; `None`
/// when no face had a surface to measure against).
pub fn tessellate_reporting_with(
    solid: &KSolid,
    options: &kernel::TessellationOptions,
) -> crate::Result<((Mesh, Topology), f64, Option<kernel::ChordError>)> {
    let tol = options.tolerance;
    let kernel::Tessellation { meshed, tolerance, chord } = kernel::tessellate_reporting_with(solid, options)?;
    let mut mesh = Mesh::default();
    let mut topo = Topology::default();

    let orig_faces: Vec<_> = solid.face_iter().collect();

    for (fi, face) in meshed.faces.iter().enumerate() {
        let name = format!("face:{fi}");
        // The class is measured on the *oriented* surface, so a recognised
        // plane's normal is the face's outward one (see `surface_class`).
        // A face the classifier cannot name keeps that oriented surface
        // itself, for the mesh kernel's normals.
        let (surface_kind, surface, v_dir, geometry) = orig_faces
            .get(fi)
            .map(|f| {
                let oriented = f.oriented_surface();
                let class = surface_class::classify(&oriented, surface_class::EXACT_TOL);
                let v_dir = v_direction(&oriented);
                let geometry = (!class.is_recognised()).then(|| Arc::new(oriented));
                (kernel::surface_kind(&f.surface()), class, v_dir, geometry)
            })
            .unwrap_or(("unknown", SurfaceClass::Other, Vec3::ZERO, None));
        let tri_start = mesh.triangle_count() as u32;
        let base = mesh.vertex_count() as u32;
        let mut area = 0.0;
        let mut centroid = Vec3::ZERO;
        let mut normal_acc = Vec3::ZERO;
        // One mesh vertex per face vertex: the shared position, the face's
        // own normal there. Two faces meeting at an edge share the position
        // by id in `meshed` and split here, as the renderers want.
        for v in &face.vertices {
            let p = meshed.positions[v.id as usize];
            mesh.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
            mesh.normals.extend_from_slice(&[v.normal.x as f32, v.normal.y as f32, v.normal.z as f32]);
        }
        for tri in &face.triangles {
            let ids = [base + tri[0], base + tri[1], base + tri[2]];
            let a = kernel::point_to_vec3(meshed.positions[face.vertices[tri[0] as usize].id as usize]);
            let b = kernel::point_to_vec3(meshed.positions[face.vertices[tri[1] as usize].id as usize]);
            let c = kernel::point_to_vec3(meshed.positions[face.vertices[tri[2] as usize].id as usize]);
            // Already wound outward.
            let n = (b - a).cross(c - a);
            let tri_area = n.length() / 2.0;
            // A triangle with no height whose corners the weld would merge
            // is not part of any surface. The tessellator drops every pole
            // sliver by id already; this is the last guard, so no consumer
            // sees a self-edge in the welded mesh. A nanometre is far below
            // any tolerance the tessellator accepts, so no real sliver is
            // lost. A *needle* is kept: three distinct samples of one
            // straight edge that the triangulation's snapped grid made a
            // triangle of, height zero in f64's last bits and every side
            // longer than the weld (a hip roof at 4:12 on the exact
            // kernel: three samples of a hip within a millimetre). Its two
            // sides along the edge are the partners of the neighbouring
            // face's triangles, so dropping it left a hole of three edges.
            let shortest = (b - a).length().min((c - b).length()).min((a - c).length());
            if crate::mesh_clean::min_altitude(a, b, c) < 1e-9 && shortest < f64::from(crate::mesh_kernel::WELD_EPS) {
                continue;
            }
            area += tri_area;
            centroid = centroid + (a + b + c) * (tri_area / 3.0);
            normal_acc = normal_acc + n;
            mesh.indices.extend_from_slice(&ids);
            mesh.face_ids.push(fi as u32);
        }
        let tri_count = mesh.triangle_count() as u32 - tri_start;
        if area > 0.0 {
            centroid = centroid * (1.0 / area);
        }
        topo.faces.push(FaceInfo {
            name,
            aliases: Vec::new(),
            surface_kind,
            surface,
            geometry,
            centroid,
            normal: normal_acc.normalized(),
            area,
            tri_start,
            tri_count,
            edges: Vec::new(),
            edge_ids: Vec::new(),
            v_dir,
        });
    }

    // Edges and vertices: walk faces so we can record which faces bound them
    // (that is what names them); an edge's polyline runs the way the first
    // face that uses it does, from the shared position table.
    let mut seen: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    let mut seen_vertices: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for (fi, face) in meshed.faces.iter().enumerate() {
        for wire in face.oriented_wires() {
            for use_ in &wire {
                let edge = &meshed.edges[use_.edge as usize];
                let idx = match seen.get(&use_.edge) {
                    Some(&i) => i,
                    None => {
                        let mut polyline: Vec<Vec3> =
                            edge.ids.iter().map(|&id| kernel::point_to_vec3(meshed.positions[id as usize])).collect();
                        if !use_.forward {
                            polyline.reverse();
                        }
                        let length = polyline.windows(2).map(|w| (w[1] - w[0]).length()).sum();
                        let i = topo.edges.len();
                        topo.edges.push(EdgeInfo {
                            name: Kind::Edge.index_name(i),
                            curve_kind: edge.kind,
                            start: polyline.first().copied().unwrap_or(Vec3::ZERO),
                            end: polyline.last().copied().unwrap_or(Vec3::ZERO),
                            length,
                            polyline,
                            faces: Vec::new(),
                            face_ids: Vec::new(),
                            dihedral_degrees: -1.0,
                            convex: true,
                        });
                        seen.insert(use_.edge, i);
                        i
                    }
                };
                let e = &mut topo.edges[idx];
                if !e.face_ids.contains(&(fi as u32)) {
                    e.face_ids.push(fi as u32);
                }
                let f = &mut topo.faces[fi];
                if !f.edge_ids.contains(&(idx as u32)) {
                    f.edge_ids.push(idx as u32);
                }
                let ends = if use_.forward { [edge.front, edge.back] } else { [edge.back, edge.front] };
                for v in ends {
                    let next = topo.vertices.len();
                    let vi = *seen_vertices.entry(v).or_insert_with(|| {
                        topo.vertices.push(VertexInfo {
                            name: Kind::Vertex.index_name(next),
                            position: kernel::point_to_vec3(meshed.positions[meshed.vertices[v as usize] as usize]),
                            faces: Vec::new(),
                            face_ids: Vec::new(),
                        });
                        next
                    });
                    let vert = &mut topo.vertices[vi];
                    if !vert.face_ids.contains(&(fi as u32)) {
                        vert.face_ids.push(fi as u32);
                    }
                }
            }
        }
    }

    // Crease angles: computed from the triangles that share each mesh edge, so
    // a seam between two tangent spline patches reads as 0 degrees while a box
    // corner reads as 90.
    {
        let dm = DihedralMap::build(&mesh, (tol.max(1e-6)) * 0.5);
        for e in &mut topo.edges {
            let (deg, convex) = dm.for_polyline(&e.polyline);
            e.dihedral_degrees = deg;
            e.convex = convex;
        }
    }

    // Names still say `face:N` here: `tessellate` does not know what feature
    // made this solid. `Context` hands the result to `cadpole_core::naming` and
    // calls `Topology::apply_names`, which also rederives every edge and
    // vertex name. Callers that never do keep the index form, which resolution
    // still accepts.
    topo.rederive_names();
    Ok(((mesh, topo), tolerance, chord))
}

#[cfg(test)]
mod tests {

    /// Two quads sharing an edge, each with its own vertices and a flat
    /// normal, folded by `degrees` about the y axis.
    fn folded_strip(degrees: f64) -> Mesh {
        let a = degrees.to_radians();
        let (c, s) = (fmath::cos(a) as f32, fmath::sin(a) as f32);
        let mut m = Mesh::default();
        // Left quad in the xy plane; right quad rises from the shared edge x=0.
        let left = [[-1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 1.0, 0.0]];
        let right = [[0.0, 0.0, 0.0], [c, 0.0, s], [c, 1.0, s], [0.0, 1.0, 0.0]];
        for (quad, n) in [(left, [0.0, 0.0, 1.0]), (right, [-s, 0.0, c])] {
            let base = m.vertex_count() as u32;
            for p in quad {
                m.positions.extend_from_slice(&p);
                m.normals.extend_from_slice(&n);
            }
            m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            m.face_ids.extend_from_slice(&[base / 4, base / 4]);
        }
        m
    }

    fn normal_at(m: &Mesh, v: usize) -> Vec3 {
        Vec3::new(m.normals[v * 3] as f64, m.normals[v * 3 + 1] as f64, m.normals[v * 3 + 2] as f64)
    }

    #[test]
    fn smooth_seams_blends_a_shallow_fold_and_keeps_a_corner() {
        // 10 degrees: the four vertices on the shared edge (1, 2 on the left
        // quad; 4, 7 on the right) all end up on the bisector, and the outer
        // vertices keep their face normal.
        let mut m = folded_strip(10.0);
        let flat_left = normal_at(&m, 0);
        let flat_right = normal_at(&m, 5);
        m.smooth_seams(30.0);
        let bisector = (flat_left + flat_right).normalized();
        for v in [1, 2, 4, 7] {
            let n = normal_at(&m, v);
            assert!(n.dot(bisector) > 0.9999, "seam vertex {v}: {n:?} vs {bisector:?}");
        }
        assert!(normal_at(&m, 0).dot(flat_left) > 0.9999);
        assert!(normal_at(&m, 5).dot(flat_right) > 0.9999);
        assert_eq!(m.normals.len(), m.positions.len());

        // 90 degrees is a box corner: nothing moves.
        let mut m = folded_strip(90.0);
        let before = m.normals.clone();
        m.smooth_seams(30.0);
        for (a, b) in before.iter().zip(&m.normals) {
            assert!((a - b).abs() < 1e-6, "corner normals changed: {before:?} -> {:?}", m.normals);
        }

        // The threshold is a threshold: a 40 degree fold is a crease at 30
        // and a seam at 45.
        let mut m = folded_strip(40.0);
        let before = m.normals.clone();
        m.smooth_seams(30.0);
        for (a, b) in before.iter().zip(&m.normals) {
            assert!((a - b).abs() < 1e-6, "40 degree fold moved at 30: {before:?} -> {:?}", m.normals);
        }
        m.smooth_seams(45.0);
        assert!((normal_at(&m, 1) - normal_at(&m, 4)).length() < 1e-6);

        // A fold of exactly the threshold is a crease, as the line overlay
        // draws it, whichever side of the cosine f32 rounding puts it:
        // a standard 30 degree chamfer keeps its edge at 30.
        for degrees in [30.0, 30.0 + 1e-6, 30.0 - 1e-6, 30.02] {
            let mut m = folded_strip(degrees);
            let before = m.normals.clone();
            m.smooth_seams(30.0);
            for (a, b) in before.iter().zip(&m.normals) {
                assert!((a - b).abs() < 1e-6, "a {degrees} degree fold moved at 30: {before:?} -> {:?}", m.normals);
            }
        }
        let mut m = folded_strip(29.9);
        m.smooth_seams(30.0);
        assert!((normal_at(&m, 1) - normal_at(&m, 4)).length() < 1e-6, "a fold just under the threshold is a seam");

        // No normals at all: area-weighted ones are computed, then smoothed.
        let mut m = folded_strip(10.0);
        m.normals.clear();
        m.smooth_seams(30.0);
        assert_eq!(m.normals.len(), m.positions.len());
        assert!(normal_at(&m, 1).dot(bisector) > 0.9999);
    }
    use super::*;
    use crate::geom::Plane;
    use crate::kernel::BoolOp;
    use crate::sketch::{Loop, P2, Sketch};

    /// A tiny deterministic PRNG so property tests need no dependency.
    struct Lcg(u64);
    impl Lcg {
        fn next_f64(&mut self, lo: f64, hi: f64) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u = ((self.0 >> 11) as f64) / ((1u64 << 53) as f64);
            lo + (hi - lo) * u
        }
    }

    fn extruded(plane: Plane, l: Loop, h: f64) -> Mesh {
        let solid = kernel::extrude(&Sketch::new(plane).with_loop(l), h).unwrap().remove(0);
        tessellate(&solid, 0.02).unwrap().0
    }

    /// The edge table names each edge's curve kind from the solid it was
    /// tessellated from, keyed by the original edge ids and not the meshed
    /// solid's fresh ones, so `edges(kind="line")` on a box finds its edges.
    #[test]
    fn edge_kinds_come_from_the_exact_solid_not_the_polyline() {
        let plane = Plane::XY;
        let sk = Sketch { plane, loops: vec![Loop::rect(P2::new(0.0, 0.0), 20.0, 10.0)], loop_names: vec![], paths: vec![], path_names: vec![] };
        let bx = kernel::extrude(&sk, 5.0).unwrap();
        let (_, topo) = tessellate(&bx[0], 0.05).unwrap();
        assert_eq!(topo.edges.len(), 12);
        assert!(topo.edges.iter().all(|e| e.curve_kind == "line"), "{:?}", topo.edges.iter().map(|e| e.curve_kind).collect::<Vec<_>>());

        let sk = Sketch { plane, loops: vec![Loop::circle(P2::new(0.0, 0.0), 6.0)], loop_names: vec![], paths: vec![], path_names: vec![] };
        let rod = kernel::extrude(&sk, 5.0).unwrap();
        let (_, topo) = tessellate(&rod[0], 0.05).unwrap();
        let kinds: Vec<_> = topo.edges.iter().map(|e| e.curve_kind).collect();
        assert!(kinds.iter().any(|k| *k == "line"), "the seam is a line: {kinds:?}");
        assert!(kinds.iter().any(|k| *k == "nurbs" || *k == "bspline"), "the rims are arcs: {kinds:?}");
        assert!(!kinds.contains(&"polyline"), "{kinds:?}");
    }

    #[test]
    fn crease_angles_separate_corners_from_smooth_seams() {
        // Box: every edge is a 90-degree convex corner.
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 20.0));
        let solid = kernel::extrude(&s, 5.0).unwrap().remove(0);
        let (_, topo) = tessellate(&solid, 0.05).unwrap();
        for e in &topo.edges {
            assert!((e.dihedral_degrees - 90.0).abs() < 1.0, "{}: {}", e.name, e.dihedral_degrees);
            assert!(e.convex, "{} should be convex", e.name);
        }
        // Cylinder: the vertical seam between the two half-cylinder faces is
        // tangent (~0), while the rims are ~90.
        let c = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 8.0));
        let solid = kernel::extrude(&c, 10.0).unwrap().remove(0);
        let (_, topo) = tessellate(&solid, 0.02).unwrap();
        let smooth: Vec<_> = topo.edges.iter().filter(|e| e.dihedral_degrees >= 0.0 && e.dihedral_degrees < 10.0).collect();
        let sharp: Vec<_> = topo.edges.iter().filter(|e| e.dihedral_degrees >= 45.0).collect();
        assert!(!smooth.is_empty(), "cylinder should have tangent seams: {:?}", topo.edges.iter().map(|e| e.dihedral_degrees).collect::<Vec<_>>());
        assert!(!sharp.is_empty(), "cylinder should have sharp rims");
        // A hole's inner wall is concave.
        let h = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0))
            .with_loop(Loop::circle(P2::new(0.0, 0.0), 4.0));
        let solid = kernel::extrude(&h, 6.0).unwrap().remove(0);
        let (_, topo) = tessellate(&solid, 0.02).unwrap();
        assert!(topo.edges.iter().any(|e| !e.convex), "the hole rim should have concave edges");
    }

    #[test]
    fn property_extrude_volume_is_area_times_height() {
        let mut rng = Lcg(42);
        for _ in 0..20 {
            let (w, d, h) = (rng.next_f64(1.0, 50.0), rng.next_f64(1.0, 50.0), rng.next_f64(0.5, 40.0));
            let m = extruded(Plane::XY, Loop::rect(P2::new(rng.next_f64(-10.0, 10.0), rng.next_f64(-10.0, 10.0)), w, d), h);
            let v = m.volume();
            assert!((v - w * d * h).abs() < 1e-6 * w * d * h + 1e-6, "box {w}x{d}x{h}: volume {v}");
            assert!(m.welded(1e-4).is_watertight(), "box mesh not watertight");
        }
        for _ in 0..10 {
            let (r, h) = (rng.next_f64(1.0, 30.0), rng.next_f64(0.5, 40.0));
            let m = extruded(Plane::XY, Loop::circle(P2::new(0.0, 0.0), r), h);
            let expect = std::f64::consts::PI * r * r * h;
            // A tessellated circle is an inscribed polygon: each half-turn
            // arc is halved until its sagitta is under the tolerance,
            // so the area deficit is
            // `1 - sin(θ)/θ` for `θ = 2π/n` with `n` the two arcs' pieces.
            let tol = 0.02;
            let mut k = 2.0f64;
            while r * (1.0 - fmath::cos(0.5 * std::f64::consts::PI / k)) > tol {
                k *= 2.0;
            }
            let n = 2.0 * k;
            let theta = 2.0 * std::f64::consts::PI / n;
            let deficit = 1.0 - fmath::sin(theta) / theta;
            let v = m.volume();
            assert!(v <= expect * (1.0 + 1e-6) && v >= expect * (1.0 - deficit - 1e-4), "cylinder r={r} h={h}: {v} vs {expect}, deficit bound {deficit}");
            assert!(m.welded(1e-4).is_watertight(), "cylinder mesh not watertight (r={r} h={h})");
        }
    }

    #[test]
    fn property_boolean_volumes_add_up() {
        // vol(A ∪ B) + vol(A ∩ B) == vol(A) + vol(B) for overlapping boxes.
        let mut rng = Lcg(7);
        for _ in 0..6 {
            let a = kernel::extrude(&Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0)), 10.0).unwrap().remove(0);
            let off = P2::new(rng.next_f64(3.0, 12.0), rng.next_f64(3.0, 12.0));
            let z0 = rng.next_f64(-4.0, 4.0);
            let b = kernel::extrude(&Sketch::new(Plane::XY.offset(z0)).with_loop(Loop::rect(off, 15.0, 12.0)), 8.0).unwrap().remove(0);
            let va = tessellate(&a, 0.02).unwrap().0.volume();
            let vb = tessellate(&b, 0.02).unwrap().0.volume();
            let u = kernel::boolean(BoolOp::Union, &a, &b, 0.05).unwrap();
            let i = kernel::boolean(BoolOp::Intersect, &a, &b, 0.05).unwrap();
            let vu = tessellate(&u, 0.02).unwrap().0.volume();
            let vi = tessellate(&i, 0.02).unwrap().0.volume();
            assert!(((vu + vi) - (va + vb)).abs() < 1e-3 * (va + vb), "union {vu} + inter {vi} != {va} + {vb} (offset {off:?}, z0 {z0})");
            let d = kernel::boolean(BoolOp::Subtract, &a, &b, 0.05).unwrap();
            let vd = tessellate(&d, 0.02).unwrap().0.volume();
            assert!((vd - (va - vi)).abs() < 1e-3 * va, "subtract {vd} != {va} - {vi}");
        }
    }

    #[test]
    fn revolved_circle_has_torus_volume() {
        let (big_r, r) = (20.0, 4.0);
        let s = Sketch::new(Plane::XZ).with_loop(Loop::circle(P2::new(big_r, 0.0), r));
        let solid = kernel::revolve(&s, Vec3::ZERO, Vec3::Z, std::f64::consts::TAU).unwrap().remove(0);
        let m = tessellate(&solid, 0.01).unwrap().0;
        let expect = 2.0 * std::f64::consts::PI * std::f64::consts::PI * big_r * r * r;
        let v = m.volume().abs();
        assert!(((v - expect) / expect).abs() < 0.02, "torus volume {v} vs {expect}");
    }

    #[test]
    fn box_mesh_has_twelve_triangles_and_named_topology() {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 20.0));
        let solid = kernel::extrude(&s, 5.0).unwrap().remove(0);
        let (mesh, topo) = tessellate(&solid, 0.05).unwrap();
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(mesh.face_ids.len(), 12);
        assert_eq!(topo.faces.len(), 6);
        assert_eq!(topo.edges.len(), 12);
        assert_eq!(topo.vertices.len(), 8);
        // Every face normal should be axis aligned and unit length.
        for f in &topo.faces {
            assert!((f.normal.length() - 1.0).abs() < 1e-6, "{:?}", f);
            let m = f.normal.x.abs().max(f.normal.y.abs()).max(f.normal.z.abs());
            assert!((m - 1.0).abs() < 1e-6, "{:?}", f.normal);
            assert_eq!(f.edges.len(), 4);
        }
        let top = topo.faces.iter().find(|f| f.normal.z > 0.9).expect("top face");
        assert!((top.area - 200.0).abs() < 1e-6);
        assert!((top.centroid.z - 5.0).abs() < 1e-6);
        // The bounding box matches the extrusion.
        let (lo, hi) = mesh.bbox().unwrap();
        assert!((lo.x + 5.0).abs() < 1e-5 && (hi.x - 5.0).abs() < 1e-5);
        assert!((lo.z).abs() < 1e-5 && (hi.z - 5.0).abs() < 1e-5);
    }
}
