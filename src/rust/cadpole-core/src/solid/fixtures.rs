//! Hand-built meshes with exactly the defect each one is named after, for
//! tests in any crate. Built by hand rather than through the kernel so a test
//! that says "an internal wall" really has one, and so the checks are tested
//! without a boolean in the loop.

use crate::geom::Vec3;
use crate::tessellate::Mesh;
use crate::fmath;

/// Append one triangle, keeping `face_ids` in step.
pub fn push_tri(m: &mut Mesh, a: Vec3, b: Vec3, c: Vec3, face: u32) {
    let base = (m.positions.len() / 3) as u32;
    let n = (b - a).cross(c - a).normalized();
    for p in [a, b, c] {
        m.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
        m.normals.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
    }
    m.indices.extend_from_slice(&[base, base + 1, base + 2]);
    m.face_ids.push(face);
}

/// Concatenate `b`'s triangles onto `m` without a boolean, face ids offset
/// past `m`'s.
pub fn append(m: &mut Mesh, b: &Mesh) {
    let base = (m.positions.len() / 3) as u32;
    let faces = m.face_ids.iter().max().map(|f| f + 1).unwrap_or(0);
    m.positions.extend_from_slice(&b.positions);
    m.normals.extend_from_slice(&b.normals);
    m.indices.extend(b.indices.iter().map(|i| i + base));
    m.face_ids.extend(b.face_ids.iter().map(|f| f + faces));
}

/// A closed axis-aligned box from `lo` to `hi`, outward normals, six face
/// groups (one per side, in the order -X +X -Y +Y -Z +Z).
pub fn box_between(lo: Vec3, hi: Vec3) -> Mesh {
    let mut m = Mesh::default();
    add_box(&mut m, lo, hi, false, 0);
    m
}

/// `box_between(0, (w,d,h))`.
pub fn box_mesh(w: f64, d: f64, h: f64) -> Mesh {
    box_between(Vec3::ZERO, Vec3::new(w, d, h))
}

/// Add a box to `m`. `flip` reverses every winding, which turns the box into
/// the *cavity* of a shell (its normals then point into the cavity, away from
/// the material, exactly like the outer shell's do).
pub fn add_box(m: &mut Mesh, lo: Vec3, hi: Vec3, flip: bool, face_base: u32) {
    let v = |i: usize| {
        Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        )
    };
    // Each side as two triangles, wound counter-clockwise seen from outside.
    let sides: [[usize; 4]; 6] = [
        [0, 4, 6, 2], // -X
        [1, 3, 7, 5], // +X
        [0, 1, 5, 4], // -Y
        [2, 6, 7, 3], // +Y
        [0, 2, 3, 1], // -Z
        [4, 5, 7, 6], // +Z
    ];
    for (f, s) in sides.iter().enumerate() {
        let (a, b, c, d) = (v(s[0]), v(s[1]), v(s[2]), v(s[3]));
        let id = face_base + f as u32;
        if flip {
            push_tri(m, a, c, b, id);
            push_tri(m, a, d, c, id);
        } else {
            push_tri(m, a, b, c, id);
            push_tri(m, a, c, d, id);
        }
    }
}

/// A closed box shell: outer box `size` cubed, walls exactly `wall` mm thick
/// on all six sides. Two shells in one mesh, the inner one inverted, which is
/// how a solid with a cavity is represented.
pub fn hollow_box(size: f64, wall: f64) -> Mesh {
    let mut m = Mesh::default();
    add_box(&mut m, Vec3::ZERO, Vec3::new(size, size, size), false, 0);
    add_box(&mut m, Vec3::new(wall, wall, wall), Vec3::new(size - wall, size - wall, size - wall), true, 6);
    m
}

/// A box with `count` triangles deleted, leaving a boundary hole. The removed
/// triangles are the two of the +Z face, so the hole is the whole top.
pub fn holed_box(w: f64, d: f64, h: f64) -> Mesh {
    let full = box_mesh(w, d, h);
    let keep = full.triangle_count() - 2;
    let mut m = Mesh { positions: full.positions.clone(), normals: full.normals.clone(), ..Mesh::default() };
    m.indices = full.indices[..keep * 3].to_vec();
    m.face_ids = full.face_ids[..keep].to_vec();
    m
}

/// Two boxes that overlap, concatenated without a boolean: every triangle is
/// fine on its own, but the two skins pass through each other.
pub fn overlapping_boxes() -> Mesh {
    let mut m = box_mesh(10.0, 10.0, 10.0);
    append(&mut m, &box_between(Vec3::new(5.0, 5.0, 5.0), Vec3::new(15.0, 15.0, 15.0)));
    m
}

/// A box with one side's winding reversed: still closed, but that side's
/// normal points into the solid.
pub fn flipped_face_box(w: f64, d: f64, h: f64) -> Mesh {
    let mut m = box_mesh(w, d, h);
    // The +Z face is the last two triangles.
    let n = m.indices.len();
    for t in [n - 6, n - 3] {
        m.indices.swap(t + 1, t + 2);
    }
    m
}

/// A box with a zero-area triangle stapled onto one corner.
pub fn box_with_degenerate(w: f64, d: f64, h: f64) -> Mesh {
    let mut m = box_mesh(w, d, h);
    let a = Vec3::new(0.0, 0.0, 0.0);
    let b = Vec3::new(w, 0.0, 0.0);
    push_tri(&mut m, a, b, a + (b - a) * 0.5, 99);
    m
}

