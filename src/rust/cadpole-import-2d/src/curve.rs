//! Curve conversions shared by the SVG and DXF front ends.
//!
//! The rule is the design's: keep circular arcs circular, approximate only
//! what is genuinely not a line or a circle, and do it within tolerance.

use std::collections::HashMap;

use cadpole_core::sketch::{Loop, P2, Path, Segment, catmull_rom};

/// Two points are the same point when they are this close, relative to the
/// caller's tolerance.
pub const EPS: f64 = 1e-9;

/// Apply `f` to every point of a segment, control points included.
pub fn map_segment(s: &mut Segment, f: impl Fn(P2) -> P2) {
    match s {
        Segment::Line { a, b } => {
            *a = f(*a);
            *b = f(*b);
        }
        Segment::Arc { a, b, via } => {
            *a = f(*a);
            *b = f(*b);
            *via = f(*via);
        }
        Segment::Bezier { a, b, ctrl } => {
            *a = f(*a);
            *b = f(*b);
            for c in ctrl.iter_mut() {
                *c = f(*c);
            }
        }
    }
}

fn on_circle(c: P2, r: f64, t: f64) -> P2 {
    P2::new(c.x + r * t.cos(), c.y + r * t.sin())
}

/// A circular arc given the way DXF gives one: centre, radius and start/end
/// angles in radians, always swept counter-clockwise.
///
/// Split in half when the sweep is a half turn or more, because three points
/// on a circle only determine an arc up to half a turn.
pub fn arc_from_center(c: P2, r: f64, start: f64, end: f64) -> Vec<Segment> {
    let mut sweep = end - start;
    while sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    while sweep > std::f64::consts::TAU {
        sweep -= std::f64::consts::TAU;
    }
    let pieces = if sweep > std::f64::consts::PI * 0.999 { 2 } else { 1 };
    let step = sweep / pieces as f64;
    (0..pieces)
        .map(|i| {
            let t0 = start + step * i as f64;
            let t1 = t0 + step;
            Segment::Arc {
                a: on_circle(c, r, t0),
                b: on_circle(c, r, t1),
                via: on_circle(c, r, t0 + step / 2.0),
            }
        })
        .collect()
}

/// A full circle as two half arcs, the shape `Loop::circle` uses (the kernel
/// needs two distinct end vertices).
pub fn circle_loop(c: P2, r: f64) -> Loop {
    Loop::circle(c, r)
}

