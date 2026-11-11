//! The tangent case: two boundaries that touch without crossing.
//!
//! A boolean kernel cuts along the curves where two boundaries *cross*. Two
//! boundaries that only touch — coplanar faces, a cylinder resting on a
//! plate, a boss whose side is tangent to a hole, an edge lying in a face —
//! give it nothing to cut along, and every kernel Cadpole has answers that badly
//! in its own way: `truck-shapeops` returns `None` (the Newton step is
//! singular where the normals are parallel, and coplanar triangles produce
//! no interference segments), and manifold's exact engine, which resolves
//! *exact* floating-point ties by symbolic perturbation, sees two
//! independently tessellated copies of "the same" plane as a sliver layer or
//! a hair-wide gap and keeps it, exactly. This module answers in two stages.
//!
//! **Stage 1 — make the ties exact (the mesh path).** Before a mesh boolean,
//! [`snap_shared_planes`] finds the planes the two operands share within
//! [`snap_eps`] — a band of a tenth of a micron, or a millionth of the pair's
//! size — and projects every vertex of the *second* operand that lies within
//! the band onto the first operand's spelling of that plane: its analytic
//! plane where it has a face there, the plane through its vertices where it
//! has only an edge or a corner. The first operand never moves. Two spellings
//! of one plane become one, and manifold's per-operation
//! convention then decides the contact ("touching cubes are merged,
//! equal-height differences produce through-holes"). Nothing moves further
//! than the band, and the band is below anything a tessellation can promise:
//! it is `WELD_EPS`, the distance at which two vertices already count as one.
//! A gap wider than that is a feature and is left alone —
//! named, not moved. This is snap rounding restricted to planes (Hobby 1999;
//! Fortune 1999), and it is what plane-based integer kernels do implicitly.
//!
//! **Stage 2 — name the case (both paths).** [`classify`] finds every
//! touching contact between the two operands — by surface identity for the
//! curved cases ([`crate::surface_class`]: parallel cylinder axes a radius
//! sum apart, a sphere a radius from a plane), by plane geometry for the
//! coplanar ones, and by triangle geometry for an edge or a corner resting
//! on a planar face — and [`describe`]/[`fix`] turn the first of them into
//! the sentence every refusal, fallback note and routing diagnostic carries:
//! the case, the faces by name, the measured distance and angle, and the
//! change to the script that resolves it.

use std::borrow::Cow;

use crate::geom::Vec3;
use crate::kernel::BoolOp;
use crate::mesh_kernel::MeshBody;
use crate::surface_class::SurfaceClass;
use crate::tessellate::Mesh;
use crate::fmath;

/// Floor of the snap band, in mm: the distance the mesh kernel already welds
/// vertices at ([`crate::mesh_kernel::WELD_EPS`]), so a move this size is
/// one the mesh path already makes for itself on every operand.
pub const SNAP_FLOOR: f64 = 1e-4;

/// The snap band as a fraction of the pair's bounding-box diagonal. `Mesh`
/// stores positions as f32, whose 24-bit significand places a coordinate to
/// about 6e-8 of its magnitude; two spellings of one plane that reached f32
/// through different f64 arithmetic (a face at `floor + wall` against one at
/// `top - lip`, a rotated copy, a face re-evaluated by truck after a
/// boolean) differ by a few of those steps. A millionth is a dozen f32 steps
/// at the far corner of the model — every representation difference, and
/// no geometric one: at a metre it is a micron, still five times finer than
/// the finest tessellation anyone exports at (0.005 mm).
pub const SNAP_RELATIVE: f64 = 1e-6;

/// How far two face normals may be apart, in radians, and still be one
/// plane for the snap. Two unit normals fitted to f32 vertices of one plane
/// agree to about 1e-7 rad on a millimetre-sized triangle; a ten-thousandth
/// leaves a wide margin, and the per-vertex bound in [`snap_shared_planes`]
/// is what keeps a large face at the edge of this band from moving.
pub const SNAP_ANGLE: f64 = 1e-4;

/// The angular band for *naming* a tangency, in radians: about 0.06°. Wider
/// than [`SNAP_ANGLE`] because it classifies rather than moves — two axes
/// this nearly parallel are tangent as far as any kernel's Newton step is
/// concerned (`double_projection` inverts `[n0 n1 dir]`, which is singular
/// long before the normals are exactly parallel).
pub const CASE_ANGLE: f64 = 1e-3;

/// How far a snapped vertex may move, in mm: `max(SNAP_FLOOR,
/// SNAP_RELATIVE × the diagonal of the two operands' joint bounding box)`.
pub fn snap_eps(a: &Mesh, b: &Mesh) -> f64 {
    let diag = match (a.bbox(), b.bbox()) {
        (Some((alo, ahi)), Some((blo, bhi))) => (ahi.max(bhi) - alo.min(blo)).length(),
        (Some((lo, hi)), None) | (None, Some((lo, hi))) => (hi - lo).length(),
        (None, None) => 0.0,
    };
    (SNAP_RELATIVE * diag).max(SNAP_FLOOR)
}

/// Which way two coplanar faces look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sense {
    /// The outward normals point at each other: the two solids are on
    /// opposite sides of the plane and *touch* there. This is `glue`'s case.
    Opposed,
    /// The outward normals agree: both solids are on the same side and the
    /// faces are *flush*. A cutter flush with a body face, or a tab whose
    /// outer face is level with the wall it sits on.
    Aligned,
}

// ------------------------------------------------------------------ faces

/// One face of an operand, as the classifier and the snap see it: a label
/// for the diagnostics, the analytic surface it lies on, its plane when it
/// has one, and its triangles.
pub struct TangentFace<'a> {
    /// How the diagnostics name it: a topological name in backticks on the
    /// mesh path, "the first body's face #3 (plane, box …)" on the exact
    /// path, where no names exist yet.
    pub label: String,
    pub class: SurfaceClass,
    /// `Some((a point on it, its outward unit normal))` when the face lies
    /// in a plane — by class, or by fit for a mesh group with no class.
    pub plane: Option<(Vec3, Vec3)>,
    pub lo: Vec3,
    pub hi: Vec3,
    pub area: f64,
    tris: Tris<'a>,
}

/// The triangles: owned (the exact path tessellates into a soup) or a range
/// of a mesh (the mesh path has them already, and copying a blade's ten
/// thousand triangles per boolean is not free).
enum Tris<'a> {
    Owned(Vec<[Vec3; 3]>),
    Range { mesh: &'a Mesh, start: usize, count: usize },
}

impl<'a> TangentFace<'a> {
    /// A face from a triangle soup, the exact path's form.
    pub fn owned(label: String, class: SurfaceClass, plane: Option<(Vec3, Vec3)>, tris: Vec<[Vec3; 3]>) -> TangentFace<'static> {
        let (lo, hi, area) = extent(tris.iter().copied());
        TangentFace { label, class, plane, lo, hi, area, tris: Tris::Owned(tris) }
    }

    /// The face's triangles, in mesh order.
    pub fn tris(&self) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        let empty: &[[Vec3; 3]] = &[];
        let (owned, range) = match &self.tris {
            Tris::Owned(v) => (v.as_slice(), None),
            Tris::Range { mesh, start, count } => (empty, Some((*mesh, *start, *count))),
        };
        owned
            .iter()
            .copied()
            .chain(range.into_iter().flat_map(|(m, s, c)| (s..s + c).filter_map(move |t| m.triangle(t))))
    }

    pub fn tri_count(&self) -> usize {
        match &self.tris {
            Tris::Owned(v) => v.len(),
            Tris::Range { count, .. } => *count,
        }
    }
}

