//! Fill rules: turning overlapping loops into the non-overlapping loops that
//! Cadpole's even-odd *containment* nesting needs.
//!
//! [`Sketch::regions`](crate::sketch::Sketch::regions) classifies loops by
//! containment: outermost is material, one level in is a hole, and so on.
//! That is exactly right when loops do not overlap, and it agrees with both
//! SVG fill rules in that case. It is wrong the moment two loops cross, which
//! real input does:
//!
//! * an Illustrator `<path>` whose subpaths overlap under `fill-rule:evenodd`,
//! * a glyph built from overlapping strokes, a heavy variable-font instance,
//!   or a composite glyph whose components touch — all defined for `nonzero`.
//!
//! [`rebuild`] runs the crossing loops through a 2D boolean under the rule the
//! source intended and hands back a set of non-overlapping loops, after which
//! containment nesting is exact. The result is polygonal: the rebuild is the
//! point at which a curve stops being a curve, so callers that can prove the
//! loops do not cross ([`loops_cross`]) should skip it and keep their arcs.

use i_overlay::core::fill_rule::FillRule as IFillRule;
use i_overlay::float::simplify::SimplifyShape;

use crate::sketch::{Loop, P2};

/// How to decide which parts of a self-overlapping set of loops are filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    /// SVG's default and the rule every font is drawn for: a point is inside
    /// when the signed crossing count around it is not zero, so two loops
    /// wound the same way merge instead of cancelling.
    NonZero,
    /// A point is inside when the crossing count is odd, so overlapping loops
    /// punch holes in each other. Common in Illustrator exports.
    EvenOdd,
}

impl FillRule {
    fn to_i_overlay(self) -> IFillRule {
        match self {
            FillRule::NonZero => IFillRule::NonZero,
            FillRule::EvenOdd => IFillRule::EvenOdd,
        }
    }

    /// `"nonzero"` / `"evenodd"` as SVG spells them.
    pub fn from_name(s: &str) -> Option<FillRule> {
        match s {
            "nonzero" => Some(FillRule::NonZero),
            "evenodd" | "even-odd" => Some(FillRule::EvenOdd),
            _ => None,
        }
    }
}

/// Rebuild `loops` under `rule` into non-overlapping loops.
///
/// Curved segments are flattened to within `tolerance` first, so the result is
/// always polygonal. Loops come back outer-then-holes, shape after shape,
/// which is the order containment nesting expects. Degenerate output (fewer
/// than three points, or zero area) is dropped.
pub fn rebuild(loops: &[Loop], rule: FillRule, tolerance: f64) -> Vec<Loop> {
    let contours: Vec<Vec<[f64; 2]>> = loops
        .iter()
        .map(|l| l.sample_tolerance(tolerance).iter().map(|p| [p.x, p.y]).collect())
        .filter(|c: &Vec<[f64; 2]>| c.len() >= 3)
        .collect();
    if contours.is_empty() {
        return Vec::new();
    }
    // i64 rather than the default i32 engine: glyph outlines arrive in font
    // units (2048/em is common) and floor plans in millimetres of a building,
    // and the integer engine's usable range is only a quarter of its type.
    let shapes = contours.simplify_shape_as::<i64>(rule.to_i_overlay());
    let mut out = Vec::new();
    for shape in shapes {
        for contour in shape {
            if contour.len() < 3 {
                continue;
            }
            let pts: Vec<P2> = contour.iter().map(|p| P2::new(p[0], p[1])).collect();
            let l = Loop::polygon(&pts);
            if l.signed_area().abs() > 0.0 {
                out.push(l);
            }
        }
    }
    out
}

/// True when any two of `loops` actually cross, so the fill rule matters.
///
/// Cheap first (bounding boxes), then a segment-intersection test on the
/// flattened polygons. Nesting — one loop entirely inside another — is *not*
/// crossing: that is the ordinary hole case both fill rules agree on.
pub fn loops_cross(loops: &[Loop], tolerance: f64) -> bool {
    let polys: Vec<Vec<P2>> = loops.iter().map(|l| l.sample_tolerance(tolerance)).collect();
    let boxes: Vec<(f64, f64, f64, f64)> = polys.iter().map(|p| bbox(p)).collect();
    for i in 0..polys.len() {
        for j in i + 1..polys.len() {
            if !boxes_overlap(boxes[i], boxes[j]) {
                continue;
            }
            if polylines_cross(&polys[i], &polys[j]) {
                return true;
            }
        }
    }
    false
}

fn bbox(p: &[P2]) -> (f64, f64, f64, f64) {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for q in p {
        b.0 = b.0.min(q.x);
        b.1 = b.1.min(q.y);
        b.2 = b.2.max(q.x);
        b.3 = b.3.max(q.y);
    }
    b
}

fn boxes_overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

