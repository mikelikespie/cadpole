//! Edge-local fillets and chamfers: the correction that makes
//! `fillet(body, edges=..., radius=)` round *those* edges and nothing else.
//!
//! # Why not just smooth-union the whole body
//!
//! `smooth_union` is how fillets happen when a shape is *built* from fields:
//! the blend radius is baked into the boolean that created the edge. It is no
//! use for `fillet(bracket.edges(...), 3)`, because by then the edge exists
//! and the two half-spaces that made it have been forgotten — and a blend
//! applied to the body as a whole would round every edge it has.
//!
//! # The construction
//!
//! An edge of a solid is locally the intersection (convex) or the union
//! (concave) of the two half-spaces its faces bound. So for a query point `p`
//! near a selected edge we rebuild those two half-spaces from the mesh:
//!
//! 1. find the closest point `c` on the edge polyline, and interpolate the
//!    two adjacent face normals `na`, `nb` there (both outward);
//! 2. `a = na . (p - c)` and `b = nb . (p - c)` are the signed distances to
//!    the two tangent planes, so the unblended solid is locally `max(a, b)`
//!    for a convex edge and `min(a, b)` for a concave one;
//! 3. the blended solid is [`smooth_intersect`]/[`smooth_union`] (fillet) or
//!    [`chamfer_intersect`]/[`chamfer_union`] (chamfer) of the same pair —
//!    which for a *planar* pair is the exact rolling-ball blend, not an
//!    approximation of one;
//! 4. the **correction** `delta = blended - unblended` is added to the body's
//!    own field.
//!
//! Step 4 is what localises it. `delta` is identically zero unless the point
//! is inside the wedge within the radius of *both* tangent planes, so it
//! vanishes a fraction of a millimetre away from the edge; a weight `w` in
//! `[0, 1]` then fades it out along the edge (so an open convex chain does not
//! round past its ends; the material a concave one adds is instead clipped at
//! the plane through its end, see [`EdgeBlendSet::apply`]) and just outside
//! the solid (so the plane extensions cannot
//! reach round the far side of a part). Where the correction is live the
//! field is *exactly* the blended one, so the surface is exactly the fillet
//! arc; everywhere else the field is exactly the body's own.
//!
//! Errors this leaves on the table, all documented in
//! `docs/starlark-api.md`: the tangent planes are re-evaluated per query
//! point but interpolated between polyline vertices, so on a curved face the
//! blend is exact only to the tessellation of that face; where three filleted
//! edges meet, the corrections combine with `max`, which gives the
//! intersection of the three blends rather than a true vertex blend; and the
//! whole thing is resampled by the mesher, so the output carries the
//! contouring error as well.

use crate::interval::{Box3, Interval};
use crate::tape::{chamfer_intersect, chamfer_union, smooth_intersect, smooth_union};
use cadpole_core::fmath;

/// What to do to the selected edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlendKind {
    /// Constant-radius rolling-ball blend.
    Fillet { radius: f64 },
    /// Plane cut at `distance` setback along each adjacent face.
    Chamfer { distance: f64 },
}

impl BlendKind {
    pub fn size(self) -> f64 {
        match self {
            BlendKind::Fillet { radius } => radius,
            BlendKind::Chamfer { distance } => distance,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BlendKind::Fillet { .. } => "fillet",
            BlendKind::Chamfer { .. } => "chamfer",
        }
    }
}

