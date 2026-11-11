//! `KSurface`: the surface truck's tessellator is handed, with Cadpole's own
//! parameter division for spline patches (docs/design.md).
//!
//! truck meshes a face by inverting its boundary polylines into `(u, v)`,
//! asking the surface for a grid of parameter lines
//! (`ParameterDivision2D::parameter_division`), and triangulating the two in
//! parameter space. Its division for a B-spline or NURBS patch samples one
//! point per cell and halves **both** directions whenever that sample misses
//! the bilinear interpolant, so a cell's level is set by its most curved
//! direction and the flat direction is halved along with it — the 64 × 60
//! grid on a 1 mm cone band that `kernel::with_cones_revolved` re-expresses
//! as a `RevolutedCurve`, and the other way round on the compressor wheel's
//! ruled blades: dense along the airfoil outline, **one** division along the
//! 40 mm ruling, so every triangle is 40 mm long and under 1 mm wide, and the
//! surface twists faster along the ruling than the interpolated vertex
//! normals of such a triangle can follow, which shows as a fanning reflection
//! on the blades.
//!
//! This newtype delegates every trait truck's `triangulation` needs to the
//! wrapped [`Surface`], except the division, which for a spline patch is
//! chosen per direction and per knot span from two bounds:
//!
//! 1. **Chordal error**, by the Filip–Magedson–Markot bound. For a C²
//!    surface with `‖S_uu‖ ≤ M_uu`, `‖S_uv‖ ≤ M_uv`, `‖S_vv‖ ≤ M_vv` over a
//!    cell of parametric size `h_u × h_v`, the piecewise-linear interpolant
//!    over the cell's two triangles is within
//!    `⅛·(M_uu·h_u² + 2·M_uv·h_u·h_v + M_vv·h_v²)` of the surface. The `M`s
//!    come from the control net: the second derivative of a B-spline is a
//!    B-spline whose control points are scaled second differences of the
//!    original's, and by the convex-hull property its norm on a knot span is
//!    at most the largest of the control points active there — per span, no
//!    evaluation. A ruled patch has `M_vv = 0` exactly, so the ruling gets one
//!    division unless the twist term `2·M_uv·h_u·h_v` alone exceeds the
//!    tolerance; a straight span of a spline gets one; a curved span gets its
//!    own count. A rational patch (Cadpole's arcs are its only non-unit weights)
//!    has no such control-net bound for the *projected* surface, so its `M`s
//!    are the largest second derivative sampled on a 5 × 5 grid per cell,
//!    with a margin — an estimate rather than a bound, but per direction and
//!    per span like the rest.
//! 2. **Normal deviation**: each direction
//!    is also divided until the surface normal turns by no more than
//!    [`DivisionBounds::max_normal_turn`] across a cell, measured at the cell
//!    corners. The chordal bound alone is blind to the blade's defect — a
//!    twisted ruled strip 1 mm wide sags well under 0.05 mm across its 40 mm
//!    ruling while its normal turns 20° — and this is what puts divisions
//!    along the ruling where it twists and none where it does not. The
//!    trade-off is triangle count against shading fidelity; see
//!    [`DEFAULT_MAX_NORMAL_DEGREES`] for the default and what it costs.
//!
//! Planes keep truck's division (boundary only), and so do the revolved
//! surfaces `with_cones_revolved` plants, whose `RevolutedCurve` already
//! divides per direction. The boundary polylines are `brepmesh`'s,
//! one polyline per edge shared by vertex id across faces.

use truck_modeling::{
    algo, BSplineSurface, BoundedCurve, Curve, Invertible, KnotVec, NurbsSurface,
    ParameterDivision1D, ParameterDivision2D, ParameterRange, ParametricCurve, ParametricSurface,
    ParametricSurface3D, Point3, SPHint2D, SearchNearestParameter, SearchParameter, Surface,
    Vector3, Vector4, D2,
};

use crate::geom::Vec3;
use crate::kernel::{point_to_vec3, vector_to_vec3};
use crate::fmath;

/// The default of [`DivisionBounds::max_normal_turn`], in degrees.
///
/// Measured on `examples/compressor_wheel.star` at the display tolerance,
/// whole mesh: the chordal bound
/// alone gives 29,778 and the blades keep one division along the ruling, so
/// the reflection still fans; 15° gives 33,094 and most rulings are still
/// undivided; **8° gives 42,218**, the twisted part of every blade patch has
/// two to five divisions along its rulings and the broad streaks are gone,
/// leaving a faint ribbing from the mesh kernel's plane-derived normals; 4°
/// gives 66,052 for no visible gain under the studio environment; 2° gives
/// 194,236 and the wheel's mesh booleans start to fail. 8° is also within a
/// degree of the chord rule's own step on a 20 mm radius at the default
/// tolerance, so it seldom divides a direction the chordal bound would not.
pub const DEFAULT_MAX_NORMAL_DEGREES: f64 = 8.0;

/// The fewest pieces a whole turn is ever cut into, whatever the chordal
/// bound says: a circle, a revolved surface's turn and a closed profile are
/// never fewer than this many segments, and an arc or a curved edge gets its
/// share of it by the angle it turns through.
/// The chord rule alone lets a circle whose radius is under the
/// tolerance collapse — a semicircle of 0.03 mm at 0.05 is two pieces,
/// its middle sample a 90° corner that `brepmesh`'s backward-step test
/// drops half the time, and the cylinder a flat strip — and a turn in
/// fewer than three pieces cannot even be unrolled round its period. Eight
/// keeps every sample's turn at 45°, far from both.
pub const MIN_PIECES_PER_TURN: usize = 8;

/// How many pieces `turn` radians of turning needs at
/// [`MIN_PIECES_PER_TURN`] a whole turn: `ceil(8·turn/2π)`, at least one.
/// The slack keeps a quarter turn at exactly two, a half turn at four,
/// however the angle rounded — on either target, since the rounding of a
/// sine can differ in the last bit between native and wasm.
pub fn pieces_for_turning(turn: f64) -> usize {
    if !(turn.is_finite() && turn > 0.0) {
        return 1;
    }
    let k = (turn * MIN_PIECES_PER_TURN as f64 / std::f64::consts::TAU - 1e-6).ceil();
    if k >= 4096.0 {
        4096
    } else {
        (k as usize).max(1)
    }
}

/// The total turning of a field of tangents, in radians: the sum of the
/// angles between consecutive ones, skipping any shorter than a millionth
/// of the longest (a pole, a cusp) — a lower bound on the turning of the
/// curve they were sampled from, and exact for an arc sampled finer than a
/// half turn.
pub fn turning_of(tangents: &[Vec3]) -> f64 {
    cumulative_turning(tangents).last().copied().unwrap_or(0.0)
}

/// [`turning_of`] up to each tangent: non-decreasing, `0` at the first.
pub fn cumulative_turning(tangents: &[Vec3]) -> Vec<f64> {
    let longest = tangents.iter().map(|d| d.length()).filter(|l| l.is_finite()).fold(0.0, f64::max);
    let mut out = Vec::with_capacity(tangents.len());
    let mut turn = 0.0;
    let mut prev: Option<Vec3> = None;
    for d in tangents {
        let len = d.length();
        if longest > 0.0 && len.is_finite() && len > 1e-6 * longest {
            let dir = *d * (1.0 / len);
            if let Some(p) = prev {
                turn += fmath::acos(p.dot(dir).clamp(-1.0, 1.0));
            }
            prev = Some(dir);
        }
        out.push(turn);
    }
    out
}

/// How far the surface turns along one parameter direction over `range`.
/// A periodic direction — the turn of a revolved surface, whichever slot an
/// inverted `Processor` put it in — turns by its extent, exactly, off the
/// axis: that is the closed period the minimum is counted in, and it costs
/// no evaluation. Any other direction turns as its derivative does along
/// the middle parameter line of the other (17 tangents): for a revolved
/// surface the profile's turning, the same on every meridian; a line
/// profile's is nothing.
fn surface_turning(surface: &Surface, ((u0, u1), (v0, v1)): ((f64, f64), (f64, f64)), along_u: bool) -> f64 {
    const N: usize = 16;
    let period = if along_u { surface.u_period() } else { surface.v_period() };
    if period.is_some() {
        return if along_u { (u1 - u0).abs() } else { (v1 - v0).abs() };
    }
    let tangents: Vec<Vec3> = (0..=N)
        .map(|i| {
            let s = i as f64 / N as f64;
            if along_u {
                vector_to_vec3(surface.uder(u0 + (u1 - u0) * s, 0.5 * (v0 + v1)))
            } else {
                vector_to_vec3(surface.vder(0.5 * (u0 + u1), v0 + (v1 - v0) * s))
            }
        })
        .collect();
    turning_of(&tangents)
}

/// The bounds Cadpole's division holds a spline patch to, besides the chordal
/// tolerance `parameter_division` is called with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DivisionBounds {
    /// The most the surface normal may turn across one grid cell, in
    /// radians. Non-positive or non-finite disables the normal bound.
    pub max_normal_turn: f64,
}

impl DivisionBounds {
    /// Bounds from a maximum normal turn in degrees.
    pub fn from_degrees(max_normal_degrees: f64) -> Self {
        DivisionBounds { max_normal_turn: max_normal_degrees.to_radians() }
    }
}

