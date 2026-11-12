//! Recognising the analytic surface a face lies on: [`SurfaceClass`].
//!
//! truck has four surface types — `Plane`, `BSplineSurface`, `NurbsSurface`
//! and `RevolutedCurve` — and Cadpole builds every curved face out of the last
//! three. A hole is an extruded circular arc, which `tsweep` turns into a
//! rational B-spline patch; a cone is a loft band ruled between two coaxial
//! arcs (`homotopy`); a vase wall is a `RevolutedCurve`. None of those
//! types says "cylinder", so `kernel::surface_kind` cannot either, and
//! everything that needs a cylinder's axis and radius — `faces(kind=
//! "cylinder")`, a `concentric` mate, an exact offset, the STEP writer — has
//! had to derive it again for itself. This module is the one place that
//! derivation lives.
//!
//! **Recognised, never assumed.** Every class is found the same way:
//!
//! 1. **Sample** the surface on a [`GRID`]×[`GRID`] net over its own
//!    parameter range — points and normals, a hair inside the edges, because
//!    the edges are where a fit that only holds in the middle shows up.
//! 2. **Fit** a candidate in closed form. A plane from the mean normal; a
//!    surface of revolution from the isoparametric circles the samples lie
//!    on, whose crossing isoline (a line or a circle) says which of
//!    cylinder, cone, sphere and torus it is; and, for a patch that is not
//!    laid out along its own circles, the textbook fits — a sphere and a
//!    cylinder by algebraic least squares, a cone from its tangent planes,
//!    which all pass through the apex.
//! 3. **Verify**: every sample lies within `tol` of the candidate, and the
//!    candidate's normal is parallel to truck's at every sample — all along
//!    or all against, within half a degree. A candidate that fails is
//!    dropped; when none survives the surface is [`SurfaceClass::Other`].
//!
//! Step 3 is the contract. A structural guess — "this is a loft between two
//! circles, so it is a cone" — is never trusted on its own: the very same
//! `homotopy` between a boss's arc and a corner's arc is a ruled surface that
//! is not a cone, and calling it one would put a wrong axis into a mate or a
//! wrong `CONICAL_SURFACE` into a STEP file, silently. What `tol` means is
//! exactly *the largest distance any sampled point may be from the claimed
//! surface*, in model units (millimetres); [`EXACT_TOL`] is the value a
//! surface Cadpole constructed exactly satisfies with six orders of magnitude to
//! spare, and it is what the tessellation stamps into every
//! [`FaceInfo`](crate::tessellate::FaceInfo). A looser `tol` recognises a
//! surface that is a cylinder *to within* that distance; the STEP writer does
//! not use one for surfaces, because it writes the face's edges exactly and a
//! surface that is only near the patch leaves them off the face.
//!
//! **Sign conventions.** A plane's `normal` is the surface's own parametric
//! normal (`du × dv`), so a face and its inverse report opposite normals —
//! `FaceInfo::normal` is the outward one, and the tessellation classifies the
//! *oriented* surface so the two agree. A cone's `axis` points from the apex
//! into the surface, which the geometry fixes. A cylinder's and a torus's axis
//! has no natural sign, so it is made canonical: positive `z`, or positive `y`
//! when `z` is zero, or positive `x` — the same `(z, y, x)` order the
//! selection sort uses. Under those rules a face and its inverse
//! classify identically, the plane normal aside.
//!
//! Three other recognisers agree with this one: `kernel::cone_of` and
//! `kernel::revolution_of` (the tessellation swap, which keep their own
//! ruling-line and meridian constructions) and
//! `cadpole_export_step::analytic`'s `recognise_surface` /
//! `revolved_analytic`.

use truck_modeling::{ParametricSurface, ParametricSurface3D, Surface};

use crate::geom::{Transform, Vec3};
use crate::fmath;

/// The tolerance at which a surface Cadpole built *exactly* — an extruded arc, a
/// loft band between coaxial arcs, a revolved line or arc — is recognised: a
/// micron. f64 evaluation of a rational patch on a metre-sized part is good
/// to about 1e-12 mm, and no tessellation runs tighter than 1e-3 mm, so a
/// micron sits in the middle of six orders of magnitude of empty space:
/// nothing exact fails it and nothing approximate passes it.
pub const EXACT_TOL: f64 = 1e-6;

/// Samples per parameter direction. 7×7 is 49 constraints against at most
/// seven unknowns, with every isoline sampled at seven points so a circle
/// fitted through three of them is checked against four more.
const GRID: usize = 7;

/// How far inside the parameter range the outermost samples sit, as a
/// fraction of the range. A B-spline's normal is undefined at a corner where
/// the control points coincide, and a revolved surface's angular range is
/// half-open; a thousandth keeps the samples on the surface proper while
/// still catching a candidate that only fits the middle.
const INSET: f64 = 1e-3;

/// How far apart two directions may be and still count as the same, as the
/// cosine of the angle: half a degree. Tight enough that a cone is never
/// called a cylinder, loose enough to survive the normal of a patch whose
/// control net is a hair off symmetric.
const NORMAL_COS: f64 = 0.999_961;

/// Direction comparisons *inside the fits* (are these two circle normals
/// parallel?), where the answer only decides which candidate is tried; the
/// candidate is then verified at `tol`, so this can be loose.
const PARALLEL_EPS: f64 = 1e-6;

/// The analytic surface a face lies on, with the parameters a mate, an
/// offset or an exporter needs. Distances in millimetres, angles in radians,
/// axes and normals unit vectors.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceClass {
    /// `normal` is the surface's parametric normal; see the module notes on
    /// sign.
    Plane { origin: Vec3, normal: Vec3 },
    /// `axis_point` is the point of the axis nearest the samples' centroid,
    /// so it lies beside the face rather than at some distant origin.
    Cylinder { axis_point: Vec3, axis: Vec3, radius: f64 },
    /// One nappe. `axis` points from the apex into the surface; the radius
    /// at height `h` along it is `h · tan(half_angle)`, with
    /// `0 < half_angle < π/2`.
    Cone { apex: Vec3, axis: Vec3, half_angle: f64 },
    Sphere { center: Vec3, radius: f64 },
    /// `center` is the centre of the spine circle, `major` its radius and
    /// `minor` the tube's. `major < minor` (a spindle torus) is still a
    /// torus here; an exporter that cannot write one says so itself.
    Torus { center: Vec3, axis: Vec3, major: f64, minor: f64 },
    /// A genuine free-form surface — or one that is analytic only to a
    /// looser tolerance than was asked for.
    Other,
}

impl Default for SurfaceClass {
    fn default() -> Self {
        SurfaceClass::Other
    }
}