fn extent(tris: impl Iterator<Item = [Vec3; 3]>) -> (Vec3, Vec3, f64) {
    let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    let mut area = 0.0;
    for t in tris {
        for p in &t {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        area += (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5;
    }
    if area == 0.0 && lo.x == f64::MAX {
        lo = Vec3::ZERO;
        hi = Vec3::ZERO;
    }
    (lo, hi, area)
}

/// The faces of a mesh body, labelled by their topological names. `flat` is
/// how far a vertex may sit off the fitted plane for a group with no class
/// to still count as planar: [`snap_eps`] of the pair, the same size the
/// snap itself moves.
pub fn faces_of_body(body: &MeshBody, flat: f64) -> Vec<TangentFace<'_>> {
    let (mesh, topo) = body;
    topo.faces
        .iter()
        .map(|f| {
            let start = f.tri_start as usize;
            let count = f.tri_count as usize;
            let (lo, hi, area) = extent((start..start + count).filter_map(|t| mesh.triangle(t)));
            let mut face = TangentFace {
                label: format!("`{}`", f.name),
                class: f.surface.clone(),
                plane: None,
                lo,
                hi,
                area,
                tris: Tris::Range { mesh, start, count },
            };
            face.plane = match &face.class {
                // The tessellation classifies the *oriented* surface, so a
                // recognised plane's normal is the outward one; the
                // area-weighted triangle normal settles the sign should a
                // transform have turned the group inside out.
                SurfaceClass::Plane { origin, normal } => {
                    let n = if f.normal.dot(*normal) < 0.0 { -*normal } else { *normal };
                    Some((*origin, n))
                }
                _ => fitted_plane(&face, flat),
            };
            face
        })
        .collect()
}

/// Does this face lie in one plane, and if so which? The vector area equals
/// the scalar area exactly when every triangle faces the same way; then the
/// points have to be on the plane through the centroid too, within `flat`.
fn fitted_plane(face: &TangentFace, flat: f64) -> Option<(Vec3, Vec3)> {
    let mut vector = Vec3::ZERO;
    let mut area = 0.0;
    let mut centroid = Vec3::ZERO;
    for t in face.tris() {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let a = n.length() * 0.5;
        vector = vector + n * 0.5;
        area += a;
        centroid = centroid + (t[0] + t[1] + t[2]) * (a / 3.0);
    }
    if area <= 0.0 || vector.length() < area * (1.0 - 1e-6) {
        return None;
    }
    let n = vector.normalized();
    let c = centroid * (1.0 / area);
    let off = face.tris().flatten().fold(0.0f64, |m, p| m.max(((p - c).dot(n)).abs()));
    (off <= flat).then_some((c, n))
}

// --------------------------------------------------------- classification

/// What kind of touch two faces make.
#[derive(Clone, Debug, PartialEq)]
pub enum TangentCase {
    /// Two planar faces in one plane that share area, facing each other
    /// (touching) or the same way (flush); `area` is what they share.
    Coplanar { sense: Sense, area: f64 },
    /// Two curved faces on one surface — a shaft in a bore of its own
    /// radius. A subtract of one from the other is empty, a union cuts
    /// nothing.
    CoincidentSurface { kind: &'static str },
    /// Tangent along a line: two parallel cylinders, a cylinder on a plane,
    /// a sphere in a bore of its own radius (a circle).
    TangentLine { kind: &'static str },
    /// Tangent at a point: two spheres, a sphere on a plane or a cylinder,
    /// two cylinders whose axes cross.
    TangentPoint { kind: &'static str },
    /// An edge of one body lies in a planar face of the other; the planar
    /// face belongs to `a` when `plane_in_a`. `interior` when the edge
    /// crosses the face's interior — a foot standing on a floor along a
    /// line — and false when it runs along the face's own boundary, which
    /// is two edges meeting: what two boxes touching along one edge do, and
    /// also what the corner of a face-to-face contact looks like from its
    /// neighbours, so it ranks below everything but a near miss.
    EdgeOnFace { plane_in_a: bool, interior: bool },
    /// A corner of one body rests on a planar face of the other; `interior`
    /// as for [`TangentCase::EdgeOnFace`].
    VertexOnFace { plane_in_a: bool, interior: bool },
    /// The two faces come within the tolerance of each other without
    /// touching or crossing.
    NearMiss,
}

impl TangentCase {
    /// The order the cases are reported in: the most explanatory first.
    fn rank(&self) -> u8 {
        match self {
            TangentCase::Coplanar { .. } => 0,
            TangentCase::CoincidentSurface { .. } => 1,
            TangentCase::TangentLine { .. } => 2,
            TangentCase::TangentPoint { .. } => 3,
            TangentCase::EdgeOnFace { interior: true, .. } => 4,
            TangentCase::VertexOnFace { interior: true, .. } => 5,
            TangentCase::EdgeOnFace { interior: false, .. } => 6,
            TangentCase::VertexOnFace { interior: false, .. } => 7,
            TangentCase::NearMiss => 8,
        }
    }

    /// The case's short name, the vocabulary the diagnostics count in.
    pub fn name(&self) -> &'static str {
        match self {
            TangentCase::Coplanar { sense: Sense::Opposed, .. } => "coplanar faces, touching",
            TangentCase::Coplanar { sense: Sense::Aligned, .. } => "coplanar faces, flush",
            TangentCase::CoincidentSurface { .. } => "coincident surfaces",
            TangentCase::TangentLine { .. } => "tangent along a line",
            TangentCase::TangentPoint { .. } => "tangent at a point",
            TangentCase::EdgeOnFace { interior: true, .. } => "edge-on-face",
            TangentCase::VertexOnFace { interior: true, .. } => "vertex-on-face",
            TangentCase::EdgeOnFace { interior: false, .. } => "edge-on-edge",
            TangentCase::VertexOnFace { interior: false, .. } => "corner-on-edge",
            TangentCase::NearMiss => "near miss",
        }
    }
}

/// One touching contact: the case, the two faces (indices into the two
/// operands' face lists), how far from exact the contact is in mm, the angle
/// between the surfaces there in radians, and any numbers the case wants
/// quoted (the radii and axis distance of two cylinders).
#[derive(Clone, Debug, PartialEq)]
pub struct TangentContact {
    pub case: TangentCase,
    pub a: usize,
    pub b: usize,
    pub distance: f64,
    pub angle: f64,
    pub detail: String,
}

/// How much of the search to do. `Cheap` is what every mesh boolean pays:
/// the analytic and planar tests, linear in the triangles near a planar
/// face. `Full` adds the near-miss search, quadratic in the triangles of
/// each candidate pair, and runs only on a refusal or a fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    Cheap,
    Full,
}

/// Every touching contact between the faces of `a` and the faces of `b`
/// within `tol` mm, most explanatory first (see [`TangentCase::rank`]), then
/// closest to exact, then by face index. Deterministic for the same input.
pub fn classify(fa: &[TangentFace], fb: &[TangentFace], tol: f64, depth: Depth) -> Vec<TangentContact> {
    let mut out: Vec<TangentContact> = Vec::new();
    let mut coplanar_a = vec![false; fa.len()];
    let mut coplanar_b = vec![false; fb.len()];
    for (i, x) in fa.iter().enumerate() {
        for (j, y) in fb.iter().enumerate() {
            if !boxes_within(x.lo, x.hi, y.lo, y.hi, tol) {
                continue;
            }
            if let Some(c) = analytic_contact(x, i, y, j, tol) {
                if matches!(c.case, TangentCase::Coplanar { .. }) {
                    coplanar_a[i] = true;
                    coplanar_b[j] = true;
                }
                out.push(c);
            }
        }
    }
    // An edge or a corner resting on a planar face. A face that is itself
    // in a coplanar contact is skipped as the plane: the edges of the
    // touching face's neighbours lie in it by construction, and naming them
    // would say the same contact three times. And a planar face parallel to
    // the plane is never the other party: two faces in one plane either
    // share area (a coplanar contact, found above) or abut along an edge
    // (ordinary neighbours in a flush union), and neither is a touch.
    let parallel = |x: &TangentFace, y: &TangentFace| match (x.plane, y.plane) {
        (Some((_, nx)), Some((_, ny))) => nx.dot(ny).abs() >= fmath::cos(CASE_ANGLE),
        _ => false,
    };
    for (i, x) in fa.iter().enumerate() {
        if x.plane.is_none() || coplanar_a[i] {
            continue;
        }
        for (j, y) in fb.iter().enumerate() {
            if coplanar_b[j] || parallel(x, y) || !boxes_within(x.lo, x.hi, y.lo, y.hi, tol) {
                continue;
            }
            if let Some((case, d)) = touch_contact(x, y, tol, true) {
                out.push(TangentContact { case, a: i, b: j, distance: d, angle: 0.0, detail: String::new() });
            }
        }
    }
    for (j, y) in fb.iter().enumerate() {
        if y.plane.is_none() || coplanar_b[j] {
            continue;
        }
        for (i, x) in fa.iter().enumerate() {
            if coplanar_a[i] || parallel(x, y) || !boxes_within(x.lo, x.hi, y.lo, y.hi, tol) {
                continue;
            }
            if let Some((case, d)) = touch_contact(y, x, tol, false) {
                out.push(TangentContact { case, a: i, b: j, distance: d, angle: 0.0, detail: String::new() });
            }
        }
    }
    if depth == Depth::Full {
        let mut seen: Vec<(usize, usize)> = out.iter().map(|c| (c.a, c.b)).collect();
        seen.sort_unstable();
        for (i, x) in fa.iter().enumerate() {
            for (j, y) in fb.iter().enumerate() {
                if seen.binary_search(&(i, j)).is_ok() || !boxes_within(x.lo, x.hi, y.lo, y.hi, tol) {
                    continue;
                }
                if let Some(d) = near_miss(x, y, tol) {
                    out.push(TangentContact { case: TangentCase::NearMiss, a: i, b: j, distance: d, angle: 0.0, detail: String::new() });
                }
            }
        }
    }
    out.sort_by(|p, q| {
        p.case
            .rank()
            .cmp(&q.case.rank())
            .then_with(|| p.distance.total_cmp(&q.distance))
            .then_with(|| p.a.cmp(&q.a))
            .then_with(|| p.b.cmp(&q.b))
    });
    out
}

fn angle_between(u: Vec3, v: Vec3) -> f64 {
    fmath::acos(u.dot(v).abs().clamp(-1.0, 1.0))
}

/// The contacts surface identity can name: coplanar planes, and the
/// tangencies between recognised cylinders, spheres and planes.
fn analytic_contact(x: &TangentFace, i: usize, y: &TangentFace, j: usize, tol: f64) -> Option<TangentContact> {
    let cos_case = fmath::cos(CASE_ANGLE);
    let sin_case = fmath::sin(CASE_ANGLE);
    let contact = |case, distance, angle, detail| Some(TangentContact { case, a: i, b: j, distance, angle, detail });
    match (x.plane, y.plane) {
        (Some((pa, na)), Some((pb, nb))) => {
            let dot = na.dot(nb);
            if dot.abs() < cos_case {
                return None; // two planes at an angle cross or miss; they are never tangent
            }
            let signed = (pb - pa).dot(na);
            let off = signed.abs().max(((pa - pb).dot(nb)).abs());
            if off > tol {
                return None;
            }
            let area = shared_area(x, y, tol);
            if area <= 1e-9 * x.area.max(y.area).max(1.0) {
                return None; // parallel planes that only abut: ordinary neighbours
            }
            let sense = if dot < 0.0 { Sense::Opposed } else { Sense::Aligned };
            // Which side of `a`'s face `b`'s plane lies on: for a touching
            // pair a positive offset is a gap and a negative one an overlap;
            // for a flush pair, `b`'s face proud of or sunk into `a`'s.
            let detail = if off <= SNAP_FLOOR * 1e-3 {
                String::new()
            } else {
                match (sense, signed > 0.0) {
                    (Sense::Opposed, true) => "a gap".to_owned(),
                    (Sense::Opposed, false) => "an overlap".to_owned(),
                    (Sense::Aligned, true) => "proud".to_owned(),
                    (Sense::Aligned, false) => "sunk".to_owned(),
                }
            };
            contact(TangentCase::Coplanar { sense, area }, off, angle_between(na, nb), detail)
        }
        (Some(plane), None) => curved_on_plane(&y.class, plane, x, y, tol).and_then(|(case, d, ang, detail)| contact(case, d, ang, detail)),
        (None, Some(plane)) => curved_on_plane(&x.class, plane, y, x, tol).and_then(|(case, d, ang, detail)| contact(case, d, ang, detail)),
        (None, None) => {
            use SurfaceClass::*;
            match (&x.class, &y.class) {
                (
                    Cylinder { axis_point: p, axis: u, radius: r },
                    Cylinder { axis_point: q, axis: v, radius: s },
                ) => {
                    let cross = u.cross(*v);
                    let sin = cross.length();
                    let radii = format!("radii {:.3} and {:.3}", r, s);
                    if sin <= sin_case {
                        let w = *q - *p;
                        let perp = w - *u * w.dot(*u);
                        let d = perp.length();
                        let angle = fmath::asin(sin.clamp(-1.0, 1.0));
                        if (r - s).abs() <= tol && d <= tol {
                            return contact(
                                TangentCase::CoincidentSurface { kind: "cylinders" },
                                d.max((r - s).abs()),
                                angle,
                                format!("{radii}, axes {} apart", mm(d)),
                            );
                        }
                        let outside = (d - (r + s)).abs();
                        let inside = (d - (r - s).abs()).abs();
                        // The line of tangency: from the first axis towards
                        // the second, a radius out — outside the first
                        // cylinder, or inside the larger one. Both faces
                        // have to reach it: the relation holds for every
                        // pair of parallel cylinders a radius sum apart, and
                        // most of those never meet.
                        let e = if d > 0.0 { perp * (1.0 / d) } else { Vec3::ZERO };
                        if outside <= tol {
                            let line = *p + e * *r;
                            if !(line_touches(x, line, *u, tol) && line_touches(y, line, *u, tol)) {
                                return None;
                            }
                            return contact(
                                TangentCase::TangentLine { kind: "cylinders, side by side" },
                                outside,
                                angle,
                                format!("axes {} apart, the sum of the {radii}", mm(d)),
                            );
                        }
                        if inside <= tol {
                            let line = if r >= s { *p + e * *r } else { *q - e * *s };
                            if !(line_touches(x, line, *u, tol) && line_touches(y, line, *u, tol)) {
                                return None;
                            }
                            return contact(
                                TangentCase::TangentLine { kind: "cylinders, one inside the other" },
                                inside,
                                angle,
                                format!("axes {} apart, the difference of the {radii}", mm(d)),
                            );
                        }
                        None
                    } else {
                        let d = ((*q - *p).dot(cross)).abs() / sin;
                        let outside = (d - (r + s)).abs();
                        if outside <= tol {
                            // The closest points of the two axes, and the
                            // point of tangency a radius along from the first.
                            let (t1, _) = closest_params(*p, *u, *q, *v);
                            let c1 = *p + *u * t1;
                            let towards = cross.normalized() * (*q - *p).dot(cross).signum();
                            let point = c1 + towards * *r;
                            if !(point_touches(x, point, tol) && point_touches(y, point, tol)) {
                                return None;
                            }
                            return contact(
                                TangentCase::TangentPoint { kind: "cylinders whose axes cross" },
                                outside,
                                fmath::asin(sin.clamp(-1.0, 1.0)),
                                format!("axes {} apart at their closest, the sum of the {radii}", mm(d)),
                            );
                        }
                        None
                    }
                }
                (Sphere { center: c1, radius: r }, Sphere { center: c2, radius: s }) => {
                    let d = (*c2 - *c1).length();
                    let radii = format!("radii {:.3} and {:.3}", r, s);
                    if d <= tol && (r - s).abs() <= tol {
                        return contact(TangentCase::CoincidentSurface { kind: "spheres" }, d.max((r - s).abs()), 0.0, radii);
                    }
                    let outside = (d - (r + s)).abs();
                    let towards = if d > 0.0 { (*c2 - *c1) * (1.0 / d) } else { Vec3::ZERO };
                    if outside <= tol {
                        if !(point_touches(x, *c1 + towards * *r, tol) && point_touches(y, *c1 + towards * *r, tol)) {
                            return None;
                        }
                        return contact(
                            TangentCase::TangentPoint { kind: "spheres, side by side" },
                            outside,
                            0.0,
                            format!("centres {} apart, the sum of the {radii}", mm(d)),
                        );
                    }
                    let inside = (d - (r - s).abs()).abs();
                    if inside <= tol {
                        let point = if r >= s { *c1 + towards * *r } else { *c2 - towards * *s };
                        if !(point_touches(x, point, tol) && point_touches(y, point, tol)) {
                            return None;
                        }
                        return contact(
                            TangentCase::TangentPoint { kind: "spheres, one inside the other" },
                            inside,
                            0.0,
                            format!("centres {} apart, the difference of the {radii}", mm(d)),
                        );
                    }
                    None
                }
                (Sphere { center, radius: rs }, Cylinder { axis_point, axis, radius: rc })
                | (Cylinder { axis_point, axis, radius: rc }, Sphere { center, radius: rs }) => {
                    let w = *center - *axis_point;
                    let d = (w - *axis * w.dot(*axis)).length();
                    let radii = format!("sphere radius {:.3}, cylinder radius {:.3}", rs, rc);
                    if d <= tol && (rs - rc).abs() <= tol {
                        return contact(
                            TangentCase::TangentLine { kind: "a sphere in a bore of its own radius, a circle of contact" },
                            d.max((rs - rc).abs()),
                            0.0,
                            radii,
                        );
                    }
                    let outside = (d - (rs + rc)).abs();
                    let foot = *axis_point + *axis * w.dot(*axis);
                    let towards = if d > 0.0 { (*center - foot) * (1.0 / d) } else { Vec3::ZERO };
                    if outside <= tol {
                        let point = foot + towards * *rc;
                        if !(point_touches(x, point, tol) && point_touches(y, point, tol)) {
                            return None;
                        }
                        return contact(
                            TangentCase::TangentPoint { kind: "a sphere on a cylinder" },
                            outside,
                            0.0,
                            format!("centre {} from the axis, the sum of the radii ({radii})", mm(d)),
                        );
                    }
                    let inside = (d - (rc - rs).abs()).abs();
                    if inside <= tol {
                        let point = if rc >= rs { foot + towards * *rc } else { *center - towards * *rs };
                        if !(point_touches(x, point, tol) && point_touches(y, point, tol)) {
                            return None;
                        }
                        return contact(
                            TangentCase::TangentPoint { kind: "a sphere inside a cylinder" },
                            inside,
                            0.0,
                            format!("centre {} from the axis, the difference of the radii ({radii})", mm(d)),
                        );
                    }
                    None
                }
                _ => None,
            }
        }
    }
}

/// A recognised curved surface against a planar face.
fn curved_on_plane(
    class: &SurfaceClass,
    (p0, n): (Vec3, Vec3),
    plane_face: &TangentFace,
    curved_face: &TangentFace,
    tol: f64,
) -> Option<(TangentCase, f64, f64, String)> {
    match class {
        SurfaceClass::Cylinder { axis_point, axis, radius } => {
            let cos = axis.dot(n).abs();
            if cos > fmath::sin(CASE_ANGLE) {
                return None; // the axis leans out of the plane: the cylinder crosses it or ends on it
            }
            let signed = (*axis_point - p0).dot(n);
            let dist = signed.abs();
            let off = (dist - radius).abs();
            if off > tol {
                return None;
            }
            // The generator line where the two meet: the axis dropped onto
            // the plane. Both faces have to reach it — a rounded slot's own
            // flat side is tangent to its own corner by construction, and
            // two copies of the slot a turn apart satisfy the same relation
            // while never coming near each other.
            let line = *axis_point - n * signed;
            if !(line_touches(plane_face, line, *axis, tol) && line_touches(curved_face, line, *axis, tol)) {
                return None;
            }
            Some((
                TangentCase::TangentLine { kind: "a cylinder on a plane" },
                off,
                fmath::asin(cos.clamp(-1.0, 1.0)),
                format!("axis {} from the plane, radius {:.3}", mm(dist), radius),
            ))
        }
        SurfaceClass::Sphere { center, radius } => {
            let signed = (*center - p0).dot(n);
            let off = (signed.abs() - radius).abs();
            if off > tol {
                return None;
            }
            let foot = *center - n * signed;
            (inside_face(plane_face.tris(), foot, n) && point_touches(curved_face, foot, tol)).then(|| {
                (
                    TangentCase::TangentPoint { kind: "a sphere on a plane" },
                    off,
                    0.0,
                    format!("centre {} from the plane, radius {:.3}", mm(signed.abs()), radius),
                )
            })
        }
        _ => None,
    }
}

/// Does the face come within `tol` of the line through `p` along `dir`?
/// Per triangle, the line is clipped to the triangle's padded box and five
/// points along the clipped stretch are measured against the triangle, so
/// a large triangle the line crosses in its interior is found without a
/// vertex near it.
fn line_touches(f: &TangentFace, p: Vec3, dir: Vec3, tol: f64) -> bool {
    let dir = dir.normalized();
    let pad = Vec3::new(tol, tol, tol);
    for t in f.tris() {
        let (lo, hi) = tri_box(&t);
        let (lo, hi) = (lo - pad, hi + pad);
        // Slab clip of the line against the box.
        let (mut t0, mut t1) = (f64::MIN, f64::MAX);
        let mut miss = false;
        for k in 0..3 {
            let (o, d) = ([p.x, p.y, p.z][k], [dir.x, dir.y, dir.z][k]);
            let (l, h) = ([lo.x, lo.y, lo.z][k], [hi.x, hi.y, hi.z][k]);
            if d.abs() < 1e-12 {
                if o < l || o > h {
                    miss = true;
                    break;
                }
            } else {
                let (a, b) = ((l - o) / d, (h - o) / d);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if miss || t1 < t0 {
            continue;
        }
        for k in 0..5 {
            let s = t0 + (t1 - t0) * (k as f64 / 4.0);
            if point_tri_distance(p + dir * s, &t) <= tol {
                return true;
            }
        }
    }
    false
}

/// Does the face come within `tol` of the point?
fn point_touches(f: &TangentFace, p: Vec3, tol: f64) -> bool {
    f.tris().any(|t| point_tri_distance(p, &t) <= tol)
}

/// The parameters of the closest points of two lines `p + u t` and
/// `q + v s` (not parallel).
fn closest_params(p: Vec3, u: Vec3, q: Vec3, v: Vec3) -> (f64, f64) {
    let w = p - q;
    let (a, b, c, d, e) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w), v.dot(w));
    let den = a * c - b * b;
    if den.abs() < 1e-18 {
        return (0.0, 0.0);
    }
    ((b * e - c * d) / den, (a * e - b * d) / den)
}

/// Does an edge or a corner of `g` rest on the planar face `f`? `None` when
/// `g` crosses `f`'s plane inside `f`'s box — a transversal pair — or does
/// not reach it. The distance is how far the resting edge or corner is off
/// the plane.
fn touch_contact(f: &TangentFace, g: &TangentFace, tol: f64, plane_in_a: bool) -> Option<(TangentCase, f64)> {
    let (p0, n) = f.plane?;
    let pad = Vec3::new(tol, tol, tol);
    let (flo, fhi) = (f.lo - pad, f.hi + pad);
    let border = border_of(f);
    let inside = |p: Vec3| inside_face(f.tris(), p, n);
    let interior = |p: Vec3| !border.iter().any(|(a, b)| point_segment_distance(p, *a, *b) <= tol);
    let mut lowest = f64::MAX;
    let mut highest = f64::MIN;
    // (how far off the plane, whether the contact is inside the face's border)
    let mut edge: Option<(f64, bool)> = None;
    let mut vertex: Option<(f64, bool)> = None;
    let better = |cur: Option<(f64, bool)>, off: f64, int: bool| match cur {
        // An interior contact outranks a boundary one; among equals, the closer.
        Some((d, i)) if (i && !int) || (i == int && d <= off) => Some((d, i)),
        _ => Some((off, int)),
    };
    let foot = |p: Vec3, d: f64| p - n * d;
    for t in g.tris() {
        let (tlo, thi) = tri_box(&t);
        if !boxes_within(tlo, thi, flo, fhi, 0.0) {
            continue;
        }
        let d = t.map(|p| (p - p0).dot(n));
        for &x in &d {
            lowest = lowest.min(x);
            highest = highest.max(x);
        }
        let near: Vec<usize> = (0..3).filter(|&k| d[k].abs() <= tol).collect();
        if near.len() >= 2 {
            for (a, b) in [(0, 1), (1, 2), (0, 2)] {
                if a >= near.len() || b >= near.len() {
                    continue;
                }
                let (k, m) = (near[a], near[b]);
                let mid = (foot(t[k], d[k]) + foot(t[m], d[m])) * 0.5;
                if inside(mid) {
                    edge = better(edge, d[k].abs().max(d[m].abs()), interior(mid));
                }
            }
        }
        if edge.is_none() {
            for &k in &near {
                let ft = foot(t[k], d[k]);
                if inside(ft) {
                    vertex = better(vertex, d[k].abs(), interior(ft));
                }
            }
        }
    }
    if lowest < -tol && highest > tol {
        return None; // `g` passes through the plane here: a crossing, which is the kernel's job
    }
    if let Some((d, interior)) = edge {
        Some((TangentCase::EdgeOnFace { plane_in_a, interior }, d))
    } else {
        vertex.map(|(d, interior)| (TangentCase::VertexOnFace { plane_in_a, interior }, d))
    }
}

/// The border of a face: the edges its own triangles do not share, keyed on
/// their endpoints to a nanometre.
fn border_of(f: &TangentFace) -> Vec<(Vec3, Vec3)> {
    let key = |p: Vec3| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64);
    let mut uses: std::collections::HashMap<((i64, i64, i64), (i64, i64, i64)), (u32, Vec3, Vec3)> = std::collections::HashMap::new();
    for t in f.tris() {
        for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (kp, kq) = (key(p), key(q));
            let e = uses.entry(if kp < kq { (kp, kq) } else { (kq, kp) }).or_insert((0, p, q));
            e.0 += 1;
        }
    }
    let mut out: Vec<(Vec3, Vec3)> = uses.into_values().filter(|(n, _, _)| *n == 1).map(|(_, p, q)| (p, q)).collect();
    out.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)).then(a.0.z.total_cmp(&b.0.z)));
    out
}