impl Default for DivisionBounds {
    fn default() -> Self {
        Self::from_degrees(DEFAULT_MAX_NORMAL_DEGREES)
    }
}

/// A surface for truck's tessellator: a [`Surface`] whose parameter division
/// is Cadpole's when it is a spline patch. See the module documentation.
#[derive(Clone, Debug)]
pub struct KSurface {
    surface: Surface,
    /// `Some` when the division is Cadpole's; `None` delegates it to truck.
    bounds: Option<DivisionBounds>,
}

impl KSurface {
    /// A surface tessellated with truck's own division: planes, and the
    /// revolved surfaces `with_cones_revolved` plants.
    pub fn trucks(surface: Surface) -> Self {
        KSurface { surface, bounds: None }
    }

    /// A surface tessellated with Cadpole's bound-driven division when it is a
    /// B-spline or NURBS patch, and truck's otherwise.
    pub fn bounded(surface: Surface, bounds: DivisionBounds) -> Self {
        let spline = matches!(surface, Surface::BSplineSurface(_) | Surface::NurbsSurface(_));
        KSurface { surface, bounds: spline.then_some(bounds) }
    }

    /// The wrapped surface.
    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    /// Whether the division is Cadpole's rather than truck's.
    pub fn is_bounded(&self) -> bool {
        self.bounds.is_some()
    }

    /// How many pieces an edge from `p` to `q` along this patch's boundary
    /// needs for the surface normal to turn by at most the normal bound
    /// across each: `1` when this surface divides by truck's rules, when the
    /// edge is not a whole side of the patch's parameter rectangle (a trimmed
    /// face's cut, a side split into several edges), or when the normal
    /// turns less than the bound along it.
    ///
    /// This is the edge half of the normal bound. A ruled blade patch 1 mm
    /// wide and 40 mm along its ruling has *straight* rulings for edges, and
    /// truck polylines a line as its two ends whatever the tolerance, so no
    /// interior grid can shorten the triangles along those two edges: every
    /// triangle touching a ruling is 40 mm long, and after a mesh boolean —
    /// where the mesh kernel derives vertex normals from the triangles'
    /// planes — those long twisted triangles are the fanning reflection on
    /// the compressor wheel's blades. The edge polyline is shared by edge id
    /// across the faces that bound it, so dividing it here keeps the mesh
    /// closed by construction; [`KCurve`] carries the count.
    pub fn edge_pieces(&self, p: Point3, q: Point3) -> usize {
        let Some(bounds) = self.bounds else { return 1 };
        if !(bounds.max_normal_turn > 0.0) || !bounds.max_normal_turn.is_finite() {
            return 1;
        }
        let (Some((u0, u1)), Some((v0, v1))) = self.surface.try_range_tuple() else { return 1 };
        let corner = |u: f64, v: f64| point_to_vec3(self.surface.subs(u, v));
        let (c00, c10, c01, c11) = (corner(u0, v0), corner(u1, v0), corner(u0, v1), corner(u1, v1));
        let scale = [c00, c10, c01, c11].iter().map(|c| c.length()).fold(1.0, f64::max);
        let (p, q) = (point_to_vec3(p), point_to_vec3(q));
        let same = |a: Vec3, b: Vec3| (a - b).length() <= 1e-6 * scale;
        let is_side = |a: Vec3, b: Vec3| (same(p, a) && same(q, b)) || (same(p, b) && same(q, a));
        // The side as a parameter line: `(u, v)` at `t` in [0, 1] along it.
        let side: Box<dyn Fn(f64) -> (f64, f64)> = if is_side(c00, c01) {
            Box::new(move |t| (u0, v0 + (v1 - v0) * t))
        } else if is_side(c10, c11) {
            Box::new(move |t| (u1, v0 + (v1 - v0) * t))
        } else if is_side(c00, c10) {
            Box::new(move |t| (u0 + (u1 - u0) * t, v0))
        } else if is_side(c01, c11) {
            Box::new(move |t| (u0 + (u1 - u0) * t, v1))
        } else {
            return 1;
        };
        const SAMPLES: usize = 9;
        let normals: Vec<Option<Vec3>> = (0..SAMPLES)
            .map(|i| {
                let (u, v) = side(i as f64 / (SAMPLES - 1) as f64);
                unit_normal(&self.surface, u, v)
            })
            .collect();
        let turn: f64 = normals
            .windows(2)
            .map(|w| match (w[0], w[1]) {
                (Some(a), Some(b)) => fmath::acos(a.dot(b).clamp(-1.0, 1.0)),
                _ => 0.0,
            })
            .sum();
        pieces_for_turn(turn, bounds.max_normal_turn)
    }
}

/// A curve for truck's tessellator: a [`Curve`] whose polyline has at least
/// `min_pieces` segments, the most any face it bounds asked for through
/// [`KSurface::edge_pieces`]. Everything else delegates to the curve.
#[derive(Clone, Debug)]
pub struct KCurve {
    curve: Curve,
    min_pieces: usize,
}

impl KCurve {
    pub fn new(curve: Curve, min_pieces: usize) -> Self {
        KCurve { curve, min_pieces: min_pieces.max(1) }
    }

    /// The wrapped curve.
    pub fn curve(&self) -> &Curve {
        &self.curve
    }

    /// The least segments the polyline will have.
    pub fn min_pieces(&self) -> usize {
        self.min_pieces
    }
}

impl ParametricCurve for KCurve {
    type Point = Point3;
    type Vector = Vector3;
    #[inline(always)]
    fn subs(&self, t: f64) -> Point3 {
        self.curve.subs(t)
    }
    #[inline(always)]
    fn der(&self, t: f64) -> Vector3 {
        self.curve.der(t)
    }
    #[inline(always)]
    fn der2(&self, t: f64) -> Vector3 {
        self.curve.der2(t)
    }
    #[inline(always)]
    fn parameter_range(&self) -> ParameterRange {
        self.curve.parameter_range()
    }
    #[inline(always)]
    fn period(&self) -> Option<f64> {
        self.curve.period()
    }
}

impl BoundedCurve for KCurve {}

impl Invertible for KCurve {
    #[inline(always)]
    fn invert(&mut self) {
        self.curve.invert();
    }
}

impl ParameterDivision1D for KCurve {
    type Point = Point3;
    /// The curve's own division at `tol`, merged with `min_pieces` equal
    /// pieces of `range` when it asks for more than one: the chordal samples
    /// stay where the curve put them, and the normal bound's are added
    /// between.
    fn parameter_division(&self, range: (f64, f64), tol: f64) -> (Vec<f64>, Vec<Point3>) {
        let (mut params, points) = self.curve.parameter_division(range, tol);
        if self.min_pieces <= 1 || range.1 <= range.0 {
            return (params, points);
        }
        let (t0, t1) = range;
        params.extend((1..self.min_pieces).map(|k| t0 + (t1 - t0) * k as f64 / self.min_pieces as f64));
        params.sort_by(f64::total_cmp);
        params.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * (t1 - t0));
        let points = params.iter().map(|&t| self.curve.subs(t)).collect();
        (params, points)
    }
}

impl ParametricSurface for KSurface {
    type Point = Point3;
    type Vector = Vector3;
    #[inline(always)]
    fn subs(&self, u: f64, v: f64) -> Point3 {
        self.surface.subs(u, v)
    }
    #[inline(always)]
    fn uder(&self, u: f64, v: f64) -> Vector3 {
        self.surface.uder(u, v)
    }
    #[inline(always)]
    fn vder(&self, u: f64, v: f64) -> Vector3 {
        self.surface.vder(u, v)
    }
    #[inline(always)]
    fn uuder(&self, u: f64, v: f64) -> Vector3 {
        self.surface.uuder(u, v)
    }
    #[inline(always)]
    fn uvder(&self, u: f64, v: f64) -> Vector3 {
        self.surface.uvder(u, v)
    }
    #[inline(always)]
    fn vvder(&self, u: f64, v: f64) -> Vector3 {
        self.surface.vvder(u, v)
    }
    #[inline(always)]
    fn parameter_range(&self) -> (ParameterRange, ParameterRange) {
        self.surface.parameter_range()
    }
    #[inline(always)]
    fn u_period(&self) -> Option<f64> {
        self.surface.u_period()
    }
    #[inline(always)]
    fn v_period(&self) -> Option<f64> {
        self.surface.v_period()
    }
}

impl ParametricSurface3D for KSurface {
    #[inline(always)]
    fn normal(&self, u: f64, v: f64) -> Vector3 {
        self.surface.normal(u, v)
    }
}

/// How far a boundary point may be from the surface for its nearest point
/// to stand in for it: the weld epsilon, because the vertex the face emits
/// is re-evaluated at the parameter found, and the weld is what joins it to
/// the neighbour's copy of the same polyline point.
const NEAREST_ACCEPTED: f64 = crate::mesh_kernel::WELD_EPS as f64;

