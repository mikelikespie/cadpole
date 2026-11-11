//! Turning a hatch pattern into line work.
//!
//! Scan-converting in a rotated frame is the whole trick: rotate the region so
//! the hatch lines are horizontal, walk `y` in steps of the pattern spacing,
//! intersect the scanline with every edge (the outer loop and its holes
//! together), sort the crossings and take them in pairs. Even-odd pairing is
//! exactly right here because Cadpole's loops are already nested by containment,
//! so a hole's crossings interleave with the outer loop's the way the rule
//! needs. Spacing is in *paper* millimetres and the loops arrive in paper
//! millimetres, so the pattern reads the same at every drawing scale.

use cadpole_core::sketch::{Loop, P2};

use crate::ir::Pattern;

/// Chord tolerance used when a hatch boundary has arcs in it. Finer than the
/// eye at any plot resolution, coarse enough that a filleted wall does not
/// produce a thousand-segment scanline.
const FLATTEN_TOLERANCE: f64 = 0.05;

/// Never emit more than this many hatch lines for one region. A pathological
/// spacing (or a region a thousand times bigger than expected) would otherwise
/// turn one export into gigabytes.
const MAX_LINES: usize = 20_000;

/// Never emit more than this many dots for one region, for the same reason
/// as [`MAX_LINES`].
const MAX_DOTS: usize = 200_000;

/// The centres of a dot screen over `outer` minus `holes`: a square grid of
/// `spacing_mm × scale` anchored to the origin, one dot per cell whose
/// centre is inside the region (even-odd, so the holes are skipped).
pub fn dots(outer: &Loop, holes: &[Loop], spacing_mm: f64, scale: f64) -> Vec<P2> {
    let s = spacing_mm * scale.max(1e-6);
    if !(s.is_finite() && s > 1e-6) {
        return Vec::new();
    }
    let loops: Vec<Vec<P2>> = std::iter::once(outer)
        .chain(holes)
        .map(|l| l.sample_tolerance(FLATTEN_TOLERANCE))
        .collect();
    let (mut lo, mut hi) = (P2::new(f64::MAX, f64::MAX), P2::new(f64::MIN, f64::MIN));
    for p in loops.iter().flatten() {
        lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    if !(lo.x.is_finite() && hi.x.is_finite()) || hi.x <= lo.x || hi.y <= lo.y {
        return Vec::new();
    }
    let (i0, i1) = ((lo.x / s).floor() as i64, (hi.x / s).ceil() as i64);
    let (j0, j1) = ((lo.y / s).floor() as i64, (hi.y / s).ceil() as i64);
    let cells = ((i1 - i0 + 1).max(0) as usize).saturating_mul((j1 - j0 + 1).max(0) as usize);
    if cells > MAX_DOTS * 4 {
        return Vec::new();
    }
    let inside = |p: P2| loops.iter().filter(|poly| point_in_polygon(poly, p)).count() % 2 == 1;
    let mut out = Vec::new();
    for j in j0..=j1 {
        for i in i0..=i1 {
            let p = P2::new((i as f64 + 0.5) * s, (j as f64 + 0.5) * s);
            if inside(p) {
                out.push(p);
                if out.len() >= MAX_DOTS {
                    return out;
                }
            }
        }
    }
    out
}

/// Even-odd point-in-polygon, half-open at the vertices so a grid point on
/// an edge is counted once.
fn point_in_polygon(poly: &[P2], p: P2) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// The hatch lines for `outer` minus `holes`, as paper-millimetre segments.
pub fn lines(outer: &Loop, holes: &[Loop], pattern: &Pattern, scale: f64) -> Vec<(P2, P2)> {
    let (angle, spacing) = match pattern {
        Pattern::Solid | Pattern::Dots { .. } => return Vec::new(),
        Pattern::Lines {
            angle_deg,
            spacing_mm,
        } => (*angle_deg, *spacing_mm),
        Pattern::Cross {
            angle_deg,
            spacing_mm,
        } => (*angle_deg, *spacing_mm),
    };
    let s = spacing * scale.max(1e-6);
    if !(s.is_finite() && s > 1e-6) {
        return Vec::new();
    }
    let mut edges: Vec<(P2, P2)> = Vec::new();
    for l in std::iter::once(outer).chain(holes) {
        let pts = l.sample_tolerance(FLATTEN_TOLERANCE);
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            if (a.x - b.x).abs() > 1e-12 || (a.y - b.y).abs() > 1e-12 {
                edges.push((a, b));
            }
        }
    }
    if edges.is_empty() {
        return Vec::new();
    }
    let mut out = family(&edges, angle, s);
    if matches!(pattern, Pattern::Cross { .. }) {
        out.extend(family(&edges, angle + 90.0, s));
    }
    out
}