/// The smallest distance between two faces that neither touch nor cross,
/// when it is within `tol`. Point-to-triangle both ways, on the triangle
/// pairs whose boxes meet; `None` as soon as any pair crosses.
fn near_miss(x: &TangentFace, y: &TangentFace, tol: f64) -> Option<f64> {
    let pad = Vec3::new(tol, tol, tol);
    let mut best = f64::MAX;
    for ta in x.tris() {
        let (alo, ahi) = tri_box(&ta);
        let (na, area_a) = crate::solid::tri_normal_area(&ta);
        if area_a == 0.0 {
            continue;
        }
        for tb in y.tris() {
            let (blo, bhi) = tri_box(&tb);
            if !boxes_within(alo - pad, ahi + pad, blo, bhi, 0.0) {
                continue;
            }
            let (nb, area_b) = crate::solid::tri_normal_area(&tb);
            if area_b == 0.0 {
                continue;
            }
            let da = [na.dot(tb[0] - ta[0]), na.dot(tb[1] - ta[0]), na.dot(tb[2] - ta[0])];
            let db = [nb.dot(ta[0] - tb[0]), nb.dot(ta[1] - tb[0]), nb.dot(ta[2] - tb[0])];
            if crate::solid::crosses(&ta, na, &da, &tb, nb, &db, 1e-9).is_some() {
                return None;
            }
            let d = ta
                .iter()
                .map(|p| point_tri_distance(*p, &tb))
                .chain(tb.iter().map(|p| point_tri_distance(*p, &ta)))
                .fold(f64::MAX, f64::min);
            best = best.min(d);
        }
    }
    // A distance below the snap floor is a touch, not a miss: two faces
    // that meet at a corner or along an edge without either being planar
    // there. What such a pair means is said by the contacts found above it.
    (best > SNAP_FLOOR && best <= tol).then_some(best)
}

