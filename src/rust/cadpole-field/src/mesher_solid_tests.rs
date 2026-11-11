//! The mesher's promise, checked with the one definition of "a solid" the
//! rest of Cadpole uses (`cadpole_core::solid::validate`): `mesh_field`
//! returns a closed, consistently oriented, embedded surface, or an error.
//!
//! These are the shapes that used to come back non-manifold or self-crossing
//! while the index-level signed edge sum said "watertight": a hard union with
//! a concave crease, a smooth union, a box rotated off the grid, a body
//! imported as a field, two capsules meeting at a neck thinner than a cell,
//! and a chamfered box and a filleted L bracket through the fillet path.

use cadpole_core::geom::Plane;
use cadpole_core::kernel;
use cadpole_core::sketch::{Loop, Sketch, P2};
use cadpole_core::solid;
use cadpole_core::tessellate::{self, Mesh, Topology};

use crate::edges::BlendKind;
use crate::fillet::{blend_body, EdgeSelection};
use crate::mdc::ContourOptions;
use crate::Field;

fn brep_box(w: f64, d: f64, h: f64) -> (Mesh, Topology) {
    let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
    let solid = kernel::extrude(&s, h).unwrap().remove(0);
    tessellate::tessellate(&solid, 0.02).unwrap()
}

fn opts(res: f64, simplify: bool) -> ContourOptions {
    ContourOptions { resolution: res, error: if simplify { res * 0.25 } else { 0.0 }, ..Default::default() }
}

/// Shapes that once meshed to a non-solid, as (name, field, resolution).
fn cases() -> Vec<(&'static str, Field, f64)> {
    let deg = std::f64::consts::PI / 180.0;
    let (bm, _) = brep_box(20.0, 10.0, 5.0);
    vec![
        ("smooth_union", Field::sphere(8.0).smooth_union(&Field::boxed([15.0, 3.0, 3.0], 0.0), 3.0), 0.5),
        ("hard_union", Field::sphere(8.0).union(&Field::boxed([10.0, 3.0, 3.0], 0.0)), 0.5),
        ("rotated_box", Field::boxed([10.0, 3.0, 2.0], 0.0).rotate([1.0, 2.0, 3.0], 33.0 * deg), 0.5),
        ("from_body_box", crate::field_from_mesh(&bm).expect("a box is a solid"), 0.5),
        (
            "capsule_neck",
            Field::capsule([0.0, 0.0, 0.0], [0.0, 10.0, 0.0], 1.6)
                .union(&Field::capsule([3.1, 0.0, 0.0], [3.1, 10.0, 0.0], 1.6)),
            0.4,
        ),
        (
            "capsule_neck_coarse",
            Field::capsule([0.0, 0.0, 0.0], [0.0, 10.0, 0.0], 1.6)
                .union(&Field::capsule([2.5, 0.0, 0.0], [2.5, 10.0, 0.0], 1.6)),
            0.6,
        ),
        ("mirrored_sphere", Field::sphere(8.0).translate([3.0, 0.0, 0.0]).mirror([1.0, 0.0, 0.0]), 0.5),
    ]
}

#[test]
fn mesh_field_returns_a_solid_for_every_reported_shape() {
    let mut bad = Vec::new();
    for (name, f, res) in cases() {
        for simplify in [false, true] {
            match crate::mesh_field(&f, &opts(res, simplify), "m") {
                Ok((m, _, _)) => {
                    let r = solid::validate(&m);
                    if !r.is_solid() {
                        bad.push(format!("{name} simplify={simplify}: {}", r.summary()));
                    }
                }
                Err(e) => bad.push(format!("{name} simplify={simplify}: refused: {e}")),
            }
        }
    }
    assert!(bad.is_empty(), "not solids:\n{}", bad.join("\n"));
}

#[test]
fn a_field_chamfer_or_fillet_of_a_plain_box_is_a_solid() {
    let (mesh, topo) = brep_box(20.0, 20.0, 10.0);
    let mut bad = Vec::new();
    for kind in [BlendKind::Chamfer { distance: 2.0 }, BlendKind::Fillet { radius: 2.0 }] {
        for res in [None, Some(0.5), Some(0.25)] {
            match blend_body(&mesh, &topo, &EdgeSelection::All, kind, res, 0.0) {
                Ok((m, _, rep)) => {
                    let r = solid::validate(&m);
                    if !r.is_solid() || !rep.watertight || !rep.vertex_manifold {
                        bad.push(format!("{kind:?} {res:?}: {}", r.summary()));
                    }
                }
                Err(e) => bad.push(format!("{kind:?} {res:?}: refused: {e}")),
            }
        }
    }
    assert!(bad.is_empty(), "not solids:\n{}", bad.join("\n"));
}

