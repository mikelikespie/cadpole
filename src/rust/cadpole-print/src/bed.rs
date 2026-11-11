//! Bed fit: will it go in the machine, and if not, by how much does it miss?
//!
//! Measured in the printer's frame, not the model's: two axes across the plate
//! and one along the build direction. When the build direction is +Z (the
//! normal case) that is just the bounding box against `bed`. The footprint is
//! also tried turned 90° on the plate, because "it fits if you turn it" is a
//! different answer from "it does not fit", and the diagonal is reported for
//! the same reason.

use cadpole_core::geom::Vec3;
use cadpole_core::tessellate::Mesh;

use crate::{BedShape, Check, Issue, Profile, Severity, num};

/// A cut that would make an oversize part fit, ready to be written as a
/// `split()` call.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitSuggestion {
    /// `"x"`, `"y"` or `"z"` in the *model's* frame — the axis to cut across
    /// first: the one that needs the most pieces.
    pub axis: &'static str,
    /// Where to cut, in model coordinates, along that axis. One entry per cut.
    pub cuts: Vec<f64>,
    /// How many pieces all the cuts make: the product of `grid`. Equal to
    /// `cuts.len() + 1` when only one axis needs cutting.
    pub pieces: usize,
    /// Equal pieces along the model's X, Y and Z. Every piece fits the build
    /// volume as laid out (on a round plate, its bounding-box diagonal fits
    /// the diameter).
    pub grid: [usize; 3],
    /// The Starlark that does it, e.g.
    /// `split(body, plane(origin=(0, 0, 128), normal=(0, 0, 1)))`.
    pub call: String,
}