/// Do two closed polylines overlap in a way the fill rule has to settle?
///
/// Two tests, because neither alone is enough. A proper segment crossing
/// catches a corner poking through an edge. A boundary that runs partly
/// inside and partly outside the other loop catches the case where the two
/// only ever *touch* at vertices — two equal-height rectangles side by side
/// share their top and bottom lines and never properly cross, yet their
/// interiors plainly overlap.
fn polylines_cross(a: &[P2], b: &[P2]) -> bool {
    // A quadratic test, capped: past this size the caller is better off
    // rebuilding unconditionally than paying for the question.
    if a.len() * b.len() > 4_000_000 {
        return true;
    }
    for i in 0..a.len() {
        let (p1, p2) = (a[i], a[(i + 1) % a.len()]);
        for j in 0..b.len() {
            let (q1, q2) = (b[j], b[(j + 1) % b.len()]);
            if segments_cross(p1, p2, q1, q2) {
                return true;
            }
        }
    }
    straddles(a, b) || straddles(b, a)
}

/// True when some of `a`'s boundary lies inside `b` and some outside, which
/// is exactly "the boundaries cross" without relying on where the vertices
/// happen to fall. Edge midpoints are used rather than vertices because a
/// shared vertex is ambiguous for a containment test and a midpoint is not.
fn straddles(a: &[P2], b: &[P2]) -> bool {
    let (mut inside, mut outside) = (false, false);
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        let m = P2::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
        if point_in_polygon(m, b) {
            inside = true;
        } else {
            outside = true;
        }
        if inside && outside {
            return true;
        }
    }
    false
}

fn point_in_polygon(p: P2, poly: &[P2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (pi, pj) = (poly[i], poly[j]);
        if (pi.y > p.y) != (pj.y > p.y) && p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn cross(o: P2, a: P2, b: P2) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Proper crossing only: touching endpoints and collinear overlaps do not
/// count, because shared vertices are how legitimately adjacent contours meet.
fn segments_cross(p1: P2, p2: P2, q1: P2, q2: P2) -> bool {
    let d1 = cross(q1, q2, p1);
    let d2 = cross(q1, q2, p2);
    let d3 = cross(p1, p2, q1);
    let d4 = cross(p1, p2, q2);
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0)) && d1 != 0.0 && d2 != 0.0 && d3 != 0.0 && d4 != 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(cx: f64, cy: f64, s: f64) -> Loop {
        Loop::rect(P2::new(cx, cy), s, s)
    }

    #[test]
    fn two_overlapping_squares_union_under_nonzero() {
        let loops = [square(0.0, 0.0, 10.0), square(6.0, 0.0, 10.0)];
        assert!(loops_cross(&loops, 0.01));
        let out = rebuild(&loops, FillRule::NonZero, 0.01);
        assert_eq!(out.len(), 1, "nonzero merges them into one loop");
        // Area of the union: 100 + 100 - the 4 x 10 overlap.
        let a = out[0].signed_area().abs();
        assert!((a - 160.0).abs() < 1e-6, "union area {a}");
    }

    #[test]
    fn two_overlapping_squares_punch_a_hole_under_evenodd() {
        let loops = [square(0.0, 0.0, 10.0), square(6.0, 0.0, 10.0)];
        let out = rebuild(&loops, FillRule::EvenOdd, 0.01);
        // The 4 x 10 overlap is crossed twice, so it is *not* filled: the
        // union's 160 minus that 40.
        let filled = 160.0 - 40.0;
        // Even-odd may return one or two shapes; their signed areas net to the
        // filled area either way.
        let net: f64 = out.iter().map(|l| l.signed_area()).sum::<f64>().abs();
        assert!((net - filled).abs() < 1e-6, "even-odd filled area {net}, loops {}", out.len());
    }

    #[test]
    fn nesting_is_not_crossing_and_survives_a_rebuild() {
        let loops = [square(0.0, 0.0, 20.0), square(0.0, 0.0, 6.0)];
        assert!(!loops_cross(&loops, 0.01), "a loop inside another does not cross it");
        let out = rebuild(&loops, FillRule::EvenOdd, 0.01);
        assert_eq!(out.len(), 2, "outer plus hole");
        let net: f64 = out.iter().map(|l| l.signed_area()).sum::<f64>().abs();
        assert!((net - (400.0 - 36.0)).abs() < 1e-6, "{net}");
    }

    #[test]
    fn disjoint_loops_come_back_unchanged_in_count() {
        let loops = [square(0.0, 0.0, 4.0), square(50.0, 0.0, 4.0)];
        assert!(!loops_cross(&loops, 0.01));
        let out = rebuild(&loops, FillRule::NonZero, 0.01);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn a_circle_survives_the_rebuild_within_tolerance() {
        let c = Loop::circle(P2::new(0.0, 0.0), 10.0);
        let out = rebuild(std::slice::from_ref(&c), FillRule::NonZero, 0.001);
        assert_eq!(out.len(), 1);
        let a = out[0].signed_area().abs();
        let exact = std::f64::consts::PI * 100.0;
        assert!((a - exact).abs() / exact < 1e-3, "area {a} vs {exact}");
    }

    #[test]
    fn fill_rule_names_match_svg() {
        assert_eq!(FillRule::from_name("nonzero"), Some(FillRule::NonZero));
        assert_eq!(FillRule::from_name("evenodd"), Some(FillRule::EvenOdd));
        assert_eq!(FillRule::from_name("wat"), None);
    }
}
