//! Mesh to field: a narrow-band signed distance to a triangle soup.
//!
//! This is what lets an existing body be rounded. A B-rep body is tessellated
//! (`Body::to_mesh`), a mesh body already is one, and either way the triangles
//! go into a [`crate::bvh::Bvh`]; a query returns the exact distance to the
//! nearest triangle, signed by the **angle-weighted pseudo-normal** of the
//! feature that turned out to be nearest (Baerentzen & Aanaes, TVCG 2005).
//!
//! The pseudo-normal matters. Taking the sign from the nearest *triangle's*
//! normal is wrong whenever the closest point is on an edge or a vertex —
//! which is most of a box — and gives a field that flips sign along the
//! diagonal bisector of every edge. Weighting a vertex's normal by the
//! incident angles, and an edge's by its two faces, makes the test exact for
//! any closed, consistently oriented surface, and only there: on an open,
//! inside-out or self-crossing mesh the inside is not defined, which is why
//! [`crate::field_from_mesh`] validates its input and refuses those. Ray
//! parity along +X is the backstop only for the degenerate query whose offset
//! is perpendicular to every normal at hand.
//!
//! The distance is exact at every range. An earlier version clamped it to a
//! band of 1.05 x the body's diagonal, on the argument that the mesher only
//! needs signs; that holds for meshing the raw field and fails the moment an
//! offset, a shell or a smooth blend is composed on top, all of which read
//! the value: `from_body(box(2,2,2)).offset(4)` was empty, and a smooth union
//! inflated a sphere 12 mm away. A query far from the body costs one walk
//! down the BVH's nearest boxes, so the cap bought little. [`MeshField::with_band`]
//! still narrows it on request, for a caller that does only want signs.

use std::collections::HashMap;

use cadpole_core::tessellate::Mesh;

use crate::bvh::{Bvh, Site, Tri};
use crate::edges::{add, cross, dot, mul, norm, sub};
use cadpole_core::fmath;

/// A body imported as a signed distance field.
pub struct MeshField {
    bvh: Bvh,
    /// Welded vertex positions.
    verts: Vec<[f64; 3]>,
    /// Corner indices per original triangle.
    tri_verts: Vec<[u32; 3]>,
    /// Which named face group each triangle came from. This is the boolean
    /// kernel's own provenance, and it is what tells the fillet which side of
    /// a seam a triangle belongs to when the seam is littered with slivers
    /// whose own normals say nothing useful.
    tri_group: Vec<u32>,
    /// Unit face normal per original triangle.
    tri_normal: Vec<[f64; 3]>,
    /// Angle-weighted pseudo-normal per welded vertex.
    vert_normal: Vec<[f64; 3]>,
    /// Sum of the adjacent face normals, per undirected welded edge.
    edge_normal: HashMap<(u32, u32), [f64; 3]>,
    lo: [f64; 3],
    hi: [f64; 3],
    /// Distances are clamped to `+/- band`; infinite unless
    /// [`MeshField::with_band`] narrowed it.
    band: f64,
    /// False when some edge is not shared by exactly two triangles: the sign
    /// test then falls back to ray parity.
    closed: bool,
}

impl std::fmt::Debug for MeshField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshField")
            .field("triangles", &self.tri_verts.len())
            .field("vertices", &self.verts.len())
            .field("closed", &self.closed)
            .field("band", &self.band)
            .finish()
    }
}

/// Weld distance, in mm. The same 0.1 um the mesh kernel uses, so a body that
/// passes `to_manifold` welds identically here.
pub const WELD_EPS: f64 = 1e-4;

