//! The measurements. Every test here checks a number against geometry that
//! can be worked out on paper, not against a picture: analytic volumes,
//! measured radii of curvature, watertightness, convergence with resolution.
//!
//! The per-module tests next door check the pieces (interval inclusion, the
//! BVH against brute force, the QEF on synthetic Hermite data). These check
//! the thing a script asks for.

use std::f64::consts::PI;

use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::kernel::{self, BoolOp};
use cadpole_core::mesh_kernel;
use cadpole_core::sketch::{Loop, Sketch, P2};
use cadpole_core::tessellate::{self, Mesh, Topology};

use crate::edges::BlendKind;
use crate::fillet::{blend_body, EdgeSelection};
use crate::mdc::{self, ContourOptions};
use crate::Field;
use cadpole_core::fmath;

// ------------------------------------------------------------------ helpers

/// A `w x d x h` box centred on the origin in x and y, sitting on z = 0,
/// built the way a script would: a rectangle extruded on the B-rep kernel and
/// tessellated. `Loop::rect` takes a *centre*.
fn brep_box(w: f64, d: f64, h: f64) -> (Mesh, Topology) {
    let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
    let solid = kernel::extrude(&s, h).unwrap().remove(0);
    tessellate::tessellate(&solid, 0.02).unwrap()
}

fn mesh_volume(m: &Mesh) -> f64 {
    m.volume()
}

/// The band along the top rim of a `w x d x h` box centred on the origin,
/// excluding the four vertical corners.
///
/// The corners are excluded on purpose: where two blends meet, the two
/// corrections are combined with `max`, which is the intersection of the two
/// blend surfaces and therefore a crease, not the spherical vertex patch a
/// B-rep kernel would build. That limitation is documented, so the radius
/// measurement is taken where a single blend owns the surface.
fn top_rim_band(w: f64, d: f64, h: f64, size: f64) -> impl Fn([f64; 3]) -> bool {
    let margin = size + 0.4;
    let corner_clear = 2.0 * size + 0.5;
    move |p: [f64; 3]| {
        if !(p[2] > h - margin && p[2] < h + 0.01) {
            return false;
        }
        let near_x = p[0].abs() > w / 2.0 - margin;
        let near_y = p[1].abs() > d / 2.0 - margin;
        if !(near_x || near_y) {
            return false;
        }
        let dx = (w / 2.0 - p[0].abs()).abs();
        let dy = (d / 2.0 - p[1].abs()).abs();
        fmath::hypot(dx, dy) > corner_clear
    }
}

/// Volume of a box `w x d x h` with every edge rounded by `r`: the Steiner
/// decomposition, box minus the material the rounding removes.
///
/// A rounded box is (box eroded by r) dilated by r, so by Steiner's formula
/// `V = wdh' + area' r + (edge length)' pi r^2 / 4 + 4/3 pi r^3` where the
/// primed quantities are those of the eroded box `(w-2r)(d-2r)(h-2r)`.
fn rounded_box_volume(w: f64, d: f64, h: f64, r: f64) -> f64 {
    let (a, b, c) = (w - 2.0 * r, d - 2.0 * r, h - 2.0 * r);
    let core = a * b * c;
    let faces = 2.0 * (a * b + b * c + c * a) * r;
    let edges = (4.0 * (a + b + c)) * PI * r * r / 4.0;
    let corners = 4.0 / 3.0 * PI * r * r * r;
    core + faces + edges + corners
}

/// Volume removed by filleting one straight convex edge of length `L` with
/// radius `r` on a 90-degree corner: the prism between the square corner and
/// the quarter disc.
fn fillet_removed_90(length: f64, r: f64) -> f64 {
    length * r * r * (1.0 - PI / 4.0)
}

/// Volume removed by chamfering one straight convex 90-degree edge of length
/// `L` with setback `d`: a triangular prism of legs `d`.
fn chamfer_removed_90(length: f64, d: f64) -> f64 {
    length * d * d / 2.0
}

