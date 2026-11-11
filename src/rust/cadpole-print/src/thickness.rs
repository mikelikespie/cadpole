//! Wall thickness: how much plastic is actually there.
//!
//! **Method.** For every triangle, start just inside the surface at its
//! centroid and cast rays *into* the solid: one straight down the inward
//! surface normal plus four more on a 18° cone around it. The wall is the shortest
//! distance at which one of them leaves the solid again through the opposite
//! surface. The cone is what makes this closer to a sphere fit than to a
//! single probe — a thin fin that the axial ray shoots straight past is caught
//! by a cone ray, and on a flat slab the axial ray is already the minimum
//! (`wall / cos 18°` for the others), so a slab measures exactly its thickness.
//!
//! A hit only counts when the surface it lands on is the far side of the
//! *same wall*, which is two tests, not one. It must face away along the ray
//! (`dot(n_hit, dir) > 0`), so we are leaving the solid rather than entering
//! it; and it must lean back towards the face we started on
//! (`dot(n_hit, n_start) < -sin FACING_DEGREES`), which is what makes it the
//! other side of a wall rather than some other surface the ray happened to
//! reach. The second test is measured against the starting triangle's own
//! normal, not against the individual cone ray, because it is a question about
//! the two *surfaces* and not about how we got there.
//!
//! Both normals are the **surface's**, not the facet's: the mesh's vertex
//! normals averaged over the triangle. On a curved wall the two differ by
//! several degrees, because a facet spanning one row of a tessellated cylinder
//! is tilted by roughly `atan(tolerance / row height)` out of the surface it
//! belongs to — 5° on the bolt holes of `examples/bracket.star` — and that
//! tilt is a property of how finely the surface was cut up, not of the part.
//! Measuring along it aims the axial ray at the rim it is standing on, and
//! testing with it makes a perpendicular neighbour look like a wall. Both
//! kernels supply vertex normals; a mesh that carries none, or whose normals
//! disagree with its winding, falls back to the facet.
//!
//! Without the facing test the check cries wolf on ordinary parts. Two
//! faces meeting at a right angle — a cylinder's rim, the top of a boss, the
//! edge of a plate — bound a convex edge, not a wall. A centroid within
//! `eps * tan 18°` of such an edge is close enough for one cone ray to clip
//! the perpendicular neighbour from inside, and `dot(n_hit, dir)` is then
//! `sin 18° > 0`, so the sliver would pass as a wall: a plain solid cylinder
//! would report a 0.13 mm wall around its rim.
//!
//! Anything below `profile.min_wall` (default 0.4 mm nozzle × 2 perimeters =
//! 0.8 mm) is reported, with the thinnest spots located. A wall thinner than
//! the perimeters that must fit in it either disappears in the slicer or
//! prints as one squashed bead with no bonding surface.
//!
//! **Where this still over-reports, and why that part is left alone.** Ray
//! casting measures a real distance, and where two faces genuinely lean
//! *towards* each other the real distance really does go to zero. The vase's
//! rim is the example in the tree: a flat annulus on top of a wall that flares
//! out near the lip, so the wedge of material just inside the outer edge
//! tapers away, and this reports a fraction of a millimetre over a couple of
//! hundred square millimetres however the profile is drawn. Those hits pass
//! the facing test because the surfaces converge, and the thin wedge is
//! geometry rather than a measurement artefact — a slicer will do just as
//! little with it. Only the perpendicular and divergent cases, where nothing
//! tapers, are rejected.
//!
//! `FACING_DEGREES` is where the line sits, and it has to be off zero: a
//! tessellated cylinder's side facets are perpendicular to its cap only to
//! within f32 rounding, so an exact `< 0` would accept half of them and the
//! false positive would come back on half the rims in the tree. Five degrees
//! also covers a draft angle of a few degrees, which is a moulding allowance
//! rather than a thin wall. Genuinely tapered walls, where the faces converge
//! by more than that, are still measured and still reported.
//!
//! A medial-axis or sphere-fit measure remains a different and much larger
//! piece of work; it would not change any of the above, because the largest
//! inscribed sphere tangent at a point a hair from a convex edge is that hair
//! wide, so a sphere fit needs the same rule about which surfaces bound a
//! wall. Two ways to refine it: report the thin area as a
//! fraction of the surface, and say when a thin region hugs an edge rather
//! than forming a band.

use cadpole_core::geom::Vec3;
use cadpole_core::render::bvh::Bvh;
use cadpole_core::tessellate::Mesh;