// ----------------------------------------------------------------- words

/// A length for a diagnostic: nanometres below a hundredth of a micron
/// (where f32 spellings of one plane differ), micrometres below a tenth
/// of a millimetre (two places under a micron, one above), millimetres to
/// three places from there up. Zero is "0.00 µm".
pub fn um(d: f64) -> String {
    let u = d * 1000.0;
    if d >= 0.1 {
        format!("{d:.3} mm")
    } else if u >= 1.0 {
        format!("{u:.1} µm")
    } else if u >= 0.01 || u < 1e-4 {
        format!("{u:.2} µm")
    } else {
        format!("{:.1} nm", u * 1000.0)
    }
}

fn mm(x: f64) -> String {
    if x >= 0.0005 || x == 0.0 { format!("{x:.3} mm") } else { format!("{x:.1e} mm") }
}

fn urad(a: f64) -> String {
    if a < 1e-3 { format!("{:.1} µrad", a * 1e6) } else { format!("{:.3}°", a.to_degrees()) }
}

/// The contact in one sentence: the case, the faces, the numbers.
pub fn describe(c: &TangentContact, fa: &[TangentFace], fb: &[TangentFace]) -> String {
    let la = &fa[c.a].label;
    let lb = &fb[c.b].label;
    match &c.case {
        TangentCase::Coplanar { sense, area } => format!(
            "faces {la} and {lb} are coplanar within {}{} (normals {} apart), {}",
            um(c.distance),
            if c.detail.is_empty() { String::new() } else { format!(", {}", c.detail) },
            urad(c.angle),
            match sense {
                Sense::Opposed => format!("facing each other: the bodies touch there over {area:.3} mm²"),
                Sense::Aligned => format!("facing the same way: the faces are flush over {area:.3} mm²"),
            }
        ),
        TangentCase::CoincidentSurface { kind } => format!(
            "{la} and {lb} are the same surface (coincident {kind}, {}, within {})",
            c.detail,
            um(c.distance)
        ),
        TangentCase::TangentLine { kind } => format!(
            "tangent {kind}: {la} and {lb} touch along a line ({}; {} from exact tangency, axes {} apart)",
            c.detail,
            um(c.distance),
            urad(c.angle)
        ),
        TangentCase::TangentPoint { kind } => format!(
            "tangent {kind}: {la} and {lb} touch at a point ({}; {} from exact tangency)",
            c.detail,
            um(c.distance)
        ),
        TangentCase::EdgeOnFace { plane_in_a, interior } => {
            let (plane, other) = if *plane_in_a { (la, lb) } else { (lb, la) };
            if *interior {
                format!("edge-on-face contact: an edge of {other} lies in {plane} within {}", um(c.distance))
            } else {
                format!("edge-on-edge contact: an edge of {other} lies along the boundary of {plane} within {}", um(c.distance))
            }
        }
        TangentCase::VertexOnFace { plane_in_a, interior } => {
            let (plane, other) = if *plane_in_a { (la, lb) } else { (lb, la) };
            if *interior {
                format!("vertex-on-face contact: a corner of {other} rests on {plane} within {}", um(c.distance))
            } else {
                format!("corner-on-edge contact: a corner of {other} rests on the boundary of {plane} within {}", um(c.distance))
            }
        }
        TangentCase::NearMiss => format!(
            "near miss: {la} and {lb} come within {} of each other without touching or crossing",
            um(c.distance)
        ),
    }
}

