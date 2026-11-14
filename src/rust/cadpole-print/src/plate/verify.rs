//! The layout's guarantees, checked on the real geometry after packing.
//!
//! The packer searches on a 1 mm raster; this module does not trust it. It
//! projects every placed piece's triangles onto the bed as they finally
//! stand and measures, exactly, what a layout promises:
//!
//! * [`GAP`]: no two pieces on a plate are closer than the spacing.
//! * [`REACH`]: every piece lies inside what every nozzle reaches.
//! * [`KEEP_OUT`]: no piece stands in an excluded corner of the plate or in
//!   the square kept for the prime tower.
//! * [`FILAMENTS`]: no plate of several pieces needs more filaments than the
//!   AMS has slots.
//! * [`ONCE`]: every piece is placed exactly once in every set of plates.
//! * [`PINNED`]: a piece pinned to a plate, a spin or a position stands
//!   there, unless a note says why it could not.
//!
//! [`check`] returns every breach; the planner packs again with a wider
//! margin when there is one, and says so in a note if that does not cure
//! it, so a plan is never handed on with pieces standing in each other.

use cadpole_core::geom::Transform;
use cadpole_core::tessellate::Mesh;

use super::{Options, Plan, PlanBody, Variant};

/// No two pieces on a plate are closer than `Options::spacing`.
pub const GAP: &str = "gap";
/// Every piece is inside `Options::region`.
pub const REACH: &str = "reach";
/// No piece stands in an excluded rectangle or the prime tower's square.
pub const KEEP_OUT: &str = "keep-out";
/// A plate of several pieces never needs more filaments than `Options::ams`.
pub const FILAMENTS: &str = "filaments";
/// Every piece is placed exactly once per set of plates.
pub const ONCE: &str = "once";
/// Pinned plates, spins and positions are honoured or explained.
pub const PINNED: &str = "pinned";

/// How much closer than asked two pieces may measure before it is a breach:
/// the meshes are single precision and are turned twice on the way here.
pub const SLACK: f64 = 1e-3;

/// How far past the spacing the closest pair of a plate is still measured,
/// in mm, for the report.
pub const LOOK: f64 = 3.0;

/// One broken guarantee.
#[derive(Clone, Debug, PartialEq)]
pub struct Breach {
    /// Which invariant: one of this module's names.
    pub invariant: &'static str,
    /// Index into `Plan::plates`.
    pub plate: usize,
    pub message: String,
}

type P = (f64, f64);

/// A piece seen from above: its triangles on the bed, and their box.
#[derive(Clone, Debug)]
pub struct Shape {
    pub tris: Vec<[P; 3]>,
    pub lo: P,
    pub hi: P,
}

impl Shape {
    /// `mesh` after `t`, projected onto the bed.
    pub fn of(mesh: &Mesh, t: &Transform) -> Shape {
        let m = mesh.transformed(t);
        let at = |i: u32| (m.positions[3 * i as usize] as f64, m.positions[3 * i as usize + 1] as f64);
        let all: Vec<[P; 3]> = m.indices.chunks_exact(3).map(|c| [at(c[0]), at(c[1]), at(c[2])]).collect();
        // A wall seen edge on is a segment on the edge of the faces it
        // joins: it adds nothing to the outline. A piece that is nothing
        // but walls keeps them.
        let seen: Vec<[P; 3]> = all.iter().copied().filter(|t| cross(sub(t[1], t[0]), sub(t[2], t[0])).abs() > 1e-9).collect();
        Shape::from_tris(if seen.is_empty() { all } else { seen })
    }

    /// The inside of the rectangle `[x0, y0, x1, y1]`: the rectangle less
    /// [`SLACK`] all round, so a piece standing against its edge is not in
    /// it.
    pub fn inside_of(r: [f64; 4]) -> Shape {
        Shape::rect([r[0] + SLACK, r[1] + SLACK, r[2] - SLACK, r[3] - SLACK])
    }

    /// The rectangle `[x0, y0, x1, y1]`.
    pub fn rect(r: [f64; 4]) -> Shape {
        let (a, b, c, d) = ((r[0], r[1]), (r[2], r[1]), (r[2], r[3]), (r[0], r[3]));
        Shape::from_tris(vec![[a, b, c], [a, c, d]])
    }

    pub fn from_tris(tris: Vec<[P; 3]>) -> Shape {
        let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
        for t in &tris {
            for p in t {
                lo = (lo.0.min(p.0), lo.1.min(p.1));
                hi = (hi.0.max(p.0), hi.1.max(p.1));
            }
        }
        Shape { tris, lo, hi }
    }
}

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn cross(a: P, b: P) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