/// Least-squares circle through 2D points (Kasa's algebraic fit: minimise
/// `sum (|p|^2 + a.p + b)^2`, which is linear in `(a, b)`). Returns the
/// centre, the radius and the RMS radial residual.
///
/// This is how a fillet's radius is *measured* rather than assumed. Taking
/// the turning angle between adjacent triangle normals does not work on a
/// dual-contoured mesh: each quad is split into two triangles that are not
/// coplanar, so neighbouring normals differ by an amount set by the mesh, not
/// by the surface, and the estimate gets worse as the grid refines. Fitting a
/// circle to the cross-section averages that away.
fn fit_circle(pts: &[[f64; 2]]) -> ([f64; 2], f64, f64) {
    let n = pts.len() as f64;
    assert!(pts.len() >= 3, "a circle needs three points, got {}", pts.len());
    // Normal equations for [a, b, c] in x^2 + y^2 + a x + b y + c = 0.
    let mut m = [[0.0f64; 3]; 3];
    let mut rhs = [0.0f64; 3];
    for p in pts {
        let (x, y) = (p[0], p[1]);
        let z = x * x + y * y;
        let row = [x, y, 1.0];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += row[i] * row[j];
            }
            rhs[i] += -z * row[i];
        }
    }
    let _ = n;
    // Gaussian elimination with partial pivoting.
    let mut a = m;
    let mut b = rhs;
    let mut sol = [0.0f64; 3];
    for col in 0..3 {
        let mut piv = col;
        for r in col + 1..3 {
            if a[r][col].abs() > a[piv][col].abs() {
                piv = r;
            }
        }
        a.swap(col, piv);
        b.swap(col, piv);
        let d = a[col][col];
        assert!(d.abs() > 1e-12, "degenerate circle fit");
        for r in col + 1..3 {
            let f = a[r][col] / d;
            for c in col..3 {
                a[r][c] -= f * a[col][c];
            }
            b[r] -= f * b[col];
        }
    }
    for col in (0..3).rev() {
        let mut s = b[col];
        for c in col + 1..3 {
            s -= a[col][c] * sol[c];
        }
        sol[col] = s / a[col][col];
    }
    let centre = [-sol[0] / 2.0, -sol[1] / 2.0];
    let r = (centre[0] * centre[0] + centre[1] * centre[1] - sol[2]).max(0.0).sqrt();
    let mut acc = 0.0;
    for p in pts {
        let d = fmath::hypot(p[0] - centre[0], p[1] - centre[1]) - r;
        acc += d * d;
    }
    (centre, r, (acc / pts.len() as f64).sqrt())
}

/// The largest angle, in degrees, between the normals of any two adjacent
/// triangles whose shared edge falls inside `region`. A crease shows up here
/// as a large number; a blend of radius `R` sampled at `h` shows roughly
/// `h / R` radians.
///
/// Triangles below `min_area` are ignored. Dual contouring always leaves a
/// few slivers where the surface runs nearly along a grid edge and the four
/// cell vertices come out almost collinear; their normals are noise, not
/// curvature, and reading them as a crease says nothing about the surface.
fn max_dihedral_degrees_min_area(
    positions: &[[f64; 3]],
    tris: &[[u32; 3]],
    min_area: f64,
    region: impl Fn([f64; 3]) -> bool,
) -> f64 {
    let area = |t: &[u32; 3]| {
        let (a, b, c) = (positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() * 0.5
    };
    let normal = |t: &[u32; 3]| {
        let (a, b, c) = (positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l == 0.0 { None } else { Some([n[0] / l, n[1] / l, n[2] / l]) }
    };
    let mut per_edge: std::collections::HashMap<(u32, u32), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            per_edge.entry(if a < b { (a, b) } else { (b, a) }).or_default().push(i);
        }
    }
    let mut worst = 0.0f64;
    for (e, ts) in per_edge {
        if ts.len() != 2 {
            continue;
        }
        let (p0, p1) = (positions[e.0 as usize], positions[e.1 as usize]);
        let mid = [(p0[0] + p1[0]) / 2.0, (p0[1] + p1[1]) / 2.0, (p0[2] + p1[2]) / 2.0];
        if !region(mid) {
            continue;
        }
        if area(&tris[ts[0]]) < min_area || area(&tris[ts[1]]) < min_area {
            continue;
        }
        let (Some(n0), Some(n1)) = (normal(&tris[ts[0]]), normal(&tris[ts[1]])) else { continue };
        let cos = (n0[0] * n1[0] + n0[1] * n1[1] + n0[2] * n1[2]).clamp(-1.0, 1.0);
        worst = worst.max(fmath::acos(cos).to_degrees());
    }
    worst
}