/// Which kernel the fix is for: the exact kernel refuses, the mesh kernel
/// resolves what it can and is told what it did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    Exact,
    Mesh,
}

/// The change to the script that resolves the contact,
/// for each case, with the number filled in. `tol` is the
/// band the contact was classified in, so a suggested interference or
/// clearance lands outside it — twice the band, never less than 0.1 mm —
/// rather than on its edge, where the same pair would be a tangency again.
pub fn fix(c: &TangentContact, op: BoolOp, fa: &[TangentFace], fb: &[TangentFace], tol: f64, path: Path) -> String {
    let la = &fa[c.a].label;
    let lb = &fb[c.b].label;
    let clear = (2.0 * tol).max(0.1);
    let radius_of = |f: &TangentFace| match f.class {
        SurfaceClass::Cylinder { radius, .. } | SurfaceClass::Sphere { radius, .. } => Some(radius),
        _ => None,
    };
    match (&c.case, op, path) {
        (TangentCase::Coplanar { sense: Sense::Opposed, .. }, BoolOp::Union, Path::Exact) => format!(
            "A face-to-face touch is glue()'s case: glue(a, b) merges the two faces when one lies \
             within the other or they match, and refuses otherwise; or overlap the bodies by at \
             least 0.5 mm so the contact is a crossing, or pass kernel=\"mesh\", where the touch \
             becomes an exact tie and the bodies merge. A body joined to its own mirror image \
             across a plane is this case too: mirror(keep=True) does the join in one step"
        ),
        (TangentCase::Coplanar { sense: Sense::Opposed, .. }, BoolOp::Union, Path::Mesh) => {
            "The mesh kernel makes the tie exact and merges the bodies there: touching bodies are one solid".to_owned()
        }
        (TangentCase::Coplanar { sense: Sense::Opposed, .. }, _, Path::Exact) => format!(
            "The tool only rests against the body: subtract removes nothing and intersect is empty. \
             Sink {lb} into the body by at least 0.5 mm if it was meant to cut"
        ),
        (TangentCase::Coplanar { sense: Sense::Opposed, .. }, _, Path::Mesh) => {
            "The mesh kernel makes the tie exact and the tool then removes nothing there (a face \
             resting on a face encloses no volume)"
                .to_owned()
        }
        (TangentCase::Coplanar { sense: Sense::Aligned, .. }, BoolOp::Union, Path::Exact) => format!(
            "Overlap the bodies by at least 0.5 mm so the contact is a crossing, cut the flush part \
             back so {la} and {lb} only meet along an edge, or pass kernel=\"mesh\", where the two \
             faces stay two faces on one plane with a 0° seam between them"
        ),
        (TangentCase::Coplanar { sense: Sense::Aligned, .. }, BoolOp::Union, Path::Mesh) => format!(
            "The mesh kernel makes the tie exact and keeps both {la} and {lb} on one plane with a 0° \
             seam between them"
        ),
        (TangentCase::Coplanar { sense: Sense::Aligned, .. }, _, Path::Exact) => format!(
            "Move the tool's face {lb} off the body's plane by at least 0.5 mm — proud of it for a \
             through cut, into it for a floor — or pass kernel=\"mesh\", where a flush tool face \
             cuts through"
        ),
        (TangentCase::Coplanar { sense: Sense::Aligned, .. }, _, Path::Mesh) => {
            "The mesh kernel makes the tie exact and a flush tool face cuts through (equal-height \
             differences produce through-holes)"
                .to_owned()
        }
        (TangentCase::CoincidentSurface { .. }, _, _) => format!(
            "A same-radius fit is not a boolean: give {lb} {clear:.3} mm of clearance (or {clear:.3} mm \
             of interference, if it was meant to cut) and say which was meant"
        ),
        (TangentCase::TangentLine { .. } | TangentCase::TangentPoint { .. }, _, _) => {
            let mesh_caveat = if path == Path::Mesh {
                "; the mesh kernel cannot make a curved contact exact, \
                 so the result is two bodies that meet without merging, or a pinched seam"
            } else {
                ""
            };
            let on_a_plane = matches!(
                c.case,
                TangentCase::TangentLine { kind: "a cylinder on a plane" } | TangentCase::TangentPoint { kind: "a sphere on a plane" }
            );
            match radius_of(&fb[c.b]).filter(|_| !on_a_plane) {
                Some(r) => format!(
                    "Give {lb} at least {clear:.3} mm of interference (radius {:.3} instead of {r:.3}) so \
                     the contact becomes a crossing (0.5 mm is the usual overlap), or as much clearance \
                     so the two do not meet{mesh_caveat}",
                    r + clear
                ),
                None => format!(
                    "Sink {lb} at least {clear:.3} mm into {la} so the contact becomes a crossing (0.5 mm \
                     is the usual overlap), or lift it as far clear so the two do not meet{mesh_caveat}"
                ),
            }
        }
        (TangentCase::EdgeOnFace { .. } | TangentCase::VertexOnFace { .. }, _, _) => {
            "A contact along an edge or at a corner encloses nothing, so the result is pinched to \
             nothing there: overlap the bodies by at least 0.5 mm where they touch, or keep them \
             as two bodies"
                .to_owned()
        }
        (TangentCase::NearMiss, _, _) => format!(
            "Extend {lb} by {:.3} mm into {la} so the two cross (0.5 mm is the usual overlap), or \
             move it {:.3} mm clear so they do not meet",
            (c.distance + 0.01).max(0.02),
            c.distance + clear
        ),
    }
}