impl SurfaceClass {
    /// The class as `Face.kind` spells it: `plane`, `cylinder`, `cone`,
    /// `sphere`, `torus`, or `other`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            SurfaceClass::Plane { .. } => "plane",
            SurfaceClass::Cylinder { .. } => "cylinder",
            SurfaceClass::Cone { .. } => "cone",
            SurfaceClass::Sphere { .. } => "sphere",
            SurfaceClass::Torus { .. } => "torus",
            SurfaceClass::Other => "other",
        }
    }

    pub fn is_recognised(&self) -> bool {
        !matches!(self, SurfaceClass::Other)
    }

    /// Unsigned distance from `p` to the surface; infinite for `Other`.
    pub fn distance(&self, p: Vec3) -> f64 {
        match *self {
            SurfaceClass::Plane { origin, normal } => (p - origin).dot(normal).abs(),
            SurfaceClass::Cylinder { axis_point, axis, radius } => (radial(p, axis_point, axis).1 - radius).abs(),
            SurfaceClass::Cone { apex, axis, half_angle } => {
                let (h, r) = radial(p, apex, axis);
                let theta = fmath::atan2(r, h);
                let d = (p - apex).length();
                // Perpendicular to the ruling while the foot of the
                // perpendicular is on the nappe; past the apex the nearest
                // point of the nappe is the apex itself.
                let off = theta - half_angle;
                if off.abs() <= std::f64::consts::FRAC_PI_2 { d * fmath::sin(off).abs() } else { d }
            }
            SurfaceClass::Sphere { center, radius } => ((p - center).length() - radius).abs(),
            SurfaceClass::Torus { center, axis, major, minor } => {
                let (h, r) = radial(p, center, axis);
                ((fmath::powi(r - major, 2) + h * h).sqrt() - minor).abs()
            }
            SurfaceClass::Other => f64::INFINITY,
        }
    }

    /// The surface's *natural* normal at a point on it: the plane's own, and
    /// otherwise "away from the axis, the apex or the spine". `None` where
    /// it is undefined (on a cylinder's axis, at a cone's apex, at a torus's
    /// axis) and for `Other`.
    pub fn normal_at(&self, p: Vec3) -> Option<Vec3> {
        match *self {
            SurfaceClass::Plane { normal, .. } => Some(normal),
            SurfaceClass::Cylinder { axis_point, axis, .. } => unit(radial_dir(p, axis_point, axis)),
            SurfaceClass::Cone { apex, axis, half_angle } => {
                let out = unit(radial_dir(p, apex, axis))?;
                unit(out * fmath::cos(half_angle) - axis * fmath::sin(half_angle))
            }
            SurfaceClass::Sphere { center, .. } => unit(p - center),
            SurfaceClass::Torus { center, axis, major, .. } => {
                let out = unit(radial_dir(p, center, axis))?;
                unit(p - (center + out * major))
            }
            SurfaceClass::Other => None,
        }
    }

    /// This class moved by `t`, so a copy's or a pattern's faces carry the
    /// class of the source without being re-recognised. Radii scale with the
    /// transform's uniform scale; a canonical axis sign is re-derived, since
    /// a rotation can turn it over.
    pub fn transformed(&self, t: &Transform) -> SurfaceClass {
        let s = t.scale_factor();
        match *self {
            SurfaceClass::Plane { origin, normal } => {
                SurfaceClass::Plane { origin: t.point(origin), normal: t.direction(normal) }
            }
            SurfaceClass::Cylinder { axis_point, axis, radius } => SurfaceClass::Cylinder {
                axis_point: t.point(axis_point),
                axis: t.direction(axis),
                radius: radius * s,
            }
            .canonical(),
            SurfaceClass::Cone { apex, axis, half_angle } => {
                SurfaceClass::Cone { apex: t.point(apex), axis: t.direction(axis), half_angle }
            }
            SurfaceClass::Sphere { center, radius } => SurfaceClass::Sphere { center: t.point(center), radius: radius * s },
            SurfaceClass::Torus { center, axis, major, minor } => SurfaceClass::Torus {
                center: t.point(center),
                axis: t.direction(axis),
                major: major * s,
                minor: minor * s,
            }
            .canonical(),
            SurfaceClass::Other => SurfaceClass::Other,
        }
    }

    /// The sign rule of the module notes, for the axes that have no natural
    /// sign.
    fn canonical(self) -> SurfaceClass {
        match self {
            SurfaceClass::Cylinder { axis_point, axis, radius } => {
                SurfaceClass::Cylinder { axis_point, axis: canonical_sign(axis), radius }
            }
            SurfaceClass::Torus { center, axis, major, minor } => {
                SurfaceClass::Torus { center, axis: canonical_sign(axis), major, minor }
            }
            other => other,
        }
    }
}

/// Recognise `surface` as one of the five analytic classes, or `Other`.
///
/// `tol` is a distance in millimetres: the class is claimed only when every
/// one of the [`GRID`]² samples lies within `tol` of it and faces the same
/// way (see the module notes). [`EXACT_TOL`] is the value for "is this
/// surface *exactly* a cylinder"; a document or export tolerance is the value
/// for "may I write it as one". A non-positive or non-finite `tol`
/// recognises nothing.
pub fn classify(surface: &Surface, tol: f64) -> SurfaceClass {
    if !(tol > 0.0 && tol.is_finite()) {
        return SurfaceClass::Other;
    }
    if let Surface::Plane(p) = surface {
        // Already analytic, with its own exact frame; a refit could only
        // lose precision.
        return SurfaceClass::Plane {
            origin: Vec3::from_point(p.origin()),
            normal: Vec3::from_vector(p.normal()).normalized(),
        };
    }
    let Some(grid) = sample(surface) else { return SurfaceClass::Other };
    // Cheapest and most common first; each is verified before it is
    // believed, so the order only decides which class a surface that is two
    // things within `tol` (a tiny patch of a huge sphere is also a plane
    // within a micron) is called.
    let candidates = [
        fit_plane(&grid),
        revolution_candidate(&grid, tol),
        fit_sphere(&grid),
        fit_cylinder(&grid),
        fit_cone(&grid),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|c| verify(c, &grid, tol))
        .map(SurfaceClass::canonical)
        .unwrap_or(SurfaceClass::Other)
}

// ---------------------------------------------------------------- samples

/// One sample: where the surface is and, when truck can say, which way it
/// faces.
struct Sample {
    p: Vec3,
    n: Option<Vec3>,
}

/// The `GRID`×`GRID` net: sample `(i, j)` is at `pts[i + GRID * j]`, `i`
/// along `u` and `j` along `v`.
struct Grid {
    pts: Vec<Sample>,
}