impl MeshField {
    /// Import one of Cadpole's render meshes. The mesh is welded by position
    /// first, because the tessellator emits one vertex per (position, normal)
    /// corner and the pseudo-normals need real adjacency.
    pub fn from_mesh(mesh: &Mesh) -> MeshField {
        let mut verts: Vec<[f64; 3]> = Vec::new();
        let mut cells: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
        let mut remap: Vec<u32> = Vec::with_capacity(mesh.vertex_count());
        let q = |v: f64| (v / WELD_EPS).floor() as i64;
        for p in mesh.positions.chunks_exact(3) {
            let p = [p[0] as f64, p[1] as f64, p[2] as f64];
            let key = (q(p[0]), q(p[1]), q(p[2]));
            let mut found = None;
            'search: for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(ids) = cells.get(&(key.0 + dx, key.1 + dy, key.2 + dz)) {
                            for &id in ids {
                                let d = sub(verts[id as usize], p);
                                if dot(d, d) <= WELD_EPS * WELD_EPS {
                                    found = Some(id);
                                    break 'search;
                                }
                            }
                        }
                    }
                }
            }
            let id = found.unwrap_or_else(|| {
                verts.push(p);
                let id = verts.len() as u32 - 1;
                cells.entry(key).or_default().push(id);
                id
            });
            remap.push(id);
        }
        let mut tri_verts = Vec::with_capacity(mesh.triangle_count());
        let mut tri_group = Vec::with_capacity(mesh.triangle_count());
        for (i, t) in mesh.indices.chunks_exact(3).enumerate() {
            let m = [remap[t[0] as usize], remap[t[1] as usize], remap[t[2] as usize]];
            if m[0] == m[1] || m[1] == m[2] || m[0] == m[2] {
                continue;
            }
            tri_verts.push(m);
            tri_group.push(mesh.face_ids.get(i).copied().unwrap_or(0));
        }
        MeshField::from_grouped(verts, tri_verts, tri_group)
    }

    /// Build from an already-welded indexed mesh with no face groups.
    pub fn from_indexed(verts: Vec<[f64; 3]>, tri_verts: Vec<[u32; 3]>) -> MeshField {
        let n = tri_verts.len();
        MeshField::from_grouped(verts, tri_verts, vec![0; n])
    }

    /// Build from an already-welded indexed mesh, keeping its face groups.
    pub fn from_grouped(
        verts: Vec<[f64; 3]>,
        tri_verts: Vec<[u32; 3]>,
        tri_group: Vec<u32>,
    ) -> MeshField {
        let mut tri_normal = Vec::with_capacity(tri_verts.len());
        let mut vert_normal = vec![[0.0f64; 3]; verts.len()];
        let mut edge_normal: HashMap<(u32, u32), [f64; 3]> = HashMap::new();
        let mut edge_uses: HashMap<(u32, u32), u32> = HashMap::new();
        let mut tris = Vec::with_capacity(tri_verts.len());
        for (ti, t) in tri_verts.iter().enumerate() {
            let p = [verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]];
            let raw = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            let n = norm(raw);
            tri_normal.push(n);
            tris.push(Tri { p, index: ti as u32 });
            for k in 0..3 {
                // Angle-weighted vertex normal: the weight that makes the
                // pseudo-normal test exact.
                let (u, v) = (
                    sub(p[(k + 1) % 3], p[k]),
                    sub(p[(k + 2) % 3], p[k]),
                );
                let angle = {
                    let c = dot(norm(u), norm(v)).clamp(-1.0, 1.0);
                    fmath::acos(c)
                };
                let vi = t[k] as usize;
                vert_normal[vi] = add(vert_normal[vi], mul(n, angle));
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = if a < b { (a, b) } else { (b, a) };
                let e = edge_normal.entry(key).or_insert([0.0; 3]);
                *e = add(*e, n);
                *edge_uses.entry(key).or_insert(0) += 1;
            }
        }
        for n in vert_normal.iter_mut() {
            *n = norm(*n);
        }
        for n in edge_normal.values_mut() {
            *n = norm(*n);
        }
        let closed = !edge_uses.is_empty() && edge_uses.values().all(|&c| c == 2);
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in &verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        MeshField {
            bvh: Bvh::build(tris),
            verts,
            tri_verts,
            tri_group,
            tri_normal,
            vert_normal,
            edge_normal,
            lo,
            hi,
            band: f64::INFINITY,
            closed,
        }
    }

    /// Narrow the band. Values beyond it are clamped to `+/- band`, which
    /// makes distant queries cheap and is fine for meshing the raw field (the
    /// mesher only needs signs away from the surface) but wrong under an
    /// offset, shell or smooth blend, which read the value.
    pub fn with_band(mut self, band: f64) -> MeshField {
        let diag = (0..3).map(|k| fmath::powi(self.hi[k] - self.lo[k], 2)).sum::<f64>().sqrt();
        self.band = band.max(1e-6).min(diag * 1.05).max(1e-6);
        self
    }

    pub fn triangle_count(&self) -> usize {
        self.tri_verts.len()
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        (self.lo, self.hi)
    }

    /// Signed distance: negative inside the solid.
    pub fn signed_distance(&self, p: [f64; 3]) -> f64 {
        // Outside the bounding box by more than the band: certainly outside.
        let mut out2 = 0.0;
        for k in 0..3 {
            let d = (self.lo[k] - p[k]).max(0.0).max(p[k] - self.hi[k]);
            out2 += d * d;
        }
        if out2 > self.band * self.band {
            return self.band;
        }
        // Cap the search at the band only when a miss would *prove* the point
        // is outside (it is already outside the bounding box). Inside the box
        // the sign has to come from the nearest triangle, so the query runs
        // uncapped and the magnitude is clamped afterwards.
        let cap = if out2 > 0.0 { self.band } else { f64::INFINITY };
        let Some(c) = self.bvh.closest(p, cap) else {
            // Nothing within the cap (only possible with a narrowed band), or
            // no triangles at all: outside.
            return if self.bvh.tris.is_empty() { self.band.min(1.0) } else { self.band };
        };
        let d = c.dist2.sqrt().min(self.band);
        if d <= 0.0 {
            return 0.0;
        }
        let tri = &self.bvh.tris[c.tri as usize];
        let orig = tri.index as usize;
        let v = sub(p, c.point);
        for n in [self.pseudo_normal(orig, c.site), self.tri_normal[orig]] {
            let s = dot(v, n);
            if s.abs() > 1e-12 * d.max(1.0) {
                return if s >= 0.0 { d } else { -d };
            }
        }
        // Degenerate: the offset is perpendicular to every normal we have.
        // Ray parity is the backstop, and when even that grazes, "outside".
        match self.bvh.ray_crossings(p) {
            Some(h) if h % 2 == 1 => -d,
            _ => d,
        }
    }

    fn pseudo_normal(&self, tri: usize, site: Site) -> [f64; 3] {
        match site {
            Site::Face => self.tri_normal[tri],
            Site::Vertex(i) => {
                let v = self.tri_verts[tri][i as usize] as usize;
                let n = self.vert_normal[v];
                if dot(n, n) > 0.5 { n } else { self.tri_normal[tri] }
            }
            Site::Edge(i) => {
                let t = self.tri_verts[tri];
                let (a, b) = (t[i as usize], t[(i as usize + 1) % 3]);
                let key = if a < b { (a, b) } else { (b, a) };
                match self.edge_normal.get(&key) {
                    Some(n) if dot(*n, *n) > 0.5 => *n,
                    _ => self.tri_normal[tri],
                }
            }
        }
    }

    /// Exposed for the fillet's edge extraction, which needs the same welded
    /// adjacency this already built: positions, triangles, unit face normals,
    /// and the face group each triangle came from.
    pub fn welded(&self) -> (&[[f64; 3]], &[[u32; 3]], &[[f64; 3]], &[u32]) {
        (&self.verts, &self.tri_verts, &self.tri_normal, &self.tri_group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An axis-aligned box as a welded indexed mesh, outward-facing.
    pub(crate) fn box_mesh(h: [f64; 3]) -> MeshField {
        let mut verts = Vec::new();
        for i in 0..8 {
            verts.push([
                if i & 1 == 0 { -h[0] } else { h[0] },
                if i & 2 == 0 { -h[1] } else { h[1] },
                if i & 4 == 0 { -h[2] } else { h[2] },
            ]);
        }
        // Each face as two CCW-from-outside triangles.
        let faces: [[u32; 4]; 6] = [
            [0, 2, 3, 1], // -z
            [4, 5, 7, 6], // +z
            [0, 1, 5, 4], // -y
            [2, 6, 7, 3], // +y
            [0, 4, 6, 2], // -x
            [1, 3, 7, 5], // +x
        ];
        let mut tris = Vec::new();
        for f in faces {
            tris.push([f[0], f[1], f[2]]);
            tris.push([f[0], f[2], f[3]]);
        }
        MeshField::from_indexed(verts, tris)
    }

    #[test]
    fn a_box_field_matches_the_analytic_box_sdf_everywhere() {
        let h = [3.0, 2.0, 1.5];
        let m = box_mesh(h);
        assert!(m.is_closed());
        let sdf = |p: [f64; 3]| {
            let q = [p[0].abs() - h[0], p[1].abs() - h[1], p[2].abs() - h[2]];
            let out =
                (fmath::powi(q[0].max(0.0), 2) + fmath::powi(q[1].max(0.0), 2) + fmath::powi(q[2].max(0.0), 2)).sqrt();
            out + q[0].max(q[1]).max(q[2]).min(0.0)
        };
        let n = 13;
        for i in 0..=n {
            for j in 0..=n {
                for k in 0..=n {
                    let p = [
                        -5.0 + 10.0 * i as f64 / n as f64,
                        -5.0 + 10.0 * j as f64 / n as f64,
                        -5.0 + 10.0 * k as f64 / n as f64,
                    ];
                    let want = sdf(p);
                    let got = m.signed_distance(p);
                    assert!((got - want).abs() < 1e-9, "{p:?}: {got} != {want}");
                }
            }
        }
    }

    /// The case a naive "nearest triangle normal" sign test gets wrong: a
    /// point whose closest feature is an edge or a corner.
    #[test]
    fn the_sign_is_right_at_edges_and_corners() {
        let m = box_mesh([1.0, 1.0, 1.0]);
        for p in [
            [1.5, 1.5, 0.0],   // nearest point is an edge
            [1.5, 1.5, 1.5],   // nearest point is a corner
            [-2.0, -2.0, 2.0], // another corner
            [0.999, 0.999, 0.999],
        ] {
            let d = m.signed_distance(p);
            let inside = p.iter().all(|c| c.abs() < 1.0);
            assert_eq!(d < 0.0, inside, "{p:?} -> {d}");
        }
    }

    #[test]
    fn the_band_clamps_but_keeps_the_sign() {
        let m = box_mesh([1.0, 1.0, 1.0]).with_band(0.5);
        assert!((m.signed_distance([10.0, 0.0, 0.0]) - 0.5).abs() < 1e-12);
        assert!((m.signed_distance([1.2, 0.0, 0.0]) - 0.2).abs() < 1e-9);
        assert!(m.signed_distance([0.0, 0.0, 0.0]) < 0.0);
    }

    /// The distance is exact at any range, not clamped to a band about the
    /// size of the body: an offset, shell or smooth blend composed on top
    /// reads the value, not just its sign.
    #[test]
    fn the_distance_is_exact_far_from_the_body() {
        let m = box_mesh([1.0, 1.0, 1.0]);
        assert!((m.signed_distance([10.0, 0.0, 0.0]) - 9.0).abs() < 1e-9);
        assert!((m.signed_distance([5.0, 5.0, 5.0]) - (48.0f64).sqrt()).abs() < 1e-9);
        assert!((m.signed_distance([3.0, 0.5, 0.0]) - 2.0).abs() < 1e-9);
    }

    /// `field.from_body(box(2, 2, 2)).offset(4)` was empty (the clamp at
    /// 1.05 x the diagonal, 3.64 mm, is below 4), and a smooth union with a
    /// sphere 12 mm away inflated the sphere. Both now match the analytic box.
    #[test]
    fn an_offset_larger_than_the_body_meshes_like_the_analytic_one() {
        use crate::mdc::ContourOptions;
        use crate::Field;
        use std::sync::Arc;
        let from = Field::from_mesh_field(Arc::new(box_mesh([1.0, 1.0, 1.0])));
        let opts = ContourOptions { resolution: 0.25, ..Default::default() };
        let (m, _, _) = crate::mesh_field(&from.offset(4.0), &opts, "m").expect("the offset is not empty");
        let pi = std::f64::consts::PI;
        let (a, r) = (2.0, 4.0);
        let want = a * a * a + 6.0 * a * a * r + 3.0 * pi * a * r * r + 4.0 / 3.0 * pi * r * r * r;
        assert!((m.volume() - want).abs() / want < 0.01, "{} vs {want}", m.volume());

        let far = Field::sphere(3.0).translate([12.0, 0.0, 0.0]);
        let analytic = Field::boxed([1.0, 1.0, 1.0], 0.0).smooth_union(&far, 8.0).compile();
        let imported = from.smooth_union(&far, 8.0).compile();
        let (mut ea, mut ei) = (analytic.evaluator(), imported.evaluator());
        for p in [[15.0, 0.0, 0.0], [12.0, 3.0, 0.0], [20.0, 5.0, 5.0], [0.0, 0.0, 0.0]] {
            let (va, vi) = (ea.eval(p), ei.eval(p));
            assert!((va - vi).abs() < 1e-6, "{p:?}: imported {vi} vs analytic {va}");
        }
    }
}