impl SearchParameter<D2> for KSurface {
    type Point = Point3;
    /// truck's exact inversion, and when it refuses — it accepts a solution
    /// only within its `1e-6` of the point — the nearest point on the
    /// surface, if that is within [`NEAREST_ACCEPTED`].
    ///
    /// A boundary polyline point can be a micron off a face's surface
    /// without anything being wrong: the polyline is the *edge's*, sampled
    /// once and shared, and an edge a boolean made is an intersection curve
    /// evaluated to truck's tolerance against surfaces that were exact. The
    /// compressor wheel's bore meets its counterbore on such a circle, two of
    /// whose samples at the display tolerance sit a hair off the bore's
    /// recognised cylinder, and truck's exact inversion refuses them. The
    /// nearest point is the right answer for a point that close, and the
    /// mesh closes at the tolerance asked for.
    fn search_parameter<H: Into<SPHint2D>>(&self, point: Point3, hint: H, trials: usize) -> Option<(f64, f64)> {
        let hint = hint.into();
        // An inversion on a surface of revolution is held to the
        // generator's range: the generator
        // continues past its ends as the same conic, so an out-of-range
        // answer is on the surface and not off the point, and only the
        // range tells it from the right one. truck's answer is taken when
        // it is in range; otherwise the bounded search in the (radius,
        // height) half-plane, then the nearest point, in range.
        let revolved = matches!(self.surface, Surface::RevolutedCurve(_));
        if let Some(found) = self.surface.search_parameter(point, hint, trials) {
            if !revolved || generator_in_range(&self.surface, found) {
                return Some(found);
            }
        }
        if revolved {
            if let Some(found) = revolved_inversion(&self.surface, point) {
                return Some(found);
            }
        }
        let hints: &[SPHint2D] = match hint {
            SPHint2D::None => &[SPHint2D::None],
            _ => &[hint, SPHint2D::None],
        };
        for &h in hints {
            let Some(nearest) = nearest_parameter(&self.surface, point, h, trials) else { continue };
            if revolved && !generator_in_range(&self.surface, nearest) {
                continue;
            }
            let (u, v) = nearest;
            let off = point_to_vec3(self.surface.subs(u, v)) - point_to_vec3(point);
            if off.length() <= NEAREST_ACCEPTED {
                return Some(nearest);
            }
        }
        None
    }
}

/// Which of `(u, v)` is the generator's parameter on a revolved surface:
/// `0` as truck lays a `RevolutedCurve` out, `1` when the face's
/// `Processor` is inverted, which truck realises by *swapping* the two
/// parameters (`Processor::subs(u, v)` is `entity.subs(v, u)` then) — the
/// upper hemisphere of a `sphere()`, every inverted revolved face. The
/// range check and the bounded inversion below read the generator's slot,
/// not slot 0: read at slot 0 on an inverted face they held the *angle* to
/// the generator's range and let a generator parameter of −1.5 through,
/// and an imprint on a sphere band walked its boundary round the axis
/// twice in the wrong direction.
fn generator_slot(surface: &Surface) -> usize {
    match surface {
        Surface::RevolutedCurve(p) if !p.orientation() => 1,
        _ => 0,
    }
}

/// Whether the generator parameter of `uv` lies on the generator of a
/// revolved surface, within a hair of its range. truck's inversion of a
/// `RevolutedCurve` accepts any `(t, angle)` that evaluates to the point,
/// and a generator that reaches the axis — a sphere octant's quarter arc
/// from its pole, a cone's ruling to its apex — continues past the
/// pole as the same conic, so the point a hair off the pole along one
/// meridian is also `(−t, angle + π)` on the extrapolated generator. Newton
/// from the pole lands on either. The wrong one puts the boundary polygon
/// on the far side of the pole in parameter space, and the face's
/// triangles on the wrong part of the sphere, with every edge round the
/// octant left open.
fn generator_in_range(surface: &Surface, uv: (f64, f64)) -> bool {
    let slot = generator_slot(surface);
    let t = if slot == 0 { uv.0 } else { uv.1 };
    let ranges = surface.try_range_tuple();
    let range = if slot == 0 { ranges.0 } else { ranges.1 };
    match range {
        Some((t0, t1)) => {
            let slack = 1e-9 * (t1 - t0).abs().max(1e-9);
            t >= t0.min(t1) - slack && t <= t0.max(t1) + slack
        }
        None => true,
    }
}

/// The parameters of `point` on a revolved surface by a bounded search: the
/// generator's parameter minimises the distance in the (radius, height)
/// half-plane over the generator's own range, then the angle is read off
/// about the axis. Everything is evaluated through the surface itself, so a
/// `Processor`'s transform is honoured without being unpacked. `None` when
/// the point is not on the surface within [`NEAREST_ACCEPTED`].
fn revolved_inversion(surface: &Surface, point: Point3) -> Option<(f64, f64)> {
    let slot = generator_slot(surface);
    let ranges = surface.try_range_tuple();
    let (t0, t1) = if slot == 0 { ranges.0? } else { ranges.1? };
    if !(t0.is_finite() && t1.is_finite()) || t1 <= t0 {
        return None;
    }
    // Everything below is written as (t, angle); the surface is asked in
    // its own order.
    let at = |t: f64, ang: f64| point_to_vec3(if slot == 0 { surface.subs(t, ang) } else { surface.subs(ang, t) });
    // The axis, from the widest generator circle among a few.
    let mut best: Option<(Vec3, f64, Vec3, f64)> = None;
    for k in 0..=8 {
        let t = t0 + (t1 - t0) * k as f64 / 8.0;
        let (a, b, c) = (at(t, 0.0), at(t, 2.0 * std::f64::consts::FRAC_PI_3), at(t, 4.0 * std::f64::consts::FRAC_PI_3));
        if let Some((centre, radius, normal)) = crate::kernel::circle_through(a, b, c) {
            if best.map_or(true, |(_, r, _, _)| radius > r) {
                // truck turns counter-clockwise about its axis: make `normal` that axis.
                let sense = (a - centre).cross(at(t, 0.1) - centre).dot(normal);
                best = Some((centre, radius, if sense >= 0.0 { normal } else { -normal }, t));
            }
        }
    }
    let (origin, _, axis, _) = best?;
    let proj = |q: Vec3| {
        let v = q - origin;
        let h = v.dot(axis);
        ((v - axis * h).length(), h)
    };
    let (pr, ph) = proj(point_to_vec3(point));
    let dist2 = |t: f64| {
        let (r, h) = proj(at(t, 0.0));
        (r - pr) * (r - pr) + (h - ph) * (h - ph)
    };
    // A coarse scan, then golden-section on the best cell.
    let n = 32;
    let mut k_best = 0;
    let mut d_best = f64::INFINITY;
    for k in 0..=n {
        let d = dist2(t0 + (t1 - t0) * k as f64 / n as f64);
        if d < d_best {
            d_best = d;
            k_best = k;
        }
    }
    let step = (t1 - t0) / n as f64;
    let (mut lo, mut hi) = ((t0 + step * (k_best as f64 - 1.0)).max(t0), (t0 + step * (k_best as f64 + 1.0)).min(t1));
    let phi = (5f64.sqrt() - 1.0) / 2.0;
    let (mut x1, mut x2) = (hi - phi * (hi - lo), lo + phi * (hi - lo));
    let (mut f1, mut f2) = (dist2(x1), dist2(x2));
    for _ in 0..100 {
        if f1 < f2 {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = hi - phi * (hi - lo);
            f1 = dist2(x1);
        } else {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = lo + phi * (hi - lo);
            f2 = dist2(x2);
        }
        if hi - lo <= 1e-15 * (t1 - t0) {
            break;
        }
    }
    let t = if f1 < f2 { x1 } else { x2 };
    // The angle from the generator's radial direction to the point's.
    let p = point_to_vec3(point);
    let radial = |q: Vec3| {
        let v = q - origin;
        v - axis * v.dot(axis)
    };
    let (qr, pr_v) = (radial(at(t, 0.0)), radial(p));
    let ang = if qr.length() <= 1e-12 || pr_v.length() <= 1e-12 {
        0.0
    } else {
        let a = fmath::atan2(qr.cross(pr_v).dot(axis), qr.dot(pr_v));
        if a < 0.0 { a + 2.0 * std::f64::consts::PI } else { a }
    };
    ((at(t, ang) - p).length() <= NEAREST_ACCEPTED).then_some(if slot == 0 { (t, ang) } else { (ang, t) })
}

/// The parameter of the point on `surface` nearest `point`, by truck's
/// Newton search from `hint` — a parameter to start from, a range, or
/// nothing, in which case a B-spline patch is first sampled over its whole
/// range (truck's own types do that for themselves). `None` when the search
/// does not converge; how far the point found is from `point` is the
/// caller's question. Shared by the boundary inversion above and by the mesh
/// kernel, which inverts a boolean's new vertices onto a free-form group's
/// surface for their normals.
pub(crate) fn nearest_parameter(surface: &Surface, point: Point3, hint: SPHint2D, trials: usize) -> Option<(f64, f64)> {
    match surface {
        Surface::Plane(p) => p.search_nearest_parameter(point, hint, trials),
        Surface::NurbsSurface(ns) => ns.search_nearest_parameter(point, hint, trials),
        Surface::RevolutedCurve(r) => r.search_nearest_parameter(point, hint, trials),
        Surface::BSplineSurface(bs) => {
            let start = match hint {
                SPHint2D::Parameter(u, v) => (u, v),
                SPHint2D::Range(ur, vr) => algo::surface::presearch(bs, point, (ur, vr), 16),
                SPHint2D::None => match bs.try_range_tuple() {
                    (Some(ur), Some(vr)) => algo::surface::presearch(bs, point, (ur, vr), 16),
                    _ => return None,
                },
            };
            algo::surface::search_nearest_parameter(bs, point, start, trials)
        }
    }
}