use crate::{Check, Issue, Profile, Severity, centroid, cluster, fmt_at, num, scale_of, tri, tri_normal_area};

/// Half-angle of the probe cone, in degrees.
const CONE_DEGREES: f64 = 18.0;
/// Rays on the cone, evenly spaced around it.
const CONE_RAYS: usize = 4;
/// How far past perpendicular the far surface must lean back towards the face
/// we started on before its hit counts as a wall, in degrees. Perpendicular is
/// a convex edge; see the facing test in the module doc.
const FACING_DEGREES: f64 = 5.0;

#[derive(Clone, Debug, Default)]
pub struct ThicknessReport {
    /// Wall thickness in mm under each triangle, in `mesh.indices` order.
    /// `f32::INFINITY` when no opposite surface was found (an open mesh, or a
    /// triangle whose normal points into the void). This is the field the
    /// `thickness` analysis view colour-maps.
    pub per_triangle: Vec<f32>,
    /// Thinnest measured wall in mm; `f64::INFINITY` when nothing measured.
    pub min: f64,
    /// Surface area, in mm², sitting over a wall thinner than the minimum.
    pub thin_area: f64,
    /// Up to eight representative thin spots: `(position, mm)`, thinnest
    /// first, spread out so they are eight *places*, not eight triangles of
    /// the same place.
    pub spots: Vec<(Vec3, f64)>,
    /// Triangles that returned a finite measurement.
    pub measured: usize,
}

pub fn check(mesh: &Mesh, profile: &Profile, issues: &mut Vec<Issue>) -> ThicknessReport {
    let mut r = ThicknessReport { min: f64::INFINITY, ..Default::default() };
    if mesh.triangle_count() == 0 {
        return r;
    }
    let bvh = Bvh::from_mesh(mesh);
    let scale = scale_of(mesh);
    // Step off the surface by enough to clear the triangle we started on,
    // but far less than any wall we would report.
    let eps = (scale * 1e-6).min(profile.min_wall * 1e-3).max(1e-9);
    let far = scale * 4.0;

    r.per_triangle = vec![f32::INFINITY; mesh.triangle_count()];
    let mut thin = Vec::new();
    for t in 0..mesh.triangle_count() {
        let v = tri(mesh, t);
        let (n, area) = tri_normal_area(&v);
        if area <= 0.0 {
            continue;
        }
        let c = centroid(&v);
        let n = surface_normal(mesh, t, n);
        let Some(d) = probe(&bvh, mesh, c, -n, eps, far) else { continue };
        r.per_triangle[t] = d as f32;
        r.measured += 1;
        if d < r.min {
            r.min = d;
        }
        if d < profile.min_wall {
            r.thin_area += area;
            thin.push((c, d));
        }
    }

    r.spots = cluster(thin, 8, (scale * 0.05).max(profile.min_wall * 2.0), true);
    if !r.spots.is_empty() {
        let (p, d) = r.spots[0];
        issues.push(Issue::new(
            Check::Thickness,
            Severity::Warning,
            format!(
                "wall is {} mm at {}, below the {} mm minimum ({} nozzle × {} perimeters); \
                 {} mm² of surface is over thin walls, in {} place{}",
                num(d),
                fmt_at(p),
                num(profile.min_wall),
                num(profile.nozzle),
                num(profile.perimeters),
                num(r.thin_area),
                r.spots.len(),
                if r.spots.len() == 1 { "" } else { "s" },
            ),
            Some(p),
            d,
        ));
    }
    r
}

/// Shortest distance from `from` to the far side of the wall, probing along
/// `dir` — the inward normal of the face we are standing on — and a cone
/// around it. `None` when nothing valid was hit.
fn probe(bvh: &Bvh, mesh: &Mesh, from: Vec3, dir: Vec3, eps: f64, far: f64) -> Option<f64> {
    let (t1, t2) = basis(dir);
    let a = CONE_DEGREES.to_radians();
    let (sa, ca) = (a.sin(), a.cos());
    let mut best = f64::INFINITY;
    for i in 0..=CONE_RAYS {
        let d = if i == 0 {
            dir
        } else {
            let phi = std::f64::consts::TAU * (i - 1) as f64 / CONE_RAYS as f64;
            (dir * ca + t1 * (sa * phi.cos()) + t2 * (sa * phi.sin())).normalized()
        };
        // Start just inside so the triangle we are standing on cannot be hit.
        let o = from + d * eps;
        if let Some(t) = hit(bvh, mesh, o, d, dir, eps, far) {
            let full = t + eps;
            if full < best {
                best = full;
            }
        }
    }
    if best.is_finite() { Some(best) } else { None }
}