/// An L bracket: a union of a plate and an upright, filleted on every edge
/// through the field path. It used to come back with 14-18 non-manifold
/// edges.
#[test]
fn a_field_fillet_of_an_l_bracket_is_a_solid() {
    let plate = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(10.0, 10.0), 20.0, 20.0));
    let plate = kernel::extrude(&plate, 5.0).unwrap().remove(0);
    let upright = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(2.5, 10.0), 5.0, 20.0));
    let upright = kernel::extrude(&upright, 20.0).unwrap().remove(0);
    let (plate, upright) =
        (tessellate::tessellate(&plate, 0.02).unwrap(), tessellate::tessellate(&upright, 0.02).unwrap());
    // Flush faces: the exact kernel declines this pair, so it goes through
    // the mesh kernel, as `union()` in a script does.
    let (mesh, topo) =
        cadpole_core::mesh_kernel::boolean(kernel::BoolOp::Union, &plate, &upright, 0.05).unwrap();
    let (m, _, rep) =
        blend_body(&mesh, &topo, &EdgeSelection::All, BlendKind::Fillet { radius: 2.0 }, None, 0.0)
            .expect("the fillet meshes");
    assert!(rep.watertight && rep.vertex_manifold);
    let r = solid::validate(&m);
    assert!(r.is_solid(), "{}", r.summary());
}

/// A field whose surface runs past the bounds it is meshed in used to come
/// back with holes and "open edges, which should be impossible"; it is a
/// named error about the bounds.
#[test]
fn a_surface_past_the_bounds_is_a_named_error() {
    let f = Field::sphere(5.0);
    let o = ContourOptions {
        resolution: 0.5,
        bounds: Some(crate::Box3::new([-3.0, -3.0, -3.0], [3.0, 3.0, 3.0])),
        ..Default::default()
    };
    let e = crate::mesh_field(&f, &o, "m").unwrap_err();
    assert!(e.contains("runs past its declared bounds"), "{e}");
}

/// The contour's own report has to see what the validator sees: an edge
/// carried by four triangles, two each way, is not "closed" just because its
/// signed use count is zero.
#[test]
fn the_edge_check_counts_uses_not_a_signed_sum() {
    use crate::mdc::ContourMesh;
    // Two tetrahedra sharing the edge 0-1, both closed and outward: the edge
    // has four triangles on it.
    let p = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, -1.0],
    ];
    let tet = |a: u32, b: u32, c: u32, d: u32| vec![[a, c, b], [a, b, d], [b, c, d], [a, d, c]];
    let mut tris = tet(0, 1, 2, 3);
    tris.extend(tet(0, 1, 4, 5));
    let m = ContourMesh { normals: vec![[0.0; 3]; p.len()], positions: p, tris, ..Default::default() };
    assert_eq!(m.open_edges(), 0, "the signed sum nets out, which is the bug's blind spot");
    assert!(!m.is_watertight(), "an edge with four triangles is not a closed 2-manifold edge");
    assert_eq!(m.non_manifold_edges(), 1);
    assert!(m.defects().is_some());
}

/// `field.from_body` on a body that is not a solid: the pseudo-normal sign is
/// only right on a closed, outward, non-self-intersecting surface. An open
/// mesh gave "open edges, which should be impossible; please report this",
/// two overlapping shells the same, and an inside-out cube a body of volume
/// -1000. Each is now refused up front, naming the input and what is wrong.
#[test]
fn from_body_refuses_a_body_that_is_not_a_solid() {
    let (cube, _) = brep_box(10.0, 10.0, 10.0);
    let tri_count = cube.indices.len() / 3;

    let mut open = cube.clone();
    open.indices.truncate((tri_count - 1) * 3);
    open.face_ids.truncate(tri_count - 1);

    let mut inside_out = cube.clone();
    for t in inside_out.indices.chunks_exact_mut(3) {
        t.swap(1, 2);
    }

    let mut overlapping = cube.clone();
    let (shifted, _) = brep_box(10.0, 10.0, 10.0);
    let base = (overlapping.positions.len() / 3) as u32;
    for (i, p) in shifted.positions.iter().enumerate() {
        overlapping.positions.push(if i % 3 == 0 { p + 5.0 } else { *p });
    }
    overlapping.normals.extend_from_slice(&shifted.normals);
    overlapping.indices.extend(shifted.indices.iter().map(|i| i + base));
    overlapping.face_ids.extend_from_slice(&shifted.face_ids);

    let o = opts(0.5, false);
    for (name, m, want) in
        [("open", &open, "hole"), ("inside out", &inside_out, "inside out"), ("overlapping", &overlapping, "pass through")]
    {
        let e = crate::field_from_mesh(m).map(|_| ()).unwrap_err();
        assert!(e.contains("not a solid") && e.contains(want), "{name}: {e}");
    }
    let f = crate::field_from_mesh(&cube).expect("a cube is a solid");
    assert!(crate::mesh_field(&f, &o, "m").is_ok());
}