impl Invertible for KSurface {
    #[inline(always)]
    fn invert(&mut self) {
        self.surface.invert();
    }
}

impl ParameterDivision2D for KSurface {
    fn parameter_division(&self, range: ((f64, f64), (f64, f64)), tol: f64) -> (Vec<f64>, Vec<f64>) {
        match self.bounds {
            Some(bounds) => interior_only(self.full_division(range, tol, bounds)),
            None => self.surface.parameter_division(range, tol),
        }
    }
}

impl KSurface {
    /// The grid lines a face's interior points are taken from, outermost
    /// lines included, over `range` (the loops' box in parameter space): the
    /// full bound-driven division for a spline patch under Cadpole's rules,
    /// truck's own per-direction division otherwise (a plane has none, a
    /// revolved surface the chord rule per direction, never fewer pieces
    /// than its turning asks for at [`MIN_PIECES_PER_TURN`] a whole turn).
    /// Both vectors run
    /// from one end of `range` to the other. `crate::brepmesh` takes the
    /// points inside the loops from it; the outermost lines are kept because
    /// the loops' box can be wider than the face, and the boundary distance
    /// test there does what `interior_only` did for truck's inclusion test.
    pub fn grid(&self, range: ((f64, f64), (f64, f64)), tol: f64) -> (Vec<f64>, Vec<f64>) {
        match self.bounds {
            Some(bounds) => self.full_division(range, tol, bounds),
            None => {
                let (mut u, mut v) = self.surface.parameter_division(range, tol);
                let ((u0, u1), (v0, v1)) = range;
                let ends = |d: &mut Vec<f64>, a: f64, b: f64| {
                    if d.first().is_none_or(|&f| f > a + 1e-12 * (b - a).abs()) {
                        d.insert(0, a);
                    }
                    if d.last().is_none_or(|&l| l < b - 1e-12 * (b - a).abs()) {
                        d.push(b);
                    }
                };
                ends(&mut u, u0, u1);
                ends(&mut v, v0, v1);
                // Never fewer lines than the surface's turning asks for at
                // `MIN_PIECES_PER_TURN` a whole turn:
                // truck's chord rule for the turn of a revolved
                // surface is `2·acos(1 − tol/r)` a piece, one piece in all
                // once the radius is under half the tolerance, and its
                // division of a profile arc as small is the arc's two ends.
                // When the division has fewer pieces it is replaced by that
                // many equal ones, as the normal bound's pieces replace an
                // edge's (`brepmesh::merge_min_pieces`).
                if !matches!(self.surface, Surface::Plane(_)) {
                    let uniform = |a: f64, b: f64, k: usize| -> Vec<f64> { (0..=k).map(|i| a + (b - a) * i as f64 / k as f64).collect() };
                    let ku = pieces_for_turning(surface_turning(&self.surface, range, true));
                    if u.len() < ku + 1 {
                        u = uniform(u0, u1, ku);
                    }
                    let kv = pieces_for_turning(surface_turning(&self.surface, range, false));
                    if v.len() < kv + 1 {
                        v = uniform(v0, v1, kv);
                    }
                }
                (u, v)
            }
        }
    }

    /// Cadpole's division of a spline patch with its outermost lines still in
    /// place: the grid lines from one end of `range` to the other, so a
    /// direction cut into `k` pieces has `k + 1` values. What truck is handed
    /// is [`interior_only`] of this.
    pub(crate) fn full_division(&self, range: ((f64, f64), (f64, f64)), tol: f64, bounds: DivisionBounds) -> (Vec<f64>, Vec<f64>) {
        match &self.surface {
            Surface::BSplineSurface(bs) => {
                let net = PolynomialNet::new(bs);
                full_division(&self.surface, &net, bs.knot_vecs(), range, tol, bounds)
            }
            Surface::NurbsSurface(ns) => match dehomogenised(ns) {
                Some(bs) => {
                    let net = PolynomialNet::new(&bs);
                    full_division(&self.surface, &net, ns.knot_vecs(), range, tol, bounds)
                }
                None => {
                    let net = SampledNet { surface: ns };
                    full_division(&self.surface, &net, ns.knot_vecs(), range, tol, bounds)
                }
            },
            _ => self.surface.parameter_division(range, tol),
        }
    }
}

// ------------------------------------------------------------------ the bounds

/// Upper bounds on the surface's second derivatives over one cell of the
/// division: `‖S_uu‖`, `‖S_uv‖`, `‖S_vv‖` in millimetres per unit parameter
/// squared.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellBounds {
    pub m_uu: f64,
    pub m_uv: f64,
    pub m_vv: f64,
}

impl CellBounds {
    /// The Filip–Magedson–Markot bound on the distance between the surface
    /// and its piecewise-linear interpolant over a cell of parametric size
    /// `h_u × h_v` split into two triangles.
    pub fn chordal_error(&self, h_u: f64, h_v: f64) -> f64 {
        0.125 * (self.m_uu * h_u * h_u + 2.0 * self.m_uv * h_u * h_v + self.m_vv * h_v * h_v)
    }
}

/// The frame one cell's bounds are taken in: the unit normal at the cell's
/// centre, and how far the normal strays from it within the cell.
///
/// The bound wants the second derivatives' components **along the normal**,
/// not their whole lengths. `‖S − L‖` in the Filip–Magedson–Markot statement
/// is the distance between the surface and its interpolant *at the same
/// parameter*, and that counts in-plane acceleration of the parameterisation
/// as if it were sag: a planar trapezoid — the nightstand's chamfer band, a
/// ruled patch whose rails differ in length — has `S_uv = B' − A' ≠ 0` and
/// no sag at all, and every ruled blade's `B' − A'` is mostly the two rails'
/// difference in *speed*, not the twist. Applied to the height
/// `f = S · n_T` above a triangle's plane instead, the same derivation gives
/// `|f − L_f| ≤ ⅛·(|f_uu| h_u² + 2|f_uv| h_u h_v + |f_vv| h_v²)` with
/// `f_ab = S_ab · n_T`, and `L_f` is constant on the triangle, so that *is*
/// the sag. `|S_ab · n_T| ≤ |S_ab · n_c| + ‖S_ab‖·‖n_T − n_c‖`, and the
/// triangle's normal lies within the cell's normals, so the stray of the
/// corner normals from the centre's stands in for the last factor.
#[derive(Clone, Copy, Debug)]
pub struct CellFrame {
    /// The unit normal at the cell's centre; `None` where the
    /// parameterisation is degenerate there.
    pub normal: Option<Vec3>,
    /// The largest `‖n − n_c‖` over the cell's corners — `2·sin(θ/2)` for a
    /// normal turned by `θ` — and 2, the whole sphere, when a corner is
    /// degenerate.
    pub stray: f64,
}

impl CellFrame {
    /// The frame of the cell `u × v` of `surface`.
    pub fn of(surface: &Surface, (u0, u1): (f64, f64), (v0, v1): (f64, f64)) -> Self {
        let normal = unit_normal(surface, 0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let stray = match normal {
            None => 2.0,
            Some(c) => [(u0, v0), (u1, v0), (u0, v1), (u1, v1)]
                .into_iter()
                .map(|(u, v)| unit_normal(surface, u, v).map_or(2.0, |n| (n - c).length()))
                .fold(0.0, f64::max),
        };
        CellFrame { normal, stray }
    }

    /// The bound on `|d · n|` over the cell for a second derivative `d`
    /// known only as a vector: its component along the centre normal plus
    /// its whole length times the stray — never more than its length.
    pub fn project(&self, d: Vec3) -> f64 {
        let len = d.length();
        match self.normal {
            Some(n) => (d.dot(n).abs() + len * self.stray).min(len),
            None => len,
        }
    }
}

/// Where the second-derivative bounds of a patch come from.
pub trait SecondDerivativeBounds {
    /// Bounds over the cell `u × v`, which lies within one knot span in each
    /// direction, on the derivatives' components along the surface normal.
    fn cell(&self, u: (f64, f64), v: (f64, f64), frame: &CellFrame) -> CellBounds;
}

/// The control-net bound of a polynomial patch: the three second-derivative
/// surfaces, whose control points are the scaled second differences of the
/// patch's.
///
/// truck's `uderivation` keeps the knot vector and drops the degree by one
/// (one more control point per row, the extra ones zero for a clamped knot
/// vector), so a knot span of the patch is the same span of every derivative
/// surface, and the control points active on it are the `d + 1` ending at
/// the span's index, `d` the derivative surface's degree.
pub struct PolynomialNet {
    suu: BSplineSurface<Vector3>,
    suv: BSplineSurface<Vector3>,
    svv: BSplineSurface<Vector3>,
}

impl PolynomialNet {
    pub fn new(surface: &BSplineSurface<Point3>) -> Self {
        let su = surface.uderivation();
        let sv = surface.vderivation();
        PolynomialNet { suu: su.uderivation(), suv: su.vderivation(), svv: sv.vderivation() }
    }
}

impl SecondDerivativeBounds for PolynomialNet {
    fn cell(&self, (u0, u1): (f64, f64), (v0, v1): (f64, f64), frame: &CellFrame) -> CellBounds {
        let (u, v) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
        CellBounds {
            m_uu: active_max(&self.suu, u, v, frame),
            m_uv: active_max(&self.suv, u, v, frame),
            m_vv: active_max(&self.svv, u, v, frame),
        }
    }
}

/// The largest projection among the control points of `s` active at
/// `(u, v)`: by the convex-hull property `s` on the whole knot-grid cell
/// containing the point is a convex combination of them, so its component
/// along any direction is at most the largest of theirs.
fn active_max(s: &BSplineSurface<Vector3>, u: f64, v: f64, frame: &CellFrame) -> f64 {
    let (uk, vk) = s.knot_vecs();
    let (du, dv) = s.degrees();
    let pts = s.control_points();
    let (nu, nv) = (pts.len(), pts.first().map_or(0, Vec::len));
    if nu == 0 || nv == 0 {
        return 0.0;
    }
    let su = span_index(uk, u).min(nu - 1);
    let sv = span_index(vk, v).min(nv - 1);
    let mut m = 0.0f64;
    for row in &pts[su.saturating_sub(du)..=su] {
        for p in &row[sv.saturating_sub(dv)..=sv] {
            m = m.max(frame.project(vector_to_vec3(*p)));
        }
    }
    m
}

/// The index `s` of the non-empty knot span `[k_s, k_{s+1})` containing `t`,
/// clamped to the first or last non-empty span outside the knot vector.
fn span_index(knots: &KnotVec, t: f64) -> usize {
    let n = knots.len();
    let mut s = knots.floor(t).unwrap_or(0).min(n.saturating_sub(2));
    while s > 0 && (s + 1 >= n || knots[s] >= knots[s + 1]) {
        s -= 1;
    }
    while s + 2 < n && knots[s] >= knots[s + 1] {
        s += 1;
    }
    s
}

/// The sampled estimate for a rational patch: the largest second derivative
/// on a 5 × 5 grid over the cell, with [`RATIONAL_MARGIN`] on top.
pub struct SampledNet<'a> {
    pub surface: &'a NurbsSurface<Vector4>,
}

