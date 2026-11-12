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