/// The first contact described, with a census of the rest: `faces …; and 2
/// more contacts (coplanar faces, touching ×1, edge-on-face ×1)`.
pub fn summary(contacts: &[TangentContact], fa: &[TangentFace], fb: &[TangentFace]) -> Option<String> {
    let first = contacts.first()?;
    let mut s = describe(first, fa, fb);
    if contacts.len() > 1 {
        let mut census: Vec<(&'static str, usize)> = Vec::new();
        for c in &contacts[1..] {
            match census.iter_mut().find(|(n, _)| *n == c.case.name()) {
                Some(e) => e.1 += 1,
                None => census.push((c.case.name(), 1)),
            }
        }
        s.push_str(&format!(
            "; and {} more contact{} ({})",
            contacts.len() - 1,
            if contacts.len() == 2 { "" } else { "s" },
            census.iter().map(|(n, k)| format!("{n} ×{k}")).collect::<Vec<_>>().join(", ")
        ));
    }
    Some(s)
}

/// [`summary`] of a full classification of two mesh bodies, with the
/// suggested fix for the first contact. What `Context` appends to a B-rep
/// refusal so the message carries the faces' names, which the exact kernel
/// does not have.
pub fn summary_for_bodies(op: BoolOp, a: &MeshBody, b: &MeshBody, tol: f64, path: Path) -> Option<String> {
    let eps = snap_eps(&a.0, &b.0);
    let fa = faces_of_body(a, eps);
    let fb = faces_of_body(b, eps);
    let contacts = classify(&fa, &fb, tol, Depth::Full);
    let first = contacts.first()?;
    Some(format!("{}. {}", summary(&contacts, &fa, &fb)?, fix(first, op, &fa, &fb, tol, path)))
}

// ------------------------------------------------------------------ snap

/// What [`snap_shared_planes`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapReport {
    /// Shared planes found with a vertex of the other operand within the band.
    pub groups: usize,
    /// Faces in those groups, over both operands.
    pub faces: usize,
    /// Vertices whose stored position changed.
    pub vertices: usize,
    /// The furthest any vertex moved from where it came in, in mm.
    pub max_move: f64,
    /// Groups declined because projecting them would have turned a
    /// well-shaped triangle over.
    pub declined: usize,
    /// The labels of each group's faces, for the note.
    pub members: Vec<Vec<String>>,
}

impl SnapReport {
    pub fn moved(&self) -> bool {
        self.vertices > 0
    }

    /// The note the user sees when something moved.
    pub fn note(&self, op: BoolOp, eps: f64) -> Option<String> {
        if !self.moved() {
            return None;
        }
        let names: Vec<String> = self
            .members
            .iter()
            .take(4)
            .map(|g| g.iter().take(6).cloned().collect::<Vec<_>>().join(", "))
            .collect();
        let list = if self.members.len() > 4 { format!("{}; …", names.join("; ")) } else { names.join("; ") };
        Some(format!(
            "mesh {}: snapped {} face group{} ({} face{}: {list}) onto {} shared plane{}, {} vert{} moved, \
             the furthest by {} — within the {} band, so two spellings of one plane become one and \
             manifold's exact engine resolves the contact as a tie instead of a sliver or a gap{}",
            op.kind_name(),
            self.groups,
            s(self.groups),
            self.faces,
            s(self.faces),
            self.groups,
            s(self.groups),
            self.vertices,
            if self.vertices == 1 { "ex" } else { "ices" },
            um(self.max_move),
            um(eps),
            if self.declined > 0 {
                format!("; {} group{} left alone because the move would have turned a triangle over", self.declined, s(self.declined))
            } else {
                String::new()
            }
        ))
    }
}