/// What a sampled maximum is scaled by to stand in for a bound.
const RATIONAL_MARGIN: f64 = 1.25;

impl SecondDerivativeBounds for SampledNet<'_> {
    fn cell(&self, (u0, u1): (f64, f64), (v0, v1): (f64, f64), frame: &CellFrame) -> CellBounds {
        const N: usize = 5;
        let mut b = CellBounds::default();
        for i in 0..N {
            let u = u0 + (u1 - u0) * i as f64 / (N - 1) as f64;
            for j in 0..N {
                let v = v0 + (v1 - v0) * j as f64 / (N - 1) as f64;
                // The normal here, not the centre's: the sample's own frame,
                // strayed by the cell's spread like the centre's would be.
                let local = CellFrame { normal: unit_normal_of(self.surface, u, v), stray: frame.stray };
                let project = |d: Vector3| finite(local.project(vector_to_vec3(d)));
                b.m_uu = b.m_uu.max(project(self.surface.uuder(u, v)));
                b.m_uv = b.m_uv.max(project(self.surface.uvder(u, v)));
                b.m_vv = b.m_vv.max(project(self.surface.vvder(u, v)));
            }
        }
        CellBounds { m_uu: b.m_uu * RATIONAL_MARGIN, m_uv: b.m_uv * RATIONAL_MARGIN, m_vv: b.m_vv * RATIONAL_MARGIN }
    }
}

fn finite(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

fn unit_normal_of(surface: &NurbsSurface<Vector4>, u: f64, v: f64) -> Option<Vec3> {
    let n = vector_to_vec3(surface.uder(u, v).cross(surface.vder(u, v)));
    let len = n.length();
    (len.is_finite() && len > 1e-12).then(|| n * (1.0 / len))
}

/// The polynomial patch a NURBS patch is when all its weights are equal —
/// every spline, line and Bézier Cadpole lifts to homogeneous form — or `None`
/// when it is genuinely rational (an arc).
fn dehomogenised(ns: &NurbsSurface<Vector4>) -> Option<BSplineSurface<Point3>> {
    let pts = ns.control_points();
    let w0 = pts.first()?.first()?.w;
    if !(w0.abs() > 1e-12) {
        return None;
    }
    let equal = pts.iter().flatten().all(|p| (p.w - w0).abs() <= 1e-12 * w0.abs());
    if !equal {
        return None;
    }
    let control_points = pts
        .iter()
        .map(|row| row.iter().map(|p| Point3::new(p.x / p.w, p.y / p.w, p.z / p.w)).collect())
        .collect();
    Some(BSplineSurface::new_unchecked(ns.knot_vecs().clone(), control_points))
}

// ---------------------------------------------------------------- the division

/// The most pieces one knot span is divided into in one direction. A guard
/// against a degenerate patch, not a tuning knob: at the display tolerance a
/// span needs a few dozen at most.
const MAX_PIECES_PER_SPAN: usize = 512;

/// The division itself: knot spans within `range`, a piece count per span and
/// direction from the chordal bound, then the normal bound on top. Both
/// vectors run from one end of `range` to the other.
fn full_division(
    surface: &Surface,
    net: &dyn SecondDerivativeBounds,
    (uknots, vknots): &(KnotVec, KnotVec),
    ((u0, u1), (v0, v1)): ((f64, f64), (f64, f64)),
    tol: f64,
    bounds: DivisionBounds,
) -> (Vec<f64>, Vec<f64>) {
    if !(u1 > u0) || !(v1 > v0) || !(tol > 0.0) {
        return (vec![u0, u1], vec![v0, v1]);
    }
    let bu = breakpoints(uknots, (u0, u1));
    let bv = breakpoints(vknots, (v0, v1));
    let cells: Vec<Vec<CellBounds>> = bu
        .windows(2)
        .map(|u| {
            bv.windows(2)
                .map(|v| {
                    let (u, v) = ((u[0], u[1]), (v[0], v[1]));
                    net.cell(u, v, &CellFrame::of(surface, u, v))
                })
                .collect()
        })
        .collect();
    let (m, n) = piece_counts(&bu, &bv, &cells, tol);
    let mut udiv = subdivide(&bu, &m);
    let mut vdiv = subdivide(&bv, &n);
    if bounds.max_normal_turn > 0.0 && bounds.max_normal_turn.is_finite() {
        refine_by_normal(surface, &mut udiv, &mut vdiv, bounds.max_normal_turn);
    }
    (udiv, vdiv)
}

/// The division without its outermost lines.
///
/// `range` is the bounding box of the face's loops in parameter space, so
/// the grid points on its four sides lie on the boundary or outside it and
/// truck's inclusion test drops them — nearly always. The test is a parity
/// count along a ray of hashed direction that treats a point within `1e-6`
/// of a boundary segment *along the ray* as on it, so once in a few thousand
/// points the ray runs almost parallel to the segment, the point a Newton
/// residual (`1e-10`) off the boundary counts as inside, spade inserts it a
/// hair from the constraint, and the face gains a boundary vertex its
/// neighbour never sees: an open edge.
/// Measured on the compressor wheel's blade at the default tolerance: six
/// open edges with the outermost lines, none without. The outermost lines
/// carry no triangle the mesh keeps, so they are not returned; a direction
/// with nothing inside them returns an empty division in both, which truck
/// reads as "no interior grid" — the strip a cone band or a flat ruled band
/// is — rather than a grid with one empty row, which it cannot index.
fn interior_only((udiv, vdiv): (Vec<f64>, Vec<f64>)) -> (Vec<f64>, Vec<f64>) {
    let inner = |d: Vec<f64>| -> Vec<f64> { d.get(1..d.len().saturating_sub(1)).map_or_else(Vec::new, <[f64]>::to_vec) };
    let (u, v) = (inner(udiv), inner(vdiv));
    if u.is_empty() || v.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (u, v)
    }
}

/// `range`'s ends and every distinct *interior* knot well inside it,
/// ascending.
///
/// The patch's first and last knots are never breakpoints, and no knot
/// within a ten-thousandth of the range of either end is: `range` is the
/// bounding box of the face's loops as Newton inverted them, so a loop that
/// runs along the patch's own boundary knot line — every untrimmed patch's
/// does — puts that line *inside* the range by the inversion residual
/// (`1e-8` to `1e-5` in parameter, larger on a small face). A grid line
/// there sits a residual from the boundary constraint, close enough that
/// truck's ray-parity inclusion test, which excludes only points within
/// `1e-6` *along a hashed ray*, admits it one time in fifty; spade then keeps
/// a sliver between the line and the boundary whose centroid the same test
/// rejects, the sliver is dropped, and the face's boundary runs through a
/// vertex its neighbour never had. Three open edges on the compressor
/// wheel's blade at the default tolerance, at grid points with `v = 0.0`
/// and `v = 1.0` exactly, were this. truck's own halving never lands on a
/// knot, which is why it never saw it.
fn breakpoints(knots: &KnotVec, (r0, r1): (f64, f64)) -> Vec<f64> {
    let eps = 1e-4 * (r1 - r0);
    let mut distinct: Vec<f64> = Vec::new();
    for &k in knots.iter() {
        if distinct.last().is_none_or(|&last| k > last + 1e-9 * (r1 - r0)) {
            distinct.push(k);
        }
    }
    let mut v = vec![r0];
    if distinct.len() > 2 {
        for &k in &distinct[1..distinct.len() - 1] {
            if k > r0 + eps && k < r1 - eps {
                v.push(k);
            }
        }
    }
    v.push(r1);
    v
}

