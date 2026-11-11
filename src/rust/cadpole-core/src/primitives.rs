//! The primitives and transforms that are not a sweep of a sketch: `mirror`,
//! `scale`, `sphere`, `torus` and `hull`.
//!
//! What each one is, and why it is built the way it is:
//!
//! * **`mirror`** is a reflection, and truck's `builder::transformed` does
//!   not know that a reflection turns a solid inside out. Every truck surface
//!   reports its *parametric* normal — a plane's `u × v`, a spline's
//!   derivative cross product, and `Processor` (the wrapper every revolved
//!   surface sits in) pushes the entity's normal through the inverse
//!   transpose and then divides by the determinant, so it too agrees with
//!   `du × dv` after the map. Under a matrix of negative determinant that
//!   direction is the reflection of the old *inward* normal: every face
//!   still says "outward" and every one of them points in. The tessellation
//!   then winds every triangle the wrong way, the volume comes out negative
//!   and the solid validator calls the body a void — the test
//!   `a_bare_reflection_is_inside_out_and_the_mirror_is_not` pins exactly
//!   that. So [`transformed`] maps the solid and then inverts every face:
//!   the boundary loops reverse, the oriented surfaces flip, and adjacent
//!   faces still traverse their shared edge in opposite directions, because
//!   the reflection did nothing to the combinatorics. On the mesh path the
//!   same rule is one swap per triangle ([`remapped_mesh_body`]). Names are
//!   the source's: a mirrored face is the same face, moved, so `blk/top` is
//!   still `blk/top` (as for `translate`).
//! * **`sphere`** and **`torus`** are surfaces of revolution, so their faces
//!   are exact `RevolutedCurve`s that [`crate::surface_class`] recognises
//!   as a sphere and a torus. The torus is a `revolve` of its tube circle,
//!   drawn as an outer and an inner arc split at the top and bottom of the
//!   tube, so it is `outer#0..2` and `inner#0..2` — three pieces each,
//!   because truck cuts a full turn into three 120° patches so that no face
//!   meets itself along a seam, the way a cylinder's wall is the group
//!   `side` of two. The sphere is *not* a `revolve`: its profile touches
//!   the axis, and every way of sweeping it leaves a pole on a face
//!   boundary, which truck's triangulator cannot map reliably (see
//!   [`sphere`]). It is two hemispherical caps sharing the equator, each
//!   with its pole inside, under the group name `surface` (`surface#0`,
//!   `surface#1`).
//! * **`scale`** is a plain affine map for truck: control points move and
//!   nothing else needs to. A uniform scale keeps every analytic class
//!   (radii scale with it); a non-uniform one turns a cylinder into an
//!   elliptic one, which is nothing [`SurfaceClass`] names, so it reports
//!   `Other` — recognised from the scaled surface, never assumed from the
//!   source's class. Normals move by the inverse transpose, not by the
//!   matrix, so a scaled mesh body still shades and validates correctly.
//! * **`hull`** is the convex hull of the operands' *tessellations*, by
//!   quickhull (manifold's, already in the tree), as a mesh body. The
//!   B-rep vertices alone would be wrong for anything curved — a sphere
//!   has two — and sampling the surfaces would just be a tessellation with
//!   a second tolerance; the document's own is the honest one, and it is
//!   what every other mesh-path result is accurate to. Coplanar triangles
//!   of the hull are one face (manifold's coplanar ids), every face is
//!   named `hull`, and the pieces take the grammar's `hull#k` under that
//!   group alias, exactly as the mesh path names a split face today.

use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::sync::Arc;

use manifold_rust::manifold::Manifold;
use truck_modeling::{builder, Face, Matrix4, Processor, RevolutedCurve, Shell, Solid, Surface, Transformed, Vector4, Wire};

use crate::context::{fmt_num, fmt_vec, Body, Context, NameSource, Span};
use crate::geom::{Plane, Transform, Vec3};
use crate::ids::BodyId;
use crate::kernel::{self, BoolOp, Disposition, KernelChoice, KSolid};
use crate::memo::{KeyBuilder, Produced};
use crate::mesh_kernel::{self, MeshBody, SourceFace};
use crate::naming::{self, NamedFace, Role, Style};
use crate::sketch::{Loop, Segment, Sketch, P2};
use crate::surface_class::{self, SurfaceClass};
use crate::tessellate::{EdgeInfo, FaceInfo, Mesh, Topology, VertexInfo};
use crate::{CadpoleError, Result};
use crate::fmath;

// ------------------------------------------------------------ transforms

/// The 3×3 linear part of `t`, row-major: `a[row][col]`.
fn linear(t: &Transform) -> [[f64; 3]; 3] {
    // cgmath stores columns: `m.y` is the second column, `m.y.x` its first row.
    let m = &t.0;
    [[m.x.x, m.y.x, m.z.x], [m.x.y, m.y.y, m.z.y], [m.x.z, m.y.z, m.z.z]]
}

fn det3(a: &[[f64; 3]; 3]) -> f64 {
    a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
}

/// The cofactor matrix: `cof(A) = det(A) · A⁻ᵀ`, which is how a normal
/// moves under `A` (up to scale). Cyclic minors carry the sign.
fn cofactor(a: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let c = |i: usize, j: usize| {
        let (i1, i2) = ((i + 1) % 3, (i + 2) % 3);
        let (j1, j2) = ((j + 1) % 3, (j + 2) % 3);
        a[i1][j1] * a[i2][j2] - a[i1][j2] * a[i2][j1]
    };
    [[c(0, 0), c(0, 1), c(0, 2)], [c(1, 0), c(1, 1), c(1, 2)], [c(2, 0), c(2, 1), c(2, 2)]]
}

fn mul3(a: &[[f64; 3]; 3], v: Vec3) -> Vec3 {
    Vec3::new(
        a[0][0] * v.x + a[0][1] * v.y + a[0][2] * v.z,
        a[1][0] * v.x + a[1][1] * v.y + a[1][2] * v.z,
        a[2][0] * v.x + a[2][1] * v.y + a[2][2] * v.z,
    )
}

/// Determinant of the linear part: negative for a reflection, the cube of
/// the factor for a uniform scale.
pub fn determinant(t: &Transform) -> f64 {
    det3(&linear(t))
}

/// Does `t` turn a right-handed frame into a left-handed one? A solid
/// mapped by such a transform is inside out until its faces are inverted.
pub fn is_reflection(t: &Transform) -> bool {
    determinant(t) < 0.0
}

/// Is `t` a similarity — a rotation, reflection or uniform scale, plus a
/// translation — so that lengths all scale by one factor and every analytic
/// surface class survives?
pub fn is_uniform(t: &Transform) -> bool {
    let a = linear(t);
    let col = |j: usize| Vec3::new(a[0][j], a[1][j], a[2][j]);
    let (x, y, z) = (col(0), col(1), col(2));
    let s2 = x.dot(x);
    let eps = 1e-9 * s2.max(1e-300);
    (y.dot(y) - s2).abs() <= eps
        && (z.dot(z) - s2).abs() <= eps
        && x.dot(y).abs() <= eps
        && y.dot(z).abs() <= eps
        && z.dot(x).abs() <= eps
}

