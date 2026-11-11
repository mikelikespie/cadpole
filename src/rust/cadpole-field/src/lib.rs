//! Cadpole field bodies: signed distance fields, and the fillets and chamfers
//! built on them.
//!
//! This crate owns four things:
//!
//! * [`Field`] — an SDF expression value (primitives, transforms, exact and
//!   blended booleans, offset/shell, twist/taper, displacement by noise), and
//!   [`tape`], the flat program it compiles into, evaluated either in plain
//!   `f64` at a point or with [`interval`] arithmetic over a box;
//! * [`mdc`] — manifold dual contouring: octree with interval culling, one
//!   QEF vertex per surface component of a cell clamped into that cell,
//!   adaptive collapse under an error threshold. The output is measured
//!   (closed, edge- and vertex-manifold, no coincident vertices, no crossing
//!   or overlapping triangles), repaired where it is not, and refused when
//!   the repair cannot fix it;
//! * [`mesh_sdf`] — the other direction: an existing B-rep or mesh body
//!   becomes a field through a BVH over its triangles plus angle-weighted
//!   pseudo-normals, so anything Cadpole can build can be rounded;
//! * [`fillet`] and [`edges`] — `fillet(body, edges=, radius=)` and
//!   `chamfer(body, edges=, distance=)`, implemented as a blend correction
//!   that is live only near the selected edges, so a radius on one edge does
//!   not round the whole body.
//!
//! It has no dependencies beyond `cadpole-core`, holds no global state, uses no
//! randomness, and builds for wasm32.
//!
//! ## What is approximate, and by how much
//!
//! Fillets and chamfers on a real body go through triangles and come back as
//! triangles, so the answer carries two errors, both reported in
//! [`fillet::FilletReport`]: the grid (about half a cell) and, on curved
//! faces only, the tessellation the tangent planes were read from. On planar
//! face pairs the blend itself is the exact rolling-ball surface — the
//! circular smooth-minimum is not an approximation there — so the only error
//! is the resampling.

pub mod bvh;
pub mod edges;
mod expr;
pub mod fillet;
pub mod interval;
pub mod mdc;
pub mod mesh_sdf;
pub mod tape;

pub use edges::{BlendEdge, BlendKind, EdgeBlendSet};
pub use expr::Field;
pub use fillet::{blend_body, default_resolution, EdgeSelection, FilletReport};
pub use interval::{Box3, Interval};
pub use mdc::{contour, ContourMesh, ContourOptions, ContourStats};
pub use mesh_sdf::MeshField;
pub use tape::Tape;

use std::sync::Arc;

use cadpole_core::tessellate::{Mesh, Topology};

/// Mesh a field into a Cadpole mesh body.
///
/// This is the one conversion from a `Field` value to geometry: the
/// caller says what resolution it wants, and gets back triangles plus a
/// single named face group. Meshing is deterministic, and the result is a
/// solid in the sense of `cadpole_core::solid::validate` -- closed, 2-manifold
/// at every edge and vertex, outward, and free of crossing or overlapping
/// triangles -- or an error that says what was wrong. That is what the mesh
/// kernel needs to boolean it against anything else.
pub fn mesh_field(
    field: &Field,
    opts: &ContourOptions,
    group_name: &str,
) -> Result<(Mesh, Topology, ContourStats), String> {
    let tape = field.compile();
    let out = contour(&tape, opts)?;
    if out.tris.is_empty() {
        return Err(
            "meshing produced no triangles: the field is empty at this resolution (nothing is \
             inside it, or every feature is smaller than one cell)"
                .to_owned(),
        );
    }
    // `contour` measures what it returns and refuses a surface that is not
    // a closed, embedded 2-manifold; this is the one property it does not
    // measure, because only the caller knows the field meant "inside" by
    // negative.
    let volume = out.volume();
    if !(volume > 0.0) {
        return Err(format!(
            "meshing produced a surface with volume {volume:.4} mm^3, which is inside out or \
             empty; the field must be negative inside the solid"
        ));
    }
    let (mesh, topo) = to_mesh_body(&out, group_name);
    Ok((mesh, topo, out.stats))
}

/// Import a body's triangles as a field.
///
/// The sign of the distance comes from the angle-weighted pseudo-normals,
/// which are right only on the boundary of a region: closed, wound
/// consistently and outward, and not passing through itself. On anything
/// else the inside is not defined -- an open mesh gives a sign that flips
/// across the gap, two overlapping shells one that flips inside the overlap,
/// an inside-out mesh a body of negative volume -- so the input is validated
/// with `cadpole_core::solid` first and refused, by what is wrong with it,
/// rather than meshed into nonsense.
pub fn field_from_mesh(mesh: &Mesh) -> Result<Field, String> {
    let report = cadpole_core::solid::validate(mesh);
    if !report.is_solid() {
        return Err(format!(
            "the body is not a solid, so it has no inside for a distance field to be negative \
             in: {}. Repair it first (import_mesh(..., repair=\"weld\") welds it and turns a \
             closed inside-out mesh outward; holes and overlapping shells have to be fixed in \
             the geometry)",
            report.problems().join("; ")
        ));
    }
    Ok(Field::from_mesh_field(Arc::new(MeshField::from_mesh(mesh))))
}

/// Flat mesh + one face group, the simple conversion the `mesh()` global uses.
fn to_mesh_body(out: &ContourMesh, group_name: &str) -> (Mesh, Topology) {
    use cadpole_core::geom::Vec3;
    use cadpole_core::tessellate::FaceInfo;

    let mut mesh = Mesh::default();
    for (p, n) in out.positions.iter().zip(&out.normals) {
        mesh.positions.extend_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32]);
        mesh.normals.extend_from_slice(&[n[0] as f32, n[1] as f32, n[2] as f32]);
    }
    let mut area = 0.0;
    let mut centroid = Vec3::ZERO;
    for t in &out.tris {
        mesh.indices.extend_from_slice(&[t[0], t[1], t[2]]);
        mesh.face_ids.push(0);
        let (a, b, c) =
            (out.positions[t[0] as usize], out.positions[t[1] as usize], out.positions[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let ta = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() * 0.5;
        area += ta;
        let cen = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0];
        centroid = centroid + Vec3::new(cen[0], cen[1], cen[2]) * ta;
    }
    let faces = vec![FaceInfo {
        name: group_name.to_owned(),
        surface_kind: "group",
        centroid: if area > 0.0 { centroid * (1.0 / area) } else { centroid },
        normal: Vec3::ZERO,
        area,
        tri_start: 0,
        tri_count: out.tris.len() as u32,
        edges: Vec::new(),
        ..Default::default()
    }];
    (mesh, Topology { faces, edges: Vec::new(), vertices: Vec::new() })
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod mesher_solid_tests;