/// Piece counts per u-span and per v-span so that every cell's chordal bound
/// is within `tol`, few pieces first.
///
/// Each span starts from the count its own pure term needs (the closed form
/// of `⅛·M·h² ≤ tol`), then the cross term is settled by adding one piece at
/// a time to a direction of the failing cell: the direction whose pure term
/// is the larger contributor, or — when the twist term dominates, where
/// dividing either direction cuts the error alike — the direction whose
/// pieces are longer in space, which keeps the cells near square rather than
/// making thin strips thinner.
fn piece_counts(bu: &[f64], bv: &[f64], cells: &[Vec<CellBounds>], tol: f64) -> (Vec<usize>, Vec<usize>) {
    let nu = bu.len() - 1;
    let nv = bv.len() - 1;
    let du = |i: usize| bu[i + 1] - bu[i];
    let dv = |j: usize| bv[j + 1] - bv[j];
    let pure = |m: f64, h: f64| ((m * h * h / (8.0 * tol)).sqrt().ceil() as usize).clamp(1, MAX_PIECES_PER_SPAN);
    let mut m: Vec<usize> = (0..nu).map(|i| (0..nv).map(|j| pure(cells[i][j].m_uu, du(i))).max().unwrap_or(1)).collect();
    let mut n: Vec<usize> = (0..nv).map(|j| (0..nu).map(|i| pure(cells[i][j].m_vv, dv(j))).max().unwrap_or(1)).collect();
    // Spatial piece lengths, for the tie-break: the parametric size of a
    // span times its share of the patch's extent in that direction, which
    // the second-difference nets do not carry; `1` per unit parameter is a
    // fair stand-in because both directions are measured the same way and
    // Cadpole's patches are parameterised on [0, 1] in both.
    for _ in 0..MAX_PIECES_PER_SPAN * 4 {
        let mut changed = false;
        for i in 0..nu {
            for j in 0..nv {
                let c = &cells[i][j];
                let h_u = du(i) / m[i] as f64;
                let h_v = dv(j) / n[j] as f64;
                if !(c.chordal_error(h_u, h_v) > tol) {
                    continue;
                }
                let (pu, pv, cross) = (c.m_uu * h_u * h_u, c.m_vv * h_v * h_v, c.m_uv * h_u * h_v);
                let split_u = if pu >= pv && pu >= cross {
                    true
                } else if pv >= cross {
                    false
                } else {
                    h_u >= h_v
                };
                match (split_u, m[i] < MAX_PIECES_PER_SPAN, n[j] < MAX_PIECES_PER_SPAN) {
                    (true, true, _) | (false, true, false) => m[i] += 1,
                    (false, _, true) | (true, false, true) => n[j] += 1,
                    _ => continue,
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    (m, n)
}

/// The breakpoints with each span cut into its count of equal pieces.
fn subdivide(b: &[f64], counts: &[usize]) -> Vec<f64> {
    let mut out = vec![b[0]];
    for (span, &k) in b.windows(2).zip(counts) {
        for p in 1..=k {
            out.push(span[0] + (span[1] - span[0]) * p as f64 / k as f64);
        }
    }
    out
}

/// Divide each interval of `udiv` (and of `vdiv`) further until the surface
/// normal turns by at most `max_turn` across it, at every line of the other
/// direction's division. Two passes: the count for an interval is set from
/// the turn measured across it, and the second pass catches an interval whose
/// normal turns faster in one part than the whole suggested.
fn refine_by_normal(surface: &Surface, udiv: &mut Vec<f64>, vdiv: &mut Vec<f64>, max_turn: f64) {
    for _ in 0..2 {
        let normals: Vec<Vec<Option<Vec3>>> = udiv
            .iter()
            .map(|&u| vdiv.iter().map(|&v| unit_normal(surface, u, v)).collect())
            .collect();
        let turn = |a: &Option<Vec3>, b: &Option<Vec3>| match (a, b) {
            (Some(a), Some(b)) => fmath::acos(a.dot(*b).clamp(-1.0, 1.0)),
            _ => 0.0,
        };
        let mut changed = false;
        let mut new_u = vec![udiv[0]];
        for i in 0..udiv.len() - 1 {
            let t = (0..vdiv.len()).map(|j| turn(&normals[i][j], &normals[i + 1][j])).fold(0.0, f64::max);
            let k = pieces_for_turn(t, max_turn);
            changed |= k > 1;
            for p in 1..=k {
                new_u.push(udiv[i] + (udiv[i + 1] - udiv[i]) * p as f64 / k as f64);
            }
        }
        let mut new_v = vec![vdiv[0]];
        for j in 0..vdiv.len() - 1 {
            let t = (0..udiv.len()).map(|i| turn(&normals[i][j], &normals[i][j + 1])).fold(0.0, f64::max);
            let k = pieces_for_turn(t, max_turn);
            changed |= k > 1;
            for p in 1..=k {
                new_v.push(vdiv[j] + (vdiv[j + 1] - vdiv[j]) * p as f64 / k as f64);
            }
        }
        *udiv = new_u;
        *vdiv = new_v;
        if !changed {
            break;
        }
    }
}

fn pieces_for_turn(turn: f64, max_turn: f64) -> usize {
    if turn.is_finite() && turn > max_turn {
        ((turn / max_turn).ceil() as usize).clamp(2, 64)
    } else {
        1
    }
}

/// The unit normal at `(u, v)`, or `None` where the parameterisation is
/// degenerate (a pole, a collapsed span) and the normal has no direction.
fn unit_normal(surface: &Surface, u: f64, v: f64) -> Option<Vec3> {
    let n = vector_to_vec3(surface.uder(u, v).cross(surface.vder(u, v)));
    let len = n.length();
    (len.is_finite() && len > 1e-12).then(|| n * (1.0 / len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Vec3;
    use crate::kernel::point_to_vec3;
    use truck_modeling::{BSplineCurve, NurbsCurve};

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn point_at(surface: &Surface, u: f64, v: f64) -> Vec3 {
        point_to_vec3(surface.subs(u, v))
    }

    /// The ruled patch between two polynomial B-spline rails, exactly as
    /// `BSplineSurface::homotopy` lays one out: the curve along `u`, the
    /// straight run along `v`.
    fn ruled(a: BSplineCurve<Point3>, b: BSplineCurve<Point3>) -> Surface {
        Surface::BSplineSurface(BSplineSurface::homotopy(a, b))
    }

    fn line(p: Vec3, q: Vec3) -> BSplineCurve<Point3> {
        BSplineCurve::new(KnotVec::bezier_knot(1), vec![p.point(), q.point()])
    }

    /// A circular arc of `radius` about the z axis from azimuth 0 to `angle`
    /// (< 180°) at height `z`, as the rational quadratic truck's
    /// `circle_arc` builds.
    fn arc(radius: f64, angle: f64, z: f64) -> NurbsCurve<Vector4> {
        // The middle control point sits at azimuth `angle / 2`, `r / cos(angle / 2)`
        // out, with weight `cos(angle / 2)`: in homogeneous form that is
        // `(r·cos, r·sin, z·w, w)` with `w` the cosine.
        let (s, c) = fmath::sin_cos(angle / 2.0);
        let mid = Vector4::new(radius * c, radius * s, z * c, c);
        let (s1, c1) = fmath::sin_cos(angle);
        NurbsCurve::new(BSplineCurve::new(
            KnotVec::bezier_knot(2),
            vec![Vector4::new(radius, 0.0, z, 1.0), mid, Vector4::new(radius * c1, radius * s1, z, 1.0)],
        ))
    }

    fn division_of(surface: Surface, tol: f64, bounds: DivisionBounds) -> (Vec<f64>, Vec<f64>) {
        let ks = KSurface::bounded(surface, bounds);
        assert!(ks.is_bounded());
        let (Some(ur), Some(vr)) = ks.try_range_tuple() else { panic!("a bounded patch") };
        ks.full_division((ur, vr), tol, bounds)
    }

    const CHORD_ONLY: DivisionBounds = DivisionBounds { max_normal_turn: 0.0 };

    /// A planar bilinear patch is flat: every second derivative is zero, the
    /// normal never turns, one division per direction under both bounds.
    #[test]
    fn a_planar_bilinear_patch_needs_one_division_per_direction() {
        let s = ruled(line(v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0)), line(v(0.0, 20.0, 0.0), v(30.0, 20.0, 0.0)));
        for bounds in [CHORD_ONLY, DivisionBounds::default()] {
            let (u, w) = division_of(s.clone(), 0.01, bounds);
            assert_eq!((u.len(), w.len()), (2, 2), "{bounds:?}: {u:?} {w:?}");
        }
        // And the bound itself is zero on the whole patch.
        let Surface::BSplineSurface(bs) = &s else { unreachable!() };
        let c = PolynomialNet::new(bs).cell((0.0, 1.0), (0.0, 1.0), &CellFrame::of(&s, (0.0, 1.0), (0.0, 1.0)));
        assert_eq!(c, CellBounds::default());
    }

    /// A planar trapezoid — the nightstand's chamfer band, ruled between two
    /// parallel rails of different length — has a twist term as a vector
    /// (`B' − A'` is the rails' difference in speed) and no sag at all: the
    /// bound is taken along the normal, so it is zero and the band stays a
    /// strip. With the whole vector it asked for 6 × 6 pieces at the display
    /// tolerance, 172 triangles a slab where 28 are right.
    #[test]
    fn a_planar_trapezoid_is_a_strip_although_its_rails_differ_in_speed() {
        let s = ruled(line(v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0)), line(v(2.0, 2.0, 0.0), v(28.0, 2.0, 0.0)));
        let Surface::BSplineSurface(bs) = &s else { unreachable!() };
        let frame = CellFrame::of(&s, (0.0, 1.0), (0.0, 1.0));
        assert!(frame.stray.abs() < 1e-12, "a plane's normal never strays: {frame:?}");
        let c = PolynomialNet::new(bs).cell((0.0, 1.0), (0.0, 1.0), &frame);
        assert!(c.m_uv < 1e-9 && c.m_uu < 1e-9 && c.m_vv < 1e-9, "{c:?}");
        let (u, w) = division_of(s.clone(), 0.05, DivisionBounds::default());
        assert_eq!((u.len(), w.len()), (2, 2), "{u:?} {w:?}");
        // Whereas the unprojected vector is the difference in speed, 4 mm/unit².
        let raw = CellFrame { normal: None, stray: 2.0 };
        let c = PolynomialNet::new(bs).cell((0.0, 1.0), (0.0, 1.0), &raw);
        assert!((c.m_uv - 4.0).abs() < 1e-9, "{c:?}");
    }

    /// The control-net bound on a known curve: `S(u, v) = (1 - v)·A + v·B`
    /// with `A(u) = (30u, 10u², 0)` (a parabola: `A'' = (0, 20, 0)`) and
    /// `B = A + (0, 0, 20)` gives `M_uu = 20`, `M_uv = M_vv = 0`, and the FMM
    /// bound then asks for `m = ⌈√(20 / (8·tol))⌉` pieces along `u`.
    #[test]
    fn the_control_net_bound_is_the_second_derivative_of_a_parabola() {
        let parabola = |z: f64| {
            // Quadratic Bézier: P0 = (0,0,z), P1 = (15, 0, z), P2 = (30, 10, z) has
            // second derivative 2·(P2 − 2P1 + P0) = (0, 20, 0).
            BSplineCurve::new(
                KnotVec::bezier_knot(2),
                vec![v(0.0, 0.0, z).point(), v(15.0, 0.0, z).point(), v(30.0, 10.0, z).point()],
            )
        };
        let s = ruled(parabola(0.0), parabola(20.0));
        let Surface::BSplineSurface(bs) = &s else { unreachable!() };
        let raw = CellFrame { normal: None, stray: 2.0 };
        let c = PolynomialNet::new(bs).cell((0.0, 1.0), (0.0, 1.0), &raw);
        assert!((c.m_uu - 20.0).abs() < 1e-9 && c.m_uv.abs() < 1e-9 && c.m_vv.abs() < 1e-9, "{c:?}");
        // Along the normal the parabola's `(0, 20, 0)` is at most 20 and at
        // least its component along the centre normal, `20·30/√(30² + 10²)`.
        let c = PolynomialNet::new(bs).cell((0.0, 1.0), (0.0, 1.0), &CellFrame::of(&s, (0.0, 1.0), (0.0, 1.0)));
        assert!(c.m_uu <= 20.0 + 1e-9 && c.m_uu >= 18.97 && c.m_uv.abs() < 1e-9 && c.m_vv.abs() < 1e-9, "{c:?}");
        for tol in [0.1, 0.05, 0.01] {
            let (u, w) = division_of(s.clone(), tol, CHORD_ONLY);
            let want = (c.m_uu / (8.0 * tol)).sqrt().ceil() as usize;
            assert_eq!(u.len() - 1, want, "tol {tol}: {u:?}");
            assert_eq!(w.len(), 2, "the ruling is straight: {w:?}");
            // The sag of every piece really is within the tolerance.
            for p in u.windows(2) {
                let (a, b) = (point_at(&s, p[0], 0.0), point_at(&s, p[1], 0.0));
                let mid = point_at(&s, 0.5 * (p[0] + p[1]), 0.0);
                let sag = (mid - (a + b) * 0.5).length();
                assert!(sag <= tol + 1e-9, "sag {sag} over {tol}");
            }
        }
    }

    /// A cylinder band — a rational arc lofted straight up — divides its arc
    /// direction by the chord rule's count within the margin the sampled
    /// rational estimate carries, never below it, and its straight direction
    /// not at all.
    #[test]
    fn a_cylinder_band_divides_u_by_the_chord_rule_and_v_not_at_all() {
        let (r, angle) = (20.0, std::f64::consts::FRAC_PI_2);
        let a = arc(r, angle, 0.0);
        let b = arc(r, angle, 5.0);
        let s = Surface::NurbsSurface(NurbsSurface::new(BSplineSurface::homotopy(
            a.into_non_rationalized(),
            b.into_non_rationalized(),
        )));
        for tol in [0.1, 0.05, 0.02] {
            let (u, w) = division_of(s.clone(), tol, CHORD_ONLY);
            let chord_rule = (angle / (2.0 * fmath::acos(1.0 - tol / r))).ceil() as usize;
            let m = u.len() - 1;
            assert!(m >= chord_rule && m <= chord_rule * 2, "tol {tol}: {m} pieces against the chord rule's {chord_rule}");
            assert_eq!(w.len(), 2, "the ruling is straight: {w:?}");
            for p in u.windows(2) {
                let (pa, pb) = (point_at(&s, p[0], 0.5), point_at(&s, p[1], 0.5));
                let mid = point_at(&s, 0.5 * (p[0] + p[1]), 0.5);
                let sag = (mid - (pa + pb) * 0.5).length();
                assert!(sag <= tol + 1e-9, "sag {sag} over {tol}");
            }
        }
        // With the normal bound the ruling still needs nothing: a cylinder's
        // normal is constant along it.
        let (_, w) = division_of(s, 0.05, DivisionBounds::default());
        assert_eq!(w.len(), 2, "{w:?}");
    }

    /// A ruled patch between two straight rails: parallel rails make a plane
    /// and the ruling gets one division; rails turned by `β` about the
    /// ruling make a twisted patch whose normal turns by `β` along the
    /// ruling, and the normal bound divides it into `⌈β / 8°⌉` pieces.
    ///
    /// The tolerance is coarse here so that the chordal bound stays silent
    /// and the count is the normal bound's alone; what the chordal bound
    /// makes of the same twist is the saddle test below.
    #[test]
    fn a_twisted_ruled_patch_divides_its_ruling_with_the_twist_and_a_straight_one_not() {
        let patch = |beta_deg: f64| {
            let (s, c) = fmath::sin_cos(beta_deg.to_radians());
            let l = 30.0;
            ruled(line(v(0.0, 0.0, 0.0), v(l, 0.0, 0.0)), line(v(0.0, 0.0, 40.0), v(l * c, l * s, 40.0)))
        };
        let coarse = 100.0;
        let (_, w) = division_of(patch(0.0), coarse, DivisionBounds::default());
        assert_eq!(w.len(), 2, "straight: {w:?}");
        let (_, w) = division_of(patch(48.0), coarse, CHORD_ONLY);
        assert_eq!(w.len(), 2, "without the normal bound a coarse tolerance divides nothing: {w:?}");
        // At least `⌈β / 8°⌉` pieces; a few more where the turn is not
        // uniform along the ruling, since the second pass re-measures each
        // piece (the saddle's normal turns faster in the middle of a
        // tilted ruling than at its ends).
        let pieces = |beta: f64| division_of(patch(beta), coarse, DivisionBounds::default()).1.len() - 1;
        for (beta, least) in [(16.0, 2), (32.0, 4), (48.0, 6)] {
            let p = pieces(beta);
            assert!(p >= least && p <= least + least / 2, "{beta}°: {p} pieces along the ruling, {least} needed");
        }
        // And the turn across every cell of the result is within the bound.
        let s = patch(48.0);
        let (u, w) = division_of(s.clone(), coarse, DivisionBounds::default());
        let max = DivisionBounds::default().max_normal_turn;
        for &uu in &u {
            for p in w.windows(2) {
                let (a, b) = (unit_normal(&s, uu, p[0]).unwrap(), unit_normal(&s, uu, p[1]).unwrap());
                let t = fmath::acos(a.dot(b).clamp(-1.0, 1.0));
                assert!(t <= max + 1e-6, "the normal turns {}° across a cell", t.to_degrees());
            }
        }
    }

    /// The twist term of the chordal bound on its own: rails 30 mm long
    /// turned 40° apart give `M_uv = ‖B' − A'‖ = 2·30·sin 20°`, and with
    /// `M_uu = M_vv = 0` the cell must be divided until `¼·M_uv·h_u·h_v ≤ tol`
    /// — in either direction, the product is what counts, and the tie goes
    /// to the longer piece so the cells come out near square.
    #[test]
    fn the_twist_term_divides_a_bilinear_saddle_by_its_product() {
        let (s, c) = fmath::sin_cos(40.0f64.to_radians());
        let patch = ruled(line(v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0)), line(v(0.0, 0.0, 30.0), v(30.0 * c, 30.0 * s, 30.0)));
        let Surface::BSplineSurface(bs) = &patch else { unreachable!() };
        let raw = CellFrame { normal: None, stray: 2.0 };
        let cell = PolynomialNet::new(bs).cell((0.0, 1.0), (0.0, 1.0), &raw);
        let m_uv = 2.0 * 30.0 * fmath::sin(20.0f64.to_radians());
        assert!((cell.m_uv - m_uv).abs() < 1e-9 && cell.m_uu.abs() < 1e-9 && cell.m_vv.abs() < 1e-9, "{cell:?}");
        // Along the normal, which the saddle's twist mostly is.
        let cell = PolynomialNet::new(bs).cell((0.0, 1.0), (0.0, 1.0), &CellFrame::of(&patch, (0.0, 1.0), (0.0, 1.0)));
        assert!(cell.m_uv <= m_uv + 1e-9 && cell.m_uv >= 0.8 * m_uv, "{cell:?} against {m_uv}");
        let m_uv = cell.m_uv;
        let tol = 0.05;
        let (u, w) = division_of(patch, tol, CHORD_ONLY);
        let (m, n) = (u.len() - 1, w.len() - 1);
        let need = (m_uv / (4.0 * tol)).ceil() as usize;
        assert!(m * n >= need, "{m} × {n} pieces cannot hold the twist to {tol}: {need} needed");
        assert!(m * n <= need * 2, "{m} × {n} pieces is more than twice the {need} needed");
        assert!(m.abs_diff(n) <= 1, "the tie-break keeps the cells square: {m} × {n}");
    }

    /// Planes and revolved surfaces keep truck's division whatever the
    /// bounds say: a plane divides nothing.
    #[test]
    fn a_plane_keeps_trucks_boundary_only_division() {
        let plane = Surface::Plane(truck_modeling::Plane::new(
            v(0.0, 0.0, 0.0).point(),
            v(1.0, 0.0, 0.0).point(),
            v(0.0, 1.0, 0.0).point(),
        ));
        let ks = KSurface::bounded(plane, DivisionBounds::default());
        assert!(!ks.is_bounded());
        let (u, w) = ks.parameter_division(((0.0, 100.0), (0.0, 100.0)), 0.001);
        assert_eq!((u.len(), w.len()), (2, 2));
    }

    /// An edge's polyline has at least the pieces its faces asked for: a
    /// line, which truck samples as its two ends, gets them evenly; a curve
    /// keeps its own chordal samples and gains the even ones between.
    #[test]
    fn a_curve_edge_carries_the_pieces_its_faces_ask_for() {
        let line = Curve::Line(truck_modeling::Line(v(0.0, 0.0, 0.0).point(), v(0.0, 0.0, 40.0).point()));
        let (t, p) = KCurve::new(line.clone(), 1).parameter_division((0.0, 1.0), 0.05);
        assert_eq!((t.len(), p.len()), (2, 2), "a line on its own is its ends");
        let (t, p) = KCurve::new(line, 4).parameter_division((0.0, 1.0), 0.05);
        assert_eq!(t, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
        assert!((point_to_vec3(p[2]) - v(0.0, 0.0, 20.0)).length() < 1e-12);
        let arc = Curve::NurbsCurve(arc(20.0, std::f64::consts::FRAC_PI_2, 0.0));
        let (own, _) = KCurve::new(arc.clone(), 1).parameter_division((0.0, 1.0), 0.05);
        let (both, pts) = KCurve::new(arc, 3).parameter_division((0.0, 1.0), 0.05);
        assert!(both.len() >= own.len() && both.len() <= own.len() + 2, "{own:?} then {both:?}");
        for want in [1.0 / 3.0, 2.0 / 3.0] {
            assert!(both.iter().any(|&t| (t - want).abs() < 1e-9), "{want} in {both:?}");
        }
        assert!(both.windows(2).all(|w| w[1] > w[0]), "ascending: {both:?}");
        assert_eq!(pts.len(), both.len());
    }

    /// The edge half of the normal bound: a twisted patch's rulings — its
    /// straight sides — need pieces, its rails none beyond their own; a
    /// cylinder's rulings need none; a segment that is not a whole side of
    /// the patch is left to the curve.
    #[test]
    fn edge_pieces_follow_the_normal_turn_along_a_patch_side() {
        let (s, c) = fmath::sin_cos(48.0f64.to_radians());
        let twisted = ruled(line(v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0)), line(v(0.0, 0.0, 40.0), v(30.0 * c, 30.0 * s, 40.0)));
        let ks = KSurface::bounded(twisted.clone(), DivisionBounds::default());
        let corner = |u: f64, w: f64| twisted.subs(u, w);
        let ruling = ks.edge_pieces(corner(0.0, 0.0), corner(0.0, 1.0));
        assert!(ruling >= 6, "48° of twist along the ruling at 8° a piece: {ruling}");
        assert_eq!(ks.edge_pieces(corner(0.0, 1.0), corner(0.0, 0.0)), ruling, "either way round");
        // Along the rail at v = 0 the normal turns too — the ruling
        // direction swings as the far rail leaves the near one — by about
        // `atan(30·sin 48° / 40)`, 29°: fewer pieces than the ruling.
        let rail = ks.edge_pieces(corner(0.0, 0.0), corner(1.0, 0.0));
        assert!(rail >= 3 && rail < ruling, "the rail turns less than the ruling: {rail} against {ruling}");
        assert_eq!(ks.edge_pieces(corner(0.0, 0.0), corner(0.5, 0.5)), 1, "not a side of the patch");
        assert_eq!(KSurface::bounded(twisted.clone(), CHORD_ONLY).edge_pieces(corner(0.0, 0.0), corner(0.0, 1.0)), 1, "no normal bound, no pieces");

        let a = arc(20.0, std::f64::consts::FRAC_PI_2, 0.0);
        let b = arc(20.0, std::f64::consts::FRAC_PI_2, 5.0);
        let band = Surface::NurbsSurface(NurbsSurface::new(BSplineSurface::homotopy(a.into_non_rationalized(), b.into_non_rationalized())));
        let ks = KSurface::bounded(band.clone(), DivisionBounds::default());
        assert_eq!(ks.edge_pieces(band.subs(0.0, 0.0), band.subs(0.0, 1.0)), 1, "a cylinder's ruling");
        let rail = ks.edge_pieces(band.subs(0.0, 0.0), band.subs(1.0, 0.0));
        assert!(rail >= 11 && rail <= 13, "a quarter turn at 8° a piece: {rail}");
        assert_eq!(KSurface::trucks(band.clone()).edge_pieces(band.subs(0.0, 0.0), band.subs(1.0, 0.0)), 1, "truck's own division asks nothing of its edges");
    }

    /// A boundary point a micron off the surface inverts to its nearest
    /// point; one a millimetre off does not.
    #[test]
    fn inversion_accepts_the_nearest_point_within_the_weld_epsilon() {
        let s = ruled(line(v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0)), line(v(0.0, 20.0, 0.0), v(30.0, 20.0, 0.0)));
        let ks = KSurface::bounded(s, DivisionBounds::default());
        let on = Point3::new(12.0, 5.0, 0.0);
        assert_eq!(ks.search_parameter(on, None, 100).map(|(u, w)| ((u * 1e9).round() / 1e9, (w * 1e9).round() / 1e9)), Some((0.4, 0.25)));
        let off = Point3::new(12.0, 5.0, 3e-5);
        let (u, w) = ks.search_parameter(off, None, 100).expect("a hair off the plane still inverts");
        assert!((u - 0.4).abs() < 1e-6 && (w - 0.25).abs() < 1e-6, "{u} {w}");
        assert_eq!(ks.search_parameter(Point3::new(12.0, 5.0, 1.0), None, 100), None, "a millimetre off is not on the face");
    }

    /// The knot-span lookup lands on the non-empty span, at the ends and on
    /// repeated knots alike.
    #[test]
    fn span_index_finds_the_non_empty_span() {
        let k = KnotVec::from(vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0]);
        assert_eq!(span_index(&k, 0.0), 2);
        assert_eq!(span_index(&k, 0.25), 2);
        assert_eq!(span_index(&k, 0.5), 3);
        assert_eq!(span_index(&k, 0.75), 3);
        assert_eq!(span_index(&k, 1.0), 3);
        assert_eq!(span_index(&k, 2.0), 3);
        assert_eq!(span_index(&k, -1.0), 2);
        assert_eq!(breakpoints(&k, (0.0, 1.0)), vec![0.0, 0.5, 1.0]);
        assert_eq!(breakpoints(&k, (0.5, 1.0)), vec![0.5, 1.0]);
        assert_eq!(breakpoints(&k, (0.2, 0.7)), vec![0.2, 0.5, 0.7]);
        // The boundary knots stay out even when the range overshoots them by
        // an inversion residual, and so does an interior knot at the range's
        // end.
        assert_eq!(breakpoints(&k, (-1e-8, 1.0 + 1e-7)), vec![-1e-8, 0.5, 1.0 + 1e-7]);
        assert_eq!(breakpoints(&k, (0.5 - 1e-8, 1.0)), vec![0.5 - 1e-8, 1.0]);
        let plain = KnotVec::from(vec![0.0, 0.0, 1.0, 1.0]);
        assert_eq!(breakpoints(&plain, (-3e-8, 1.0 + 1e-7)), vec![-3e-8, 1.0 + 1e-7]);
    }
}