/// A surface normal under `t`: the inverse transpose of the linear part,
/// normalised. For a rotation that is the rotation; for a reflection it is
/// the reflection (a reflected solid's outward normal is its old outward
/// normal reflected); for a non-uniform scale it is what keeps the normal
/// perpendicular to the scaled surface, which the matrix itself does not.
pub fn normal_map(t: &Transform, n: Vec3) -> Vec3 {
    let a = linear(t);
    let d = det3(&a);
    if d.abs() < 1e-300 {
        return t.direction(n);
    }
    (mul3(&cofactor(&a), n) * (1.0 / d)).normalized()
}

/// The reflection across `plane`: a Householder map about its unit normal,
/// conjugated by the translation to its origin. Its own inverse.
pub fn reflection(plane: &Plane) -> Transform {
    let n = plane.normal.normalized();
    let nv = [n.x, n.y, n.z];
    let col = |j: usize| {
        let d = |i: usize| if i == j { 1.0 } else { 0.0 };
        Vector4::new(d(0) - 2.0 * nv[0] * nv[j], d(1) - 2.0 * nv[1] * nv[j], d(2) - 2.0 * nv[2] * nv[j], 0.0)
    };
    let h = Matrix4::from_cols(col(0), col(1), col(2), Vector4::new(0.0, 0.0, 0.0, 1.0));
    let o = plane.origin.vector();
    Transform(Matrix4::from_translation(o) * h * Matrix4::from_translation(-o))
}

/// The scale by `factors` (one per axis) about `center`.
pub fn scaling(center: Vec3, factors: Vec3) -> Transform {
    let c = center.vector();
    let s = Matrix4::from_nonuniform_scale(factors.x, factors.y, factors.z);
    Transform(Matrix4::from_translation(c) * s * Matrix4::from_translation(-c))
}

/// `s` under `t`, still outward-oriented: truck maps the geometry, and when
/// `t` is a reflection every face is inverted afterwards (see the module
/// notes for why truck's own result is inside out).
pub fn transformed(s: &KSolid, t: &Transform) -> KSolid {
    let moved = kernel::transformed(s, t);
    if !is_reflection(t) {
        return moved;
    }
    inverted(moved)
}

/// Every face of `s` inverted: the same surfaces, the boundary loops walked
/// the other way. Adjacent faces still oppose each other along their shared
/// edges, so a closed shell stays closed and consistently oriented.
fn inverted(s: KSolid) -> KSolid {
    let mut shells = s.into_boundaries();
    for sh in shells.iter_mut() {
        for f in sh.iter_mut() {
            f.invert();
        }
    }
    Solid::new_unchecked(shells)
}

/// `s` reflected across `plane`, outward-oriented.
pub fn mirrored(s: &KSolid, plane: &Plane) -> KSolid {
    transformed(s, &reflection(plane))
}

/// `plane` as a feature argument reads.
pub fn fmt_plane(p: &Plane) -> String {
    format!("plane(origin={}, normal={})", fmt_vec(p.origin), fmt_vec(p.normal))
}

// ------------------------------------------------------------- primitives