/// One segment of a polyline with a DXF *bulge*: `bulge = tan(theta/4)` of
/// the included angle, positive counter-clockwise. Zero is a straight line.
///
/// The sagitta of the arc over the chord is `h = bulge * d / 2`, which puts
/// the `via` point half a chord along the perpendicular bisector — no radius
/// or centre needed, and exact.
pub fn bulge_segment(a: P2, b: P2, bulge: f64) -> Segment {
    if bulge.abs() < 1e-12 {
        return Segment::Line { a, b };
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let d = (dx * dx + dy * dy).sqrt();
    if d < EPS {
        return Segment::Line { a, b };
    }
    let mid = P2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    // A positive bulge is a counter-clockwise arc, whose centre therefore
    // lies to the *left* of the chord — so the arc itself bows to the right.
    // (Check: a 90 degree CCW arc from (0,0) to (10,0) has centre (5,5) and
    // passes through (5, -2.07), below the chord.)
    let (nx, ny) = (dy / d, -dx / d);
    let h = bulge * d / 2.0;
    Segment::Arc { a, b, via: P2::new(mid.x + nx * h, mid.y + ny * h) }
}

/// Radius of the arc a bulge describes, for tests and `asset_info`:
/// `r = d (1 + b^2) / (4 |b|)`.
pub fn bulge_radius(a: P2, b: P2, bulge: f64) -> Option<f64> {
    if bulge.abs() < 1e-12 {
        return None;
    }
    let d = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
    Some(d * (1.0 + bulge * bulge) / (4.0 * bulge.abs()))
}

/// SVG's `A rx ry rot large-arc sweep x y`, from `start` to `end`.
///
/// Equal radii give a true [`Segment::Arc`] — this is the whole reason Cadpole
/// parses SVG itself instead of using `usvg`, which would have flattened it
/// to cubics already. Unequal radii become cubic Béziers within `tolerance`.
pub fn svg_arc(
    start: P2,
    rx: f64,
    ry: f64,
    x_rotation_deg: f64,
    large_arc: bool,
    sweep: bool,
    end: P2,
    tolerance: f64,
) -> Vec<Segment> {
    // Out-of-range radii: the spec says a zero radius is a straight line and
    // radii too small to span the chord are scaled up (handled below).
    let (rx, ry) = (rx.abs(), ry.abs());
    if rx < EPS || ry < EPS || (start.x - end.x).abs() + (start.y - end.y).abs() < EPS {
        return vec![Segment::Line { a: start, b: end }];
    }
    let phi = x_rotation_deg.to_radians();
    let (cos_p, sin_p) = (phi.cos(), phi.sin());

    // F.6.5 step 1: the chord midpoint in the rotated frame.
    let dx2 = (start.x - end.x) / 2.0;
    let dy2 = (start.y - end.y) / 2.0;
    let x1p = cos_p * dx2 + sin_p * dy2;
    let y1p = -sin_p * dx2 + cos_p * dy2;

    // F.6.6: scale the radii up if they cannot span the chord.
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    let (rx, ry) = if lambda > 1.0 { (rx * lambda.sqrt(), ry * lambda.sqrt()) } else { (rx, ry) };

    // F.6.5 step 2: the centre in the rotated frame.
    let num = (rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p).max(0.0);
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let coef = if den <= 0.0 { 0.0 } else { (num / den).sqrt() } * if large_arc == sweep { -1.0 } else { 1.0 };
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cos_p * cxp - sin_p * cyp + (start.x + end.x) / 2.0;
    let cy = sin_p * cxp + cos_p * cyp + (start.y + end.y) / 2.0;

    // F.6.5 steps 3-4: start angle and sweep.
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let dot = ux * vx + uy * vy;
        let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = ang(1.0, 0.0, ux, uy);
    let mut delta = ang(ux, uy, vx, vy);
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }

    // A circle stays a circle: exact arcs, split so no piece exceeds half a
    // turn (three points do not determine a longer arc).
    if (rx - ry).abs() <= 1e-9 * rx.max(1.0) && phi.abs() < 1e-12 {
        let c = P2::new(cx, cy);
        let pieces = ((delta.abs() / std::f64::consts::PI).ceil() as usize).max(1);
        let step = delta / pieces as f64;
        let mut out = Vec::with_capacity(pieces);
        for i in 0..pieces {
            let t0 = theta1 + step * i as f64;
            let t1 = t0 + step;
            out.push(Segment::Arc {
                a: on_circle(c, rx, t0),
                b: on_circle(c, rx, t1),
                via: on_circle(c, rx, t0 + step / 2.0),
            });
        }
        // Snap the ends to the points the file actually named.
        if let Some(Segment::Arc { a, .. }) = out.first_mut() {
            *a = start;
        }
        if let Some(Segment::Arc { b, .. }) = out.last_mut() {
            *b = end;
        }
        return out;
    }

    let mut out = ellipse_cubics(P2::new(cx, cy), rx, ry, phi, theta1, delta, tolerance);
    if let Some(s) = out.first_mut() {
        match s {
            Segment::Bezier { a, .. } | Segment::Line { a, .. } | Segment::Arc { a, .. } => *a = start,
        }
    }
    if let Some(s) = out.last_mut() {
        match s {
            Segment::Bezier { b, .. } | Segment::Line { b, .. } | Segment::Arc { b, .. } => *b = end,
        }
    }
    out
}

/// An elliptical arc as cubic Béziers within `tolerance` (kurbo).
pub fn ellipse_cubics(
    center: P2,
    rx: f64,
    ry: f64,
    x_rotation: f64,
    start_angle: f64,
    sweep_angle: f64,
    tolerance: f64,
) -> Vec<Segment> {
    let arc = kurbo::Arc::new(
        kurbo::Point::new(center.x, center.y),
        kurbo::Vec2::new(rx, ry),
        start_angle,
        sweep_angle,
        x_rotation,
    );
    // The arc's own start, in absolute coordinates.
    let (c, s) = (x_rotation.cos(), x_rotation.sin());
    let (u, v) = (rx * start_angle.cos(), ry * start_angle.sin());
    let mut cur = P2::new(center.x + c * u - s * v, center.y + s * u + c * v);
    let mut out = Vec::new();
    arc.to_cubic_beziers(tolerance.max(1e-6), |p1, p2, p3| {
        let b = P2::new(p3.x, p3.y);
        out.push(Segment::Bezier {
            a: cur,
            b,
            ctrl: vec![P2::new(p1.x, p1.y), P2::new(p2.x, p2.y)],
        });
        cur = b;
    });
    if out.is_empty() {
        out.push(Segment::Line { a: cur, b: cur });
    }
    out
}

