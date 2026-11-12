//! Hand-built meshes for the tests, with exactly the defect each one is named
//! after. Built by hand rather than through the kernel so a test that says
//! "a 0.6 mm wall" really has a 0.6 mm wall, and so the checks are tested
//! without a boolean in the loop.
//!
//! The closed boxes, the hollow box and the broken boxes (a hole, a flipped
//! face, a degenerate triangle, two skins through each other) are
//! [`cadpole_core::solid::fixtures`]: the validator's own tests use them, and
//! "the same defect" should mean the same mesh on both sides. What is here is
//! the shapes only the print checks care about.

use cadpole_core::geom::Vec3;
use cadpole_core::tessellate::Mesh;

pub use cadpole_core::solid::fixtures::*;

/// A prism: the polygon `pts` (x, z), counter-clockwise, triangulated by
/// `cap` (index triples into `pts`), swept along +Y from 0 to `depth`.
pub fn prism(pts: &[(f64, f64)], cap: &[[usize; 3]], depth: f64) -> Mesh {
    let mut m = Mesh::default();
    let p = |i: usize, y: f64| Vec3::new(pts[i].0, y, pts[i].1);
    // Sides.
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        let (a, b, c, d) = (p(i, 0.0), p(i, depth), p(j, depth), p(j, 0.0));
        push_tri(&mut m, a, b, c, i as u32);
        push_tri(&mut m, a, c, d, i as u32);
    }
    let base = pts.len() as u32;
    for t in cap {
        push_tri(&mut m, p(t[0], 0.0), p(t[1], 0.0), p(t[2], 0.0), base);
        push_tri(&mut m, p(t[0], depth), p(t[2], depth), p(t[1], depth), base + 1);
    }
    m
}

/// Fan triangulation, valid for a convex polygon.
pub fn fan(n: usize) -> Vec<[usize; 3]> {
    (1..n - 1).map(|i| [0, i, i + 1]).collect()
}

/// A prism leaning `degrees` off vertical: its +X side is a downward-facing
/// overhang at exactly that angle, its -X side the matching upward-facing
/// slope, and the top and bottom are flat.
///
/// `base` is the footprint along X, `height` the Z extent, `depth` the Y
/// extent. The overhanging face has area `depth * height / cos(degrees)`.
pub fn leaning_prism(base: f64, height: f64, degrees: f64, depth: f64) -> Mesh {
    let t = height * degrees.to_radians().tan();
    let pts = [(0.0, 0.0), (base, 0.0), (base + t, height), (t, height)];
    prism(&pts, &fan(4), depth)
}

/// A plate with a single upstanding rib: a `width` x `base` slab in the XZ
/// plane with a `rib` mm wide, `rib_height` mm tall fin centred on top of it,
/// swept `depth` along Y.
///
/// The rib is the thin feature: its two faces are parallel and `rib` apart, so
/// the wall under each of them is exactly `rib`, and nothing else in the shape
/// is thinner than `base`. Every other edge in it is a right angle, which is
/// the case a thickness check must *not* call a thin wall.
pub fn ribbed_plate(width: f64, base: f64, rib: f64, rib_height: f64, depth: f64) -> Mesh {
    let (l, r) = ((width - rib) / 2.0, (width + rib) / 2.0);
    let top = base + rib_height;
    let pts = [
        (0.0, 0.0),
        (width, 0.0),
        (width, base),
        (r, base),
        (r, top),
        (l, top),
        (l, base),
        (0.0, base),
    ];
    // Non-convex: the slab as a fan from corner 0, then the rib.
    let cap = [[0, 1, 2], [0, 2, 3], [0, 3, 6], [0, 6, 7], [3, 4, 5], [3, 5, 6]];
    prism(&pts, &cap, depth)
}

/// An L-bracket: two arms of `thick` mm, `arm` mm long, in the XZ plane,
/// swept `depth` along Y. Printed on its back (the flat face down) it needs
/// no support; stood on the end of one arm it is tall, tippy and overhangs.
pub fn l_bracket(arm: f64, thick: f64, depth: f64) -> Mesh {
    let pts = [(0.0, 0.0), (arm, 0.0), (arm, thick), (thick, thick), (thick, arm), (0.0, arm)];
    // Non-convex: split into the two rectangles that make the L.
    let cap = [[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]];
    prism(&pts, &cap, depth)
}