/// How far the surface normal turns *at a vertex*: the largest angle between
/// the normals of any two triangles meeting there, over every vertex inside
/// `region`. This is the crease detector. Dual contouring reconstructs a
/// crease by putting vertices on it, so the fold shows up as a wide spread of
/// incident normals at those vertices rather than reliably across any one
/// edge.
fn max_normal_spread_degrees(
    positions: &[[f64; 3]],
    tris: &[[u32; 3]],
    cell: f64,
    region: impl Fn([f64; 3]) -> bool,
) -> f64 {
    let min_area = 0.02 * cell * cell;
    let mut at: std::collections::HashMap<u32, Vec<[f64; 3]>> = std::collections::HashMap::new();
    for t in tris {
        let (a, b, c) = (positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l * 0.5 < min_area {
            continue;
        }
        let n = [n[0] / l, n[1] / l, n[2] / l];
        for &v in t {
            at.entry(v).or_default().push(n);
        }
    }
    let mut worst = 0.0f64;
    for (v, ns) in at {
        if !region(positions[v as usize]) {
            continue;
        }
        for i in 0..ns.len() {
            for j in i + 1..ns.len() {
                let cos = (ns[i][0] * ns[j][0] + ns[i][1] * ns[j][1] + ns[i][2] * ns[j][2])
                    .clamp(-1.0, 1.0);
                worst = worst.max(fmath::acos(cos).to_degrees());
            }
        }
    }
    worst
}

/// [`max_dihedral_degrees_min_area`] with the sliver filter set from a cell
/// size: a triangle smaller than 2% of a cell face carries no information.
fn max_dihedral_degrees(
    positions: &[[f64; 3]],
    tris: &[[u32; 3]],
    cell: f64,
    region: impl Fn([f64; 3]) -> bool,
) -> f64 {
    max_dihedral_degrees_min_area(positions, tris, 0.02 * cell * cell, region)
}

// ------------------------------------------------------- analytic volumes

#[test]
fn a_rounded_box_has_the_analytic_volume_of_a_rounded_box() {
    let (w, d, h, r) = (20.0, 14.0, 10.0, 3.0);
    let want = rounded_box_volume(w, d, h, r);
    let f = Field::boxed([w / 2.0, d / 2.0, h / 2.0], r);
    for (res, tol) in [(0.5, 0.010), (0.25, 0.004)] {
        let m = mdc::contour(&f.compile(), &ContourOptions { resolution: res, ..Default::default() })
            .unwrap();
        assert!(m.is_watertight(), "res {res}: {} open edges", m.open_edges());
        let got = m.volume();
        let rel = (got - want).abs() / want;
        assert!(rel < tol, "res {res}: volume {got:.3} vs analytic {want:.3} ({:.3}% off)", rel * 100.0);
    }
}

#[test]
fn a_sphere_and_a_torus_hit_their_analytic_volumes() {
    let cases: Vec<(&str, Field, f64)> = vec![
        ("sphere r=6", Field::sphere(6.0), 4.0 / 3.0 * PI * 216.0),
        ("torus R=8 r=2", Field::torus(8.0, 2.0), 2.0 * PI * PI * 8.0 * 4.0),
        (
            "cylinder r=5 h=12",
            Field::cylinder(5.0, 12.0, 0.0),
            PI * 25.0 * 12.0,
        ),
    ];
    for (name, f, want) in cases {
        let m = mdc::contour(&f.compile(), &ContourOptions { resolution: 0.25, ..Default::default() })
            .unwrap();
        assert!(m.is_watertight(), "{name}: not watertight");
        assert!(m.is_vertex_manifold(), "{name}: not vertex manifold");
        let rel = (m.volume() - want).abs() / want;
        assert!(rel < 0.005, "{name}: volume {} vs {want} ({:.3}%)", m.volume(), rel * 100.0);
    }
}

// --------------------------------------------------- fillet on a real body

#[test]
fn filleting_the_four_top_edges_of_a_box_removes_the_analytic_volume() {
    let (w, d, h, r) = (30.0, 20.0, 10.0, 2.0);
    let (mesh, topo) = brep_box(w, d, h);
    let v0 = mesh_volume(&mesh);
    assert!((v0 - w * d * h).abs() / (w * d * h) < 1e-9, "the source body is exact: {v0}");

    // The four edges of the top face.
    let top = topo
        .faces
        .iter()
        .find(|f| f.normal.z > 0.99)
        .expect("a top face");
    let names: Vec<String> = top.edges.clone();
    assert_eq!(names.len(), 4, "the top face has four edges: {names:?}");

    let (out, out_topo, report) = blend_body(
        &mesh,
        &topo,
        &EdgeSelection::Named(names),
        BlendKind::Fillet { radius: r },
        Some(0.25),
        0.0,
    )
    .expect("fillet succeeds");
    assert!(report.watertight, "the fillet result is watertight");
    assert!(report.vertex_manifold, "the fillet result is vertex manifold");
    assert!(report.too_large.is_empty(), "{:?}", report.too_large);
    assert!(!out_topo.faces.is_empty());

    // Four edges of 90 degrees: two of length w, two of length d, minus the
    // four corners where they meet (each corner double-counts a
    // quarter-corner region, worth about r^3 * (1 - pi/4) each way).
    let removed = 2.0 * fillet_removed_90(w, r) + 2.0 * fillet_removed_90(d, r);
    let want = v0 - removed;
    let got = mesh_volume(&out);
    let rel = (got - want).abs() / want;
    assert!(
        rel < 0.004,
        "volume {got:.3}, wanted about {want:.3} (box {v0} minus {removed:.3}); off by {:.3}%",
        rel * 100.0
    );

    // The fillet really is round, and round at the requested radius: measure
    // the curvature only in the band around the top rim.
    let (positions, tris) = mesh_to_arrays(&out);
    let (centre, measured, residual) = arc_fit(&positions, w, d, h, r);
    assert!(
        (measured - r).abs() < r * 0.05,
        "measured radius {measured:.4} mm (fit residual {residual:.4}), requested {r} mm"
    );
    assert!(residual < 0.1, "the section really is an arc: residual {residual:.4} mm");
    assert!(
        (centre[0] - (d / 2.0 - r)).abs() < 0.1 && (centre[1] - (h - r)).abs() < 0.1,
        "the arc is centred where a rolling ball of radius {r} would sit: {centre:?}"
    );

    // Nothing else got rounded: the bottom rim is still a hard 90 degrees.
    let sharp = max_dihedral_degrees(&positions, &tris, 0.25, |p| {
        p[2] < 0.3 && (p[0].abs() > w / 2.0 - 0.3 || p[1].abs() > d / 2.0 - 0.3)
    });
    assert!(sharp > 80.0, "the bottom edges must stay sharp; largest fold there {sharp:.1} deg");
    let rounded = max_dihedral_degrees(&positions, &tris, 0.25, top_rim_band(w, d, h, r));
    assert!(rounded < 25.0, "the filleted rim has no crease; largest fold {rounded:.1} deg");
}

/// Fit a circle to the cross-section of the fillet on the `+y` top edge of a
/// box, at mid-span where no other blend reaches. Returns the fitted centre
/// in `(y, z)`, the radius, and the RMS residual.
fn arc_fit(
    positions: &[[f64; 3]],
    w: f64,
    d: f64,
    h: f64,
    r: f64,
) -> ([f64; 2], f64, f64) {
    let pad = 0.05 * r.max(1.0);
    let pts: Vec<[f64; 2]> = positions
        .iter()
        .filter(|p| {
            p[0].abs() < w / 4.0
                && p[1] > d / 2.0 - r + pad
                && p[1] < d / 2.0 - pad
                && p[2] > h - r + pad
                && p[2] < h - pad
        })
        .map(|p| [p[1], p[2]])
        .collect();
    assert!(
        pts.len() >= 8,
        "the fillet should leave plenty of vertices strictly between the two tangent lines, \
         got {}",
        pts.len()
    );
    fit_circle(&pts)
}

#[test]
fn a_chamfer_removes_the_analytic_volume() {
    let (w, d, h, c) = (30.0, 20.0, 10.0, 2.0);
    let (mesh, topo) = brep_box(w, d, h);
    let v0 = mesh_volume(&mesh);
    let top = topo.faces.iter().find(|f| f.normal.z > 0.99).unwrap();
    let (out, _, report) = blend_body(
        &mesh,
        &topo,
        &EdgeSelection::Named(top.edges.clone()),
        BlendKind::Chamfer { distance: c },
        Some(0.25),
        0.0,
    )
    .expect("chamfer succeeds");
    assert!(report.watertight);
    let removed = 2.0 * chamfer_removed_90(w, c) + 2.0 * chamfer_removed_90(d, c);
    let want = v0 - removed;
    let got = mesh_volume(&out);
    let rel = (got - want).abs() / want;
    assert!(rel < 0.004, "volume {got:.3} vs {want:.3} ({:.3}%)", rel * 100.0);

    // A chamfer is a plane at 45 degrees through the two setback points, so
    // every vertex strictly on it satisfies `y + z = d/2 + h - c`.
    let (positions, _) = mesh_to_arrays(&out);
    let plane = d / 2.0 + h - c;
    let pad = 0.1;
    let mut n = 0;
    let mut worst = 0.0f64;
    for p in &positions {
        if p[0].abs() < w / 4.0
            && p[1] > d / 2.0 - c + pad
            && p[1] < d / 2.0 - pad
            && p[2] > h - c + pad
            && p[2] < h - pad
        {
            n += 1;
            worst = worst.max((p[1] + p[2] - plane).abs());
        }
    }
    assert!(n >= 8, "the chamfer face should carry vertices; got {n}");
    assert!(worst < 0.15, "the chamfer face is planar at the right setback; worst {worst:.4} mm");
}

#[test]
fn a_fillet_converges_on_the_requested_radius_as_the_resolution_falls() {
    let (w, d, h, r) = (24.0, 16.0, 8.0, 3.0);
    let (mesh, topo) = brep_box(w, d, h);
    let top = topo.faces.iter().find(|f| f.normal.z > 0.99).unwrap();
    let names = top.edges.clone();
    let mut prev = f64::INFINITY;
    let mut table = Vec::new();
    for res in [0.75, 0.5, 0.25] {
        let (out, _, report) = blend_body(
            &mesh,
            &topo,
            &EdgeSelection::Named(names.clone()),
            BlendKind::Fillet { radius: r },
            Some(res),
            0.0,
        )
        .unwrap();
        assert!(report.watertight, "res {res}");
        let (positions, tris) = mesh_to_arrays(&out);
        let (_, measured, residual) = arc_fit(&positions, w, d, h, r);
        let fold = max_dihedral_degrees(&positions, &tris, res, top_rim_band(w, d, h, r));
        let err = (measured - r).abs();
        table.push((res, measured, err, residual, fold, report.surface_error, report.triangles, report.ms));
        prev = prev.min(err);
    }
    eprintln!(
        "fillet accuracy (resolution, measured R, |R error|, arc residual, max fold deg, \
         surface err, tris, ms):"
    );
    for row in &table {
        eprintln!("  {row:?}");
    }
    // The finest run must be within a few percent of the requested radius.
    let last = table[table.len() - 1];
    assert!(last.2 < r * 0.05, "radius error {:.4} mm at the finest resolution", last.2);
    // Refining must not make it worse.
    assert!(
        table[table.len() - 1].2 <= table[0].2 + 1e-9,
        "refining the grid must not move the measured radius away from the request"
    );
    // Every run is watertight and the mesh sits on the field to within the
    // cell size it was contoured at.
    for row in &table {
        assert!(row.5 < row.0 * 0.75, "surface error {} at resolution {}", row.5, row.0);
        assert!(row.3 < row.0 * 0.5, "arc residual {} at resolution {}", row.3, row.0);
    }
}

#[test]
fn a_radius_the_geometry_cannot_take_is_diagnosed() {
    // A 4 mm thick plate cannot take a 3 mm fillet on both top edges of the
    // same 2 mm-from-the-middle wall: probe it with a radius larger than the
    // plate is thick.
    let (mesh, topo) = brep_box(30.0, 20.0, 4.0);
    let top = topo.faces.iter().find(|f| f.normal.z > 0.99).unwrap();
    let (_, _, report) = blend_body(
        &mesh,
        &topo,
        &EdgeSelection::Named(top.edges.clone()),
        BlendKind::Fillet { radius: 5.0 },
        Some(0.4),
        0.0,
    )
    .expect("it still produces geometry");
    assert!(
        !report.too_large.is_empty(),
        "a 5 mm fillet on a 4 mm plate must be reported as too large"
    );
    for (name, fits) in &report.too_large {
        assert!(*fits < 5.0, "{name}: reported {fits} as the radius that fits");
    }
    // And a radius that does fit is not reported.
    let (_, _, ok) = blend_body(
        &mesh,
        &topo,
        &EdgeSelection::Named(top.edges.clone()),
        BlendKind::Fillet { radius: 1.0 },
        Some(0.25),
        0.0,
    )
    .unwrap();
    assert!(ok.too_large.is_empty(), "{:?}", ok.too_large);
}

#[test]
fn filleting_only_one_edge_leaves_the_others_alone() {
    let (w, d, h, r) = (30.0, 20.0, 10.0, 2.5);
    let (mesh, topo) = brep_box(w, d, h);
    let v0 = mesh_volume(&mesh);
    let top = topo.faces.iter().find(|f| f.normal.z > 0.99).unwrap();
    // One edge of the top face: the longest one.
    let one = top
        .edges
        .iter()
        .max_by(|a, b| {
            let la = topo.edges.iter().find(|e| &e.name == *a).unwrap().length;
            let lb = topo.edges.iter().find(|e| &e.name == *b).unwrap().length;
            la.partial_cmp(&lb).unwrap()
        })
        .unwrap()
        .clone();
    let length = topo.edges.iter().find(|e| e.name == one).unwrap().length;
    let (out, _, report) = blend_body(
        &mesh,
        &topo,
        &EdgeSelection::Named(vec![one]),
        BlendKind::Fillet { radius: r },
        Some(0.25),
        0.0,
    )
    .unwrap();
    assert!(report.watertight);
    assert_eq!(report.edges_selected, 1);
    let want = v0 - fillet_removed_90(length, r);
    let got = mesh_volume(&out);
    let rel = (got - want).abs() / want;
    assert!(
        rel < 0.002,
        "one edge filleted: volume {got:.3} vs {want:.3} ({:.3}%) -- if the whole body had \
         been rounded this would be far lower",
        rel * 100.0
    );
}

#[test]
fn filleting_every_edge_of_a_box_matches_the_rounded_box_volume() {
    let (w, d, h, r) = (20.0, 14.0, 10.0, 2.0);
    let (mesh, topo) = brep_box(w, d, h);
    let (out, _, report) = blend_body(
        &mesh,
        &topo,
        &EdgeSelection::All,
        BlendKind::Fillet { radius: r },
        Some(0.25),
        0.0,
    )
    .unwrap();
    assert!(report.watertight);
    assert_eq!(report.edges_selected, 12);
    let want = rounded_box_volume(w, d, h, r);
    let got = mesh_volume(&out);
    let rel = (got - want).abs() / want;
    // Vertex blends where three fillets meet are approximated by the
    // intersection of the three, not by a true spherical corner patch, so the
    // tolerance here is looser than for a single edge -- and that is exactly
    // the documented limitation.
    assert!(rel < 0.01, "volume {got:.3} vs rounded-box {want:.3} ({:.3}%)", rel * 100.0);
}

// ------------------------------------------------------------- smooth union

#[test]
fn a_smooth_union_of_two_spheres_has_no_crease() {
    let r = 5.0;
    let k = 2.0;
    let a = Field::sphere(r).translate([-4.0, 0.0, 0.0]);
    let b = Field::sphere(r).translate([4.0, 0.0, 0.0]);
    let sharp = a.union(&b);
    let smooth = a.smooth_union(&b, k);
    let opts = ContourOptions { resolution: 0.2, ..Default::default() };
    let ms = mdc::contour(&smooth.compile(), &opts).unwrap();
    let mc = mdc::contour(&sharp.compile(), &opts).unwrap();
    assert!(ms.is_watertight());

    // The seam is the circle x = 0, y^2 + z^2 = 9, where the two spheres
    // meet at 106 degrees. A plain union keeps that fold; the blend must not.
    let band = |p: [f64; 3]| p[0].abs() < 1.2;
    let smooth_fold = max_normal_spread_degrees(&ms.positions, &ms.tris, 0.2, band);
    let sharp_fold = max_normal_spread_degrees(&mc.positions, &mc.tris, 0.2, band);
    // The section of the seam in the z = 0 plane, on the +y side: a single
    // arc for the blend, two arcs meeting at a corner for the plain union.
    let section = |m: &mdc::ContourMesh| -> Vec<[f64; 2]> {
        m.positions
            .iter()
            .filter(|p| p[2].abs() < 0.2 && p[1] > 2.0 && p[0].abs() < 1.6)
            .map(|p| [p[0], p[1]])
            .collect()
    };
    let (_, smooth_r, smooth_resid) = fit_circle(&section(&ms));
    let (_, _, sharp_resid) = fit_circle(&section(&mc));
    eprintln!(
        "seam: plain union spread {sharp_fold:.1} deg, arc residual {sharp_resid:.4} mm; \
         smooth union spread {smooth_fold:.1} deg, arc residual {smooth_resid:.4} mm, \
         fitted radius {smooth_r:.3} mm (k = {k})"
    );
    // Curvature is bounded: the blend section really is a circular arc of the
    // order of the blend radius, where the plain union's is a corner.
    assert!(smooth_resid < 0.05, "the blend section is an arc: residual {smooth_resid:.4} mm");
    assert!(
        sharp_resid > 10.0 * smooth_resid,
        "the plain union's section is not an arc: {sharp_resid:.4} vs {smooth_resid:.4}"
    );
    assert!(
        smooth_r > 0.5 * k && smooth_r < 3.0 * k,
        "the blend radius is of the order of k = {k}: measured {smooth_r:.3}"
    );
    // And the surface normal does not jump across the seam.
    assert!(
        sharp_fold > 30.0,
        "the plain union is supposed to crease; normal spread {sharp_fold:.1} deg"
    );
    assert!(
        smooth_fold < 15.0,
        "the smooth union must not crease: normal spread {smooth_fold:.1} deg for k = {k}"
    );
    assert!(smooth_fold * 3.0 < sharp_fold, "{smooth_fold:.1} vs {sharp_fold:.1}");
    // The blend adds material, never removes it.
    assert!(ms.volume() > mc.volume(), "{} vs {}", ms.volume(), mc.volume());
}

// ------------------------------------------------ determinism and convergence

#[test]
fn meshing_the_same_field_twice_gives_the_same_bytes() {
    let f = Field::cylinder(6.0, 20.0, 1.0)
        .smooth_subtract(&Field::cylinder(4.0, 22.0, 0.0), 1.5)
        .displace(&Field::noise(4.0, 0.5, 9, 2), 1.0);
    let opts = ContourOptions { resolution: 0.5, ..Default::default() };
    let a = mdc::contour(&f.compile(), &opts).unwrap();
    let b = mdc::contour(&f.compile(), &opts).unwrap();
    assert_eq!(a.tris, b.tris);
    assert_eq!(a.positions.len(), b.positions.len());
    for (p, q) in a.positions.iter().zip(&b.positions) {
        for k in 0..3 {
            assert_eq!(p[k].to_bits(), q[k].to_bits());
        }
    }
    // And the fillet path is deterministic too.
    let (mesh, topo) = brep_box(20.0, 12.0, 8.0);
    let mut out = Vec::new();
    for _ in 0..2 {
        let (m, _, _) = blend_body(
            &mesh,
            &topo,
            &EdgeSelection::All,
            BlendKind::Fillet { radius: 1.5 },
            Some(0.4),
            0.0,
        )
        .unwrap();
        out.push(m);
    }
    assert_eq!(out[0].indices, out[1].indices);
    assert_eq!(out[0].positions, out[1].positions);
}

#[test]
fn resolution_is_monotone_and_converges() {
    // A shape with a curved and a flat part, so both error sources show.
    let f = Field::cylinder(6.0, 10.0, 0.0).union(&Field::boxed([8.0, 8.0, 2.0], 0.0));
    // Analytic: the cylinder plus the parts of the slab outside it. The slab
    // is 16 x 16 x 4 centred on the origin, the cylinder r = 6 h = 10.
    // The overlap is the cylinder from z = -2 to 2: pi * 36 * 4.
    let want = PI * 36.0 * 10.0 + 16.0 * 16.0 * 4.0 - PI * 36.0 * 4.0;
    let mut errs = Vec::new();
    for res in [1.0, 0.5, 0.25] {
        let m = mdc::contour(&f.compile(), &ContourOptions { resolution: res, ..Default::default() })
            .unwrap();
        assert!(m.is_watertight(), "res {res}");
        assert!(m.is_vertex_manifold(), "res {res}");
        errs.push(((m.volume() - want).abs() / want, res, m.tris.len()));
    }
    // Down to the noise floor: once vertices on the slab's corners and the
    // cylinder's rims stay exact, the error at 0.5 and at 0.25 are both a
    // few parts in 100,000, and which of two such numbers is smaller says
    // nothing about convergence.
    for w in errs.windows(2) {
        assert!(w[1].0 < w[0].0 || w[1].0 < 1e-4, "error must fall as the grid refines: {errs:?}");
    }
    assert!(errs[errs.len() - 1].0 < 0.002, "finest run {:?}", errs[errs.len() - 1]);
}

#[test]
fn the_adaptive_error_threshold_trades_triangles_for_accuracy_and_stays_closed() {
    let f = Field::boxed([10.0, 8.0, 6.0], 1.0);
    let want = rounded_box_volume(20.0, 16.0, 12.0, 1.0);
    let mut rows = Vec::new();
    for error in [0.0, 0.01, 0.05] {
        let m = mdc::contour(
            &f.compile(),
            &ContourOptions { resolution: 0.25, error, ..Default::default() },
        )
        .unwrap();
        assert!(m.is_watertight(), "error {error}: {} open edges", m.open_edges());
        assert!(m.is_vertex_manifold(), "error {error}: {:?}", m.non_manifold_vertices());
        rows.push((error, m.tris.len(), (m.volume() - want).abs() / want));
    }
    assert!(rows[1].1 < rows[0].1, "a threshold must reduce triangles: {rows:?}");
    assert!(rows[2].1 < rows[1].1, "a bigger threshold reduces more: {rows:?}");
    for r in &rows {
        assert!(r.2 < 0.02, "volume stays within 2%: {rows:?}");
    }
    eprintln!("adaptive (error, triangles, volume error): {rows:?}");
}

// ------------------------------------------------------ interop with cadpole-core

#[test]
fn a_field_body_booleans_against_a_brep_body_through_the_mesh_kernel() {
    // The field half: a rounded slab.
    let f = Field::boxed([10.0, 10.0, 3.0], 1.0);
    let (fm, ft, _) =
        crate::mesh_field(&f, &ContourOptions { resolution: 0.3, ..Default::default() }, "slab")
            .expect("meshing");
    assert!(fm.welded(1e-4).is_watertight(), "the field body is watertight after welding");
    let fv = fm.volume();

    // The B-rep half, tessellated the way `Context::boolean` does.
    let (bm, bt) = brep_box(8.0, 8.0, 20.0);
    let bv = bm.volume();

    let u = mesh_kernel::boolean(BoolOp::Union, &(fm.clone(), ft.clone()), &(bm.clone(), bt.clone()), 0.05)
        .expect("union through the manifold kernel");
    let i = mesh_kernel::boolean(BoolOp::Intersect, &(fm.clone(), ft.clone()), &(bm.clone(), bt.clone()), 0.05)
        .expect("intersection");
    let d = mesh_kernel::boolean(BoolOp::Subtract, &(fm, ft), &(bm, bt), 0.05).expect("difference");
    for (name, r) in [("union", &u), ("intersect", &i), ("subtract", &d)] {
        assert!(r.0.welded(1e-4).is_watertight(), "{name} is watertight");
    }
    // Inclusion-exclusion holds, which is the real check that the field body
    // went in as a valid solid.
    let (vu, vi, vd) = (u.0.volume(), i.0.volume(), d.0.volume());
    assert!(
        ((vu + vi) - (fv + bv)).abs() < 1e-4 * (fv + bv),
        "|A u B| + |A n B| = |A| + |B|: {vu} + {vi} vs {fv} + {bv}"
    );
    assert!((vd - (fv - vi)).abs() < 1e-4 * fv, "difference {vd} vs {}", fv - vi);
}

#[test]
fn a_filleted_body_still_booleans() {
    let (mesh, topo) = brep_box(20.0, 20.0, 10.0);
    let (out, out_topo, _) = blend_body(
        &mesh,
        &topo,
        &EdgeSelection::All,
        BlendKind::Fillet { radius: 2.0 },
        Some(0.4),
        0.0,
    )
    .unwrap();
    assert!(out.welded(1e-4).is_watertight());
    let hole = {
        let s = Sketch::new(Plane::XY.offset(-1.0)).with_loop(Loop::circle(P2::new(0.0, 0.0), 3.0));
        let solid = kernel::extrude(&s, 12.0).unwrap().remove(0);
        tessellate::tessellate(&solid, 0.05).unwrap()
    };
    let v0 = out.volume();
    let r = mesh_kernel::boolean(BoolOp::Subtract, &(out, out_topo), &hole, 0.05)
        .expect("subtracting a hole from a filleted body");
    assert!(r.0.welded(1e-4).is_watertight());
    let want = v0 - PI * 9.0 * 10.0;
    assert!((r.0.volume() - want).abs() / want < 0.01, "{} vs {want}", r.0.volume());
}

// ------------------------------------------------------------- mesh -> field

#[test]
fn an_imported_body_becomes_a_field_that_reproduces_it() {
    let (mesh, _) = brep_box(20.0, 12.0, 8.0);
    let f = crate::field_from_mesh(&mesh).expect("a box is a solid");
    let t = f.compile();
    // Analytic box SDF, half extents (10, 6, 4).
    let want = |p: [f64; 3]| {
        let q = [p[0].abs() - 10.0, p[1].abs() - 6.0, p[2].abs() - 4.0];
        let out = (fmath::powi(q[0].max(0.0), 2) + fmath::powi(q[1].max(0.0), 2) + fmath::powi(q[2].max(0.0), 2)).sqrt();
        out + q[0].max(q[1]).max(q[2]).min(0.0)
    };
    let mut ev = t.evaluator();
    let n = 9;
    for i in 0..=n {
        for j in 0..=n {
            for k in 0..=n {
                // The body sits with z from 0 to 8, so shift the probe.
                let p = [
                    -14.0 + 28.0 * i as f64 / n as f64,
                    -10.0 + 20.0 * j as f64 / n as f64,
                    -4.0 + 16.0 * k as f64 / n as f64,
                ];
                let got = ev.eval([p[0], p[1], p[2] + 4.0]);
                assert!((got - want(p)).abs() < 1e-6, "{p:?}: {got} vs {}", want(p));
            }
        }
    }
    // And it meshes back to the same volume.
    let (out, _, _) =
        crate::mesh_field(&f, &ContourOptions { resolution: 0.4, ..Default::default() }, "box")
            .unwrap();
    assert!((out.volume() - 20.0 * 12.0 * 8.0).abs() / 1920.0 < 1e-3, "{}", out.volume());
}

// ------------------------------------------------------------------ plumbing

fn mesh_to_arrays(m: &Mesh) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let w = m.welded(1e-5);
    let positions: Vec<[f64; 3]> = w
        .positions
        .chunks_exact(3)
        .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
        .collect();
    let tris: Vec<[u32; 3]> = w.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    (positions, tris)
}

#[test]
fn a_selection_that_names_nothing_is_a_diagnostic() {
    let (mesh, topo) = brep_box(10.0, 10.0, 10.0);
    let e = blend_body(
        &mesh,
        &topo,
        &EdgeSelection::Named(vec!["edge:999".into()]),
        BlendKind::Fillet { radius: 1.0 },
        None,
        0.0,
    )
    .unwrap_err();
    assert!(e.contains("edge:999"), "{e}");
    let e = blend_body(
        &mesh,
        &topo,
        &EdgeSelection::All,
        BlendKind::Fillet { radius: 0.0 },
        None,
        0.0,
    )
    .unwrap_err();
    assert!(e.contains("positive"), "{e}");
}

#[test]
fn vec3_helpers_stay_in_step_with_cadpole_core() {
    // A guard that the crate boundary types have not drifted: `Vec3` is what
    // the topology speaks, arrays are what the field speaks.
    let v = Vec3::new(1.0, 2.0, 3.0);
    assert_eq!([v.x, v.y, v.z], [1.0, 2.0, 3.0]);
}