impl Grid {
    fn at(&self, i: usize, j: usize) -> &Sample {
        &self.pts[i + GRID * j]
    }

    /// The `u`-isoline at `v_j`.
    fn row(&self, j: usize) -> Vec<Vec3> {
        (0..GRID).map(|i| self.at(i, j).p).collect()
    }

    /// The `v`-isoline at `u_i`.
    fn col(&self, i: usize) -> Vec<Vec3> {
        (0..GRID).map(|j| self.at(i, j).p).collect()
    }

    fn centroid(&self) -> Vec3 {
        self.pts.iter().fold(Vec3::ZERO, |a, s| a + s.p) * (1.0 / self.pts.len() as f64)
    }

    fn normals(&self) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        self.pts.iter().filter_map(|s| s.n.map(|n| (s.p, n)))
    }
}

fn sample(s: &Surface) -> Option<Grid> {
    let (ur, vr) = s.try_range_tuple();
    let (u0, u1) = ur?;
    let (v0, v1) = vr?;
    if !(u0.is_finite() && u1.is_finite() && v0.is_finite() && v1.is_finite()) || u1 <= u0 || v1 <= v0 {
        return None;
    }
    let t = |k: usize| INSET + (1.0 - 2.0 * INSET) * k as f64 / (GRID - 1) as f64;
    let mut pts = Vec::with_capacity(GRID * GRID);
    let mut with_normal = 0usize;
    for j in 0..GRID {
        for i in 0..GRID {
            let (u, v) = (u0 + (u1 - u0) * t(i), v0 + (v1 - v0) * t(j));
            let p = s.subs(u, v);
            if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
                return None;
            }
            let n = s.normal(u, v);
            let n = if n.x.is_finite() && n.y.is_finite() && n.z.is_finite() {
                unit(Vec3::from_vector(n))
            } else {
                None
            };
            with_normal += usize::from(n.is_some());
            pts.push(Sample { p: Vec3::from_point(p), n });
        }
    }
    // A pole or a collapsed corner may have no normal; a surface with no
    // normals anywhere is not one this module can say anything about.
    if with_normal * 2 < GRID * GRID {
        return None;
    }
    Some(Grid { pts })
}

/// Does `cand` explain every sample, in position and in facing?
fn verify(cand: &SurfaceClass, g: &Grid, tol: f64) -> bool {
    let (mut along, mut against) = (0usize, 0usize);
    for s in &g.pts {
        // Written so that a NaN distance fails.
        if !(cand.distance(s.p) <= tol) {
            return false;
        }
        let Some(n) = s.n else { continue };
        let Some(m) = cand.normal_at(s.p) else { continue };
        let d = m.dot(n);
        if d >= NORMAL_COS {
            along += 1;
        } else if d <= -NORMAL_COS {
            against += 1;
        } else {
            return false;
        }
    }
    along + against >= 3 && (along == 0 || against == 0)
}

// ---------------------------------------------------------------- fits

/// The mean normal, when the normals agree well enough for a plane to be
/// worth verifying.
fn fit_plane(g: &Grid) -> Option<SurfaceClass> {
    let (sum, count) = g.normals().fold((Vec3::ZERO, 0usize), |(a, c), (_, n)| (a + n, c + 1));
    if sum.length() < 0.9 * count as f64 {
        return None;
    }
    Some(SurfaceClass::Plane { origin: g.centroid(), normal: unit(sum)? })
}

/// A circle through sampled points, or `None` when they are not on one.
struct Circle {
    centre: Vec3,
    radius: f64,
    normal: Vec3,
}

/// The circle through three well-spread points of `pts`, if every point is on
/// it to within `tol`.
fn circle_of(pts: &[Vec3], tol: f64) -> Option<Circle> {
    let n = pts.len();
    let (centre, radius, normal) = circle_through(pts[0], pts[n / 3], pts[2 * n / 3])?;
    let on = |p: Vec3| {
        let d = p - centre;
        let h = d.dot(normal);
        let r = (d - normal * h).length();
        (fmath::powi(r - radius, 2) + h * h).sqrt() <= tol
    };
    if pts.iter().all(|&p| on(p)) { Some(Circle { centre, radius, normal }) } else { None }
}

/// The circle through three points: centre, radius and the unit normal of
/// its plane, or `None` when they are collinear or two coincide.
fn circle_through(p: Vec3, q: Vec3, r: Vec3) -> Option<(Vec3, f64, Vec3)> {
    let (a, b) = (p - r, q - r);
    let axb = a.cross(b);
    if !(axb.length() > 1e-9 * a.length() * b.length()) {
        return None;
    }
    let centre = r + (b * a.dot(a) - a * b.dot(b)).cross(axb) * (1.0 / (2.0 * axb.dot(axb)));
    Some((centre, (p - centre).length(), axb.normalized()))
}

/// Are the points on the straight line through the first and the last?
fn is_line(pts: &[Vec3], tol: f64) -> bool {
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let Some(d) = unit(b - a) else { return false };
    pts.iter().all(|&p| {
        let v = p - a;
        (v - d * v.dot(d)).length() <= tol
    })
}

/// A surface of revolution read off the isolines: one family of isolines
/// must be coaxial circles and the crossing family the profile, a line
/// (cylinder, cone, or a plane) or a circle (sphere, torus). Both ways round
/// are tried, because truck lays a revolved surface out as (curve, angle)
/// and a swept or lofted arc as (arc, run).
fn revolution_candidate(g: &Grid, tol: f64) -> Option<SurfaceClass> {
    let rows: Vec<Vec<Vec3>> = (0..GRID).map(|j| g.row(j)).collect();
    let cols: Vec<Vec<Vec3>> = (0..GRID).map(|i| g.col(i)).collect();
    revolution_from(&rows, &cols, g.centroid(), tol).or_else(|| revolution_from(&cols, &rows, g.centroid(), tol))
}