fn s(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// One operand's positions during the snap: the working copy, made only
/// when something has to move, and the input to measure moves against.
struct Working<'m> {
    input: &'m Mesh,
    positions: Option<Vec<f32>>,
    /// Vertex -> triangles that use it, built on first use.
    star: Option<Vec<Vec<u32>>>,
}

impl<'m> Working<'m> {
    fn new(input: &'m Mesh) -> Working<'m> {
        Working { input, positions: None, star: None }
    }

    fn positions(&self) -> &[f32] {
        self.positions.as_deref().unwrap_or(&self.input.positions)
    }

    fn get(&self, v: usize) -> Vec3 {
        let p = self.positions();
        Vec3::new(p[v * 3] as f64, p[v * 3 + 1] as f64, p[v * 3 + 2] as f64)
    }

    fn original(&self, v: usize) -> Vec3 {
        let p = &self.input.positions;
        Vec3::new(p[v * 3] as f64, p[v * 3 + 1] as f64, p[v * 3 + 2] as f64)
    }

    fn set(&mut self, v: usize, p: [f32; 3]) {
        let pos = self.positions.get_or_insert_with(|| self.input.positions.clone());
        pos[v * 3..v * 3 + 3].copy_from_slice(&p);
    }

    fn star(&mut self) -> &[Vec<u32>] {
        if self.star.is_none() {
            let mut star: Vec<Vec<u32>> = vec![Vec::new(); self.input.vertex_count()];
            for (t, tri) in self.input.indices.chunks_exact(3).enumerate() {
                for &v in tri {
                    star[v as usize].push(t as u32);
                }
            }
            self.star = Some(star);
        }
        self.star.as_deref().unwrap()
    }

    fn triangle(&self, t: usize) -> [Vec3; 3] {
        let i = &self.input.indices[t * 3..t * 3 + 3];
        [self.get(i[0] as usize), self.get(i[1] as usize), self.get(i[2] as usize)]
    }

    fn finish(self) -> Cow<'m, Mesh> {
        match self.positions {
            Some(positions) => Cow::Owned(Mesh { positions, ..self.input.clone() }),
            None => Cow::Borrowed(self.input),
        }
    }
}

/// A vertex's move as one edit, so a group can be undone.
struct Edit {
    vertex: usize,
    before: [f32; 3],
}

/// Snap the planes `a` and `b` share within `eps` (see the module notes).
/// **`a` never moves**: it is the accumulator of every fold and the
/// body a tool is applied to, so its spelling of every plane is the one
/// that is kept, and `b`'s vertices are what move onto it. Returns the two
/// meshes — `a` always borrowed, `b` borrowed when nothing moved, so a pair
/// with no shared plane costs one scan and no copy — and what was done.
///
/// **The plane.** A group seeded by a face of `a` snaps to that face's
/// analytic plane (truck's, carried through provenance: what the script
/// asked for, and the plane `a`'s own vertices are the f32 spelling of). A
/// group seeded by a face of `b` — a plane `a` has no face on but reaches
/// with an edge or a corner — snaps to the plane of `b`'s normal through
/// the mean of `a`'s vertices in the band, so `a`'s spelling still wins.
///
/// **Order and determinism.** Seeds are `a`'s planar faces largest first,
/// then `b`'s; a face joins the first seed whose plane it lies in; every
/// vertex of `b` within `eps` of a seed's plane is projected onto it, and a
/// vertex near two planes (a corner) is projected onto each in turn and
/// never taken further than `eps` from where it came in.
pub fn snap_shared_planes<'m>(
    a: &'m Mesh,
    fa: &[TangentFace],
    b: &'m Mesh,
    fb: &[TangentFace],
    eps: f64,
) -> (Cow<'m, Mesh>, Cow<'m, Mesh>, SnapReport) {
    let mut report = SnapReport::default();
    let mut work = Working::new(b);
    let faces: [&[TangentFace]; 2] = [fa, fb];
    let a_at = |v: usize| Vec3::new(a.positions[v * 3] as f64, a.positions[v * 3 + 1] as f64, a.positions[v * 3 + 2] as f64);
    let Some((alo, ahi)) = a.bbox() else { return (Cow::Borrowed(a), Cow::Borrowed(b), report) };
    let Some((blo, bhi)) = b.bbox() else { return (Cow::Borrowed(a), Cow::Borrowed(b), report) };
    // Does a box reach the plane? Its corners have to lie on both sides of
    // it, or within the band.
    let straddles = |lo: Vec3, hi: Vec3, p0: Vec3, n: Vec3| {
        let (mut cmin, mut cmax) = (f64::MAX, f64::MIN);
        for x in [lo.x, hi.x] {
            for y in [lo.y, hi.y] {
                for z in [lo.z, hi.z] {
                    let d = (Vec3::new(x, y, z) - p0).dot(n);
                    cmin = cmin.min(d);
                    cmax = cmax.max(d);
                }
            }
        }
        cmin <= eps && cmax >= -eps
    };

    let mut seeds: Vec<(usize, usize)> = Vec::new();
    for (o, fs) in faces.iter().enumerate() {
        seeds.extend(fs.iter().enumerate().filter(|(_, f)| f.plane.is_some()).map(|(i, _)| (o, i)));
    }
    seeds.sort_by(|&(o1, i1), &(o2, i2)| {
        o1.cmp(&o2).then(faces[o2][i2].area.total_cmp(&faces[o1][i1].area)).then(i1.cmp(&i2))
    });
    let mut taken = [vec![false; fa.len()], vec![false; fb.len()]];
    let cos_snap = fmath::cos(SNAP_ANGLE);

    for (o, i) in seeds {
        if taken[o][i] {
            continue;
        }
        let (p0, n) = faces[o][i].plane.unwrap();
        let (p0, n) = if o == 0 {
            // `a`'s plane: `b` has to reach it.
            if !straddles(blo, bhi, p0, n) {
                continue;
            }
            if !(0..b.vertex_count()).any(|v| ((work.get(v) - p0).dot(n)).abs() <= eps) {
                continue;
            }
            (p0, n)
        } else {
            // `b`'s plane: placed through `a`'s vertices in its band, if any.
            if !straddles(alo, ahi, p0, n) {
                continue;
            }
            let (mut sum, mut count) = (0.0, 0usize);
            for v in 0..a.vertex_count() {
                let d = (a_at(v) - p0).dot(n);
                if d.abs() <= eps {
                    sum += d;
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            (p0 + n * (sum / count as f64), n)
        };

        // Members: faces of either operand in this plane, for the report.
        let mut members: Vec<String> = Vec::new();
        for (p, fs) in faces.iter().enumerate() {
            for (k, f) in fs.iter().enumerate() {
                if taken[p][k] {
                    continue;
                }
                let Some((_, fnorm)) = f.plane else { continue };
                if fnorm.dot(n).abs() < cos_snap {
                    continue;
                }
                if f.tris().flatten().all(|q| ((q - p0).dot(n)).abs() <= eps) {
                    taken[p][k] = true;
                    members.push(f.label.clone());
                }
            }
        }

        // Project `b`'s vertices in the band.
        let mut edits: Vec<Edit> = Vec::new();
        let mut furthest = 0.0f64;
        for v in 0..b.vertex_count() {
            let cur = work.get(v);
            let d = (cur - p0).dot(n);
            if d.abs() > eps {
                continue;
            }
            let target = cur - n * d;
            let next = [target.x as f32, target.y as f32, target.z as f32];
            let after = Vec3::new(next[0] as f64, next[1] as f64, next[2] as f64);
            let total = (after - work.original(v)).length();
            if total > eps {
                continue; // a corner already moved by another plane: stay put
            }
            let pos = work.positions();
            let before = [pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]];
            if before.iter().zip(&next).all(|(x, y)| x.to_bits() == y.to_bits()) {
                continue;
            }
            work.set(v, next);
            edits.push(Edit { vertex: v, before });
            furthest = furthest.max(total);
        }
        if edits.is_empty() {
            // Already one plane to the bit: an exact tie, and not news.
            continue;
        }
        // The gate: no well-shaped triangle at a moved vertex may turn over.
        // At this band that would take a feature finer than the weld
        // distance; guarded against anyway, per group, so a refusal costs
        // one plane and not the snap.
        let mut flipped = false;
        'check: for e in &edits {
            let star = work.star()[e.vertex].clone();
            for t in star {
                let t = t as usize;
                let after = work.triangle(t);
                let ids = &work.input.indices[t * 3..t * 3 + 3];
                let before = [
                    work.original(ids[0] as usize),
                    work.original(ids[1] as usize),
                    work.original(ids[2] as usize),
                ];
                let n0 = (before[1] - before[0]).cross(before[2] - before[0]);
                let n1 = (after[1] - after[0]).cross(after[2] - after[0]);
                let alt = crate::mesh_clean::min_altitude(before[0], before[1], before[2]);
                if alt >= 10.0 * eps && n0.dot(n1) <= 0.0 {
                    flipped = true;
                    break 'check;
                }
            }
        }
        if flipped {
            for e in edits.iter().rev() {
                work.set(e.vertex, e.before);
            }
            report.declined += 1;
            continue;
        }
        report.groups += 1;
        report.faces += members.len();
        report.vertices += edits.len();
        report.max_move = report.max_move.max(furthest);
        report.members.push(members);
    }
    (Cow::Borrowed(a), work.finish(), report)
}