/// One selected edge, as the blend needs it: a polyline with the outward
/// normals of both adjacent faces at every vertex.
#[derive(Clone, Debug)]
pub struct BlendEdge {
    pub name: String,
    pub points: Vec<[f64; 3]>,
    /// Outward normal of the first adjacent face at each point.
    pub na: Vec<[f64; 3]>,
    /// Outward normal of the second adjacent face at each point.
    pub nb: Vec<[f64; 3]>,
    /// True when material is on the inside of the fold (a box's outer edges).
    pub convex: bool,
    /// The polyline's first vertex repeats as its last.
    pub closed: bool,
    /// The first (`.0`) or last (`.1`) end stops on a face across the edge,
    /// where both of the edge's faces end too (the end faces of an extruded
    /// L). Only there is a concave blend clipped at the plane through the
    /// end; an end where the faces go on (a seam of a round boss's base, a
    /// boss's convex corner) fades out like a convex one.
    pub capped: (bool, bool),
    lo: [f64; 3],
    hi: [f64; 3],
    /// Arc length from the first point to each point.
    cum: Vec<f64>,
    /// `cos(half the angle between the two outward normals)`, minimised along
    /// the edge. The blend's far vertex sits `size / cos_half` from the edge
    /// line, so this is what sets the influence radius.
    cos_half: f64,
}

impl BlendEdge {
    pub fn new(
        name: String,
        points: Vec<[f64; 3]>,
        na: Vec<[f64; 3]>,
        nb: Vec<[f64; 3]>,
        convex: bool,
    ) -> BlendEdge {
        let closed = points.len() > 2
            && dist2(points[0], points[points.len() - 1]) < 1e-18;
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &points {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let mut cos_half: f64 = 1.0;
        for i in 0..points.len() {
            let between = fmath::acos(dot(na[i], nb[i]).clamp(-1.0, 1.0));
            cos_half = cos_half.min(fmath::cos(between * 0.5));
        }
        let mut cum = Vec::with_capacity(points.len());
        let mut run = 0.0;
        for i in 0..points.len() {
            if i > 0 {
                run += dist2(points[i - 1], points[i]).sqrt();
            }
            cum.push(run);
        }
        BlendEdge { name, points, na, nb, convex, closed, capped: (false, false), lo, hi, cum, cos_half: cos_half.max(0.15) }
    }

    /// Total length of the polyline.
    pub fn length(&self) -> f64 {
        self.points.windows(2).map(|w| dist2(w[0], w[1]).sqrt()).sum()
    }

    /// Closest point on the polyline, the interpolated face normals there, how
    /// far "past" an open end the query point is (0 when it is not), and the
    /// same signed: negative inside the chain, by the arc length from the
    /// closest point to the nearer open end (`-inf` on a closed chain).
    fn closest(&self, p: [f64; 3]) -> (f64, [f64; 3], [f64; 3], [f64; 3], f64, f64, bool) {
        let mut best = f64::INFINITY;
        let mut bi = 0usize;
        let mut bt = 0.0f64;
        for i in 0..self.points.len().saturating_sub(1) {
            let (a, b) = (self.points[i], self.points[i + 1]);
            let ab = sub(b, a);
            let l2 = dot(ab, ab);
            let t = if l2 <= 0.0 { 0.0 } else { (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0) };
            let c = add(a, mul(ab, t));
            let d = dist2(p, c);
            if d < best {
                best = d;
                bi = i;
                bt = t;
            }
        }
        let (a, b) = (self.points[bi], self.points[bi + 1]);
        let c = add(a, mul(sub(b, a), bt));
        let na = norm(lerp(self.na[bi], self.na[bi + 1], bt));
        let nb = norm(lerp(self.nb[bi], self.nb[bi + 1], bt));
        // Overrun past an open end, measured along the terminal tangent.
        let mut past = 0.0f64;
        if !self.closed {
            if bi == 0 && bt <= 0.0 {
                let t0 = norm(sub(self.points[1], self.points[0]));
                past = past.max(dot(sub(self.points[0], p), t0));
            }
            let last = self.points.len() - 2;
            if bi == last && bt >= 1.0 {
                let n = self.points.len();
                let t1 = norm(sub(self.points[n - 1], self.points[n - 2]));
                past = past.max(dot(sub(p, self.points[n - 1]), t1));
            }
        }
        let past = past.max(0.0);
        let foot = self.cum[bi] + (self.cum[bi + 1] - self.cum[bi]) * bt;
        let total = self.cum[self.cum.len() - 1];
        // Whether the nearer open end stops on a face across the edge.
        let capped = !self.closed && if foot <= total - foot { self.capped.0 } else { self.capped.1 };
        let signed = if self.closed {
            f64::NEG_INFINITY
        } else if past > 0.0 {
            past
        } else {
            -foot.min(total - foot).max(0.0)
        };
        (best.sqrt(), c, na, nb, past, signed, capped)
    }
}

/// The selected edges of one `fillet`/`chamfer` call, compiled into a form
/// the tape can evaluate.
#[derive(Debug)]
pub struct EdgeBlendSet {
    pub kind: BlendKind,
    pub edges: Vec<BlendEdge>,
    /// Beyond this distance from an edge polyline the correction is zero.
    influence: f64,
    /// Where the correction stops being applied at all, measured in the
    /// "outside the solid" direction. The blend surface itself always lies at
    /// or below 0, so widening this cannot move the surface.
    gate: f64,
    /// Bound on `|correction|`, for the interval evaluator.
    delta_max: f64,
    any_convex: bool,
    any_concave: bool,
}

impl EdgeBlendSet {
    pub fn new(kind: BlendKind, edges: Vec<BlendEdge>) -> EdgeBlendSet {
        let size = kind.size().abs();
        let mut cos_half: f64 = 1.0;
        let mut any_convex = false;
        let mut any_concave = false;
        for e in &edges {
            cos_half = cos_half.min(e.cos_half);
            if e.convex {
                any_convex = true;
            } else {
                any_concave = true;
            }
        }
        let gate = 0.5 * size;
        // The blend only differs from the sharp corner inside the rhombus
        // bounded by the two offset planes; its far vertex sits at
        // size / cos_half from the edge line. The gate widens that by half a
        // size, and a further factor of two is slack for curved faces.
        let influence = 2.0 * (size + gate) / cos_half + 1e-9;
        let delta_max = match kind {
            // hypot(u, v) - max(u, v) <= (sqrt(2) - 1) * max(u, v), and the
            // gate caps max(u, v) at size + gate.
            BlendKind::Fillet { radius } => (std::f64::consts::SQRT_2 - 1.0) * (radius.abs() + gate),
            BlendKind::Chamfer { distance } => distance.abs(),
        };
        EdgeBlendSet { kind, edges, influence, gate, delta_max, any_convex, any_concave }
    }