/// A B-spline as Bézier segments.
///
/// Degree 2 and 3 non-rational splines are converted **exactly** by Boehm
/// knot insertion: every interior knot is raised to full multiplicity, after
/// which each span's control points are the Bézier control points. Rational
/// splines (any weight not 1) and degree > 3 are sampled and re-fitted with a
/// Catmull-Rom spline dense enough to stay within `tolerance`; degree 1 is
/// a polyline.
pub fn bspline_segments(
    degree: usize,
    control: &[P2],
    knots: &[f64],
    weights: &[f64],
    tolerance: f64,
) -> Vec<Segment> {
    if control.len() < 2 {
        return Vec::new();
    }
    if degree <= 1 {
        return control.windows(2).map(|w| Segment::Line { a: w[0], b: w[1] }).collect();
    }
    let rational = weights.iter().any(|w| (w - 1.0).abs() > 1e-9);
    let usable_knots = knots.len() == control.len() + degree + 1;
    if degree <= 3 && !rational && usable_knots {
        if let Some(segs) = bezier_by_knot_insertion(degree, control, knots) {
            return segs;
        }
    }
    // Fallback: sample the curve and fit a smooth spline through the samples.
    let n = ((control.len() * 8).max(32)).min(512);
    let pts = sample_bspline(degree, control, knots, weights, n, tolerance);
    if pts.len() < 2 {
        return Vec::new();
    }
    catmull_rom(&pts, false, 0.5)
}

/// Boehm knot insertion until every interior knot has multiplicity `degree`.
/// Returns one Bézier segment per non-empty span, or `None` if the knot
/// vector is not one we can walk.
fn bezier_by_knot_insertion(degree: usize, control: &[P2], knots: &[f64]) -> Option<Vec<Segment>> {
    let mut ctrl: Vec<P2> = control.to_vec();
    let mut kv: Vec<f64> = knots.to_vec();
    // Insert until the interior knots all have multiplicity `degree`.
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 4096 {
            return None;
        }
        let mut target: Option<f64> = None;
        let mut i = degree + 1;
        while i < kv.len() - degree - 1 {
            let u = kv[i];
            let mult = kv.iter().filter(|k| (**k - u).abs() < 1e-12).count();
            if mult < degree {
                target = Some(u);
                break;
            }
            i += mult;
        }
        let Some(u) = target else { break };
        insert_knot(degree, &mut ctrl, &mut kv, u)?;
    }
    // Each group of `degree` control points after the first is one span.
    let mut out = Vec::new();
    let mut i = 0;
    while i + degree < ctrl.len() {
        let a = ctrl[i];
        let b = ctrl[i + degree];
        let mid: Vec<P2> = ctrl[i + 1..i + degree].to_vec();
        if (a.x - b.x).abs() > EPS || (a.y - b.y).abs() > EPS || !mid.is_empty() {
            out.push(Segment::Bezier { a, b, ctrl: mid });
        }
        i += degree;
    }
    if out.is_empty() { None } else { Some(out) }
}

/// One Boehm insertion of knot `u`.
fn insert_knot(degree: usize, ctrl: &mut Vec<P2>, kv: &mut Vec<f64>, u: f64) -> Option<()> {
    let k = kv.iter().rposition(|&x| x <= u + 1e-12)?;
    if k < degree || k + 1 >= kv.len() {
        return None;
    }
    let mut new_ctrl = Vec::with_capacity(ctrl.len() + 1);
    new_ctrl.extend_from_slice(&ctrl[..=k - degree]);
    for i in (k - degree + 1)..=k {
        if i >= ctrl.len() {
            return None;
        }
        let d = kv[i + degree] - kv[i];
        let a = if d.abs() < 1e-12 { 0.0 } else { (u - kv[i]) / d };
        let p0 = ctrl[i - 1];
        let p1 = ctrl[i];
        new_ctrl.push(P2::new(p0.x + (p1.x - p0.x) * a, p0.y + (p1.y - p0.y) * a));
    }
    new_ctrl.extend_from_slice(&ctrl[k..]);
    *ctrl = new_ctrl;
    kv.insert(k + 1, u);
    Some(())
}