// ------------------------------------------------------------- geometry

/// Do two boxes come within `tol` of each other on every axis?
pub(crate) fn boxes_within(alo: Vec3, ahi: Vec3, blo: Vec3, bhi: Vec3, tol: f64) -> bool {
    let gap = |a0: f64, a1: f64, b0: f64, b1: f64| (b0 - a1).max(a0 - b1);
    gap(alo.x, ahi.x, blo.x, bhi.x) <= tol && gap(alo.y, ahi.y, blo.y, bhi.y) <= tol && gap(alo.z, ahi.z, blo.z, bhi.z) <= tol
}

pub(crate) fn tri_box(t: &[Vec3; 3]) -> (Vec3, Vec3) {
    (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]))
}

/// Is `p` inside one of `tris`, measured in the plane they all lie in?
pub(crate) fn inside_face(mut tris: impl Iterator<Item = [Vec3; 3]>, p: Vec3, n: Vec3) -> bool {
    tris.any(|t| {
        let c0 = (t[1] - t[0]).cross(p - t[0]).dot(n);
        let c1 = (t[2] - t[1]).cross(p - t[1]).dot(n);
        let c2 = (t[0] - t[2]).cross(p - t[2]).dot(n);
        (c0 >= 0.0 && c1 >= 0.0 && c2 >= 0.0) || (c0 <= 0.0 && c1 <= 0.0 && c2 <= 0.0)
    })
}

/// The area two coplanar faces share, exactly: every triangle of `b` clipped
/// against every triangle of `a` whose box it meets, in `a`'s plane
/// ([`crate::solid::coplanar_overlap`]). Two faces that merely abut share
/// nothing; a boss's bottom on a plate shares the boss's bottom, not the
/// plate triangle whose centroid happens to fall under it — which is what
/// the diagnostics quote, so an estimate will not do. `tol` pads the
/// triangle boxes, because the two faces are within it of one plane, not
/// on it.
pub(crate) fn shared_area(a: &TangentFace, b: &TangentFace, tol: f64) -> f64 {
    let mut total = 0.0;
    for ta in a.tris() {
        let (na, area) = crate::solid::tri_normal_area(&ta);
        if area == 0.0 {
            continue;
        }
        let (alo, ahi) = tri_box(&ta);
        for tb in b.tris() {
            let (blo, bhi) = tri_box(&tb);
            if !boxes_within(alo, ahi, blo, bhi, tol) {
                continue;
            }
            if let Some(shared) = crate::solid::coplanar_overlap(&ta, na, &tb, 1e-9) {
                total += shared;
            }
        }
    }
    total
}

/// Distance from a point to a triangle: the perpendicular when the foot
/// lands inside, and the nearest edge otherwise.
pub(crate) fn point_tri_distance(p: Vec3, t: &[Vec3; 3]) -> f64 {
    let n = (t[1] - t[0]).cross(t[2] - t[0]);
    let l = n.length();
    if l < 1e-18 {
        return t.iter().map(|q| (p - *q).length()).fold(f64::MAX, f64::min);
    }
    let n = n * (1.0 / l);
    let foot = p - n * (p - t[0]).dot(n);
    if inside_face(std::iter::once(*t), foot, n) {
        return (p - foot).length();
    }
    [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
        .iter()
        .map(|(a, b)| point_segment_distance(p, *a, *b))
        .fold(f64::MAX, f64::min)
}

pub(crate) fn point_segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 < 1e-18 {
        return (p - a).length();
    }
    (p - (a + ab * ((p - a).dot(ab) / l2).clamp(0.0, 1.0))).length()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers a diagnostic prints: float noise in hundredths of a
    /// micron, a real gap in microns, a feature in millimetres.
    #[test]
    fn lengths_are_printed_at_the_scale_they_matter() {
        assert_eq!(um(4e-5), "0.04 µm");
        assert_eq!(um(3.8e-6), "3.8 nm");
        assert_eq!(um(0.0), "0.00 µm");
        assert_eq!(um(1e-16), "0.00 µm", "below a tenth of a nanometre is zero");
        assert_eq!(um(3e-3), "3.0 µm");
        assert_eq!(um(0.03), "30.0 µm");
        assert_eq!(um(0.25), "0.250 mm");
        assert_eq!(urad(0.0), "0.0 µrad");
        assert_eq!(urad(2e-4), "200.0 µrad");
        assert_eq!(urad(0.02), "1.146°");
    }

    /// The band: the weld distance for anything up to 100 mm across, and a
    /// millionth of the pair's diagonal beyond — 1.7 µm on a metre cube,
    /// where f32 places a coordinate to about 0.06 µm.
    #[test]
    fn the_band_is_the_floor_up_to_a_hundred_millimetres_and_relative_beyond() {
        let m = |extent: f64| Mesh {
            positions: vec![0.0, 0.0, 0.0, extent as f32, extent as f32, extent as f32],
            normals: vec![0.0; 6],
            indices: vec![],
            face_ids: vec![],
            colors: Vec::new(),
        };
        assert_eq!(snap_eps(&m(10.0), &m(10.0)), SNAP_FLOOR);
        assert_eq!(snap_eps(&m(50.0), &m(50.0)), SNAP_FLOOR);
        let metre = snap_eps(&m(1000.0), &m(1000.0));
        assert!((metre - SNAP_RELATIVE * 1000.0 * 3f64.sqrt()).abs() < 1e-12 && metre > SNAP_FLOOR, "{metre}");
        // The joint box: a small tool beside a large body takes the body's scale.
        assert_eq!(snap_eps(&m(1000.0), &m(1.0)), metre);
        assert_eq!(snap_eps(&Mesh::default(), &Mesh::default()), SNAP_FLOOR);
    }

    /// A face's border is the edges its triangles do not share: four for a
    /// quad split in two, whichever way it was split.
    #[test]
    fn the_border_of_a_quad_is_its_four_sides() {
        let (a, b, c, d) = (Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        let face = TangentFace::owned("q".into(), SurfaceClass::Other, None, vec![[a, b, c], [a, c, d]]);
        let border = border_of(&face);
        assert_eq!(border.len(), 4, "{border:?}");
        assert!(inside_face(face.tris(), Vec3::new(1.0, 1.0, 0.0), Vec3::Z));
        assert!(!inside_face(face.tris(), Vec3::new(3.0, 1.0, 0.0), Vec3::Z));
        // The fitted plane of a flat face is found; a folded one is not planar.
        assert!(fitted_plane(&face, 1e-4).is_some());
        let folded = TangentFace::owned("f".into(), SurfaceClass::Other, None, vec![[a, b, c], [a, c, Vec3::new(0.0, 2.0, 2.0)]]);
        assert!(fitted_plane(&folded, 1e-4).is_none());
    }
}