    pub fn influence(&self) -> f64 {
        self.influence
    }

    /// The blended field at `p`, given the body's own field `base` there.
    ///
    /// A convex edge's correction only removes material, so fading it out
    /// past an open end is harmless. A concave edge's adds material, and past
    /// an end the body's field is only the distance to the end face, so any
    /// fade would let the added material bulge out through the end face. So
    /// the added material is intersected with the end half-space instead:
    /// per concave edge the result is `min(base, max(base + delta, s))`,
    /// where `s` is the signed distance past the nearer open end. That is
    /// exactly the fillet stopped flush at a planar end face, it is continuous,
    /// and it changes nothing inside the chain, where `s` is negative and the
    /// blended field is already below it wherever the blend is solid.
    pub fn apply(&self, base: f64, p: [f64; 3]) -> f64 {
        base + self.correction_given(base, p)
    }

    /// The correction to add to the body's own field at `p`, without the
    /// end-face clamp that [`apply`](Self::apply) adds for concave edges (it
    /// needs the body's field).
    pub fn correction(&self, p: [f64; 3]) -> f64 {
        self.correction_given(f64::INFINITY, p)
    }

    fn correction_given(&self, base: f64, p: [f64; 3]) -> f64 {
        let mut pos = 0.0f64;
        let mut neg = 0.0f64;
        for e in &self.edges {
            if !within(p, e.lo, e.hi, self.influence) {
                continue;
            }
            let (dist, c, na, nb, past, signed, capped) = e.closest(p);
            if dist > self.influence {
                continue;
            }
            let v = sub(p, c);
            let a = dot(na, v);
            let b = dot(nb, v);
            let blended = match self.kind {
                BlendKind::Fillet { radius } => {
                    if e.convex { smooth_intersect(a, b, radius) } else { smooth_union(a, b, radius) }
                }
                BlendKind::Chamfer { distance } => {
                    if e.convex {
                        chamfer_intersect(a, b, distance)
                    } else {
                        chamfer_union(a, b, distance)
                    }
                }
            };
            let plain = if e.convex { a.max(b) } else { a.min(b) };
            let delta = blended - plain;
            if delta == 0.0 {
                continue;
            }
            // Radial gate: the blend surface lies entirely where the sharp
            // corner is at or inside the solid, so fading the correction out
            // just outside it cannot move the surface, but it does stop the
            // two infinite tangent planes reaching round the rest of the part.
            let m = if e.convex { a.max(b) } else { -(a.min(b)) };
            let w_radial = fade(m, 0.0, self.gate);
            // Longitudinal fade so an open chain does not cut past its ends;
            // a concave one that ends on a face across it is clipped to its
            // end half-space below instead.
            let clip = !e.convex && capped;
            let w_end = if clip { 1.0 } else { fade(past, 0.0, self.kind.size().abs().max(1e-9)) };
            let w = w_radial * w_end;
            if w <= 0.0 {
                continue;
            }
            let mut d = w * delta;
            if clip {
                d = d.max(signed - base).min(0.0);
            }
            if d > 0.0 {
                pos = pos.max(d);
            } else {
                neg = neg.min(d);
            }
        }
        pos + neg
    }