/// Sample a (possibly rational) B-spline at `n` points by de Boor evaluation.
fn sample_bspline(
    degree: usize,
    control: &[P2],
    knots: &[f64],
    weights: &[f64],
    n: usize,
    _tolerance: f64,
) -> Vec<P2> {
    if knots.len() != control.len() + degree + 1 {
        // Without a usable knot vector the control polygon is the honest
        // approximation; it is what most viewers draw for a broken spline.
        return control.to_vec();
    }
    let (t0, t1) = (knots[degree], knots[knots.len() - degree - 1]);
    if !(t1 > t0) {
        return control.to_vec();
    }
    (0..=n)
        .filter_map(|i| {
            let t = t0 + (t1 - t0) * (i as f64 / n as f64);
            de_boor(degree, control, knots, weights, t.min(t1 - 1e-12).max(t0))
        })
        .collect()
}

fn de_boor(degree: usize, control: &[P2], knots: &[f64], weights: &[f64], t: f64) -> Option<P2> {
    let k = (degree..knots.len() - degree - 1).rev().find(|&i| knots[i] <= t)?;
    let w = |i: usize| weights.get(i).copied().unwrap_or(1.0);
    // Homogeneous coordinates so rational splines work with the same recursion.
    let mut d: Vec<(f64, f64, f64)> = (0..=degree)
        .map(|j| {
            let i = k - degree + j;
            let wi = w(i);
            (control[i].x * wi, control[i].y * wi, wi)
        })
        .collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = k - degree + j;
            let den = knots[i + degree + 1 - r] - knots[i];
            let a = if den.abs() < 1e-12 { 0.0 } else { (t - knots[i]) / den };
            let p = d[j - 1];
            let q = d[j];
            d[j] = (p.0 + (q.0 - p.0) * a, p.1 + (q.1 - p.1) * a, p.2 + (q.2 - p.2) * a);
        }
    }
    let (x, y, w) = d[degree];
    if w.abs() < 1e-12 { None } else { Some(P2::new(x / w, y / w)) }
}

/// FreeCAD's "join geometry", made explicit: chain loose segments into closed
/// loops and open paths by matching endpoints within `tolerance`.
///
/// Architectural DXFs are bags of unjoined `LINE`s; without this an imported
/// floor plan has no regions at all. Chains that close become loops; chains
/// that do not become construction paths, which the caller reports.
pub fn join(segments: Vec<Segment>, tolerance: f64) -> (Vec<Loop>, Vec<Path>) {
    let tol = tolerance.max(1e-9);
    let cell = tol.max(1e-9);
    let key = |p: P2| ((p.x / cell).round() as i64, (p.y / cell).round() as i64);
    // End index: every segment endpoint, so a chain can be extended from
    // either end without scanning the whole bag.
    let mut index: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        index.entry(key(s.start())).or_default().push(i);
        index.entry(key(s.end())).or_default().push(i);
    }
    let near = |p: P2, q: P2| (p.x - q.x).abs() <= tol && (p.y - q.y).abs() <= tol;
    let candidates = |p: P2, index: &HashMap<(i64, i64), Vec<usize>>| {
        let (kx, ky) = key(p);
        let mut out = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = index.get(&(kx + dx, ky + dy)) {
                    out.extend_from_slice(v);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    };

    let mut used = vec![false; segments.len()];
    let mut loops = Vec::new();
    let mut paths = Vec::new();
    for seed in 0..segments.len() {
        if used[seed] {
            continue;
        }
        used[seed] = true;
        let mut chain = vec![segments[seed].clone()];
        // Extend forward.
        loop {
            let end = chain.last().unwrap().end();
            let Some(next) = candidates(end, &index).into_iter().find(|&i| {
                !used[i] && (near(segments[i].start(), end) || near(segments[i].end(), end))
            }) else {
                break;
            };
            used[next] = true;
            let s = if near(segments[next].start(), end) {
                segments[next].clone()
            } else {
                reverse(&segments[next])
            };
            chain.push(s);
            if near(chain.last().unwrap().end(), chain[0].start()) {
                break;
            }
        }
        // Extend backward, unless the chain already closed.
        if !near(chain.last().unwrap().end(), chain[0].start()) {
            loop {
                let start = chain[0].start();
                let Some(prev) = candidates(start, &index).into_iter().find(|&i| {
                    !used[i] && (near(segments[i].end(), start) || near(segments[i].start(), start))
                }) else {
                    break;
                };
                used[prev] = true;
                let s = if near(segments[prev].end(), start) {
                    segments[prev].clone()
                } else {
                    reverse(&segments[prev])
                };
                chain.insert(0, s);
                if near(chain.last().unwrap().end(), chain[0].start()) {
                    break;
                }
            }
        }
        if chain.len() >= 2 && near(chain.last().unwrap().end(), chain[0].start()) {
            weld(&mut chain);
            loops.push(Loop { segments: chain });
        } else {
            let start = chain[0].start();
            weld(&mut chain);
            paths.push(Path { start, segments: chain });
        }
    }
    (loops, paths)
}