/// First hit along the ray `d` that is the far side of the wall we are inside
/// of: a facet we are leaving through (`n_facet . d > 0`, so the ray is on its
/// way out of the solid and not into it) whose *surface* also leans back
/// towards the face we started on. `axis` is that face's inward surface
/// normal, so `n_hit . axis` is positive exactly when the two outward normals
/// are more than a right angle apart — when the material between them is a
/// wall and not a convex edge.
fn hit(bvh: &Bvh, mesh: &Mesh, o: Vec3, d: Vec3, axis: Vec3, eps: f64, far: f64) -> Option<f64> {
    let h = bvh.intersect(
        [o.x as f32, o.y as f32, o.z as f32],
        [d.x as f32, d.y as f32, d.z as f32],
        eps as f32,
        far as f32,
    )?;
    let t = bvh.tri(h.tri).tri as usize;
    let (facet, _) = tri_normal_area(&tri(mesh, t));
    if facet.dot(d) <= 0.0 {
        return None;
    }
    let n = surface_normal(mesh, t, facet);
    if n.dot(axis) > FACING_DEGREES.to_radians().sin() { Some(h.t as f64) } else { None }
}

/// The outward normal of the *surface* under triangle `t`: its three vertex
/// normals averaged. `facet` is the normal from the winding, used when the
/// mesh carries no normals for those vertices, when they cancel out, or when
/// they point the other way — none of which describes the surface.
fn surface_normal(mesh: &Mesh, t: usize, facet: Vec3) -> Vec3 {
    let mut sum = Vec3::ZERO;
    for k in 0..3 {
        let v = mesh.indices[t * 3 + k] as usize * 3;
        let Some(n) = mesh.normals.get(v..v + 3) else { return facet };
        sum = sum + Vec3::new(n[0] as f64, n[1] as f64, n[2] as f64);
    }
    let l = sum.length();
    if !l.is_finite() || l <= 1e-9 {
        return facet;
    }
    let n = sum * (1.0 / l);
    if n.dot(facet) > 0.5 { n } else { facet }
}