    /// A conservative interval for the correction over a whole box: zero when
    /// no selected edge can reach it, and the signed bound otherwise.
    pub fn correction_bound(&self, b: Box3) -> Interval {
        let reach = self.influence + b.radius();
        let c = b.center();
        let mut convex = false;
        let mut concave = false;
        for e in &self.edges {
            if within(c, e.lo, e.hi, reach) {
                if e.convex {
                    convex = true;
                } else {
                    concave = true;
                }
            }
        }
        if !convex && !concave {
            return Interval::point(0.0);
        }
        let lo = if concave { -self.delta_max } else { 0.0 };
        let hi = if convex { self.delta_max } else { 0.0 };
        Interval::new(lo, hi)
    }

    pub fn has_convex(&self) -> bool {
        self.any_convex
    }

    pub fn has_concave(&self) -> bool {
        self.any_concave
    }

    /// World bounds of every selected edge, grown by the influence radius.
    pub fn bounds(&self) -> Option<Box3> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for e in &self.edges {
            for k in 0..3 {
                lo[k] = lo[k].min(e.lo[k]);
                hi[k] = hi[k].max(e.hi[k]);
            }
        }
        if !lo[0].is_finite() {
            return None;
        }
        Some(Box3::new(lo, hi).expanded(self.influence))
    }

    /// Where a rolling ball of the requested radius has to sit for each edge:
    /// the centre of the blend arc at the midpoint of every polyline segment,
    /// and the direction it must fit in. `fillet`'s "this radius does not fit"
    /// diagnostic probes the body's own field at these points.
    pub fn ball_centres(&self) -> Vec<(String, [f64; 3], f64, bool)> {
        let size = self.kind.size().abs();
        let mut out = Vec::new();
        for e in &self.edges {
            for i in 0..e.points.len().saturating_sub(1) {
                let c = mul(add(e.points[i], e.points[i + 1]), 0.5);
                let na = norm(lerp(e.na[i], e.na[i + 1], 0.5));
                let nb = norm(lerp(e.nb[i], e.nb[i + 1], 0.5));
                let bisector = norm(add(na, nb));
                let half = (fmath::acos(dot(na, nb).clamp(-1.0, 1.0))) * 0.5;
                // The rolling ball touches both faces: its centre is
                // size / cos(half) from the edge along the bisector, on the
                // material side for a convex edge and outside for a concave.
                let d = size / fmath::cos(half).abs().max(0.2);
                let dir = if e.convex { -1.0 } else { 1.0 };
                out.push((e.name.clone(), add(c, mul(bisector, d * dir)), size, e.convex));
            }
        }
        out
    }
}