/// A radial direction perpendicular to `axis`: the world axis least aligned
/// with it, orthogonalised.
fn radial(axis: Vec3) -> Vec3 {
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let pick = axes
        .iter()
        .copied()
        .min_by(|a, b| a.dot(axis).abs().partial_cmp(&b.dot(axis).abs()).unwrap())
        .unwrap();
    (pick - axis * pick.dot(axis)).normalized()
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// The profile a sphere is the revolution of, as the namer sees it: one arc
/// from pole to pole through the equator, in a plane through the centre
/// that holds the axis (sketch x radial, sketch y along the axis).
pub fn sphere_profile(center: Vec3, r: f64) -> Sketch {
    let axis = Vec3::Z;
    let e1 = radial(axis);
    let plane = Plane::new(center, e1.cross(axis), e1).expect("a frame from a unit axis");
    let arc = Segment::Arc { a: P2::new(0.0, r), b: P2::new(0.0, -r), via: P2::new(r, 0.0) };
    Sketch::new(plane).with_loop(Loop { segments: vec![arc] })
}

/// Rodrigues' rotation of `v` about the unit `axis` by `angle`.
fn rotate_about(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = fmath::sin_cos(angle);
    v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

/// How many arcs the sphere's equator is cut into: truck's own division of
/// a full turn, so the sphere's equator vertices sit where a `revolve`'s
/// seams do.
const TURN_DIVISION: usize = 3;

/// An exact sphere of radius `r` about `center`: two hemispheres, each
/// the surface of revolution of a quarter meridian about `z`, sharing the
/// equator as three arcs. Each face's pole is *interior* to it — the one
/// place on a surface of revolution a boundary must never pass.
///
/// Why not lunes meeting at the poles, or `rsweep` of the profile. truck
/// maps a face's boundary into parameter space point by point, each search
/// started from the previous point's parameter. A boundary through a pole
/// restarts the search *at* the pole, where the projected profile folds
/// back on itself and the projection's derivative divides by the radius,
/// zero: which side of the fold Newton lands on is a matter of rounding,
/// and the far side — the profile's extension past the pole — is on the
/// same sphere, so the answer passes truck's own check with `u` outside
/// the face's range. One lune in three then lost a strip of triangles for
/// centres such as `(20, 0, 0)`, and closed fine for `(10, 0, 0)`. Sweeping
/// the profile edge leaves the shell open at the poles (each zero-radius
/// pole circle belongs to one patch), and sweeping the half-disc face closes
/// it with three zero-area faces from the edge on the axis. A cap, by
/// contrast, is what truck's triangulator is written for: its boundary is
/// an open curve in parameter space that truck closes along the pole line
/// itself, so no boundary point is ever searched from a pole.
///
/// The absolute boundaries run clockwise about `+z`, which is *decreasing*
/// `v` in the revolution's parameters: that is the traversal truck closes
/// via the pole line (the other way round it would close along the equator
/// and get a zero-area polygon), and it makes the polygon counter-clockwise
/// in parameter space, which is what marks it as a face rather than a
/// hole. The upper hemisphere's parametric normal then points inward
/// (`∂u` north × `∂v` east), so that face is inverted; the lower one's
/// points outward and stays. The volume check below is a safety net, not
/// the rule.
pub fn sphere(center: Vec3, r: f64) -> Result<KSolid> {
    if !(r.is_finite() && finite(center)) {
        return Err(CadpoleError::invalid("sphere: radius and centre must be finite"));
    }
    if r <= 0.0 {
        return Err(CadpoleError::invalid("sphere: r must be positive"));
    }
    let axis = Vec3::Z;
    let e1 = radial(axis);
    let north = builder::vertex((center + axis * r).point());
    let south = builder::vertex((center - axis * r).point());
    let step = TAU / TURN_DIVISION as f64;
    let dir = |angle: f64| rotate_about(e1, axis, angle);
    let equator: Vec<_> = (0..TURN_DIVISION).map(|i| builder::vertex((center + dir(i as f64 * step) * r).point())).collect();
    let arcs: Vec<_> = (0..TURN_DIVISION)
        .map(|i| {
            let via = (center + dir((i as f64 + 0.5) * step) * r).point();
            builder::circle_arc(&equator[i], &equator[(i + 1) % TURN_DIVISION], via)
        })
        .collect();
    let k = std::f64::consts::FRAC_1_SQRT_2;
    let profile = |pole: &truck_modeling::Vertex, up: f64| {
        let via = (center + (e1 + axis * up) * (r * k)).point();
        let curve = builder::circle_arc(&equator[0], pole, via).curve();
        Surface::RevolutedCurve(Processor::new(RevolutedCurve::by_revolution(curve, center.point(), axis.vector())))
    };
    let clockwise = || Wire::from(arcs.iter().rev().map(|a| a.inverse()).collect::<Vec<_>>());
    let upper = Face::try_new(vec![clockwise()], profile(&north, 1.0))
        .map_err(|e| CadpoleError::kernel(format!("sphere: {e}")))?
        .inverse();
    let lower =
        Face::try_new(vec![clockwise()], profile(&south, -1.0)).map_err(|e| CadpoleError::kernel(format!("sphere: {e}")))?;
    let mut shell = Shell::new();
    shell.push(upper);
    shell.push(lower);
    let solid = Solid::try_new(vec![shell]).map_err(|e| CadpoleError::kernel(format!("sphere: {e}")))?;
    // Which way is out is read from the solid itself, as `loft` does,
    // rather than trusted from the meridians' direction: a
    // hand-built face has no sweep for truck to orient it by.
    if kernel::soup_volume(&solid, r * 0.2)? < 0.0 {
        return Ok(inverted(solid));
    }
    Ok(solid)
}

/// The profile a torus is the revolution of: the tube's circle drawn as two
/// arcs split at the top and the bottom of the tube, so segment 0 is the
/// outer half (`outer`) and segment 1 the inner half (`inner`). Sketch x is
/// radial, sketch y runs along `axis`.
pub fn torus_profile(center: Vec3, axis: Vec3, major: f64, minor: f64) -> Result<Sketch> {
    let axis = axis.normalized();
    let e1 = radial(axis);
    let plane = Plane::new(center, e1.cross(axis), e1)?;
    let top = P2::new(major, minor);
    let bottom = P2::new(major, -minor);
    let outer = Segment::Arc { a: top, b: bottom, via: P2::new(major + minor, 0.0) };
    let inner = Segment::Arc { a: bottom, b: top, via: P2::new(major - minor, 0.0) };
    Ok(Sketch::new(plane).with_loop(Loop { segments: vec![outer, inner] }))
}

/// An exact torus: a tube of radius `minor` around a circle of radius
/// `major` about `axis` through `center`, by [`kernel::revolve`] of
/// [`torus_profile`]. Six `RevolutedCurve` faces, each of which classifies
/// as the torus. A spindle torus (`minor >= major`) crosses itself, or
/// touches its own axis, and is refused.
pub fn torus(center: Vec3, axis: Vec3, major: f64, minor: f64) -> Result<KSolid> {
    if !(major.is_finite() && minor.is_finite() && finite(center) && finite(axis)) {
        return Err(CadpoleError::invalid("torus: radii, centre and axis must be finite"));
    }
    if major <= 0.0 || minor <= 0.0 {
        return Err(CadpoleError::invalid("torus: major and minor radii must be positive"));
    }
    if minor >= major {
        return Err(CadpoleError::invalid(format!(
            "torus: the minor radius ({}) must be smaller than the major ({}); a tube as fat as its ring \
             crosses its own axis (a spindle torus), which is not a solid",
            fmt_num(minor),
            fmt_num(major)
        )));
    }
    if axis.length() == 0.0 {
        return Err(CadpoleError::invalid("torus: axis must be non-zero"));
    }
    let sketch = torus_profile(center, axis, major, minor)?;
    let mut solids = kernel::revolve(&sketch, center, axis.normalized(), TAU)?;
    if solids.len() != 1 {
        return Err(CadpoleError::kernel("torus: expected exactly one solid"));
    }
    Ok(solids.remove(0))
}

/// The class of every face of `s`, in face order, recognised from the
/// surfaces themselves.
pub fn classes_of(s: &KSolid) -> Vec<SurfaceClass> {
    surfaces_of(s).into_iter().map(|(c, _)| c).collect()
}

/// [`classes_of`], and for every face no class names, its own oriented
/// surface — what [`FaceInfo::geometry`] holds, so a later mesh boolean can
/// still evaluate normals on a squashed cylinder's wall.
pub fn surfaces_of(s: &KSolid) -> Vec<(SurfaceClass, Option<Arc<Surface>>)> {
    s.face_iter()
        .map(|f| {
            let oriented = f.oriented_surface();
            let class = surface_class::classify(&oriented, surface_class::EXACT_TOL);
            let geometry = (!class.is_recognised()).then(|| Arc::new(oriented));
            (class, geometry)
        })
        .collect()
}

// -------------------------------------------------------------- mesh path

/// A named mesh body under an affine map `t`, keeping every name.
///
/// Positions move by `t`, normals by its inverse transpose, and when `t` is
/// a reflection every triangle's winding is swapped so the surface stays
/// outward. Each face's area, centroid and normal are measured again from
/// the moved triangles rather than scaled, because a non-uniform scale
/// changes them in ways no single factor describes. Surface classes follow
/// the map under a similarity; under a non-uniform scale a plane stays a
/// plane and everything else becomes `Other` — unless `surfaces` supplies
/// the classes recognised from the moved B-rep, one per face in face order,
/// with the patch of each face no class names ([`surfaces_of`]), which is
/// what the B-rep path does. Otherwise a face's own patch moves under the
/// same map, as a copy's does.
pub fn remapped_mesh_body(
    (mesh, topo): &MeshBody,
    t: &Transform,
    surfaces: Option<&[(SurfaceClass, Option<Arc<Surface>>)]>,
) -> MeshBody {
    let flip = is_reflection(t);
    let uniform = is_uniform(t);
    // The vertices move but stay the same vertices, so a scan's colours
    // come along unchanged.
    let mut out = Mesh {
        positions: mesh.positions.clone(),
        normals: mesh.normals.clone(),
        indices: mesh.indices.clone(),
        face_ids: mesh.face_ids.clone(),
        colors: mesh.colors.clone(),
    };
    for p in out.positions.chunks_exact_mut(3) {
        let v = t.point(Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
        p[0] = v.x as f32;
        p[1] = v.y as f32;
        p[2] = v.z as f32;
    }
    for n in out.normals.chunks_exact_mut(3) {
        let v = normal_map(t, Vec3::new(n[0] as f64, n[1] as f64, n[2] as f64));
        n[0] = v.x as f32;
        n[1] = v.y as f32;
        n[2] = v.z as f32;
    }
    if flip {
        for tri in out.indices.chunks_exact_mut(3) {
            tri.swap(1, 2);
        }
    }
    let surfaces = surfaces.filter(|c| c.len() == topo.faces.len());
    let faces = topo
        .faces
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut area = 0.0;
            let mut centroid = Vec3::ZERO;
            let mut normal_acc = Vec3::ZERO;
            for k in f.tri_start..f.tri_start + f.tri_count {
                if let Some([a, b, c]) = out.triangle(k as usize) {
                    let n = (b - a).cross(c - a);
                    let ta = n.length() / 2.0;
                    area += ta;
                    centroid = centroid + (a + b + c) * (ta / 3.0);
                    normal_acc = normal_acc + n;
                }
            }
            let (surface, geometry) = match surfaces {
                Some(c) => c[i].clone(),
                None => {
                    let class = if uniform {
                        f.surface.transformed(t)
                    } else {
                        match &f.surface {
                            SurfaceClass::Plane { origin, normal } => {
                                SurfaceClass::Plane { origin: t.point(*origin), normal: normal_map(t, *normal) }
                            }
                            _ => SurfaceClass::Other,
                        }
                    };
                    (class, f.geometry.as_ref().map(|g| Arc::new(g.transformed(t.0))))
                }
            };
            FaceInfo {
                surface,
                geometry,
                centroid: if area > 0.0 { centroid * (1.0 / area) } else { t.point(f.centroid) },
                normal: if normal_acc.length() > 0.0 { normal_acc.normalized() } else { normal_map(t, f.normal) },
                area,
                v_dir: if f.v_dir == Vec3::ZERO { Vec3::ZERO } else { t.direction(f.v_dir) },
                ..f.clone()
            }
        })
        .collect();
    let edges = topo
        .edges
        .iter()
        .map(|e| {
            let polyline: Vec<Vec3> = e.polyline.iter().map(|p| t.point(*p)).collect();
            let start = t.point(e.start);
            let end = t.point(e.end);
            let length = if polyline.len() >= 2 {
                polyline.windows(2).map(|w| (w[1] - w[0]).length()).sum()
            } else {
                (end - start).length()
            };
            EdgeInfo { start, end, length, polyline, ..e.clone() }
        })
        .collect();
    let vertices = topo.vertices.iter().map(|v| VertexInfo { position: t.point(v.position), ..v.clone() }).collect();
    (out, Topology { faces, edges, vertices })
}

/// The named surfaces of a mesh body as the triangles they cover: what a
/// boolean's result faces are matched against, so a mirrored
/// or scaled B-rep keeps its names through the next boolean.
pub fn catalogue_of((mesh, topo): &MeshBody, tol: f64) -> naming::SurfaceCatalogue {
    let mut cat = naming::SurfaceCatalogue::new(crate::context::match_tolerance(tol));
    for f in &topo.faces {
        let tris: Vec<[Vec3; 3]> =
            (f.tri_start..f.tri_start + f.tri_count).filter_map(|x| mesh.triangle(x as usize)).collect();
        cat.add_face(NamedFace { name: f.name.clone(), aliases: f.aliases.clone(), role: Role::Kept }, tris);
    }
    cat.finish();
    cat
}

/// The convex hull of `points` as a mesh body, every face named `group`.
///
/// Quickhull, from the mesh kernel's own library. Manifold merges coplanar
/// triangles under one id, so the hull of a box is six faces, and every id
/// is named `group`; the pieces then take `group#k` under the group alias
/// when the topology's names are applied, as any split face does.
pub fn hull_of_points(points: &[Vec3], group: &str, tol: f64) -> Result<MeshBody> {
    let _span = kernel::trace::Span::start(format!("hull points={}", points.len()));
    if points.len() < 4 {
        return Err(CadpoleError::invalid(format!(
            "hull: needs at least four points that are not all in one plane, got {}",
            points.len()
        )));
    }
    let pts: Vec<manifold_rust::linalg::Vec3> =
        points.iter().map(|p| manifold_rust::linalg::Vec3::new(p.x, p.y, p.z)).collect();
    let m = Manifold::hull(&pts);
    if m.status() != manifold_rust::types::Error::NoError || m.is_empty() || m.num_tri() == 0 {
        return Err(CadpoleError::kernel(
            "hull: the points have no volume between them (all in one plane, or on one line), so their \
             hull is not a solid",
        ));
    }
    let mg = m.get_mesh_gl(-1);
    let max_id = mg.face_id.iter().copied().max().unwrap_or(0) as usize;
    let groups: Vec<SourceFace> = (0..=max_id)
        .map(|_| SourceFace {
            name: group.to_owned(),
            aliases: Vec::new(),
            surface_kind: "group",
            surface: SurfaceClass::Other,
            geometry: None,
        })
        .collect();
    let mb = mesh_kernel::from_manifold(&m, &groups, tol)?;
    let report = crate::solid::validate_with(&mb.0, &crate::solid::SolidOptions { pair_limit: 0, ..Default::default() });
    if !report.oriented() {
        return Err(CadpoleError::kernel(format!("hull produced a surface that is not a solid: {}", report.problems().join("; "))));
    }
    Ok(mb)
}

// ---------------------------------------------------------------- context

impl Context {
    /// Mirror `body` across `plane`.
    ///
    /// `keep = false`: the mirror image replaces the body, as `translate`
    /// moves one. `keep = true, merge = false`: the body stays and the image
    /// is a second body. `keep = true, merge = true`: the two are fused into
    /// one body where they touch or overlap — the "model half, mirror it"
    /// flow — and are left as two when they never meet, with an info
    /// diagnostic saying so. Returns the ids in the order `[source, mirror]`
    /// when there are two.
    ///
    /// A half that touches the plane on a face meets its image on that very
    /// face, back to back, and `union`'s pre-pass answers that with
    /// [`crate::contact::glue`]: the two faces in the plane are dropped and
    /// the shells stitched along their boundary, so there is no internal
    /// face and no boolean ran. A half that crosses the plane
    /// overlaps its image and takes the ordinary boolean route, mesh
    /// fallback included. The names of both operands travel through the
    /// element map from their tessellations, so a face the fuse kept is
    /// still called what the source called it.
    pub fn mirror(
        &mut self,
        body: BodyId,
        plane: &Plane,
        keep: bool,
        merge: bool,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        let t = reflection(plane);
        let (src_feature, material, src_body, layer) = {
            let src = self.body(body)?;
            (src.feature.clone(), src.material.clone(), src.body.clone(), src.layer.clone())
        };
        let image = remapped_mesh_body(self.body_mesh(body)?, &t, None);
        let moved = match &src_body {
            Body::Brep(s) => Body::Brep(transformed(s, &t)),
            Body::Mesh(_) => Body::Mesh(image.clone()),
        };
        let mut args = BTreeMap::new();
        args.insert("plane".into(), fmt_plane(plane));
        args.insert("keep".into(), keep.to_string());
        if keep {
            args.insert("merge".into(), merge.to_string());
        }
        let fid = self.new_feature("mirror", name.clone(), vec![src_feature], args, span.clone())?;
        let tol = self.tolerance;
        // The image's identity: the source, the plane, the feature
        // the names are minted under and the bounds the image was made to.
        let key = {
            let mut k = KeyBuilder::new("mirror");
            k.put(&fid).body(&self.body(body)?.key).put(plane).bool(keep).bool(merge);
            k.put(&self.tessellation_for(body, None)?.0);
            self.routing_key(&mut k);
            k.finish()
        };
        let names_of = |b: &Body, mb: &MeshBody| match b {
            Body::Brep(_) => NameSource::Inherited { feature: fid.clone(), sources: Arc::new(catalogue_of(mb, tol)) },
            Body::Mesh(_) => NameSource::Intrinsic,
        };
        if !keep {
            let names = names_of(&moved, &image);
            self.consume_body(body)?;
            let id = self.add_body_realised(&fid, moved, name, image, names)?;
            let b = self.body_mut(id)?;
            b.material = material;
            b.layer = layer;
            b.key = key.body_key(0);
            return Ok(vec![id]);
        }
        let apart = match (&src_body, &moved) {
            (Body::Brep(a), Body::Brep(b)) => {
                matches!(kernel::disposition(a, b, self.boolean_tolerance)?, Disposition::Separate)
            }
            _ => mesh_kernel::separated(&self.body_mesh(body)?.0, &image.0),
        };
        if !merge || apart {
            if merge {
                self.info(
                    "mirror: the body and its mirror image never touch, so there is nothing to fuse: \
                     this is two bodies, the original and the image"
                        .to_owned(),
                    &fid,
                    span,
                );
            }
            let names = names_of(&moved, &image);
            let id = self.add_body_realised(&fid, moved, name, image, names)?;
            let b = self.body_mut(id)?;
            b.material = material;
            b.layer = layer;
            b.key = key.body_key(1);
            return Ok(vec![body, id]);
        }
        let produced = self.memoised("mirror", &fid, span.clone(), key, |ctx| {
            let a_names = match &src_body {
                Body::Brep(_) => NameSource::Inherited {
                    feature: fid.clone(),
                    sources: Arc::new(catalogue_of(ctx.body_mesh(body)?, tol)),
                },
                Body::Mesh(_) => NameSource::Intrinsic,
            };
            let b_names = names_of(&moved, &image);
            let (fused, names) = ctx.boolean_pair(
                BoolOp::Union,
                src_body,
                &a_names,
                &moved,
                &b_names,
                KernelChoice::Auto,
                &fid,
                span.clone(),
            )?;
            if fused.kind_name() == "mesh" {
                ctx.set_feature_arg(&fid, "kind", "mesh");
            }
            Ok(Produced::one(fused, names))
        })?;
        self.consume_body(body)?;
        let id = self.add_produced(&fid, produced, name)?[0];
        self.body_mut(id)?.material = material;
        self.body_mut(id)?.layer = layer;
        Ok(vec![id])
    }

    /// Scale `body` by `factors` (one per axis) about `center`.
    ///
    /// A uniform scale is a similarity and goes the way a `translate` goes:
    /// the source's tessellation and names move with it, radii and areas
    /// scale by the factor. A non-uniform scale keeps the names the same
    /// way but measures every face again and recognises every surface
    /// again, because a cylinder squashed along one axis is no cylinder.
    pub fn scale(
        &mut self,
        body: BodyId,
        center: Vec3,
        factors: Vec3,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<BodyId> {
        if !(finite(factors) && finite(center)) {
            return Err(CadpoleError::invalid("scale: factors and centre must be finite"));
        }
        if factors.x <= 0.0 || factors.y <= 0.0 || factors.z <= 0.0 {
            return Err(CadpoleError::invalid(format!(
                "scale: factors must be positive, got {} (a negative factor is a mirror; use mirror())",
                fmt_vec(factors)
            )));
        }
        let t = scaling(center, factors);
        let uniform = factors.x == factors.y && factors.y == factors.z;
        let mut args = BTreeMap::new();
        if uniform {
            args.insert("factor".into(), fmt_num(factors.x));
        } else {
            args.insert("factors".into(), fmt_vec(factors));
        }
        args.insert("center".into(), fmt_vec(center));
        if uniform {
            return self.transform(body, &t, "scale", args, name, span);
        }
        let (src_feature, material, src_body, layer) = {
            let src = self.body(body)?;
            (src.feature.clone(), src.material.clone(), src.body.clone(), src.layer.clone())
        };
        let moved = match &src_body {
            Body::Brep(s) => Body::Brep(transformed(s, &t)),
            Body::Mesh(m) => Body::Mesh(remapped_mesh_body(m, &t, None)),
        };
        // The B-rep says what its scaled surfaces are; the mesh body's
        // classes are the conservative rule in `remapped_mesh_body`.
        let surfaces = match &moved {
            Body::Brep(s) => Some(surfaces_of(s)),
            Body::Mesh(_) => None,
        };
        let image = remapped_mesh_body(self.body_mesh(body)?, &t, surfaces.as_deref());
        let fid = self.new_feature("scale", name.clone(), vec![src_feature], args, span)?;
        let key = {
            let mut k = KeyBuilder::new("scale");
            k.put(&fid).body(&self.body(body)?.key).put(&center).put(&factors);
            k.put(&self.tessellation_for(body, None)?.0).f64(self.tolerance);
            k.finish().body_key(0)
        };
        let names = match &moved {
            Body::Brep(_) => {
                NameSource::Inherited { feature: fid.clone(), sources: Arc::new(catalogue_of(&image, self.tolerance)) }
            }
            Body::Mesh(_) => NameSource::Intrinsic,
        };
        self.consume_body(body)?;
        let id = self.add_body_realised(&fid, moved, name, image, names)?;
        let b = self.body_mut(id)?;
        b.material = material;
        b.layer = layer;
        b.key = key;
        Ok(id)
    }

    /// An exact sphere: two caps named `surface#0` and `surface#1`
    /// under the group alias `surface`.
    pub fn sphere(&mut self, center: Vec3, r: f64, name: Option<String>, span: Option<Span>) -> Result<BodyId> {
        let mut args = BTreeMap::new();
        args.insert("r".into(), fmt_num(r));
        args.insert("center".into(), fmt_vec(center));
        let fid = self.new_feature("sphere", name.clone(), vec![], args, span.clone())?;
        let mut key = KeyBuilder::new("sphere");
        key.put(&fid).put(&center).f64(r);
        let produced = self.memoised("sphere", &fid, span, key.finish(), |_| {
            let solid = sphere(center, r)?;
            let generator = Arc::new(naming::Generator::revolve_styled(
                &sphere_profile(center, r),
                center,
                Vec3::Z,
                TAU,
                Style::Sphere,
            ));
            Ok(Produced::one(Body::Brep(solid), NameSource::Generator { feature: fid.clone(), generator }))
        })?;
        Ok(self.add_produced(&fid, produced, name)?[0])
    }

    /// An exact torus: faces named `outer` and `inner` (three
    /// pieces each, under those group aliases).
    pub fn torus(
        &mut self,
        center: Vec3,
        axis: Vec3,
        major: f64,
        minor: f64,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<BodyId> {
        let mut args = BTreeMap::new();
        args.insert("major".into(), fmt_num(major));
        args.insert("minor".into(), fmt_num(minor));
        args.insert("center".into(), fmt_vec(center));
        args.insert("axis".into(), fmt_vec(axis));
        let fid = self.new_feature("torus", name.clone(), vec![], args, span.clone())?;
        let mut key = KeyBuilder::new("torus");
        key.put(&fid).put(&center).put(&axis).f64(major).f64(minor);
        let produced = self.memoised("torus", &fid, span, key.finish(), |_| {
            let solid = torus(center, axis, major, minor)?;
            let sketch = torus_profile(center, axis, major, minor)?;
            let generator =
                Arc::new(naming::Generator::revolve_styled(&sketch, center, axis.normalized(), TAU, Style::Torus));
            Ok(Produced::one(Body::Brep(solid), NameSource::Generator { feature: fid.clone(), generator }))
        })?;
        Ok(self.add_produced(&fid, produced, name)?[0])
    }

    /// The convex hull of `bodies` as one mesh body. The inputs are
    /// consumed, as a union's are; the result carries the first one's
    /// material.
    pub fn hull(&mut self, bodies: &[BodyId], name: Option<String>, span: Option<Span>) -> Result<BodyId> {
        if bodies.is_empty() {
            return Err(CadpoleError::invalid("hull needs at least one body"));
        }
        let mut inputs = Vec::new();
        let mut names = Vec::new();
        for id in bodies {
            let b = self.body(*id)?;
            if !inputs.contains(&b.feature) {
                inputs.push(b.feature.clone());
            }
            names.push(b.name.clone());
        }
        let material = self.body(bodies[0])?.material.clone();
        let layer = self.body(bodies[0])?.layer.clone();
        let mut args = BTreeMap::new();
        args.insert("bodies".into(), names.join(", "));
        args.insert("kind".into(), "mesh".into());
        let fid = self.new_feature("hull", name.clone(), inputs, args, span.clone())?;
        let tol = self.tolerance;
        let mut key = KeyBuilder::new("hull");
        key.put(&fid).tag("bodies").u64(bodies.len() as u64);
        for id in bodies {
            key.body(&self.body(*id)?.key).put(&self.tessellation_for(*id, None)?.0);
        }
        key.f64(tol);
        let produced = self.memoised("hull", &fid, span.clone(), key.finish(), |ctx| {
            let mut points = Vec::new();
            for id in bodies {
                let (mesh, _) = ctx.body_mesh(*id)?;
                points.extend(mesh.positions.chunks_exact(3).map(|p| Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64)));
            }
            let built = hull_of_points(&points, &naming::full_name(&fid.0, "hull"), tol);
            ctx.absorb_kernel_notes(&fid, span.clone());
            let mb = built?;
            ctx.info(
                format!(
                    "hull is a mesh body: the convex hull of {} bod{} tessellated at tolerance {} ({} triangles in \
                     {} face{}). It keeps face names, selection, STL and 3MF, and has no exact surfaces or STEP",
                    bodies.len(),
                    if bodies.len() == 1 { "y" } else { "ies" },
                    fmt_num(tol),
                    mb.0.triangle_count(),
                    mb.1.faces.len(),
                    if mb.1.faces.len() == 1 { "" } else { "s" }
                ),
                &fid,
                span.clone(),
            );
            Ok(Produced::one(Body::Mesh(mb), NameSource::Intrinsic))
        })?;
        for id in bodies {
            self.consume_body(*id)?;
        }
        let id = self.add_produced(&fid, produced, name)?[0];
        self.body_mut(id)?.material = material;
        self.body_mut(id)?.layer = layer;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solid;
    use crate::tessellate;
    use std::f64::consts::PI;

    const TOL: f64 = 0.05;

    fn near(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1e-12)
    }

    /// A box with one corner at the origin, as `box()` builds it.
    fn block(w: f64, d: f64, h: f64) -> (Sketch, KSolid) {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(w / 2.0, d / 2.0), w, d));
        let b = kernel::extrude(&s, h).unwrap().remove(0);
        (s, b)
    }

    fn rod(r: f64, h: f64) -> KSolid {
        kernel::extrude(&Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), r)), h).unwrap().remove(0)
    }

    /// The block tessellated and named the way `Context` names a `box`, so
    /// the mesh path is tested with the names it really carries.
    fn named_block(w: f64, d: f64, h: f64, feature: &str) -> (KSolid, MeshBody) {
        let (s, b) = block(w, d, h);
        let mut mb = tessellate::tessellate(&b, TOL).unwrap();
        let g = naming::Generator::extrude(&s, h, "box");
        let named = naming::name_faces(&g, feature, &mb.1.probes(&mb.0));
        mb.1.apply_names(named);
        (b, mb)
    }

    fn face<'a>(mb: &'a MeshBody, name: &str) -> &'a FaceInfo {
        mb.1.faces.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no face {name}: {:?}", names(mb)))
    }

    fn names(mb: &MeshBody) -> Vec<String> {
        mb.1.faces.iter().map(|f| f.name.clone()).collect()
    }

    /// truck's `transformed` maps a solid through a reflection faithfully
    /// and leaves every face pointing inward: the volume of the bare result
    /// is negative. `mirrored` inverts the faces afterwards, and the mirror
    /// image is a solid of the same volume whose every face points out —
    /// on the tessellation and on the recognised surfaces alike.
    #[test]
    fn a_bare_reflection_is_inside_out_and_the_mirror_is_not() {
        let (_, b) = block(20.0, 10.0, 6.0);
        let t = reflection(&Plane::YZ);
        assert!(is_reflection(&t) && near(determinant(&t), -1.0, 1e-12));
        let bare = kernel::transformed(&b, &t);
        let bare_volume = kernel::soup_volume(&bare, TOL).unwrap();
        assert!(bare_volume < 0.0, "truck's reflected box should be inside out, got {bare_volume}");

        let m = mirrored(&b, &Plane::YZ);
        assert_eq!(kernel::face_count(&m), 6);
        assert!(near(kernel::soup_volume(&m, TOL).unwrap(), 1200.0, 1e-9));
        let (mesh, topo) = tessellate::tessellate(&m, TOL).unwrap();
        assert!(solid::validate(&mesh).is_solid());
        let (lo, hi) = mesh.bbox().unwrap();
        assert!((lo - Vec3::new(-20.0, 0.0, 0.0)).length() < 1e-9 && (hi - Vec3::new(0.0, 10.0, 6.0)).length() < 1e-9);
        let centre = Vec3::new(-10.0, 5.0, 3.0);
        for (f, class) in topo.faces.iter().zip(classes_of(&m)) {
            assert!((f.centroid - centre).dot(f.normal) > 0.0, "{} points in", f.name);
            match class {
                SurfaceClass::Plane { normal, .. } => {
                    assert!((normal - f.normal).length() < 1e-9, "oriented surface of {} disagrees: {normal:?} vs {:?}", f.name, f.normal)
                }
                other => panic!("{other:?}"),
            }
        }
    }

    /// On the mesh path the mirror image keeps every name, its volume and
    /// its orientation; a face keeps its name while its normal reflects; and
    /// mirroring twice is the identity.
    #[test]
    fn a_mirrored_mesh_body_keeps_its_names_and_its_orientation() {
        let (_, mb) = named_block(20.0, 10.0, 6.0, "blk");
        let img = remapped_mesh_body(&mb, &reflection(&Plane::YZ), None);
        assert_eq!(names(&img), names(&mb));
        assert!(near(img.0.volume(), mb.0.volume(), 1e-9) && img.0.volume() > 0.0);
        assert!(solid::validate(&img.0).is_solid());
        let right = face(&img, "blk/right");
        assert!(near(right.centroid.x, -20.0, 1e-9) && right.normal.x < -0.999, "{right:?}");
        let top = face(&img, "blk/top");
        assert!(near(top.centroid.z, 6.0, 1e-9) && top.normal.z > 0.999);
        assert!(near(top.area, 200.0, 1e-9));
        match &top.surface {
            SurfaceClass::Plane { normal, .. } => assert!(normal.z > 0.999),
            other => panic!("{other:?}"),
        }
        // Vertex normals moved with the surface: unit, and agreeing with
        // the face they sit on.
        for (t, tri) in img.0.indices.chunks_exact(3).enumerate() {
            let f = &img.1.faces[img.0.face_ids[t] as usize];
            for &v in tri {
                let n = Vec3::new(img.0.normals[v as usize * 3] as f64, img.0.normals[v as usize * 3 + 1] as f64, img.0.normals[v as usize * 3 + 2] as f64);
                assert!(near(n.length(), 1.0, 1e-5) && n.dot(f.normal) > 0.999, "{} {n:?}", f.name);
            }
        }
        let back = remapped_mesh_body(&img, &reflection(&Plane::YZ), None);
        for (a, b) in back.0.positions.iter().zip(&mb.0.positions) {
            assert!((a - b).abs() < 1e-5);
        }
        for (a, b) in back.1.edges.iter().zip(&mb.1.edges) {
            assert!(near(a.length, b.length, 1e-9) && (a.start - b.start).length() < 1e-9);
        }
    }

    /// A reflection is a similarity, so a cylinder stays a cylinder of the
    /// same radius — recognised from the mirrored surface, and carried on
    /// the mirrored mesh body's faces.
    #[test]
    fn a_mirrored_cylinder_is_a_cylinder_of_the_same_radius() {
        let r = rod(5.0, 12.0);
        let plane = Plane::new(Vec3::new(3.0, 1.0, 2.0), Vec3::new(1.0, 1.0, 0.5), Vec3::X).unwrap();
        let m = mirrored(&r, &plane);
        let classes = classes_of(&m);
        assert_eq!(classes.iter().filter(|c| matches!(c, SurfaceClass::Plane { .. })).count(), 2, "{classes:?}");
        let cyls: Vec<_> = classes.iter().filter(|c| matches!(c, SurfaceClass::Cylinder { .. })).collect();
        assert_eq!(cyls.len(), 2, "{classes:?}");
        for c in cyls {
            if let SurfaceClass::Cylinder { radius, axis, .. } = c {
                assert!(near(*radius, 5.0, 1e-9));
                assert!(near(axis.dot(normal_map(&reflection(&plane), Vec3::Z)).abs(), 1.0, 1e-9), "{axis:?}");
            }
        }
        assert!(near(kernel::soup_volume(&m, TOL).unwrap(), kernel::soup_volume(&r, TOL).unwrap(), 1e-9));
        let mb = tessellate::tessellate(&r, TOL).unwrap();
        let img = remapped_mesh_body(&mb, &reflection(&plane), None);
        assert_eq!(img.1.faces.iter().filter(|f| matches!(f.surface, SurfaceClass::Cylinder { radius, .. } if near(radius, 5.0, 1e-9))).count(), 2);
    }

    /// A sphere: two exact caps, each recognised as the same sphere, a
    /// solid at the requested tolerance with no sliver faces, and the
    /// textbook volume to within the chord error — at the centre that
    /// broke the lune construction, among others.
    #[test]
    fn a_sphere_is_two_caps_of_one_sphere() {
        for c in [Vec3::new(1.0, 2.0, 3.0), Vec3::new(20.0, 0.0, 0.0), Vec3::new(-15.0, 0.1, 1.0)] {
            let s = sphere(c, 5.0).unwrap();
            let ((mesh, _), used) = tessellate::tessellate_reporting(&s, TOL).unwrap();
            assert_eq!(used, TOL, "{c:?}: not made at the requested tolerance");
            assert!(solid::validate(&mesh).is_solid(), "{c:?}: {}", solid::validate(&mesh).summary());
        }
        let c = Vec3::new(1.0, 2.0, 3.0);
        let s = sphere(c, 10.0).unwrap();
        assert_eq!(kernel::face_count(&s), 2);
        for class in classes_of(&s) {
            match class {
                SurfaceClass::Sphere { center, radius } => {
                    assert!((center - c).length() < 1e-9 && near(radius, 10.0, 1e-9), "{center:?} {radius}")
                }
                other => panic!("{other:?}"),
            }
        }
        let ((mesh, topo), used) = tessellate::tessellate_reporting(&s, TOL).unwrap();
        assert_eq!(used, TOL, "not made at the requested tolerance");
        assert!(solid::validate(&mesh).is_solid(), "{}", solid::validate(&mesh).summary());
        assert!(topo.faces.iter().all(|f| f.tri_count > 2), "{:?}", topo.faces.iter().map(|f| f.tri_count).collect::<Vec<_>>());
        // Inscribed, so a little low: 1.1 % at 0.05 mm on r = 10, most of
        // it in the fans around the poles.
        let want = 4.0 / 3.0 * PI * 1000.0;
        let got = mesh.volume();
        assert!(got < want && near(got, want, 0.02), "volume {got}, expected {want}");
        // No degenerate edge: the equator's three arcs, and nothing at the poles.
        assert_eq!(topo.edges.len(), 3, "{:?}", topo.edges.iter().map(|e| (&e.name, e.length)).collect::<Vec<_>>());
        assert!(topo.edges.iter().all(|e| near(e.length, TAU * 10.0 / 3.0, 0.01)));
        // And no degenerate triangle in the raw mesh either: truck spans one
        // between the two ends of the pole line at each pole, and the
        // tessellation drops it (not only the weld, which STL never sees).
        assert!(solid::validate(&mesh).degenerate.is_empty());
        let thinnest = (0..mesh.triangle_count())
            .map(|t| {
                let [a, b, c] = mesh.triangle(t).unwrap();
                crate::mesh_clean::min_altitude(a, b, c)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(thinnest > 1e-3, "thinnest triangle {thinnest} mm");
        assert!(sphere(c, 0.0).is_err() && sphere(c, f64::NAN).is_err());
    }

    /// A torus: six exact patches recognised as the same torus, the
    /// textbook volume 2π²Rr², and a tilted axis carried through.
    #[test]
    fn a_torus_is_six_patches_with_the_textbook_volume() {
        let t = torus(Vec3::ZERO, Vec3::Z, 10.0, 3.0).unwrap();
        assert_eq!(kernel::face_count(&t), 6);
        for class in classes_of(&t) {
            match class {
                SurfaceClass::Torus { center, axis, major, minor } => {
                    assert!(center.length() < 1e-9 && axis.z > 0.999 && near(major, 10.0, 1e-9) && near(minor, 3.0, 1e-9));
                }
                other => panic!("{other:?}"),
            }
        }
        let ((mesh, topo), used) = tessellate::tessellate_reporting(&t, TOL).unwrap();
        assert_eq!(used, TOL);
        assert!(solid::validate(&mesh).is_solid(), "{}", solid::validate(&mesh).summary());
        assert!(topo.faces.iter().all(|f| f.tri_count > 2));
        // Inscribed, so a little low: the chord error on a tube of radius 3
        // at 0.05 mm is a couple of percent of its area.
        let want = 2.0 * PI * PI * 10.0 * 9.0;
        assert!(mesh.volume() < want && near(mesh.volume(), want, 0.03), "volume {}, expected {want}", mesh.volume());

        let axis = Vec3::new(1.0, 1.0, 1.0).normalized();
        let tilted = torus(Vec3::new(5.0, 0.0, 0.0), axis, 8.0, 2.0).unwrap();
        for class in classes_of(&tilted) {
            match class {
                SurfaceClass::Torus { center, axis: a, major, minor } => {
                    assert!((center - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-9);
                    assert!(near(a.dot(axis).abs(), 1.0, 1e-9) && near(major, 8.0, 1e-9) && near(minor, 2.0, 1e-9));
                }
                other => panic!("{other:?}"),
            }
        }
        // A spindle torus is not a solid, and is refused rather than built.
        assert!(torus(Vec3::ZERO, Vec3::Z, 3.0, 3.0).is_err());
        assert!(torus(Vec3::ZERO, Vec3::Z, 3.0, 5.0).is_err());
    }

    /// The sphere's and the torus's faces are named from their profile: one
    /// segment named `surface`, or an `outer` and an `inner` arc, in three
    /// pieces each under the group alias.
    #[test]
    fn sphere_and_torus_faces_are_named_from_the_profile() {
        let s = sphere(Vec3::ZERO, 10.0).unwrap();
        let mut mb = tessellate::tessellate(&s, TOL).unwrap();
        let g = naming::Generator::revolve_styled(&sphere_profile(Vec3::ZERO, 10.0), Vec3::ZERO, Vec3::Z, TAU, Style::Sphere);
        mb.1.apply_names(naming::name_faces(&g, "ball", &mb.1.probes(&mb.0)));
        let mut got = names(&mb);
        got.sort();
        assert_eq!(got, ["ball/surface#0", "ball/surface#1"]);
        assert!(mb.1.faces.iter().all(|f| f.aliases.contains(&"ball/surface".to_owned()) && f.aliases.contains(&"ball/seg[0.0]".to_owned())), "{:?}", mb.1.faces[0].aliases);

        let t = torus(Vec3::ZERO, Vec3::Z, 10.0, 3.0).unwrap();
        let mut mb = tessellate::tessellate(&t, TOL).unwrap();
        let profile = torus_profile(Vec3::ZERO, Vec3::Z, 10.0, 3.0).unwrap();
        let g = naming::Generator::revolve_styled(&profile, Vec3::ZERO, Vec3::Z, TAU, Style::Torus);
        mb.1.apply_names(naming::name_faces(&g, "ring", &mb.1.probes(&mb.0)));
        let mut got = names(&mb);
        got.sort();
        assert_eq!(got, ["ring/inner#0", "ring/inner#1", "ring/inner#2", "ring/outer#0", "ring/outer#1", "ring/outer#2"]);
        // `outer` really is the outside of the tube: its normal points away
        // from the axis, `inner`'s towards it. (The centroid of a 120° patch
        // is not on the surface, so it cannot say.)
        for f in &mb.1.faces {
            let away = f.normal.dot(Vec3::new(f.centroid.x, f.centroid.y, 0.0).normalized());
            assert!(if f.name.starts_with("ring/outer") { away > 0.0 } else { away < 0.0 }, "{} {away}", f.name);
        }
    }

    /// A non-uniform scale turns the wall of a rod into an elliptic
    /// cylinder, which is nothing the recogniser names; a uniform one keeps
    /// it a cylinder with the scaled radius. Both keep the caps planes.
    #[test]
    fn a_non_uniform_scale_makes_a_cylinder_freeform_and_a_uniform_one_keeps_it() {
        let r = rod(5.0, 12.0);
        let squashed = transformed(&r, &scaling(Vec3::ZERO, Vec3::new(2.0, 1.0, 1.0)));
        let classes = classes_of(&squashed);
        assert_eq!(classes.iter().filter(|c| matches!(c, SurfaceClass::Plane { .. })).count(), 2, "{classes:?}");
        assert_eq!(classes.iter().filter(|c| matches!(c, SurfaceClass::Other)).count(), 2, "{classes:?}");
        // Each tessellated afresh, so equal only to the chord error.
        assert!(near(kernel::soup_volume(&squashed, TOL).unwrap(), 2.0 * kernel::soup_volume(&r, TOL).unwrap(), 0.01));

        let doubled = transformed(&r, &scaling(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0)));
        let classes = classes_of(&doubled);
        assert_eq!(classes.iter().filter(|c| matches!(c, SurfaceClass::Cylinder { radius, .. } if near(*radius, 10.0, 1e-9))).count(), 2, "{classes:?}");
        assert!(near(kernel::soup_volume(&doubled, TOL).unwrap(), 8.0 * kernel::soup_volume(&r, TOL).unwrap(), 0.01));

        // The mesh path under the same non-uniform scale: measured again,
        // still a solid, its plane normals still perpendicular to the caps
        // and its wall class conservative.
        let mb = tessellate::tessellate(&r, TOL).unwrap();
        let t = scaling(Vec3::ZERO, Vec3::new(2.0, 1.0, 1.0));
        assert!(!is_uniform(&t) && is_uniform(&scaling(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0))));
        let img = remapped_mesh_body(&mb, &t, None);
        assert!(solid::validate(&img.0).is_solid());
        assert!(near(img.0.volume(), 2.0 * mb.0.volume(), 1e-6));
        assert_eq!(img.1.faces.iter().filter(|f| matches!(f.surface, SurfaceClass::Plane { .. })).count(), 2);
        assert_eq!(img.1.faces.iter().filter(|f| matches!(f.surface, SurfaceClass::Other)).count(), 2);
        // With the B-rep's verdict supplied, the faces carry it.
        let img = remapped_mesh_body(&mb, &t, Some(&surfaces_of(&squashed)));
        assert_eq!(img.1.faces.iter().filter(|f| matches!(f.surface, SurfaceClass::Other)).count(), 2);
        // The wall no class names carries its own scaled patch for its
        // normals; the planes need none.
        assert_eq!(img.1.faces.iter().filter(|f| f.geometry.is_some()).count(), 2);
        assert!(img.1.faces.iter().all(|f| f.geometry.is_some() == matches!(f.surface, SurfaceClass::Other)));
        // A normal under a non-uniform scale is not the scaled normal: the
        // diagonal of a unit square scaled 2× in x leans the other way.
        let n = normal_map(&t, Vec3::new(1.0, 1.0, 0.0).normalized());
        assert!(near(n.x / n.y, 0.5, 1e-9), "{n:?}");
    }

    /// The hull of two equal spheres is a capsule, to within the chord
    /// error; the hull of a box alone is the box, six faces, because
    /// coplanar triangles are one face; and every face is `hull`.
    #[test]
    fn a_hull_of_two_spheres_is_a_capsule_and_a_hull_of_a_box_has_six_faces() {
        let points = |s: &KSolid| -> Vec<Vec3> {
            let (m, _) = tessellate::tessellate(s, TOL).unwrap();
            m.positions.chunks_exact(3).map(|p| Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64)).collect()
        };
        let mut pts = points(&sphere(Vec3::ZERO, 5.0).unwrap());
        pts.extend(points(&sphere(Vec3::new(20.0, 0.0, 0.0), 5.0).unwrap()));
        let h = hull_of_points(&pts, "h/hull", TOL).unwrap();
        assert!(solid::validate(&h.0).is_solid());
        let capsule = 4.0 / 3.0 * PI * 125.0 + PI * 25.0 * 20.0;
        let got = h.0.volume();
        assert!(got < capsule && near(got, capsule, 0.02), "volume {got}, capsule {capsule}");
        for f in &h.1.faces {
            assert!(f.name == "h/hull" || (f.name.starts_with("h/hull#") && f.aliases.contains(&"h/hull".to_owned())), "{}", f.name);
        }

        let (_, b) = block(20.0, 10.0, 6.0);
        let h = hull_of_points(&points(&b), "h/hull", TOL).unwrap();
        assert_eq!(h.1.faces.len(), 6, "{:?}", names(&h));
        assert!(near(h.0.volume(), 1200.0, 1e-6));
        assert!(h.1.faces.iter().any(|f| f.normal.z > 0.999 && near(f.area, 200.0, 1e-6)));

        // A hull of a sphere and a box is bigger than either and fits its box.
        let mut pts = points(&sphere(Vec3::ZERO, 5.0).unwrap());
        pts.extend(points(&kernel::transformed(&b, &Transform::translation(Vec3::new(10.0, -5.0, -3.0)))));
        let h = hull_of_points(&pts, "h/hull", TOL).unwrap();
        assert!(solid::validate(&h.0).is_solid());
        let v = h.0.volume();
        assert!(v > 1200.0 && v > 4.0 / 3.0 * PI * 125.0 && v < 35.0 * 10.0 * 10.0, "{v}");

        // Flat points have no hull that is a solid.
        assert!(hull_of_points(&[Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X + Vec3::Y], "h/hull", TOL).is_err());
    }
}