/// Any two unit vectors perpendicular to `d` and to each other.
fn basis(d: Vec3) -> (Vec3, Vec3) {
    let a = if d.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let t1 = d.cross(a).normalized();
    (t1, d.cross(t1).normalized())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    fn run(m: &Mesh, min_wall: f64) -> (ThicknessReport, Vec<Issue>) {
        let p = Profile { min_wall, ..Profile::default() };
        let mut issues = Vec::new();
        let r = check(m, &p, &mut issues);
        (r, issues)
    }

    #[test]
    fn a_solid_box_measures_its_own_size() {
        let (r, issues) = run(&box_mesh(20.0, 20.0, 20.0), 0.8);
        assert!((r.min - 20.0).abs() < 1e-3, "min {}", r.min);
        assert_eq!(r.measured, 12);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn a_thin_shell_is_flagged_at_its_real_thickness() {
        // 0.6 mm walls against a 0.8 mm minimum.
        let (r, issues) = run(&hollow_box(20.0, 0.6), 0.8);
        assert!((r.min - 0.6).abs() < 1e-3, "min {}", r.min);
        assert_eq!(issues.len(), 1);
        let m = &issues[0].message;
        assert!(m.contains("0.6 mm"), "{m}");
        assert!(m.contains("0.8 mm minimum"), "{m}");
        // Every triangle of both shells sits over a 0.6 mm wall: 6 faces x 2
        // triangles x 2 shells.
        assert_eq!(r.measured, 24);
        // Outer 20 mm cube (2400 mm²) plus the inner 18.8 mm cube (2120.6).
        assert!((r.thin_area - (2400.0 + 6.0 * 18.8 * 18.8)).abs() < 1e-3, "thin area {}", r.thin_area);
    }

    #[test]
    fn a_thick_shell_is_not_flagged() {
        let (r, issues) = run(&hollow_box(20.0, 1.6), 0.8);
        assert!((r.min - 1.6).abs() < 1e-3, "min {}", r.min);
        assert_eq!(r.thin_area, 0.0);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn the_minimum_default_is_two_perimeters_of_the_nozzle() {
        let p = Profile::with_nozzle(0.4, 2.0);
        assert!((p.min_wall - 0.8).abs() < 1e-12);
        // 1.0 mm walls pass two perimeters of 0.4 and fail three.
        let m = hollow_box(20.0, 1.0);
        assert!(check(&m, &p, &mut Vec::new()).thin_area == 0.0);
        let p3 = Profile::with_nozzle(0.4, 3.0);
        assert!(check(&m, &p3, &mut Vec::new()).thin_area > 0.0);
    }

    #[test]
    fn the_per_triangle_field_lines_up_with_the_mesh() {
        let m = hollow_box(20.0, 1.6);
        let (r, _) = run(&m, 0.8);
        assert_eq!(r.per_triangle.len(), m.triangle_count());
        assert!(r.per_triangle.iter().all(|t| (t - 1.6).abs() < 1e-2), "{:?}", r.per_triangle);
    }

    /// `cylinder(8, 11)` as `cadpole check` sees it: through the kernel and the
    /// tessellator, fan-triangulated caps and all.
    fn kernel_cylinder(r: f64, h: f64) -> Mesh {
        use cadpole_core::sketch::{Loop, P2, Sketch};
        use cadpole_core::geom::Plane;
        let s = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), r));
        let solid = cadpole_core::kernel::extrude(&s, h).expect("extrude").remove(0);
        cadpole_core::tessellate::tessellate(&solid, 0.05).expect("tessellate").0
    }

    #[test]
    fn a_solid_cylinder_has_no_thin_wall_at_its_rims() {
        // A plain solid boss, no thin wall anywhere in it, must not report a
        // wall on the rim where a cone ray clips the perpendicular side face
        // from inside.
        // The thinnest wall in a solid cylinder is the shorter of its height
        // and its diameter.
        let m = kernel_cylinder(8.0, 11.0);
        assert!(cadpole_core::solid::validate(&m).is_solid(), "the fixture must be a solid");
        let (r, issues) = run(&m, 0.8);
        assert!(issues.is_empty(), "a solid cylinder is not a thin wall: {issues:?}");
        assert!((r.min - 11.0).abs() < 0.1, "min {} should be the 11 mm height", r.min);
        assert_eq!(r.thin_area, 0.0);
        assert!(r.per_triangle.iter().all(|t| *t >= 10.9), "no triangle sits over a thin wall");
    }

    #[test]
    fn a_slab_thinner_than_two_perimeters_is_still_caught() {
        // box(20, 20, 0.6): both large faces sit over 0.6 mm of plastic. The
        // four rim strips are 0.6 mm *edges*, and the wall under them is the
        // 20 mm across the slab, so the thin area is the two faces.
        let (r, issues) = run(&box_mesh(20.0, 20.0, 0.6), 0.8);
        assert!((r.min - 0.6).abs() < 1e-4, "min {}", r.min);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].message.contains("0.6 mm"), "{}", issues[0].message);
        assert!((r.thin_area - 800.0).abs() < 1e-4, "thin area {}", r.thin_area);
    }

    #[test]
    fn a_rib_thinner_than_two_perimeters_is_still_caught() {
        // A 0.4 mm fin on a 3 mm plate: every edge in the shape is a right
        // angle, so the facing test must keep the rib's own two faces while
        // rejecting all of them.
        let m = ribbed_plate(20.0, 3.0, 0.4, 10.0, 10.0);
        assert!(cadpole_core::solid::validate(&m).is_solid(), "the fixture must be a solid");
        let (r, issues) = run(&m, 0.8);
        assert!((r.min - 0.4).abs() < 1e-4, "min {}", r.min);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].message.contains("0.4 mm"), "{}", issues[0].message);
        // The rib's two 10 x 10 faces, and nothing else: the plate is 3 mm.
        assert!((r.thin_area - 200.0).abs() < 1e-4, "thin area {}", r.thin_area);
        // And it is reported on the rib, not on a corner of the plate.
        let (p, _) = r.spots[0];
        assert!(p.z > 3.0, "the thin spot is on the rib, at {p:?}");
    }

    #[test]
    fn an_open_mesh_measures_nothing_rather_than_guessing() {
        let (r, issues) = run(&holed_box(20.0, 20.0, 20.0), 0.8);
        // The five remaining faces still see each other across the box.
        assert!(r.min.is_finite());
        assert!(issues.is_empty(), "an open box is not a thin-wall problem: {issues:?}");
    }
}