/// A smooth 1 -> 0 ramp: 1 at or below `a`, 0 at or above `b`.
#[inline]
fn fade(x: f64, a: f64, b: f64) -> f64 {
    if b <= a {
        return if x <= a { 1.0 } else { 0.0 };
    }
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

#[inline]
fn within(p: [f64; 3], lo: [f64; 3], hi: [f64; 3], slack: f64) -> bool {
    (0..3).all(|k| p[k] >= lo[k] - slack && p[k] <= hi[k] + slack)
}

#[inline]
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub(crate) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub(crate) fn mul(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub(crate) fn dist2(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = sub(a, b);
    dot(d, d)
}
#[inline]
pub(crate) fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = dot(v, v).sqrt();
    if l < 1e-15 { [0.0, 0.0, 1.0] } else { mul(v, 1.0 / l) }
}
#[inline]
fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    add(mul(a, 1.0 - t), mul(b, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One straight convex edge along Y, between the +X face and the +Z face
    /// of a box corner: na = +X, nb = +Z.
    fn corner_edge(convex: bool) -> BlendEdge {
        let pts = vec![[0.0, -10.0, 0.0], [0.0, 10.0, 0.0]];
        BlendEdge::new(
            "e".into(),
            pts,
            vec![[1.0, 0.0, 0.0]; 2],
            vec![[0.0, 0.0, 1.0]; 2],
            convex,
        )
    }

    #[test]
    fn the_correction_is_zero_away_from_the_edge() {
        let set = EdgeBlendSet::new(BlendKind::Fillet { radius: 2.0 }, vec![corner_edge(true)]);
        for p in [[-20.0, 0.0, -20.0], [0.0, 40.0, 0.0], [-9.0, 0.0, 0.0], [0.0, 0.0, -9.0]] {
            assert_eq!(set.correction(p), 0.0, "{p:?}");
        }
    }

    #[test]
    fn the_blended_surface_is_the_arc_of_the_requested_radius() {
        // Body: the quarter space {x <= 0, z <= 0} near the edge, whose own
        // field is max(x, z). Filleting by r must put the surface on the arc
        // centred at (-r, ., -r).
        let r = 2.5;
        let set = EdgeBlendSet::new(BlendKind::Fillet { radius: r }, vec![corner_edge(true)]);
        for k in 0..=10 {
            let a = std::f64::consts::FRAC_PI_2 * k as f64 / 10.0;
            let (x, z) = (-r + r * fmath::cos(a), -r + r * fmath::sin(a));
            let p = [x, 0.0, z];
            let base = x.max(z);
            let f = base + set.correction(p);
            assert!(f.abs() < 1e-9, "({x}, {z}) should be on the fillet: {f}");
        }
        // The sharp corner is now outside the solid.
        let base = 0.0f64;
        assert!(base + set.correction([0.0, 0.0, 0.0]) > 0.0);
    }

    #[test]
    fn a_chamfer_cuts_the_plane_at_the_requested_setback() {
        let d = 1.5;
        let set = EdgeBlendSet::new(BlendKind::Chamfer { distance: d }, vec![corner_edge(true)]);
        for (x, z) in [(-d, 0.0), (0.0, -d), (-d / 2.0, -d / 2.0)] {
            let base = x.max(z);
            let f = base + set.correction([x, 0.0, z]);
            assert!(f.abs() < 1e-9, "({x}, {z}) should be on the chamfer: {f}");
        }
    }

    #[test]
    fn a_concave_edge_gains_material() {
        let r = 2.0;
        let set = EdgeBlendSet::new(BlendKind::Fillet { radius: r }, vec![corner_edge(false)]);
        // The local solid is min(x, z); the fillet arc is centred at (r, ., r).
        for k in 0..=10 {
            let a = std::f64::consts::FRAC_PI_2 * k as f64 / 10.0;
            let (x, z) = (r - r * fmath::cos(a), r - r * fmath::sin(a));
            let base = x.min(z);
            let f = base + set.correction([x, 0.0, z]);
            assert!(f.abs() < 1e-9, "({x}, {z}) should be on the fillet: {f}");
        }
    }

    #[test]
    fn an_open_chain_fades_out_past_its_ends() {
        let r = 2.0;
        let set = EdgeBlendSet::new(BlendKind::Fillet { radius: r }, vec![corner_edge(true)]);
        // On the edge line, well inside the chain, the correction is live.
        assert!(set.correction([0.0, 0.0, 0.0]) > 0.0);
        // Past the end by more than the radius it is gone.
        assert_eq!(set.correction([0.0, 10.0 + r + 0.01, 0.0]), 0.0);
    }

    #[test]
    fn a_concave_blend_adds_nothing_past_an_open_end() {
        // An inside corner {x >= 0 or z >= 0 is air}: the local solid is
        // min(x, z), cut off by the planar end faces y = +-10 that the edge
        // runs between, so the body's field is max(min(x, z), |y| - 10).
        for kind in [BlendKind::Fillet { radius: 3.0 }, BlendKind::Chamfer { distance: 3.0 }] {
            // Both ends stop on the end faces: capped.
            let mut edge = corner_edge(false);
            edge.capped = (true, true);
            let set = EdgeBlendSet::new(kind, vec![edge]);
            for y in [10.01f64, 10.3, 11.0, 12.0, -10.3, -11.5] {
                for (x, z) in [(0.3f64, 0.3f64), (0.6, 0.2), (0.8, 0.8), (1.0, 0.1)] {
                    let base = x.min(z).max(y.abs() - 10.0);
                    let f = set.apply(base, [x, y, z]);
                    assert!(f > 0.0, "{kind:?}: ({x}, {y}, {z}) is past the end face, yet inside: {f}");
                }
            }
            // Inside the chain the blend is still there, right up to the end.
            for y in [0.0f64, 9.0, 9.9, -9.9] {
                let base = 0.3f64.max(y.abs() - 10.0);
                assert!(set.apply(base, [0.3, y, 0.3]) < 0.0, "{kind:?}: no blend at y = {y}");
            }
        }
    }

    #[test]
    fn the_interval_bound_encloses_every_sampled_correction() {
        let set = EdgeBlendSet::new(BlendKind::Fillet { radius: 2.0 }, vec![corner_edge(true)]);
        for bx in [
            Box3::new([-3.0, -1.0, -3.0], [1.0, 1.0, 1.0]),
            Box3::new([-0.2, -0.2, -0.2], [0.2, 0.2, 0.2]),
            Box3::new([20.0, 20.0, 20.0], [21.0, 21.0, 21.0]),
        ] {
            let iv = set.correction_bound(bx);
            let n = 8;
            for i in 0..=n {
                for j in 0..=n {
                    for k in 0..=n {
                        let p = [
                            bx.x.lo + bx.x.width() * i as f64 / n as f64,
                            bx.y.lo + bx.y.width() * j as f64 / n as f64,
                            bx.z.lo + bx.z.width() * k as f64 / n as f64,
                        ];
                        let c = set.correction(p);
                        assert!(c >= iv.lo - 1e-9 && c <= iv.hi + 1e-9, "{c} escapes {iv:?}");
                    }
                }
            }
        }
    }
}