/// The distance from `p` to the segment `a`-`b`.
fn point_segment(p: P, a: P, b: P) -> f64 {
    let (d, v) = (sub(b, a), sub(p, a));
    let len2 = d.0 * d.0 + d.1 * d.1;
    let t = if len2 > 0.0 { ((v.0 * d.0 + v.1 * d.1) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (x, y) = (v.0 - t * d.0, v.1 - t * d.1);
    (x * x + y * y).sqrt()
}

/// Whether the segments properly cross or touch.
fn segments_meet(a: P, b: P, c: P, d: P) -> bool {
    let (d1, d2) = (cross(sub(b, a), sub(c, a)), cross(sub(b, a), sub(d, a)));
    let (d3, d4) = (cross(sub(d, c), sub(a, c)), cross(sub(d, c), sub(b, c)));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) && (d1 != 0.0 || d2 != 0.0) && (d3 != 0.0 || d4 != 0.0)
}

fn segment_segment(a: P, b: P, c: P, d: P) -> f64 {
    if segments_meet(a, b, c, d) {
        return 0.0;
    }
    point_segment(a, c, d).min(point_segment(b, c, d)).min(point_segment(c, a, b)).min(point_segment(d, a, b))
}

fn inside(p: P, t: &[P; 3]) -> bool {
    let (a, b, c) = (cross(sub(t[1], t[0]), sub(p, t[0])), cross(sub(t[2], t[1]), sub(p, t[1])), cross(sub(t[0], t[2]), sub(p, t[2])));
    let area = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    area != 0.0 && ((a >= 0.0 && b >= 0.0 && c >= 0.0) || (a <= 0.0 && b <= 0.0 && c <= 0.0))
}

/// The distance between two triangles in the plane; 0 when they share a
/// point.
pub fn triangle_distance(s: &[P; 3], t: &[P; 3]) -> f64 {
    if inside(s[0], t) || inside(t[0], s) {
        return 0.0;
    }
    let mut d = f64::INFINITY;
    for i in 0..3 {
        for j in 0..3 {
            d = d.min(segment_segment(s[i], s[(i + 1) % 3], t[j], t[(j + 1) % 3]));
            if d == 0.0 {
                return 0.0;
            }
        }
    }
    d
}

/// The least distance between two shapes, exact for their triangles, when
/// it is under `limit`; anything at or over `limit` is returned as
/// infinity. 0 means they overlap or touch.
pub fn separation(a: &Shape, b: &Shape, limit: f64) -> f64 {
    let limit = limit.max(1e-9);
    // The window both shapes come near.
    let x0 = (a.lo.0.max(b.lo.0)) - limit;
    let y0 = (a.lo.1.max(b.lo.1)) - limit;
    let x1 = (a.hi.0.min(b.hi.0)) + limit;
    let y1 = (a.hi.1.min(b.hi.1)) + limit;
    if x0 > x1 || y0 > y1 {
        return f64::INFINITY;
    }
    let near = |t: &[P; 3]| {
        let lo = (t[0].0.min(t[1].0).min(t[2].0), t[0].1.min(t[1].1).min(t[2].1));
        let hi = (t[0].0.max(t[1].0).max(t[2].0), t[0].1.max(t[1].1).max(t[2].1));
        (lo.0 <= x1 && hi.0 >= x0 && lo.1 <= y1 && hi.1 >= y0).then_some((lo, hi))
    };
    // A grid over the window, B's triangles in every cell they come within
    // `limit` of, so each of A's meets only its neighbours.
    let g = limit.max(((x1 - x0) / 256.0).max((y1 - y0) / 256.0)).max(0.5);
    let (nx, ny) = ((((x1 - x0) / g).floor() as usize) + 1, (((y1 - y0) / g).floor() as usize) + 1);
    let cells = |lo: P, hi: P, grow: f64| {
        let cx = |x: f64| (((x - x0) / g).floor().max(0.0) as usize).min(nx - 1);
        let cy = |y: f64| (((y - y0) / g).floor().max(0.0) as usize).min(ny - 1);
        (cx(lo.0 - grow), cy(lo.1 - grow), cx(hi.0 + grow), cy(hi.1 + grow))
    };
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
    for (k, t) in b.tris.iter().enumerate() {
        if let Some((lo, hi)) = near(t) {
            let (i0, j0, i1, j1) = cells(lo, hi, limit);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    grid[j * nx + i].push(k as u32);
                }
            }
        }
    }
    let mut seen: Vec<u32> = vec![0; b.tris.len()];
    let mut best = f64::INFINITY;
    for (n, t) in a.tris.iter().enumerate() {
        let Some((lo, hi)) = near(t) else { continue };
        let (i0, j0, i1, j1) = cells(lo, hi, 0.0);
        for j in j0..=j1 {
            for i in i0..=i1 {
                for &k in &grid[j * nx + i] {
                    if seen[k as usize] == n as u32 + 1 {
                        continue;
                    }
                    seen[k as usize] = n as u32 + 1;
                    let d = triangle_distance(t, &b.tris[k as usize]);
                    if d < best {
                        best = d;
                        if best == 0.0 {
                            return 0.0;
                        }
                    }
                }
            }
        }
    }
    if best < limit { best } else { f64::INFINITY }
}