fn revolution_from(rings: &[Vec<Vec3>], profiles: &[Vec<Vec3>], centroid: Vec3, tol: f64) -> Option<SurfaceClass> {
    let circles: Vec<Circle> = rings.iter().filter_map(|r| circle_of(r, tol)).collect();
    if circles.len() < 2 {
        return None;
    }
    let axis = circles[0].normal;
    let c0 = circles[0].centre;
    for c in &circles[1..] {
        if c.normal.cross(axis).length() > PARALLEL_EPS {
            return None;
        }
        let d = c.centre - c0;
        if (d - axis * d.dot(axis)).length() > tol {
            return None;
        }
    }
    let height = |p: Vec3| (p - c0).dot(axis);
    let rho = |p: Vec3| radial(p, c0, axis).1;
    // The profile: the crossing isoline that runs furthest, so a pole or a
    // collapsed corner does not pick a degenerate one.
    let extent = |l: &Vec<Vec3>| (l[l.len() - 1] - l[0]).length();
    let profile = profiles.iter().max_by(|a, b| extent(a).total_cmp(&extent(b)))?;
    if is_line(profile, tol) {
        let (p0, p1) = (profile[0], profile[profile.len() - 1]);
        let (r0, r1) = (rho(p0), rho(p1));
        let (h0, h1) = (height(p0), height(p1));
        let (dr, dh) = (r1 - r0, h1 - h0);
        if dr.abs() <= tol {
            let axis_point = c0 + axis * height(centroid);
            return Some(SurfaceClass::Cylinder { axis_point, axis, radius: 0.5 * (r0 + r1) });
        }
        if dh.abs() <= tol {
            return Some(SurfaceClass::Plane { origin: c0 + axis * h0, normal: axis });
        }
        // The ruling reaches the axis where its radius reaches zero; that is
        // the apex, and the surface lies on the side of it the samples are.
        let apex_h = h0 - r0 * dh / dr;
        let apex = c0 + axis * apex_h;
        let axis = if 0.5 * (h0 + h1) >= apex_h { axis } else { -axis };
        return Some(SurfaceClass::Cone { apex, axis, half_angle: fmath::atan((dr / dh).abs()) });
    }
    let c = circle_of(profile, tol)?;
    // A meridian's plane contains the axis.
    if c.normal.dot(axis).abs() > PARALLEL_EPS {
        return None;
    }
    let major = rho(c.centre);
    let center = c0 + axis * height(c.centre);
    if major <= tol {
        return Some(SurfaceClass::Sphere { center, radius: c.radius });
    }
    Some(SurfaceClass::Torus { center, axis, major, minor: c.radius })
}

/// Kasa's algebraic sphere fit: `2 p·c + (r² − |c|²) = |p|²` is linear in
/// `(c, k)`.
fn fit_sphere(g: &Grid) -> Option<SurfaceClass> {
    let mut ata = [[0.0f64; 4]; 4];
    let mut atb = [0.0f64; 4];
    for s in &g.pts {
        let row = [2.0 * s.p.x, 2.0 * s.p.y, 2.0 * s.p.z, 1.0];
        let b = s.p.dot(s.p);
        for i in 0..4 {
            for j in 0..4 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * b;
        }
    }
    let x = solve(&mut ata, &mut atb)?;
    let center = Vec3::new(x[0], x[1], x[2]);
    let r2 = x[3] + center.dot(center);
    if !(r2 > 0.0) {
        return None;
    }
    Some(SurfaceClass::Sphere { center, radius: r2.sqrt() })
}

/// A cylinder's normals are all perpendicular to its axis, so the axis is the
/// cross product of the two most-separated normals; the section is then a
/// circle fitted in the plane perpendicular to it.
fn fit_cylinder(g: &Grid) -> Option<SurfaceClass> {
    let ns: Vec<Vec3> = g.normals().map(|(_, n)| n).collect();
    let mut best = (0.0f64, Vec3::ZERO);
    for (i, a) in ns.iter().enumerate() {
        for b in &ns[i + 1..] {
            let c = a.cross(*b);
            if c.length() > best.0 {
                best = (c.length(), c);
            }
        }
    }
    if best.0 < PARALLEL_EPS {
        return None;
    }
    let axis = unit(best.1)?;
    let (e1, e2) = frame(axis);
    let flat: Vec<(f64, f64)> = g.pts.iter().map(|s| (s.p.dot(e1), s.p.dot(e2))).collect();
    let (cx, cy, radius) = fit_circle_2d(&flat)?;
    let axis_point = e1 * cx + e2 * cy + axis * g.centroid().dot(axis);
    Some(SurfaceClass::Cylinder { axis_point, axis, radius })
}

/// A cone's normals all make the same angle with its axis, so the axis is
/// perpendicular to every difference of two normals; its tangent planes all
/// pass through the apex, which is therefore their least-squares
/// intersection; and the half-angle is then read off the samples.
fn fit_cone(g: &Grid) -> Option<SurfaceClass> {
    let samples: Vec<(Vec3, Vec3)> = g.normals().collect();
    let ns: Vec<Vec3> = samples.iter().map(|(_, n)| *n).collect();
    // The best-conditioned pair of differences: the longest one, then the
    // one least parallel to it.
    let mut pair = (0.0f64, 0usize, 0usize);
    for i in 0..ns.len() {
        for j in i + 1..ns.len() {
            let l = (ns[i] - ns[j]).length();
            if l > pair.0 {
                pair = (l, i, j);
            }
        }
    }
    if pair.0 < PARALLEL_EPS {
        return None;
    }
    let d1 = ns[pair.1] - ns[pair.2];
    let mut best = (0.0f64, Vec3::ZERO);
    for n in &ns {
        let c = d1.cross(*n - ns[pair.2]);
        if c.length() > best.0 {
            best = (c.length(), c);
        }
    }
    if best.0 < PARALLEL_EPS {
        return None;
    }
    let axis = unit(best.1)?;
    let mut ata = [[0.0f64; 3]; 3];
    let mut atb = [0.0f64; 3];
    for (p, n) in &samples {
        let row = [n.x, n.y, n.z];
        let b = n.dot(*p);
        for i in 0..3 {
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * b;
        }
    }
    let x = solve(&mut ata, &mut atb)?;
    let apex = Vec3::new(x[0], x[1], x[2]);
    let mean_h: f64 = g.pts.iter().map(|s| (s.p - apex).dot(axis)).sum::<f64>() / g.pts.len() as f64;
    let axis = if mean_h >= 0.0 { axis } else { -axis };
    let mut angle = 0.0;
    for s in &g.pts {
        let (h, r) = radial(s.p, apex, axis);
        if h <= 0.0 {
            return None;
        }
        angle += fmath::atan2(r, h);
    }
    let half_angle = angle / g.pts.len() as f64;
    if !(half_angle > 0.0 && half_angle < std::f64::consts::FRAC_PI_2 - 1e-9) {
        return None;
    }
    Some(SurfaceClass::Cone { apex, axis, half_angle })
}

/// Kasa's algebraic circle fit in the plane: `A x + B y + C = x² + y²`.
fn fit_circle_2d(pts: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let mut ata = [[0.0f64; 3]; 3];
    let mut atb = [0.0f64; 3];
    for &(x, y) in pts {
        let row = [x, y, 1.0];
        let b = x * x + y * y;
        for i in 0..3 {
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * b;
        }
    }
    let x = solve(&mut ata, &mut atb)?;
    let (cx, cy) = (x[0] / 2.0, x[1] / 2.0);
    let r2 = x[2] + cx * cx + cy * cy;
    if !(r2 > 0.0) {
        return None;
    }
    Some((cx, cy, r2.sqrt()))
}