/// A sharp apex on a grid plane is several cells' vertices meeting at one
/// point, which welds into a correct solid. Counting every coincident pair
/// as a defect moved those vertices to their mass points and cut the tip of
/// a cone off by two thirds of a cell, in output that was already a solid.
#[test]
fn a_cone_keeps_its_apex() {
    for res in [0.5, 1.0] {
        for simplify in [false, true] {
            let (m, _, stats) = crate::mesh_field(&Field::cone(8.0, 0.0, 12.0), &opts(res, simplify), "m")
                .unwrap_or_else(|e| panic!("res={res} simplify={simplify}: {e}"));
            let r = solid::validate(&m);
            assert!(r.is_solid(), "res={res} simplify={simplify}: {}", r.summary());
            let top = m.positions.chunks(3).map(|p| p[2] as f64).fold(f64::MIN, f64::max);
            assert!((top - 6.0).abs() < 1e-6, "res={res} simplify={simplify}: apex at z={top}");
            assert_eq!(stats.repaired_vertices, 0, "res={res} simplify={simplify}");
        }
    }
}

/// The on-axis edges of a prism imported as a field lie on grid planes; they
/// stay where they are, not pulled in by a repair or by the clamp's inset.
#[test]
fn a_prism_through_field_from_mesh_keeps_its_edges() {
    let n = 16;
    let pts: Vec<P2> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            P2::new(8.0 * cadpole_core::fmath::cos(a), 8.0 * cadpole_core::fmath::sin(a))
        })
        .collect();
    let s = Sketch::new(Plane::XY).with_loop(Loop::polygon(&pts));
    let solid = kernel::extrude(&s, 12.0).unwrap().remove(0);
    let (mesh, _) = tessellate::tessellate(&solid, 0.02).unwrap();
    let f = crate::field_from_mesh(&mesh).unwrap();
    for res in [0.3, 0.5] {
        let (m, _, _) = crate::mesh_field(&f, &opts(res, false), "m").unwrap_or_else(|e| panic!("res={res}: {e}"));
        let r = solid::validate(&m);
        assert!(r.is_solid(), "res={res}: {}", r.summary());
        let xs: Vec<f64> = m.positions.chunks(3).map(|p| p[0] as f64).collect();
        let (lo, hi) = xs.iter().fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        assert!((hi - 8.0).abs() < 1e-5 && (lo + 8.0).abs() < 1e-5, "res={res}: x from {lo} to {hi}");
    }
}

/// Two spheres in a hard union, and the same pair blended: the egg shape
/// a figure's head is often made of. The adaptive collapse used to leave
/// folds along the crease (pairs of adjacent triangles facing opposite ways,
/// tips up to a millimetre apart), which the validator reports and the next
/// mesh boolean can read as coincident surface. Measured on the output,
/// they are repaired like any other defect.
#[test]
fn a_simplified_contour_has_no_folds() {
    let r = 17.0;
    let small = Field::sphere(0.72 * r).translate([0.45 * r, 0.0, -0.18 * r]);
    let mut bad = Vec::new();
    for (name, f) in [("hard", Field::sphere(r).union(&small)), ("smooth", Field::sphere(r).smooth_union(&small, 0.4 * r))] {
        let (m, _, _) = crate::mesh_field(&f, &opts(0.3, true), "m").expect("meshes");
        let folds = solid::mesh_folds(&m);
        if let Some(w) = folds.first() {
            bad.push(format!("{name}: {} folds, worst at {:?}, tips {:.3} mm apart", folds.len(), w.at, w.tip_gap));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