impl SplitSuggestion {
    /// A sentence to append to a bed-fit diagnostic.
    pub fn advice(&self) -> String {
        let cut: Vec<String> = ["X", "Y", "Z"]
            .iter()
            .zip(self.grid)
            .filter(|(_, n)| *n > 1)
            .map(|(a, n)| format!("{n} across {a}"))
            .collect();
        if self.pieces == 2 {
            format!("; `{}` cuts it into two pieces that do fit", self.call)
        } else if cut.len() == 1 {
            format!("; cutting it into {} pieces would fit — start with `{}`", self.pieces, self.call)
        } else {
            format!(
                "; cutting it into {} pieces ({}) would fit — start with `{}`",
                self.pieces,
                cut.join(", "),
                self.call
            )
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BedReport {
    /// Extent across the plate and along the build direction, in mm.
    pub size: Vec3,
    /// The build volume it was measured against.
    pub bed: [f64; 3],
    pub bed_shape: BedShape,
    pub fits: bool,
    /// Fits once turned 90° on the plate.
    pub fits_rotated: bool,
    /// Fits when turned at some angle on the plate, across its diagonal
    /// (footprint only; a hint, not a placement — the slicer still has to
    /// orient it). The whole footprint is turned and measured, so a long part
    /// is not offered the diagonal when it is too wide to lie along it. On a
    /// circular plate this is the same test as `fits`, because a circle has no
    /// corners to reach into.
    pub fits_diagonal: bool,
    /// The turn, in degrees about the build direction, at which the footprint
    /// fits when only a turn other than 0 or 90 does; `None` otherwise.
    pub diagonal_degrees: Option<f64>,
    /// How far over the build volume it is on each axis, in mm; 0 where it
    /// fits. Same axis order as `size`.
    pub excess: Vec3,
    /// When it does not fit: where to cut it so it does.
    pub split: Option<SplitSuggestion>,
}

pub fn check(mesh: &Mesh, profile: &Profile, issues: &mut Vec<Issue>) -> BedReport {
    let up = profile.up();
    let (e1, e2) = plate_axes(up);
    let (Some(x), Some(y), Some(z)) = (extent(mesh, e1), extent(mesh, e2), extent(mesh, up)) else {
        return BedReport {
            bed: profile.bed,
            bed_shape: profile.bed_shape,
            fits: true,
            fits_rotated: true,
            fits_diagonal: true,
            ..Default::default()
        };
    };
    let (bx, by, bz) = (profile.bed[0], profile.bed[1], profile.bed[2]);
    // On a circular plate what has to fit is the footprint's smallest
    // enclosing circle: its bounding box's diagonal is right for a
    // rectangle, and up to sqrt(2) too big for anything round.
    let circle = match profile.bed_shape {
        BedShape::Rectangular => None,
        BedShape::Circular => Some(enclosing_diameter(&footprint(mesh, e1, e2))),
    };
    let footprint_fits = |a: f64, b: f64| match circle {
        None => a <= bx && b <= by,
        Some(d) => d <= bx,
    };
    let diagonal_degrees = match profile.bed_shape {
        BedShape::Rectangular if z <= bz && !(x <= bx && y <= by) && !(y <= bx && x <= by) => {
            fitting_turn(&footprint(mesh, e1, e2), bx, by)
        }
        _ => None,
    };
    let excess = match circle {
        None => Vec3::new((x - bx).max(0.0), (y - by).max(0.0), (z - bz).max(0.0)),
        // The plate has no X or Y of its own: both carry how far the
        // enclosing circle is over the plate's diameter.
        Some(d) => Vec3::new((d - bx).max(0.0), (d - bx).max(0.0), (z - bz).max(0.0)),
    };
    let mut r = BedReport {
        size: Vec3::new(x, y, z),
        bed: profile.bed,
        bed_shape: profile.bed_shape,
        fits: footprint_fits(x, y) && z <= bz,
        fits_rotated: footprint_fits(y, x) && z <= bz,
        fits_diagonal: match profile.bed_shape {
            BedShape::Rectangular => {
                diagonal_degrees.is_some() || ((x <= bx && y <= by) || (y <= bx && x <= by)) && z <= bz
            }
            BedShape::Circular => footprint_fits(x, y) && z <= bz,
        },
        excess,
        diagonal_degrees,
        split: None,
    };
    if r.fits {
        r.excess = Vec3::ZERO;
        return r;
    }
    if r.fits_rotated {
        issues.push(Issue::new(
            Check::BedFit,
            Severity::Warning,
            format!(
                "{} x {} x {} mm does not fit the {} x {} x {} mm bed as laid out, but does turned 90° on the plate",
                num(x),
                num(y),
                num(z),
                num(bx),
                num(by),
                num(bz)
            ),
            None,
            r.excess.x.max(r.excess.y).max(r.excess.z),
        ));
        return r;
    }
    r.split = suggest_split(mesh, profile, Vec3::new(x, y, z), circle);
    let mut over = Vec::new();
    match circle {
        None => {
            for (a, v) in [("X", r.excess.x), ("Y", r.excess.y)] {
                if v > 0.0 {
                    over.push(format!("{} mm on {a}", num(v)));
                }
            }
        }
        Some(d) => {
            if r.excess.x > 0.0 {
                over.push(format!(
                    "{} mm across the {} mm round plate (its footprint needs a {} mm circle)",
                    num(r.excess.x),
                    num(bx),
                    num(d)
                ));
            }
        }
    }
    if r.excess.z > 0.0 {
        over.push(format!("{} mm on Z", num(r.excess.z)));
    }
    let over = over.join(", ");
    issues.push(Issue::new(
        Check::BedFit,
        Severity::Error,
        format!(
            "{} x {} x {} mm does not fit the {} x {} x {} mm build volume: over by {}{}{}",
            num(x),
            num(y),
            num(z),
            num(bx),
            num(by),
            num(bz),
            over,
            r.diagonal_degrees
                .map(|t| format!(" (it would fit across the plate diagonal, turned {}°)", num(t)))
                .unwrap_or_default(),
            r.split.as_ref().map(SplitSuggestion::advice).unwrap_or_default(),
        ),
        None,
        r.excess.x.max(r.excess.y).max(r.excess.z),
    ));
    r
}

/// Where to cut an oversize part so the pieces fit.
///
/// Every axis that is over gets the fewest equal pieces that bring it within
/// the build volume, so the pieces fit on every axis at once, not just the
/// worst one; on a round plate the two plate counts are chosen together, the
/// fewest pieces whose bounding-box diagonal fits the diameter. The cut
/// offered first is across the axis that needs the most pieces. Only the
/// usual +Z build direction gets a suggestion: with a tilted build direction
/// the cut plane is not an axis plane and `split()` would need a rotated
/// `plane(...)`, which is more guessing than a diagnostic should do.
///
/// `circle` is the footprint's measured enclosing diameter on a round plate:
/// when it fits the plate, the part is over on Z only and is cut across Z
/// alone, whatever its bounding box's diagonal says.
fn suggest_split(mesh: &Mesh, profile: &Profile, size: Vec3, circle: Option<f64>) -> Option<SplitSuggestion> {
    if (profile.up() - Vec3::Z).length() > 1e-9 {
        return None;
    }
    let (lo, hi) = mesh.bbox()?;
    let [bx, by, bz] = profile.bed;
    if !(bx > 0.0 && by > 0.0 && bz > 0.0) || !(size.x.is_finite() && size.y.is_finite() && size.z.is_finite()) {
        return None;
    }
    let count = |extent: f64, limit: f64| (extent / limit).ceil().max(1.0) as usize;
    let (nx, ny) = match profile.bed_shape {
        BedShape::Rectangular => (count(size.x, bx), count(size.y, by)),
        BedShape::Circular if circle.is_some_and(|c| c <= bx) => (1, 1),
        BedShape::Circular => {
            // Pieces of w x h fit when hypot(w, h) <= D. Past
            // x / nx <= D / sqrt(2) a larger nx can never need fewer pieces.
            let d = bx;
            let most = count(size.x, d / std::f64::consts::SQRT_2) + 1;
            let mut best: Option<(usize, usize)> = None;
            for nx in 1..=most {
                let w = size.x / nx as f64;
                if w >= d {
                    continue;
                }
                let ny = count(size.y, (d * d - w * w).sqrt());
                if best.map(|(a, b)| nx * ny < a * b).unwrap_or(true) {
                    best = Some((nx, ny));
                }
            }
            best?
        }
    };
    let grid = [nx, ny, count(size.z, bz)];
    let pieces: usize = grid.iter().product();
    if pieces < 2 {
        return None;
    }
    let axes = [
        ("x", grid[0], size.x / bx, lo.x, hi.x, Vec3::X),
        ("y", grid[1], size.y / by, lo.y, hi.y, Vec3::Y),
        ("z", grid[2], size.z / bz, lo.z, hi.z, Vec3::Z),
    ];
    let (axis, n, _, a, b, normal) = axes.into_iter().max_by(|p, q| {
        p.1.cmp(&q.1).then(p.2.partial_cmp(&q.2).unwrap_or(std::cmp::Ordering::Equal))
    })?;
    let cuts: Vec<f64> = (1..n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
    let first = *cuts.first()?;
    let call = format!(
        "split(body, plane(origin=({}, {}, {}), normal=({}, {}, {})))",
        num(if axis == "x" { first } else { 0.0 }),
        num(if axis == "y" { first } else { 0.0 }),
        num(if axis == "z" { first } else { 0.0 }),
        num(normal.x),
        num(normal.y),
        num(normal.z),
    );
    Some(SplitSuggestion { axis, cuts, pieces, grid, call })
}

/// The vertices projected onto the plate, in the `(e1, e2)` frame.
fn footprint(mesh: &Mesh, e1: Vec3, e2: Vec3) -> Vec<[f64; 2]> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in mesh.positions.chunks_exact(3) {
        if seen.insert([p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]) {
            let v = Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
            out.push([v.dot(e1), v.dot(e2)]);
        }
    }
    out
}

/// Diameter of the smallest circle enclosing `pts`: Welzl's algorithm in its
/// iterative form, expected linear time once the points are shuffled (here
/// by a fixed permutation, so the answer never depends on a seed).
fn enclosing_diameter(pts: &[[f64; 2]]) -> f64 {
    if pts.is_empty() {
        return 0.0;
    }
    let mut p = pts.to_vec();
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    for i in (1..p.len()).rev() {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        p.swap(i, (state >> 33) as usize % (i + 1));
    }
    let d2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    let inside = |c: [f64; 2], r2: f64, q: [f64; 2]| d2(c, q) <= r2 * (1.0 + 1e-12) + 1e-12;
    let two = |a: [f64; 2], b: [f64; 2]| ([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5], d2(a, b) * 0.25);
    let three = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        let (bx, by) = (b[0] - a[0], b[1] - a[1]);
        let (cx, cy) = (c[0] - a[0], c[1] - a[1]);
        let d = 2.0 * (bx * cy - by * cx);
        if d.abs() < 1e-18 {
            // Collinear: the circle on the farthest pair.
            return [two(a, b), two(a, c), two(b, c)]
                .into_iter()
                .max_by(|u, v| u.1.partial_cmp(&v.1).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap();
        }
        let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
        let ux = (cy * b2 - by * c2) / d;
        let uy = (bx * c2 - cx * b2) / d;
        ([a[0] + ux, a[1] + uy], ux * ux + uy * uy)
    };
    let (mut c, mut r2) = (p[0], 0.0);
    for i in 1..p.len() {
        if inside(c, r2, p[i]) {
            continue;
        }
        (c, r2) = (p[i], 0.0);
        for j in 0..i {
            if inside(c, r2, p[j]) {
                continue;
            }
            (c, r2) = two(p[i], p[j]);
            for k in 0..j {
                if !inside(c, r2, p[k]) {
                    (c, r2) = three(p[i], p[j], p[k]);
                }
            }
        }
    }
    2.0 * r2.sqrt()
}

/// The smallest turn in `(0°, 180°)` at which the footprint fits a `bx` x
/// `by` plate, if any.
///
/// Measured on the footprint's convex hull, turned in quarter-degree steps
/// and kept only where the turned extents really are within the plate, so an
/// answer is always a placement that fits; a window narrower than a step can
/// be missed, which only loses a hint.
fn fitting_turn(pts: &[[f64; 2]], bx: f64, by: f64) -> Option<f64> {
    let hull = convex_hull(pts);
    if hull.is_empty() {
        return None;
    }
    let tol = 1e-9 * bx.max(by).max(1.0);
    (1..720).map(|i| i as f64 * 0.25).find(|deg| {
        let t = deg.to_radians();
        let (c, s) = (t.cos(), t.sin());
        let (mut u0, mut u1, mut v0, mut v1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for p in &hull {
            let u = p[0] * c - p[1] * s;
            let v = p[0] * s + p[1] * c;
            u0 = u0.min(u);
            u1 = u1.max(u);
            v0 = v0.min(v);
            v1 = v1.max(v);
        }
        u1 - u0 <= bx + tol && v1 - v0 <= by + tol
    })
}

/// Andrew's monotone chain: the hull's vertices, counter-clockwise.
fn convex_hull(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p = pts.to_vec();
    p.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<[f64; 2]> = Vec::with_capacity(2 * p.len());
    for pass in 0..2 {
        let start = h.len();
        let iter: Box<dyn Iterator<Item = &[f64; 2]>> =
            if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in iter {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 {
                h.pop();
            }
            h.push(q);
        }
        h.pop();
    }
    h
}

/// Two unit vectors across the plate, perpendicular to the build direction.
/// For the usual +Z build direction these are +X and +Y, so the numbers read
/// as the model's own X and Y.
fn plate_axes(up: Vec3) -> (Vec3, Vec3) {
    if (up - Vec3::Z).length() < 1e-9 {
        return (Vec3::X, Vec3::Y);
    }
    let a = if up.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let e1 = up.cross(a).normalized();
    (e1, up.cross(e1).normalized())
}

fn extent(mesh: &Mesh, axis: Vec3) -> Option<f64> {
    if mesh.positions.is_empty() {
        return None;
    }
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in mesh.positions.chunks_exact(3) {
        let h = Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64).dot(axis);
        lo = lo.min(h);
        hi = hi.max(h);
    }
    Some(hi - lo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    fn run(m: &Mesh, bed: [f64; 3]) -> (BedReport, Vec<Issue>) {
        let p = Profile { bed, ..Profile::default() };
        let mut issues = Vec::new();
        let r = check(m, &p, &mut issues);
        (r, issues)
    }

    #[test]
    fn a_small_part_fits_the_default_bed() {
        let (r, issues) = run(&box_mesh(100.0, 80.0, 60.0), [256.0, 256.0, 256.0]);
        assert!(r.fits);
        assert_eq!(r.excess, Vec3::ZERO);
        assert!(issues.is_empty());
    }

    #[test]
    fn an_oversize_part_reports_the_excess_per_axis() {
        // 300 x 100 x 300 against a 256 cube: 44 mm over on X and on Z.
        let (r, issues) = run(&box_mesh(300.0, 100.0, 300.0), [256.0, 256.0, 256.0]);
        assert!(!r.fits);
        assert!(!r.fits_rotated, "300 is over 256 whichever way round it goes");
        assert!((r.excess.x - 44.0).abs() < 1e-9, "{:?}", r.excess);
        assert_eq!(r.excess.y, 0.0);
        assert!((r.excess.z - 44.0).abs() < 1e-9, "{:?}", r.excess);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        let m = &issues[0].message;
        assert!(m.contains("44 mm on X"), "{m}");
        assert!(m.contains("44 mm on Z"), "{m}");
        assert!(!m.contains("on Y"), "{m}");
    }

    #[test]
    fn turning_it_ninety_degrees_is_reported_as_a_warning_not_an_error() {
        // 200 deep, 120 wide against a 150 x 250 plate.
        let (r, issues) = run(&box_mesh(200.0, 120.0, 50.0), [150.0, 250.0, 256.0]);
        assert!(!r.fits);
        assert!(r.fits_rotated);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert!(issues[0].message.contains("turned 90"), "{}", issues[0].message);
    }

    #[test]
    fn an_oversize_part_is_told_where_to_cut_it() {
        // 400 mm tall on a 256 cube: two 200 mm pieces fit.
        let (r, issues) = run(&box_mesh(100.0, 100.0, 400.0), [256.0, 256.0, 256.0]);
        let s = r.split.expect("a part that does not fit should be offered a split");
        assert_eq!(s.axis, "z");
        assert_eq!(s.pieces, 2);
        assert_eq!(s.cuts.len(), 1);
        assert!((s.cuts[0] - 200.0).abs() < 1e-9, "cut at {:?}", s.cuts);
        assert!(s.call.contains("normal=(0, 0, 1)"), "{}", s.call);
        assert!(issues[0].message.contains("split(body, plane("), "{}", issues[0].message);
    }

    #[test]
    fn a_part_three_times_too_long_is_told_to_make_three_pieces() {
        let (r, _) = run(&box_mesh(700.0, 50.0, 50.0), [256.0, 256.0, 256.0]);
        let s = r.split.expect("a split");
        assert_eq!(s.axis, "x");
        assert_eq!(s.pieces, 3);
        assert_eq!(s.cuts.len(), 2);
    }

    #[test]
    fn a_part_over_on_two_axes_is_not_promised_two_pieces() {
        // 50 x 50 on a 40 x 40 plate: halving either axis leaves 50 x 25,
        // still 10 mm over. It takes four pieces.
        let (r, issues) = run(&box_mesh(50.0, 50.0, 10.0), [40.0, 40.0, 40.0]);
        let s = r.split.expect("a split");
        assert_eq!(s.pieces, 4, "{s:?}");
        assert_eq!(s.grid, [2, 2, 1], "{s:?}");
        let m = &issues[0].message;
        assert!(!m.contains("two pieces that do fit"), "{m}");
        assert!(m.contains("4 pieces"), "{m}");
    }

    #[test]
    fn a_split_for_a_round_plate_makes_pieces_that_fit_the_circle() {
        // 180 x 180 on a 200 mm circle: halves are 90 x 180, whose diagonal
        // (201 mm) is still over, so it takes more than two pieces.
        let p = Profile { bed: [200.0, 200.0, 200.0], bed_shape: BedShape::Circular, ..Profile::default() };
        let mut issues = Vec::new();
        let r = check(&box_mesh(180.0, 180.0, 10.0), &p, &mut issues);
        let s = r.split.expect("a part that does not fit is offered a split");
        let (px, py) = (180.0 / s.grid[0] as f64, 180.0 / s.grid[1] as f64);
        assert!((px * px + py * py).sqrt() <= 200.0, "{s:?}");
        assert_eq!(s.pieces, s.grid.iter().product::<usize>());
        assert!(s.pieces > 2, "{s:?}");
    }

    #[test]
    fn a_part_that_fits_is_offered_no_split() {
        let (r, _) = run(&box_mesh(10.0, 10.0, 10.0), [256.0, 256.0, 256.0]);
        assert!(r.split.is_none());
    }

    #[test]
    fn a_circular_bed_measures_the_footprints_diagonal_not_its_sides() {
        // 200 x 200 fits inside a 220 mm square plate but its diagonal is
        // 283 mm, so it does not fit a 220 mm circle.
        let p = Profile { bed: [220.0, 220.0, 250.0], bed_shape: BedShape::Circular, ..Profile::default() };
        let mut issues = Vec::new();
        let r = check(&box_mesh(200.0, 200.0, 50.0), &p, &mut issues);
        assert!(!r.fits, "a 200 x 200 square does not fit a 220 mm circle");
        assert_eq!(r.bed_shape, BedShape::Circular);
        // A 150 mm square (diagonal 212) does.
        let mut issues = Vec::new();
        let r = check(&box_mesh(150.0, 150.0, 50.0), &p, &mut issues);
        assert!(r.fits, "a 150 x 150 square fits a 220 mm circle");
        assert!(issues.is_empty());
    }

    /// A regular `n`-gon of circumradius `r` in XY, `h` tall along Z.
    fn disc(r: f64, h: f64, n: usize) -> Mesh {
        let pts: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                (r * a.cos(), r * a.sin())
            })
            .collect();
        // A prism sweeps (x, z) along Y; swap Y and Z so the polygon lies on
        // the plate. The fit test does not care which way the triangles wind.
        let mut m = prism(&pts, &fan(n), h);
        for p in m.positions.chunks_exact_mut(3) {
            p.swap(1, 2);
        }
        m
    }

    #[test]
    fn a_round_part_is_measured_by_its_enclosing_circle_not_its_bounding_box() {
        let p = Profile { bed: [200.0, 200.0, 200.0], bed_shape: BedShape::Circular, ..Profile::default() };
        // A 198 mm disc and a 142 mm disc both fit a 200 mm round plate, though
        // their bounding-box diagonals are 280 and 201 mm.
        for r in [99.0, 71.0] {
            let mut issues = Vec::new();
            let rep = check(&disc(r, 10.0, 96), &p, &mut issues);
            assert!(rep.fits, "a {} mm disc fits a 200 mm circle: {issues:?}", 2.0 * r);
            assert!(issues.is_empty(), "{issues:?}");
        }
        // A 220 mm disc does not, and the message says by how much.
        let mut issues = Vec::new();
        let rep = check(&disc(110.0, 10.0, 96), &p, &mut issues);
        assert!(!rep.fits);
        let m = &issues[0].message;
        assert!(!m.contains("over by ;") && !m.trim_end().ends_with("over by"), "{m}");
        assert!(m.contains("over by 20 mm"), "{m}");
        assert!(rep.excess.x > 19.9 && rep.excess.x < 20.1, "{:?}", rep.excess);
        assert!(issues[0].value > 19.9, "{:?}", issues[0]);
        // A square whose sides are under the diameter but whose corners are
        // not still gets a number, not an empty list.
        let mut issues = Vec::new();
        let rep = check(&box_mesh(180.0, 180.0, 10.0), &p, &mut issues);
        assert!(!rep.fits);
        assert!(issues[0].message.contains("over by 54.56 mm"), "{}", issues[0].message);
    }

    /// A round part whose footprint fits the round plate but is too tall is
    /// cut across Z only: the plate cuts used to be sized from the bounding
    /// box's diagonal, so a 198 mm disc 300 mm tall was told to cut into 8.
    #[test]
    fn a_round_part_over_only_on_z_is_cut_across_z_only() {
        let p = Profile { bed: [200.0, 200.0, 200.0], bed_shape: BedShape::Circular, ..Profile::default() };
        let mut issues = Vec::new();
        let rep = check(&disc(99.0, 300.0, 96), &p, &mut issues);
        assert!(!rep.fits);
        let split = rep.split.expect("a split is offered");
        assert_eq!(split.grid, [1, 1, 2]);
        assert_eq!(split.pieces, 2);
        assert_eq!(split.axis, "z");
        assert!(issues[0].message.contains("two pieces that do fit"), "{}", issues[0].message);
    }

    #[test]
    fn the_diagonal_hint_needs_the_whole_footprint_to_fit_not_its_diagonal() {
        // On a 256 square plate a w x d rectangle fits at 45 degrees only if
        // (w + d) / sqrt(2) <= 256, i.e. w + d <= 362.04.
        for (d, fits) in [(200.0, false), (100.0, false), (63.0, false), (62.0, true), (40.0, true)] {
            let (r, issues) = run(&box_mesh(300.0, d, 10.0), [256.0, 256.0, 256.0]);
            assert_eq!(r.fits_diagonal, fits, "300 x {d}");
            assert_eq!(issues[0].message.contains("diagonal"), fits, "300 x {d}: {}", issues[0].message);
        }
    }

    #[test]
    fn the_diagonal_is_offered_when_it_is_the_only_way() {
        // 300 x 40 on a 256 square plate: the diagonal is 362 mm.
        let (r, issues) = run(&box_mesh(300.0, 40.0, 50.0), [256.0, 256.0, 256.0]);
        assert!(!r.fits && !r.fits_rotated && r.fits_diagonal);
        assert!(issues[0].message.contains("diagonal"), "{}", issues[0].message);
    }
}