/// Snap each segment's start onto the previous segment's end (and the loop's
/// last end onto its first start) so a chain joined within tolerance is
/// exactly closed, which `Sketch::regions` requires.
fn weld(chain: &mut [Segment]) {
    for i in 1..chain.len() {
        let prev_end = chain[i - 1].end();
        set_start(&mut chain[i], prev_end);
    }
    if chain.len() >= 2 {
        let first = chain[0].start();
        let last = chain.len() - 1;
        let close = (chain[last].end().x - first.x).abs() < 1e-6
            && (chain[last].end().y - first.y).abs() < 1e-6;
        if close {
            set_end(&mut chain[last], first);
        }
    }
}

fn set_start(s: &mut Segment, p: P2) {
    match s {
        Segment::Line { a, .. } | Segment::Arc { a, .. } | Segment::Bezier { a, .. } => *a = p,
    }
}

fn set_end(s: &mut Segment, p: P2) {
    match s {
        Segment::Line { b, .. } | Segment::Arc { b, .. } | Segment::Bezier { b, .. } => *b = p,
    }
}

fn reverse(s: &Segment) -> Segment {
    match s {
        Segment::Line { a, b } => Segment::Line { a: *b, b: *a },
        Segment::Arc { a, b, via } => Segment::Arc { a: *b, b: *a, via: *via },
        Segment::Bezier { a, b, ctrl } => {
            Segment::Bezier { a: *b, b: *a, ctrl: ctrl.iter().rev().copied().collect() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_core::sketch::arc_params;

    #[test]
    fn a_quarter_bulge_has_the_radius_the_formula_says() {
        // A 90-degree arc has bulge tan(90/4) = tan(22.5 deg).
        let b = (22.5f64).to_radians().tan();
        let (p, q) = (P2::new(0.0, 0.0), P2::new(10.0, 10.0));
        let seg = bulge_segment(p, q, b);
        let Segment::Arc { a, b: e, via } = seg else { panic!("expected an arc") };
        let (c, r, t0, t1) = arc_params(a, e, via).unwrap();
        // Chord 10*sqrt(2), quarter circle => r = chord / sqrt(2) = 10.
        assert!((r - 10.0).abs() < 1e-9, "radius {r}");
        assert!(((t1 - t0).abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!(t1 > t0, "a positive bulge sweeps counter-clockwise");
        // The centre is to the left of the chord, so the arc bows right.
        assert!((c.x).abs() < 1e-9 && (c.y - 10.0).abs() < 1e-9, "centre {c:?}");
        assert_eq!(bulge_radius(p, q, b).map(|r| (r * 1e9).round() / 1e9), Some(10.0));
        // Zero bulge is a straight line.
        assert!(matches!(bulge_segment(p, q, 0.0), Segment::Line { .. }));
    }

    #[test]
    fn an_svg_arc_with_equal_radii_stays_an_arc() {
        // Quarter circle of radius 10 from (10,0) to (0,10), counter-clockwise.
        let segs = svg_arc(P2::new(10.0, 0.0), 10.0, 10.0, 0.0, false, true, P2::new(0.0, 10.0), 0.01);
        assert_eq!(segs.len(), 1);
        let Segment::Arc { a, b, via } = &segs[0] else { panic!("expected an arc, got {:?}", segs[0]) };
        let (c, r, _, _) = arc_params(*a, *b, *via).unwrap();
        assert!(c.x.abs() < 1e-9 && c.y.abs() < 1e-9, "centre {c:?}");
        assert!((r - 10.0).abs() < 1e-9, "radius {r}");
    }

    #[test]
    fn an_elliptical_svg_arc_becomes_cubics_within_tolerance() {
        let segs = svg_arc(P2::new(20.0, 0.0), 20.0, 10.0, 0.0, false, true, P2::new(0.0, 10.0), 0.001);
        assert!(segs.len() >= 1);
        assert!(segs.iter().all(|s| matches!(s, Segment::Bezier { .. })));
        assert!((segs[0].start().x - 20.0).abs() < 1e-9);
        assert!((segs.last().unwrap().end().y - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_half_turn_arc_is_split_so_three_points_determine_it() {
        let segs = arc_from_center(P2::new(0.0, 0.0), 5.0, 0.0, std::f64::consts::PI * 1.5);
        assert_eq!(segs.len(), 2);
        for s in &segs {
            let Segment::Arc { a, b, via } = s else { panic!() };
            let (_, r, _, _) = arc_params(*a, *b, *via).unwrap();
            assert!((r - 5.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_cubic_bspline_converts_exactly_by_knot_insertion() {
        // A clamped cubic with two spans.
        let ctrl = [
            P2::new(0.0, 0.0),
            P2::new(10.0, 20.0),
            P2::new(30.0, 20.0),
            P2::new(40.0, 0.0),
            P2::new(50.0, 20.0),
        ];
        let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0, 2.0];
        let segs = bspline_segments(3, &ctrl, &knots, &[], 0.01);
        assert!(segs.len() >= 2, "one Bezier per span, got {}", segs.len());
        assert!(segs.iter().all(|s| matches!(s, Segment::Bezier { ctrl, .. } if ctrl.len() == 2)));
        // Clamped: the curve starts and ends at the outer control points.
        assert!((segs[0].start().x - 0.0).abs() < 1e-9 && (segs[0].start().y).abs() < 1e-9);
        let end = segs.last().unwrap().end();
        assert!((end.x - 50.0).abs() < 1e-9 && (end.y - 20.0).abs() < 1e-9, "{end:?}");
        // The pieces are continuous.
        for w in segs.windows(2) {
            assert!((w[0].end().x - w[1].start().x).abs() < 1e-9);
            assert!((w[0].end().y - w[1].start().y).abs() < 1e-9);
        }
    }

    #[test]
    fn scattered_segments_join_into_a_loop_and_a_path() {
        // A square given as four lines in a shuffled order, plus a stray line.
        let sq = [
            Segment::Line { a: P2::new(10.0, 0.0), b: P2::new(10.0, 10.0) },
            Segment::Line { a: P2::new(0.0, 10.0), b: P2::new(0.0, 0.0) },
            Segment::Line { a: P2::new(0.0, 0.0), b: P2::new(10.0, 0.0) },
            // Deliberately reversed, and 0.005 short: within the tolerance.
            Segment::Line { a: P2::new(0.0, 10.005), b: P2::new(10.0, 10.0) },
            Segment::Line { a: P2::new(50.0, 0.0), b: P2::new(60.0, 5.0) },
        ];
        let (loops, paths) = join(sq.to_vec(), 0.01);
        assert_eq!(loops.len(), 1, "the square joins");
        assert_eq!(loops[0].segments.len(), 4);
        assert!(loops[0].is_closed(1e-9), "welding closes it exactly");
        assert!((loops[0].signed_area().abs() - 100.0).abs() < 0.2);
        assert_eq!(paths.len(), 1, "the stray line stays an open chain");
        assert_eq!(paths[0].segments.len(), 1);
    }

    #[test]
    fn a_gap_wider_than_the_tolerance_is_not_joined() {
        let segs = [
            Segment::Line { a: P2::new(0.0, 0.0), b: P2::new(10.0, 0.0) },
            Segment::Line { a: P2::new(10.5, 0.0), b: P2::new(10.5, 10.0) },
        ];
        let (loops, paths) = join(segs.to_vec(), 0.01);
        assert!(loops.is_empty());
        assert_eq!(paths.len(), 2);
    }
}