/// A solid with exactly one fold in it: a 10 mm box with a slit cut 9 mm
/// into its top at x = 5, `width` wide at the top and closing to an edge at
/// the bottom. The two slit walls face each other (normals antiparallel, to
/// within the tilt `width` gives them over 9 mm), share the slit's bottom
/// edge, and their free corners at the top are `width` apart: that is a
/// fold with tip gap `width`, at (5, 5, 1). Everything else is a plain box,
/// so the surface is closed, consistently oriented, embedded and of positive
/// volume — a solid by every test, carrying the one defect those tests are
/// blind to. It is the impeller's fin, in a shape a person can draw.
///
/// `width` a hair (0.05) is the fin a grazing cut leaves; 0 is an exact fin,
/// whose tips weld into one vertex and whose walls then double up (the
/// validator reports *that* as an internal face); a millimetre or more is a
/// real slit, a few degrees open, and no fold at all.
///
/// Face ids: 0 bottom, 1 and 2 the two halves of the top, 3 left, 4 right,
/// 5 front, 6 back, 7 the left slit wall, 8 the right slit wall — so the
/// fold's faces are (7, 8).
pub fn slit_box(width: f64) -> Mesh {
    let h = width / 2.0;
    let (b0, b1) = (Vec3::new(5.0, 0.0, 1.0), Vec3::new(5.0, 10.0, 1.0));
    let (l0, l1) = (Vec3::new(5.0 - h, 0.0, 10.0), Vec3::new(5.0 - h, 10.0, 10.0));
    let (r0, r1) = (Vec3::new(5.0 + h, 0.0, 10.0), Vec3::new(5.0 + h, 10.0, 10.0));
    let c = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    // Each face as a polygon fanned from its first vertex (the notched front
    // and back are fanned from the slit's bottom vertex, which sees every
    // other corner), with the direction the face's normal must point. The
    // winding is fixed up per face below, so the list is about shape only.
    let faces: [(Vec<Vec3>, Vec3); 9] = [
        (vec![c(0.0, 0.0, 0.0), c(10.0, 0.0, 0.0), c(10.0, 10.0, 0.0), c(0.0, 10.0, 0.0)], c(0.0, 0.0, -1.0)),
        (vec![c(0.0, 0.0, 10.0), l0, l1, c(0.0, 10.0, 10.0)], c(0.0, 0.0, 1.0)),
        (vec![r0, c(10.0, 0.0, 10.0), c(10.0, 10.0, 10.0), r1], c(0.0, 0.0, 1.0)),
        (vec![c(0.0, 0.0, 0.0), c(0.0, 10.0, 0.0), c(0.0, 10.0, 10.0), c(0.0, 0.0, 10.0)], c(-1.0, 0.0, 0.0)),
        (vec![c(10.0, 0.0, 0.0), c(10.0, 10.0, 0.0), c(10.0, 10.0, 10.0), c(10.0, 0.0, 10.0)], c(1.0, 0.0, 0.0)),
        (vec![b0, r0, c(10.0, 0.0, 10.0), c(10.0, 0.0, 0.0), c(0.0, 0.0, 0.0), c(0.0, 0.0, 10.0), l0], c(0.0, -1.0, 0.0)),
        (vec![b1, l1, c(0.0, 10.0, 10.0), c(0.0, 10.0, 0.0), c(10.0, 10.0, 0.0), c(10.0, 10.0, 10.0), r1], c(0.0, 1.0, 0.0)),
        // The slit walls: the bottom edge b0-b1 is in each wall's first
        // triangle, so exactly those two triangles share it.
        (vec![b0, b1, l1, l0], c(1.0, 0.0, 0.0)),
        (vec![b0, b1, r1, r0], c(-1.0, 0.0, 0.0)),
    ];
    let mut m = Mesh::default();
    for (id, (poly, outward)) in faces.iter().enumerate() {
        let n = (poly[1] - poly[0]).cross(poly[2] - poly[0]);
        let flip = n.dot(*outward) < 0.0;
        for k in 1..poly.len() - 1 {
            let (a, b) = if flip { (poly[k + 1], poly[k]) } else { (poly[k], poly[k + 1]) };
            push_tri(&mut m, poly[0], a, b, id as u32);
        }
    }
    m
}

/// A torus about Z: major radius `big`, tube radius `small`, `n` segments
/// around the ring and `k` around the tube. Outward normals, one face group.
pub fn torus_mesh(big: f64, small: f64, n: usize, k: usize) -> Mesh {
    use std::f64::consts::TAU;
    let p = |i: usize, j: usize| {
        let (u, v) = (TAU * i as f64 / n as f64, TAU * j as f64 / k as f64);
        let r = big + small * fmath::cos(v);
        Vec3::new(r * fmath::cos(u), r * fmath::sin(u), small * fmath::sin(v))
    };
    let mut m = Mesh::default();
    for i in 0..n {
        for j in 0..k {
            let (a, b, c, d) = (p(i, j), p(i + 1, j), p(i + 1, j + 1), p(i, j + 1));
            push_tri(&mut m, a, b, c, 0);
            push_tri(&mut m, a, c, d, 0);
        }
    }
    m
}