/// Boxes joined exactly (`cadpole_core::gridjoin`), as one closed mesh with
/// shared edges -- the shape a story of a box-built model has.
pub fn joined(boxes: &[([f64; 3], [f64; 3])]) -> Mesh {
    let gb: Vec<cadpole_core::gridjoin::GridBox> = boxes
        .iter()
        .map(|(lo, hi)| cadpole_core::gridjoin::GridBox { lo: *lo, hi: *hi, sides: [0, 1, 2, 3, 4, 5] })
        .collect();
    let g = cadpole_core::gridjoin::union_boxes(&gb, 1e-3, 6).expect("a union of boxes");
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

/// A closed surface of revolution about +Z: `profile` is `(radius, z)` from
/// the bottom of the axis to the top, both ends on the axis (radius 0),
/// turned in `n` steps. Wound so its normals point out.
pub fn revolve(profile: &[(f64, f64)], n: usize) -> Mesh {
    let mut m = Mesh::default();
    let at = |k: usize, i: usize| {
        let a = std::f64::consts::TAU * (i % n) as f64 / n as f64;
        let (r, z) = profile[k];
        Vec3::new(r * a.cos(), r * a.sin(), z)
    };
    for k in 0..profile.len() - 1 {
        for i in 0..n {
            let (a, b, c, d) = (at(k, i), at(k, i + 1), at(k + 1, i + 1), at(k + 1, i));
            if profile[k].0 > 0.0 {
                push_tri(&mut m, a, b, c, k as u32);
            }
            if profile[k + 1].0 > 0.0 {
                push_tri(&mut m, a, c, d, k as u32);
            }
        }
    }
    m
}

/// A ball of radius `r` resting on z = 0, `rings` steps from pole to pole.
pub fn ball_profile(r: f64, rings: usize) -> Vec<(f64, f64)> {
    (0..=rings)
        .map(|k| {
            let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * k as f64 / rings as f64;
            let rad = if k == 0 || k == rings { 0.0 } else { r * a.cos() };
            (rad, r * (1.0 + a.sin()))
        })
        .collect()
}

/// A ball of radius `r` on a round stalk of radius `stalk`, the stalk
/// standing on z = 0 and `height` tall to where the ball's centre is.
pub fn lollipop_profile(r: f64, stalk: f64, height: f64) -> Vec<(f64, f64)> {
    let join = (r * r - stalk * stalk).sqrt();
    let a0 = (-join / r).asin();
    let mut p = vec![(0.0, 0.0), (stalk, 0.0)];
    let steps = 32;
    for k in 0..steps {
        let a = a0 + (std::f64::consts::FRAC_PI_2 - a0) * k as f64 / steps as f64;
        p.push((r * a.cos(), height + r * a.sin()));
    }
    p.push((0.0, height + r));
    p
}

/// `m` moved by `d`.
pub fn translated(m: &Mesh, d: Vec3) -> Mesh {
    let mut out = m.clone();
    for p in out.positions.chunks_exact_mut(3) {
        p[0] += d.x as f32;
        p[1] += d.y as f32;
        p[2] += d.z as f32;
    }
    out
}

/// A block `size` wide and tall (X, Z) and `depth` deep (Y), on z = 0,
/// with a round hole `d` across through it along Y at its centre, and its
/// topology: the bore is face 3, `block/bore`, a cylinder. `teardrop`
/// gives the hole a 45° peaked top instead (two flat faces, 4), the
/// shape that prints without support. `n` steps round, a multiple of 8.
pub fn block_with_bore(
    size: f64,
    depth: f64,
    d: f64,
    n: usize,
    teardrop: bool,
) -> (Mesh, cadpole_core::tessellate::Topology) {
    use cadpole_core::surface_class::SurfaceClass;
    use cadpole_core::tessellate::{FaceInfo, Topology};
    let (h, r) = (size / 2.0, d / 2.0);
    let ring = |i: usize| -> ((f64, f64), (f64, f64), bool) {
        let a = std::f64::consts::TAU * (i % n) as f64 / n as f64;
        let (c, s) = (a.cos(), a.sin());
        let outer = h / c.abs().max(s.abs());
        let deg = a.to_degrees();
        let (t, peak) = if teardrop && deg > 45.0 + 1e-9 && deg < 135.0 - 1e-9 {
            let t = if deg <= 90.0 { r * 2f64.sqrt() / (c + s) } else { r * 2f64.sqrt() / (s - c) };
            (t, true)
        } else {
            (r, false)
        };
        ((h + outer * c, h + outer * s), (h + t * c, h + t * s), peak)
    };
    let p = |xz: (f64, f64), y: f64| Vec3::new(xz.0, y, xz.1);
    let mut m = Mesh::default();
    for i in 0..n {
        let ((o0, i0, k0), (o1, i1, k1)) = (ring(i), ring(i + 1));
        // Front (y = 0) and back caps, the outer sides, the bore.
        push_tri(&mut m, p(o0, 0.0), p(o1, 0.0), p(i1, 0.0), 0);
        push_tri(&mut m, p(o0, 0.0), p(i1, 0.0), p(i0, 0.0), 0);
        push_tri(&mut m, p(o0, depth), p(i1, depth), p(o1, depth), 1);
        push_tri(&mut m, p(o0, depth), p(i0, depth), p(i1, depth), 1);
        push_tri(&mut m, p(o0, 0.0), p(o1, depth), p(o1, 0.0), 2);
        push_tri(&mut m, p(o0, 0.0), p(o0, depth), p(o1, depth), 2);
        // A segment between two peak points, or from the 45 degree
        // tangent point into the peak, is on the flat top.
        let flat = teardrop && (k0 || k1);
        let face = if flat { 4 } else { 3 };
        push_tri(&mut m, p(i0, 0.0), p(i1, 0.0), p(i1, depth), face);
        push_tri(&mut m, p(i0, 0.0), p(i1, depth), p(i0, depth), face);
    }
    let names = ["block/front", "block/back", "block/side", "block/bore", "block/peak"];
    let topo = Topology {
        faces: names
            .iter()
            .enumerate()
            .map(|(k, name)| FaceInfo {
                name: (*name).to_owned(),
                surface: if k == 3 {
                    SurfaceClass::Cylinder { axis_point: Vec3::new(h, depth / 2.0, h), axis: Vec3::Y, radius: r }
                } else {
                    SurfaceClass::Other
                },
                ..FaceInfo::default()
            })
            .collect(),
        ..Topology::default()
    };
    (m, topo)
}