/// Each placement's shape on its plate.
pub fn shapes(bodies: &[PlanBody<'_>], plan: &Plan) -> Vec<Shape> {
    plan.placements
        .iter()
        .map(|pl| {
            let mesh = super::concat(plan.pieces[pl.piece].bodies.iter().map(|&b| bodies[b].mesh));
            Shape::of(&mesh, &pl.transform)
        })
        .collect()
}

/// Check every guarantee of `plan`. Also fills in each plate's
/// `closest`: the least distance between two of its pieces, when it is
/// under the spacing plus [`LOOK`].
pub fn check(bodies: &[PlanBody<'_>], plan: &mut Plan, opts: &Options) -> Vec<Breach> {
    let shapes = shapes(bodies, plan);
    let mut out = Vec::new();
    let look = opts.spacing + LOOK;
    for (p, plate) in plan.plates.iter_mut().enumerate() {
        let mut closest = f64::INFINITY;
        let name = |k: usize| plan.pieces[plan.placements[k].piece].name.clone();
        for (n, &i) in plate.placements.iter().enumerate() {
            for &j in &plate.placements[n + 1..] {
                let d = separation(&shapes[i], &shapes[j], look);
                closest = closest.min(d);
                if d < opts.spacing - SLACK {
                    out.push(Breach {
                        invariant: GAP,
                        plate: p,
                        message: format!("{} and {} are {:.2} mm apart, under the {} mm spacing", name(i), name(j), d, opts.spacing),
                    });
                }
            }
        }
        plate.closest = closest.is_finite().then_some(closest);
        if plate.oversize {
            continue;
        }
        for &i in &plate.placements {
            let s = &shapes[i];
            let r = opts.region;
            if s.lo.0 < r[0] - SLACK || s.lo.1 < r[1] - SLACK || s.hi.0 > r[2] + SLACK || s.hi.1 > r[3] + SLACK {
                out.push(Breach {
                    invariant: REACH,
                    plate: p,
                    message: format!(
                        "{} spans x {:.1}..{:.1}, y {:.1}..{:.1}, outside {:?}",
                        name(i),
                        s.lo.0,
                        s.hi.0,
                        s.lo.1,
                        s.hi.1,
                        r
                    ),
                });
            }
            for keep in opts.exclude.iter().chain(plate.tower.iter()) {
                if separation(s, &Shape::inside_of(*keep), 1.0) == 0.0 {
                    out.push(Breach { invariant: KEEP_OUT, plate: p, message: format!("{} stands in {:?}", name(i), keep) });
                }
            }
        }
        let mono = plate.variant == Some(Variant::Mono);
        if !mono && opts.ams > 0 && plate.colours.len() as u32 > opts.ams && plate.placements.len() > 1 {
            out.push(Breach {
                invariant: FILAMENTS,
                plate: p,
                message: format!("{} filaments on a plate of {} slots", plate.colours.len(), opts.ams),
            });
        }
        if !mono && plate.colours.len() > 1 && opts.tower.is_some() && plate.tower.is_none() {
            out.push(Breach { invariant: KEEP_OUT, plate: p, message: "several filaments and no square kept for the prime tower".to_owned() });
        }
    }
    // Once per set: a set is the coupons' plates, or one variant's.
    let mut sets: Vec<Option<Variant>> = Vec::new();
    for plate in &plan.plates {
        if !sets.contains(&plate.variant) {
            sets.push(plate.variant);
        }
    }
    for set in sets {
        for (i, piece) in plan.pieces.iter().enumerate() {
            let n = plan.placements.iter().filter(|pl| pl.piece == i && plan.plates[pl.plate].variant == set).count();
            let want = usize::from(piece.pin.coupon == set.is_none());
            if n != want {
                out.push(Breach { invariant: ONCE, plate: 0, message: format!("{} is placed {n} times in a set, not {want}", piece.name) });
            }
        }
    }
    for pl in &plan.placements {
        let piece = &plan.pieces[pl.piece];
        if plan.unpinned.contains(&pl.piece) {
            continue;
        }
        if let Some(a) = piece.pin.spin {
            if ((pl.spin - a + 180.0).rem_euclid(360.0) - 180.0).abs() > 1e-6 {
                out.push(Breach { invariant: PINNED, plate: pl.plate, message: format!("{} is turned {}°, pinned to {a}°", piece.name, pl.spin) });
            }
        }
        if piece.pin.no_spin && pl.spin != 0.0 {
            out.push(Breach { invariant: PINNED, plate: pl.plate, message: format!("{} is turned {}°, pinned to no turn", piece.name, pl.spin) });
        }
        if let Some((x, y)) = piece.pin.at {
            let f = pl.footprint;
            let (cx, cy) = ((f[0] + f[2]) * 0.5, (f[1] + f[3]) * 0.5);
            if (cx - x).abs() > opts.cell || (cy - y).abs() > opts.cell {
                out.push(Breach {
                    invariant: PINNED,
                    plate: pl.plate,
                    message: format!("{} is centred at ({cx:.1}, {cy:.1}), pinned to ({x}, {y})", piece.name),
                });
            }
        }
        if let Some(n) = piece.pin.plate {
            if plan.plates[pl.plate].number != n {
                out.push(Breach {
                    invariant: PINNED,
                    plate: pl.plate,
                    message: format!("{} is on plate {} of its set, pinned to {n}", piece.name, plan.plates[pl.plate].number),
                });
            }
        }
    }
    out
}