/// Gaussian elimination with partial pivoting; `N` is 3 or 4 here.
fn solve<const N: usize>(a: &mut [[f64; N]; N], b: &mut [f64; N]) -> Option<[f64; N]> {
    for col in 0..N {
        let mut pivot = col;
        for r in col + 1..N {
            if a[r][col].abs() > a[pivot][col].abs() {
                pivot = r;
            }
        }
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for r in col + 1..N {
            let f = a[r][col] / a[col][col];
            for c in col..N {
                a[r][c] -= f * a[col][c];
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = [0.0f64; N];
    for i in (0..N).rev() {
        let mut s = b[i];
        for j in i + 1..N {
            s -= a[i][j] * x[j];
        }
        x[i] = s / a[i][i];
    }
    if x.iter().all(|v| v.is_finite()) { Some(x) } else { None }
}

// ---------------------------------------------------------------- vectors

fn unit(v: Vec3) -> Option<Vec3> {
    let l = v.length();
    if l > 1e-12 && l.is_finite() { Some(v * (1.0 / l)) } else { None }
}

/// Height along `axis` from `origin`, and distance from the axis.
fn radial(p: Vec3, origin: Vec3, axis: Vec3) -> (f64, f64) {
    let d = p - origin;
    let h = d.dot(axis);
    (h, (d - axis * h).length())
}

/// The (unnormalised) direction from the axis out to `p`.
fn radial_dir(p: Vec3, origin: Vec3, axis: Vec3) -> Vec3 {
    let d = p - origin;
    d - axis * d.dot(axis)
}

/// Two unit vectors spanning the plane perpendicular to `axis`.
fn frame(axis: Vec3) -> (Vec3, Vec3) {
    let a = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let e1 = (a - axis * a.dot(axis)).normalized();
    (e1, axis.cross(e1))
}

/// Positive `z`; failing that positive `y`; failing that positive `x`.
fn canonical_sign(a: Vec3) -> Vec3 {
    let lead = if a.z.abs() > PARALLEL_EPS {
        a.z
    } else if a.y.abs() > PARALLEL_EPS {
        a.y
    } else {
        a.x
    };
    if lead < 0.0 { -a } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Plane;
    use crate::kernel::{self, KSolid};
    use crate::loft::{LoftPlan, Section, SectionMatch};
    use crate::sketch::{Loop, Path, Sketch, P2};
    use std::f64::consts::{FRAC_PI_4, PI, TAU};
    use truck_modeling::{builder, BSplineSurface, Curve, Invertible, KnotVec, Line, Point3, Processor, Rad, RevolutedCurve, Vector3};

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn parallel(a: Vec3, b: Vec3) -> bool {
        a.cross(b).length() < 1e-9
    }

    fn same_dir(a: Vec3, b: Vec3) -> bool {
        parallel(a, b) && a.dot(b) > 0.0
    }

    fn near_v(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-9
    }

    /// The same class with the same parameters, to floating-point noise.
    fn same_class(a: &SurfaceClass, b: &SurfaceClass) -> bool {
        use SurfaceClass::*;
        match (a, b) {
            (Plane { origin: oa, normal: na }, Plane { origin: ob, normal: nb }) => {
                near_v(*na, *nb) && near((*oa - *ob).dot(*na), 0.0)
            }
            (
                Cylinder { axis_point: pa, axis: aa, radius: ra },
                Cylinder { axis_point: pb, axis: ab, radius: rb },
            ) => near_v(*aa, *ab) && near(*ra, *rb) && near_v(*pa - *aa * (*pa - *pb).dot(*aa), *pb),
            (Cone { apex: pa, axis: aa, half_angle: ha }, Cone { apex: pb, axis: ab, half_angle: hb }) => {
                near_v(*pa, *pb) && near_v(*aa, *ab) && near(*ha, *hb)
            }
            (Sphere { center: ca, radius: ra }, Sphere { center: cb, radius: rb }) => near_v(*ca, *cb) && near(*ra, *rb),
            (
                Torus { center: ca, axis: aa, major: ma, minor: na },
                Torus { center: cb, axis: ab, major: mb, minor: nb },
            ) => near_v(*ca, *cb) && near_v(*aa, *ab) && near(*ma, *mb) && near(*na, *nb),
            (Other, Other) => true,
            _ => false,
        }
    }

    /// Every face of `s`, oriented as the face is (outward normal), with
    /// its class.
    fn classes(s: &KSolid) -> Vec<(String, SurfaceClass)> {
        s.face_iter()
            .map(|f| (kernel::surface_kind(&f.surface()).to_owned(), classify(&f.oriented_surface(), EXACT_TOL)))
            .collect()
    }

    fn circle_loft(sections: &[(f64, f64)]) -> KSolid {
        let secs: Vec<Section> = sections
            .iter()
            .map(|&(z, r)| Section::Profile(Sketch::new(Plane::XY.offset(z)).with_loop(Loop::circle(P2::new(0.0, 0.0), r))))
            .collect();
        let plan = LoftPlan::new(&secs, true, SectionMatch::Index).unwrap();
        kernel::loft(&plan).unwrap().remove(0)
    }

    fn revolved(curve: Curve, origin: Vec3, axis: Vec3) -> Surface {
        Surface::RevolutedCurve(Processor::new(RevolutedCurve::by_revolution(curve, origin.point(), axis.vector())))
    }

    fn arc(p0: Vec3, p1: Vec3, via: Vec3) -> Curve {
        builder::circle_arc(&builder::vertex(p0.point()), &builder::vertex(p1.point()), via.point()).curve()
    }

    /// The six faces of a box are truck planes, and each comes out with its
    /// own outward normal — the orientation of the face is what the
    /// tessellation hands in, and the class must respect it.
    #[test]
    fn a_boxs_faces_are_planes_with_outward_normals() {
        let b = kernel::extrude(&Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 10.0)), 6.0)
            .unwrap()
            .remove(0);
        let centre = Vec3::new(0.0, 0.0, 3.0);
        let mut seen = Vec::new();
        for (_, c) in classes(&b) {
            let SurfaceClass::Plane { origin, normal } = c else { panic!("a box face is a plane, got {c:?}") };
            assert!(near(normal.length(), 1.0));
            assert!((origin - centre).dot(normal) > 0.0, "the normal points out: origin {origin:?} normal {normal:?}");
            seen.push(normal);
        }
        for want in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
            assert!(seen.iter().any(|n| same_dir(*n, want)), "no face faces {want:?}");
        }
    }

    /// `extrude` of a circle: the wall is a rational patch from `tsweep`
    /// (`kind == "nurbs"`), recognised as a cylinder with the circle's axis
    /// and radius, and an axis point on the axis.
    #[test]
    fn an_extruded_circles_wall_is_a_cylinder_with_the_right_axis_and_radius() {
        let rod = kernel::extrude(&Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(3.0, -2.0), 5.0)), 12.0)
            .unwrap()
            .remove(0);
        let mut walls = 0;
        for (kind, c) in classes(&rod) {
            match c {
                SurfaceClass::Plane { .. } => assert_eq!(kind, "plane"),
                SurfaceClass::Cylinder { axis_point, axis, radius } => {
                    assert_eq!(kind, "nurbs", "the wall is a NURBS patch to truck");
                    assert!(near(radius, 5.0), "radius {radius}");
                    assert!(same_dir(axis, Vec3::Z), "axis {axis:?} (canonical sign is +Z)");
                    assert!(near(axis_point.x, 3.0) && near(axis_point.y, -2.0), "axis point {axis_point:?}");
                    walls += 1;
                }
                other => panic!("an extruded circle has planes and cylinders only, got {other:?}"),
            }
        }
        assert!(walls >= 1);
    }

    /// A loft between two circles of different radius is a cone: its
    /// apex is where the radius reaches zero, its half-angle is the taper,
    /// and its axis points from the apex into the band. The same loft with
    /// equal radii is a cylinder.
    #[test]
    fn a_two_circle_loft_is_a_cone_and_a_same_radius_loft_is_a_cylinder() {
        // r 10 at z 0 to r 4 at z 6: the apex is at z 10, the half-angle 45°.
        let cone = circle_loft(&[(0.0, 10.0), (6.0, 4.0)]);
        let mut bands = 0;
        for (kind, c) in classes(&cone) {
            match c {
                SurfaceClass::Plane { .. } => {}
                SurfaceClass::Cone { apex, axis, half_angle } => {
                    assert_eq!(kind, "nurbs");
                    assert!(near(apex.x, 0.0) && near(apex.y, 0.0) && near(apex.z, 10.0), "apex {apex:?}");
                    assert!(same_dir(axis, -Vec3::Z), "the band lies below the apex: axis {axis:?}");
                    assert!(near(half_angle, FRAC_PI_4), "half angle {half_angle}");
                    bands += 1;
                }
                other => panic!("a cone has caps and cone bands only, got {other:?}"),
            }
        }
        assert!(bands >= 1);

        let cyl = circle_loft(&[(0.0, 10.0), (6.0, 10.0)]);
        assert!(
            classes(&cyl).iter().any(|(_, c)| matches!(c, SurfaceClass::Cylinder { radius, .. } if near(*radius, 10.0))),
            "{:?}",
            classes(&cyl)
        );
        assert!(!classes(&cyl).iter().any(|(_, c)| matches!(c, SurfaceClass::Cone { .. })));
    }

    /// A line revolved about an axis: parallel to it a cylinder, slanted a
    /// cone, perpendicular a plane.
    #[test]
    fn a_revolved_line_is_a_cylinder_a_cone_or_a_plane_by_its_angle_to_the_axis() {
        let line = |a: Vec3, b: Vec3| Curve::Line(Line(a.point(), b.point()));
        let s = revolved(line(Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 9.0)), Vec3::ZERO, Vec3::Z);
        match classify(&s, EXACT_TOL) {
            SurfaceClass::Cylinder { radius, axis, .. } => {
                assert!(near(radius, 4.0) && same_dir(axis, Vec3::Z));
            }
            other => panic!("{other:?}"),
        }
        // r 2 at z 0 to r 5 at z 3: 45°, apex at z = -2.
        let s = revolved(line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 3.0)), Vec3::ZERO, Vec3::Z);
        match classify(&s, EXACT_TOL) {
            SurfaceClass::Cone { apex, axis, half_angle } => {
                assert!(near(apex.z, -2.0) && near(apex.x, 0.0), "apex {apex:?}");
                assert!(same_dir(axis, Vec3::Z));
                assert!(near(half_angle, FRAC_PI_4));
            }
            other => panic!("{other:?}"),
        }
        let s = revolved(line(Vec3::new(2.0, 0.0, 7.0), Vec3::new(6.0, 0.0, 7.0)), Vec3::ZERO, Vec3::Z);
        match classify(&s, EXACT_TOL) {
            SurfaceClass::Plane { origin, normal } => {
                assert!(near(origin.z, 7.0) && parallel(normal, Vec3::Z));
            }
            other => panic!("{other:?}"),
        }
    }

    /// A semicircle whose centre is on the axis revolves into a sphere.
    #[test]
    fn a_revolved_semicircle_is_a_sphere() {
        let s = revolved(
            arc(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -5.0), Vec3::new(5.0, 0.0, 0.0)),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::Z,
        );
        match classify(&s, EXACT_TOL) {
            SurfaceClass::Sphere { center, radius } => {
                assert!(center.length() < 1e-9, "centre {center:?}");
                assert!(near(radius, 5.0));
            }
            other => panic!("{other:?}"),
        }
        // Off the origin, about a tilted axis through the sphere's centre.
        let c = Vec3::new(3.0, -1.0, 2.0);
        let axis = Vec3::new(1.0, 1.0, 1.0).normalized();
        let (e1, _) = frame(axis);
        let s = revolved(arc(c + axis * 4.0, c - axis * 4.0, c + e1 * 4.0), c, axis);
        match classify(&s, EXACT_TOL) {
            SurfaceClass::Sphere { center, radius } => {
                assert!((center - c).length() < 1e-9 && near(radius, 4.0), "{center:?} {radius}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// A circle whose centre is off the axis revolves into a torus with the
    /// centre's distance as the major radius and the circle's as the minor,
    /// both from a bare `RevolutedCurve` and through `kernel::revolve`.
    #[test]
    fn a_revolved_off_axis_circle_is_a_torus_with_the_right_radii() {
        let s = revolved(
            arc(Vec3::new(12.0, 0.0, 0.0), Vec3::new(8.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 2.0)),
            Vec3::ZERO,
            Vec3::Z,
        );
        match classify(&s, EXACT_TOL) {
            SurfaceClass::Torus { center, axis, major, minor } => {
                assert!(center.length() < 1e-9, "centre {center:?}");
                assert!(same_dir(axis, Vec3::Z));
                assert!(near(major, 10.0) && near(minor, 2.0), "{major} {minor}");
            }
            other => panic!("{other:?}"),
        }
        let ring = kernel::revolve(
            &Sketch::new(Plane::XZ).with_loop(Loop::circle(P2::new(10.0, 3.0), 2.0)),
            Vec3::ZERO,
            Vec3::Z,
            TAU,
        )
        .unwrap()
        .remove(0);
        let cs = classes(&ring);
        assert!(!cs.is_empty());
        for (kind, c) in &cs {
            assert_eq!(kind, "revolved");
            match c {
                SurfaceClass::Torus { center, axis, major, minor } => {
                    assert!(near(center.z, 3.0) && near(center.x, 0.0) && near(center.y, 0.0), "{center:?}");
                    assert!(same_dir(*axis, Vec3::Z));
                    assert!(near(*major, 10.0) && near(*minor, 2.0));
                }
                other => panic!("{other:?}"),
            }
        }
    }

    /// A bilinear patch — what `homotopy` between two lines is — is a plane,
    /// found by fitting, since truck calls it a `BSplineSurface`.
    #[test]
    fn a_bilinear_patch_is_a_plane() {
        let bs = BSplineSurface::new(
            (KnotVec::bezier_knot(1), KnotVec::bezier_knot(1)),
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 4.0)],
                vec![Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 0.0, 4.0)],
            ],
        );
        match classify(&Surface::BSplineSurface(bs), EXACT_TOL) {
            SurfaceClass::Plane { normal, origin } => {
                assert!(parallel(normal, Vec3::Y), "{normal:?}");
                assert!(near(origin.y, 0.0));
            }
            other => panic!("{other:?}"),
        }
    }

    /// A spline extrusion is free-form: its walls are `Other`, its caps
    /// planes, and nothing else is claimed.
    #[test]
    fn a_spline_extrusion_is_other() {
        let pts = [P2::new(10.0, 0.0), P2::new(0.0, 8.0), P2::new(-10.0, 0.0), P2::new(0.0, -8.0)];
        let s = kernel::extrude(&Sketch::new(Plane::XY).with_loop(Loop::spline(&pts, 0.5).unwrap()), 5.0)
            .unwrap()
            .remove(0);
        let cs = classes(&s);
        assert_eq!(cs.iter().filter(|(_, c)| matches!(c, SurfaceClass::Plane { .. })).count(), 2, "{cs:?}");
        assert_eq!(cs.iter().filter(|(_, c)| *c == SurfaceClass::Other).count(), cs.len() - 2, "{cs:?}");
    }

    /// A revolved cubic is not a quadric and must not be claimed as one.
    #[test]
    fn a_revolved_bezier_is_other() {
        let v0 = builder::vertex(Point3::new(3.0, 0.0, 0.0));
        let v1 = builder::vertex(Point3::new(3.0, 0.0, 10.0));
        let e = builder::bezier(&v0, &v1, vec![Point3::new(6.0, 0.0, 3.0), Point3::new(1.0, 0.0, 7.0)]);
        let s = builder::rsweep(&e, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Rad(1.0));
        let face = s.face_iter().next().unwrap();
        assert_eq!(classify(&face.surface(), EXACT_TOL), SurfaceClass::Other);
    }

    /// The tolerance contract, on a cone band with one control point nudged
    /// a tenth of a micron: at `EXACT_TOL` it is `Other` — no false positive
    /// — and at a hundredth of a millimetre it is the cone it nearly is.
    #[test]
    fn a_cone_band_perturbed_beyond_tol_is_other_and_within_tol_is_a_cone() {
        let cone = circle_loft(&[(0.0, 10.0), (6.0, 4.0)]);
        let band = cone
            .face_iter()
            .map(|f| f.surface())
            .find(|s| matches!(s, Surface::NurbsSurface(_)))
            .expect("a NURBS band");
        assert!(matches!(classify(&band, EXACT_TOL), SurfaceClass::Cone { .. }));
        let Surface::NurbsSurface(mut ns) = band else { unreachable!() };
        // Move an interior-ish control point radially by 1e-4 mm: the
        // surface near it moves by a fraction of that, well over a micron.
        let cp = ns.control_points()[1][0];
        let w = cp.w;
        let scaled = truck_modeling::Vector4::new(cp.x * (1.0 + 1e-5), cp.y * (1.0 + 1e-5), cp.z, w);
        *ns.control_point_mut(1, 0) = scaled;
        let bent = Surface::NurbsSurface(ns);
        assert_eq!(classify(&bent, EXACT_TOL), SurfaceClass::Other, "a bent band is not a cone to a micron");
        assert!(matches!(classify(&bent, 1e-2), SurfaceClass::Cone { .. }), "but it is one to a hundredth");
    }

    /// A face and its inverse classify the same, the plane normal's sign
    /// aside: a cylinder's and a torus's axis is canonical, a cone's is fixed
    /// by its apex.
    #[test]
    fn a_surface_and_its_inverse_classify_the_same_axis_sign_aside() {
        let cone = circle_loft(&[(0.0, 10.0), (6.0, 4.0)]);
        let rod = kernel::extrude(&Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 5.0)), 12.0)
            .unwrap()
            .remove(0);
        let ring = kernel::revolve(
            &Sketch::new(Plane::XZ).with_loop(Loop::circle(P2::new(10.0, 0.0), 2.0)),
            Vec3::ZERO,
            Vec3::Z,
            TAU,
        )
        .unwrap()
        .remove(0);
        let mut checked = 0;
        for solid in [&cone, &rod, &ring] {
            for f in solid.face_iter() {
                let s = f.surface();
                let a = classify(&s, EXACT_TOL);
                let b = classify(&s.inverse(), EXACT_TOL);
                match (&a, &b) {
                    (SurfaceClass::Plane { origin: oa, normal: na }, SurfaceClass::Plane { origin: ob, normal: nb }) => {
                        assert!(same_dir(*na, -*nb), "{a:?} vs {b:?}");
                        assert!(near((*oa - *ob).dot(*na), 0.0));
                    }
                    _ => assert!(same_class(&a, &b), "{a:?} vs {b:?}"),
                }
                checked += 1;
            }
        }
        assert!(checked >= 6);
    }

    /// A moved class is the class of the moved surface.
    #[test]
    fn a_transformed_class_follows_the_transform() {
        let c = SurfaceClass::Cylinder { axis_point: Vec3::new(1.0, 0.0, 0.0), axis: Vec3::Z, radius: 2.0 };
        let t = Transform::rotation(Vec3::ZERO, Vec3::X, PI / 2.0);
        match c.transformed(&t) {
            SurfaceClass::Cylinder { axis_point, axis, radius } => {
                assert!(near(radius, 2.0));
                // +Z turned about +X by 90° is -Y; canonical sign makes it +Y.
                assert!(same_dir(axis, Vec3::Y), "{axis:?}");
                assert!(near(axis_point.x, 1.0));
            }
            other => panic!("{other:?}"),
        }
        let s = SurfaceClass::Sphere { center: Vec3::ZERO, radius: 3.0 };
        assert_eq!(s.transformed(&Transform::translation(Vec3::new(0.0, 5.0, 0.0))), SurfaceClass::Sphere {
            center: Vec3::new(0.0, 5.0, 0.0),
            radius: 3.0
        });
    }

    /// The distance and normal of each class, which are what `verify` trusts.
    #[test]
    fn distances_and_normals_are_the_textbook_ones() {
        let cyl = SurfaceClass::Cylinder { axis_point: Vec3::ZERO, axis: Vec3::Z, radius: 2.0 };
        assert!(near(cyl.distance(Vec3::new(3.0, 0.0, 7.0)), 1.0));
        assert!(same_dir(cyl.normal_at(Vec3::new(0.0, 2.0, 1.0)).unwrap(), Vec3::Y));
        let cone = SurfaceClass::Cone { apex: Vec3::ZERO, axis: Vec3::Z, half_angle: FRAC_PI_4 };
        assert!(near(cone.distance(Vec3::new(3.0, 0.0, 3.0)), 0.0));
        assert!(near(cone.distance(Vec3::new(0.0, 0.0, -1.0)), 1.0), "past the apex the apex is nearest");
        let n = cone.normal_at(Vec3::new(3.0, 0.0, 3.0)).unwrap();
        assert!(same_dir(n, Vec3::new(1.0, 0.0, -1.0)), "{n:?}");
        let torus = SurfaceClass::Torus { center: Vec3::ZERO, axis: Vec3::Z, major: 10.0, minor: 2.0 };
        assert!(near(torus.distance(Vec3::new(12.0, 0.0, 0.0)), 0.0));
        assert!(near(torus.distance(Vec3::new(10.0, 0.0, 3.0)), 1.0));
        assert!(same_dir(torus.normal_at(Vec3::new(10.0, 0.0, 2.0)).unwrap(), Vec3::Z));
        assert_eq!(SurfaceClass::Other.distance(Vec3::ZERO), f64::INFINITY);
        assert!(SurfaceClass::Other.normal_at(Vec3::ZERO).is_none());
    }

    /// A nonsensical tolerance recognises nothing rather than everything.
    #[test]
    fn a_bad_tolerance_recognises_nothing() {
        let rod = kernel::extrude(&Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 5.0)), 12.0)
            .unwrap()
            .remove(0);
        let s = rod.face_iter().map(|f| f.surface()).find(|s| matches!(s, Surface::NurbsSurface(_))).unwrap();
        assert_eq!(classify(&s, 0.0), SurfaceClass::Other);
        assert_eq!(classify(&s, f64::NAN), SurfaceClass::Other);
        assert_eq!(classify(&s, -1.0), SurfaceClass::Other);
    }

    /// The exact primitives are recognised face by face: every
    /// patch of a `sphere` is the same sphere and every patch of a `torus`
    /// the same torus, with the parameters they were built from — which is
    /// what a `concentric` mate or a STEP `SPHERICAL_SURFACE` will read.
    #[test]
    fn the_sphere_and_torus_primitives_classify_as_such_on_every_face() {
        let c = Vec3::new(2.0, -1.0, 4.0);
        let cs = classes(&crate::primitives::sphere(c, 7.0).unwrap());
        assert_eq!(cs.len(), 2, "{cs:?}");
        for (kind, class) in &cs {
            assert_eq!(kind, "revolved");
            match class {
                SurfaceClass::Sphere { center, radius } => assert!((*center - c).length() < 1e-9 && near(*radius, 7.0)),
                other => panic!("{other:?}"),
            }
        }
        let axis = Vec3::new(0.0, 1.0, 1.0).normalized();
        let cs = classes(&crate::primitives::torus(c, axis, 9.0, 2.5).unwrap());
        assert_eq!(cs.len(), 6, "{cs:?}");
        for (kind, class) in &cs {
            assert_eq!(kind, "revolved");
            match class {
                SurfaceClass::Torus { center, axis: a, major, minor } => {
                    assert!((*center - c).length() < 1e-9 && parallel(*a, axis) && near(*major, 9.0) && near(*minor, 2.5));
                }
                other => panic!("{other:?}"),
            }
        }
        // A reflection keeps both classes; a non-uniform scale keeps neither
        // (recognised from the scaled surface, not assumed from the source).
        let m = crate::primitives::mirrored(&crate::primitives::sphere(c, 7.0).unwrap(), &Plane::XZ);
        for (_, class) in classes(&m) {
            match class {
                SurfaceClass::Sphere { center, radius } => {
                    assert!((center - Vec3::new(2.0, 1.0, 4.0)).length() < 1e-9 && near(radius, 7.0))
                }
                other => panic!("{other:?}"),
            }
        }
        let squashed = crate::primitives::transformed(
            &crate::primitives::sphere(c, 7.0).unwrap(),
            &crate::primitives::scaling(c, Vec3::new(1.0, 1.0, 0.5)),
        );
        assert!(classes(&squashed).iter().all(|(_, c)| *c == SurfaceClass::Other), "{:?}", classes(&squashed));
    }

    /// A revolved profile drawn as a path: the vase-style construction the
    /// examples use, checked here on a profile of one line and one arc so
    /// the two classes come out side by side from one `revolve`.
    #[test]
    fn a_revolved_path_of_a_line_and_an_arc_gives_a_cylinder_beside_a_torus() {
        // In XZ: a ring 2 mm from the axis, up the outside at r = 6 with a
        // half-round bulge out to r = 8 on the way.
        let profile = Path::new(P2::new(2.0, 0.0))
            .line_to(P2::new(6.0, 0.0))
            .line_to(P2::new(6.0, 4.0))
            .arc_to(P2::new(6.0, 8.0), P2::new(8.0, 6.0))
            .line_to(P2::new(6.0, 10.0))
            .line_to(P2::new(2.0, 10.0))
            .close()
            .unwrap();
        let s = kernel::revolve(&Sketch::new(Plane::XZ).with_loop(profile), Vec3::ZERO, Vec3::Z, TAU).unwrap().remove(0);
        let cs = classes(&s);
        assert!(cs.iter().any(|(_, c)| matches!(c, SurfaceClass::Cylinder { radius, .. } if near(*radius, 6.0))), "{cs:?}");
        assert!(
            cs.iter().any(|(_, c)| matches!(c, SurfaceClass::Torus { major, minor, .. } if near(*major, 6.0) && near(*minor, 2.0))),
            "{cs:?}"
        );
        assert!(cs.iter().filter(|(_, c)| matches!(c, SurfaceClass::Plane { .. })).count() >= 2, "{cs:?}");
    }
}