/// One family of parallel lines at `angle_deg`, spacing `s`.
fn family(edges: &[(P2, P2)], angle_deg: f64, s: f64) -> Vec<(P2, P2)> {
    let a = angle_deg.to_radians();
    let (c, sn) = (a.cos(), a.sin());
    // Into the frame where the lines are horizontal, and back again.
    let fwd = |p: P2| P2::new(p.x * c + p.y * sn, -p.x * sn + p.y * c);
    let back = |p: P2| P2::new(p.x * c - p.y * sn, p.x * sn + p.y * c);

    let rot: Vec<(P2, P2)> = edges.iter().map(|(u, v)| (fwd(*u), fwd(*v))).collect();
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for (u, v) in &rot {
        lo = lo.min(u.y).min(v.y);
        hi = hi.max(u.y).max(v.y);
    }
    if !(lo.is_finite() && hi.is_finite()) || hi - lo < 1e-9 {
        return Vec::new();
    }
    // Anchor the family to the world origin, not to the region: two adjacent
    // hatched regions then share one continuous pattern, which is what a
    // drafter expects and what makes the output stable under translation.
    let first = (lo / s).ceil() * s;
    let count = ((hi - first) / s).floor() as i64;
    if count < 0 {
        return Vec::new();
    }
    // Clamp before the walk, not during it: a per-scanline check would keep
    // intersecting every edge long after the output was being thrown away, so
    // a pathological spacing would cost `MAX_LINES * edges` for nothing.
    let count = count.min(MAX_LINES as i64);
    let mut out = Vec::new();
    let mut xs: Vec<f64> = Vec::new();
    for i in 0..=count {
        let y = first + i as f64 * s;
        xs.clear();
        for (u, v) in &rot {
            let (p, q) = if u.y <= v.y { (*u, *v) } else { (*v, *u) };
            // Half-open in y so a vertex shared by two edges is counted once.
            if y >= p.y && y < q.y {
                let t = (y - p.y) / (q.y - p.y);
                xs.push(p.x + (q.x - p.x) * t);
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in xs.chunks_exact(2) {
            if pair[1] - pair[0] > 1e-9 {
                out.push((back(P2::new(pair[0], y)), back(P2::new(pair[1], y))));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_length(segs: &[(P2, P2)]) -> f64 {
        segs.iter()
            .map(|(a, b)| ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt())
            .sum()
    }

    #[test]
    fn a_square_hatches_to_roughly_its_area_over_the_spacing() {
        let sq = Loop::rect(P2::new(50.0, 50.0), 100.0, 100.0);
        let segs = lines(
            &sq,
            &[],
            &Pattern::Lines {
                angle_deg: 0.0,
                spacing_mm: 5.0,
            },
            1.0,
        );
        // 100 mm tall, one line every 5 mm, each 100 mm long.
        assert!((19..=21).contains(&segs.len()), "{} lines", segs.len());
        for (a, b) in &segs {
            assert!((b.x - a.x).abs() > 99.0, "full width: {a:?} {b:?}");
        }
        // Total inked length approximates area / spacing.
        let expected = 100.0 * 100.0 / 5.0;
        assert!((total_length(&segs) - expected).abs() / expected < 0.06);
    }

    #[test]
    fn holes_are_skipped() {
        let outer = Loop::rect(P2::new(0.0, 0.0), 100.0, 100.0);
        let hole = Loop::rect(P2::new(0.0, 0.0), 50.0, 50.0);
        let solid = lines(
            &outer,
            &[],
            &Pattern::Lines {
                angle_deg: 0.0,
                spacing_mm: 2.0,
            },
            1.0,
        );
        let holed = lines(
            &outer,
            &[hole.clone()],
            &Pattern::Lines {
                angle_deg: 0.0,
                spacing_mm: 2.0,
            },
            1.0,
        );
        let removed = total_length(&solid) - total_length(&holed);
        // The hole removes about its area divided by the spacing.
        assert!(
            (removed - 50.0 * 50.0 / 2.0).abs() / (50.0 * 50.0 / 2.0) < 0.08,
            "{removed}"
        );
        // And nothing is drawn inside it.
        for (a, b) in &holed {
            let mid = P2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            assert!(
                !hole.contains_point(mid),
                "hatch inside the hole at {mid:?}"
            );
        }
    }

    #[test]
    fn a_cross_pattern_draws_both_families() {
        let sq = Loop::rect(P2::new(0.0, 0.0), 40.0, 40.0);
        let one = lines(
            &sq,
            &[],
            &Pattern::Lines {
                angle_deg: 45.0,
                spacing_mm: 4.0,
            },
            1.0,
        );
        let two = lines(
            &sq,
            &[],
            &Pattern::Cross {
                angle_deg: 45.0,
                spacing_mm: 4.0,
            },
            1.0,
        );
        assert!(
            two.len() > one.len() * 3 / 2,
            "{} vs {}",
            two.len(),
            one.len()
        );
    }

    #[test]
    fn a_dot_screen_fills_the_region_and_skips_the_hole() {
        let outer = Loop::rect(P2::new(0.0, 0.0), 40.0, 40.0);
        let hole = Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0);
        let all = dots(&outer, &[], 2.0, 1.0);
        let holed = dots(&outer, &[hole.clone()], 2.0, 1.0);
        // 40 / 2 = 20 cells a side.
        assert!((380..=420).contains(&all.len()), "{}", all.len());
        assert!(holed.len() < all.len());
        for c in &holed {
            assert!(!hole.contains_point(*c), "a dot inside the hole at {c:?}");
            assert!(outer.contains_point(*c));
        }
    }

    #[test]
    fn scale_thins_the_pattern_and_solid_draws_nothing() {
        let sq = Loop::rect(P2::new(0.0, 0.0), 40.0, 40.0);
        let tight = lines(&sq, &[], &Pattern::ANSI31, 1.0);
        let loose = lines(&sq, &[], &Pattern::ANSI31, 2.0);
        assert!(loose.len() < tight.len());
        assert!(lines(&sq, &[], &Pattern::Solid, 1.0).is_empty());
    }

    #[test]
    fn the_pattern_is_anchored_to_the_origin_so_neighbours_line_up() {
        let left = Loop::rect(P2::new(-20.0, 0.0), 40.0, 40.0);
        let right = Loop::rect(P2::new(20.0, 0.0), 40.0, 40.0);
        let p = Pattern::Lines {
            angle_deg: 0.0,
            spacing_mm: 5.0,
        };
        let a = lines(&left, &[], &p, 1.0);
        let b = lines(&right, &[], &p, 1.0);
        let ys_a: Vec<i64> = a.iter().map(|(u, _)| (u.y * 1e6) as i64).collect();
        let ys_b: Vec<i64> = b.iter().map(|(u, _)| (u.y * 1e6) as i64).collect();
        assert_eq!(ys_a, ys_b, "adjacent regions share one continuous pattern");
    }
}
