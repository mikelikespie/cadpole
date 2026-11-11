//! Checks *between* bodies: what a multi-body print gets wrong that no body
//! gets wrong on its own.
//!
//! A print-in-place mechanism — a herringbone planetary, say — is several bodies exported
//! together and never unioned, and its correctness is the gaps between them:
//! big enough to print as separate parts, small enough to mesh. Every body
//! passes `print_check` alone; the print fails *between* them. Three rules,
//! each a finding with a place to look and the two faces involved:
//!
//! | check | rule | severity |
//! | --- | --- | --- |
//! | `fuse` | the minimum gap between two bodies is below [`Profile::min_gap`], or the bodies touch or overlap: they come off the printer as one part | error — the mechanism does not work |
//! | `layer_gap` | a horizontal gap (a face parallel to the plate over another body's face parallel to the plate) is not a whole number of layers, to ±[`LAYER_TOLERANCE`]: the slicer rounds it to the layer grid, up or down at its whim, and 0.3 mm becomes 0.2 (fused) or 0.4 (fine) | warning — it may print |
//! | `bridge` | a body that does not touch the bed has nothing under its lowest face within [`REACH`]: it prints in the air, and its own overhang check could not see that because a body checked alone takes its underside for the bed | warning |
//!
//! **Where the numbers come from.** FDM clearances: one
//! extrusion is 0.42–0.45 mm wide at a 0.4 mm nozzle and two walls closer
//! than about a third of that fuse; a vertical bore prints 0.1–0.15 mm
//! undersize and a 30 mm circle is out by ~0.1 mm radially, so 0.3 mm is the
//! gap that survives both, and the default. Axial gaps are checked to
//! ±0.02 mm of a layer multiple, and facing horizontal faces are paired up
//! to 2 mm apart.
//!
//! **What is not judged, on purpose.** A bridged gap that *is* whole layers
//! and above the fuse threshold gets no finding: whether the bridge prints
//! depends on its unsupported span, which needs the outline of the region
//! over air and a `max_bridge` the profile does not have, and on the slicer's
//! own bridge detection, which differs between slicers. A body whose lowest
//! point is on a curved face (a ball) has no "lowest face" to pair, and is
//! not judged either. Gaps between faces that are not parallel to the plate
//! are the `fuse` rule's business only: the slicer rounds heights, not
//! widths.

use cadpole_core::clearance::{self, Tree};
use cadpole_core::geom::Vec3;
use cadpole_core::tessellate::{Mesh, Topology};

use crate::{Check, Issue, Profile, Severity, centroid, fmt_at, num, tri, tri_normal_area};

/// How far a horizontal gap may be from a whole number of layers, in mm.
pub const LAYER_TOLERANCE: f64 = 0.02;

/// How far below a body's underside another body's top face is still paired
/// with it, in mm. Above this the pair is two parts on the plate, not a gap.
pub const REACH: f64 = 2.0;

/// Faces within this angle of the build plate count as horizontal.
const HORIZONTAL_DEGREES: f64 = 1.0;

/// A gap below this, in mm, is a touch: mesh positions are f32, so two
/// coincident faces can come back a fraction of a micron apart.
const TOUCH: f64 = 1e-4;

/// A horizontal face's triangles may differ in height by this much, in mm:
/// half the layer tolerance, so a face that passes is flat enough to judge.
const FLAT_EPS: f64 = LAYER_TOLERANCE / 2.0;

/// One body of the set, as the checks see it.
#[derive(Clone, Copy)]
pub struct BodyRef<'a> {
    pub name: &'a str,
    pub mesh: &'a Mesh,
    /// For face names; a finding says `face:N` without it.
    pub topo: Option<&'a Topology>,
}

impl BodyRef<'_> {
    /// The face's name, which carries the feature that made it
    /// (`sun/side[3]`); without a topology, `<body>/face:N` so the two
    /// sides of a pair are still told apart.
    fn face_name(&self, id: u32) -> String {
        match self.topo.and_then(|t| t.faces.get(id as usize)) {
            Some(f) => f.name.clone(),
            None => format!("{}/face:{id}", self.name),
        }
    }
}

/// One finding about the set. `body` is the index in the input the finding
/// attaches to (the first of a pair); `other` the second body when there is
/// one.
#[derive(Clone, Debug)]
pub struct Finding {
    pub body: usize,
    pub other: Option<usize>,
    pub issue: Issue,
}

/// Run the three rules over a set of bodies printed together. Fewer than two
/// bodies is nothing to check. `tolerance` is the tessellation tolerance,
/// reported as the uncertainty of every gap.
pub fn check(bodies: &[BodyRef<'_>], profile: &Profile, tolerance: f64) -> Vec<Finding> {
    let mut out = Vec::new();
    if bodies.len() < 2 {
        return out;
    }
    let trees: Vec<Tree> = bodies.iter().map(|b| Tree::build(b.mesh)).collect();
    fuse(bodies, &trees, profile, tolerance, &mut out);
    // A body the fuse rule caught is attached to something, rightly or
    // not; asking whether it is in the air would answer the wrong question.
    let mut fused = vec![false; bodies.len()];
    for f in &out {
        fused[f.body] = true;
        if let Some(o) = f.other {
            fused[o] = true;
        }
    }
    let faces = horizontal_faces(bodies, profile);
    layer_gaps(bodies, &faces, profile, &mut out);
    bridges(bodies, &faces, profile, &fused, &mut out);
    // Errors first, then the smallest gap: the one most likely to fuse leads.
    out.sort_by(|a, b| {
        b.issue.severity.cmp(&a.issue.severity).then(
            a.issue.value.partial_cmp(&b.issue.value).unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    out
}

// ---------------------------------------------------------------------------
// fuse
// ---------------------------------------------------------------------------

fn fuse(bodies: &[BodyRef<'_>], trees: &[Tree], profile: &Profile, tolerance: f64, out: &mut Vec<Finding>) {
    let min_gap = profile.min_gap;
    for i in 0..bodies.len() {
        for j in i + 1..bodies.len() {
            let (Some(bi), Some(bj)) = (trees[i].bounds(), trees[j].bounds()) else { continue };
            // Boxes further apart than the threshold cannot hold a gap below
            // it, and one body inside another's box is the case that matters.
            if clearance::box_distance(bi, bj) >= min_gap {
                continue;
            }
            let Some(c) = clearance::closest(&trees[i], &trees[j], tolerance) else { continue };
            let (a, b) = (bodies[i], bodies[j]);
            let faces = format!("{} and {}", a.face_name(c.face_a), b.face_name(c.face_b));
            let at = (c.a_point + c.b_point) * 0.5;
            let message = if c.overlap {
                format!(
                    "{} and {} overlap: they share material at {}, between {faces}; printed together they come out as one part",
                    a.name,
                    b.name,
                    fmt_at(at)
                )
            } else if c.distance < TOUCH {
                // Below f32's grain at any model size: the surfaces meet.
                format!(
                    "{} and {} touch at {}, between {faces}; printed together they fuse into one part (a gap of at least {} mm keeps them apart)",
                    a.name,
                    b.name,
                    fmt_at(at),
                    num(min_gap)
                )
            } else if c.distance < min_gap {
                format!(
                    "gap of {} mm between {} and {} at {}, between {faces}, is below the {} mm fuse threshold: printed together they come out as one part (±{} mm, the tessellation tolerance)",
                    num(c.distance),
                    a.name,
                    b.name,
                    fmt_at(at),
                    num(min_gap),
                    num(tolerance)
                )
            } else {
                continue;
            };
            out.push(Finding {
                body: i,
                other: Some(j),
                issue: Issue::new(Check::Fuse, Severity::Error, message, Some(at), c.distance),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// horizontal faces
// ---------------------------------------------------------------------------

/// A face parallel to the build plate, with its plan-view outline.
struct HFace {
    body: usize,
    face: u32,
    /// Normal along the build direction (a top) rather than against it (an
    /// underside).
    up: bool,
    /// Height along the build direction.
    height: f64,
    /// Plan-view bounds, in the basis `Frame` makes.
    lo: (f64, f64),
    hi: (f64, f64),
    /// The triangles, projected to plan view.
    tris: Vec<[(f64, f64); 3]>,
    /// Plan-view centroid, area-weighted.
    centroid: (f64, f64),
}

/// A basis across the build plate, so plan view is two numbers.
struct Frame {
    up: Vec3,
    e1: Vec3,
    e2: Vec3,
}

impl Frame {
    fn new(up: Vec3) -> Frame {
        let seed = if up.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let e1 = seed.cross(up).normalized();
        let e2 = up.cross(e1).normalized();
        Frame { up, e1, e2 }
    }
    fn plan(&self, p: Vec3) -> (f64, f64) {
        (p.dot(self.e1), p.dot(self.e2))
    }
    fn lift(&self, (x, y): (f64, f64), height: f64) -> Vec3 {
        self.e1 * x + self.e2 * y + self.up * height
    }
}

fn horizontal_faces(bodies: &[BodyRef<'_>], profile: &Profile) -> Vec<HFace> {
    let frame = Frame::new(profile.up());
    let cos_limit = HORIZONTAL_DEGREES.to_radians().cos();
    let mut out = Vec::new();
    for (bi, b) in bodies.iter().enumerate() {
        let mesh = b.mesh;
        let n = mesh.triangle_count();
        if n == 0 {
            continue;
        }
        // Group by face id, in index order: the group is one topology face
        // (or a mesh body's face group), and it counts only when every one of
        // its triangles is horizontal at one height.
        let ids = |t: usize| mesh.face_ids.get(t).copied().unwrap_or(0);
        let mut t = 0;
        while t < n {
            let id = ids(t);
            let start = t;
            while t < n && ids(t) == id {
                t += 1;
            }
            let mut sign: Option<bool> = None;
            let mut height = (f64::INFINITY, f64::NEG_INFINITY);
            let mut tris = Vec::new();
            let mut ok = true;
            let (mut cx, mut cy, mut area_sum) = (0.0, 0.0, 0.0);
            for k in start..t {
                let v = tri(mesh, k);
                let (nrm, area) = tri_normal_area(&v);
                if area <= 0.0 {
                    continue;
                }
                let s = nrm.dot(frame.up);
                if s.abs() < cos_limit {
                    ok = false;
                    break;
                }
                let up = s > 0.0;
                if sign.is_some_and(|prev| prev != up) {
                    ok = false;
                    break;
                }
                sign = Some(up);
                for p in &v {
                    let h = p.dot(frame.up);
                    height.0 = height.0.min(h);
                    height.1 = height.1.max(h);
                }
                let c = frame.plan(centroid(&v));
                cx += c.0 * area;
                cy += c.1 * area;
                area_sum += area;
                tris.push([frame.plan(v[0]), frame.plan(v[1]), frame.plan(v[2])]);
            }
            let Some(up) = sign else { continue };
            if !ok || tris.is_empty() || height.1 - height.0 > FLAT_EPS {
                continue;
            }
            let mut lo = (f64::INFINITY, f64::INFINITY);
            let mut hi = (f64::NEG_INFINITY, f64::NEG_INFINITY);
            for tr in &tris {
                for p in tr {
                    lo = (lo.0.min(p.0), lo.1.min(p.1));
                    hi = (hi.0.max(p.0), hi.1.max(p.1));
                }
            }
            out.push(HFace {
                body: bi,
                face: id,
                up,
                height: (height.0 + height.1) * 0.5,
                lo,
                hi,
                tris,
                centroid: (cx / area_sum, cy / area_sum),
            });
        }
    }
    out
}

/// Do two plan-view outlines share area? Their boxes first; then a centroid
/// of one inside a triangle of the other, either way; then a pair of edges
/// that cross, which is the case two offset squares of one size present.
fn plan_overlap(a: &HFace, b: &HFace) -> bool {
    if a.lo.0 > b.hi.0 || b.lo.0 > a.hi.0 || a.lo.1 > b.hi.1 || b.lo.1 > a.hi.1 {
        return false;
    }
    let inside = |p: (f64, f64), t: &[(f64, f64); 3]| {
        let s = |u: (f64, f64), v: (f64, f64)| (v.0 - u.0) * (p.1 - u.1) - (v.1 - u.1) * (p.0 - u.0);
        let (d0, d1, d2) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
        (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
    };
    let tri_centroid = |t: &[(f64, f64); 3]| ((t[0].0 + t[1].0 + t[2].0) / 3.0, (t[0].1 + t[1].1 + t[2].1) / 3.0);
    for ta in &a.tris {
        let c = tri_centroid(ta);
        if b.tris.iter().any(|tb| inside(c, tb)) {
            return true;
        }
    }
    for tb in &b.tris {
        let c = tri_centroid(tb);
        if a.tris.iter().any(|ta| inside(c, ta)) {
            return true;
        }
    }
    let edges = |t: &[(f64, f64); 3]| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])];
    let cross = |p: (f64, f64), q: (f64, f64), r: (f64, f64), s: (f64, f64)| {
        let d = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        let (d1, d2, d3, d4) = (d(r, s, p), d(r, s, q), d(p, q, r), d(p, q, s));
        ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    };
    for ta in &a.tris {
        for tb in &b.tris {
            for (p, q) in edges(ta) {
                for (r, s) in edges(tb) {
                    if cross(p, q, r, s) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// A downward face of one body over an upward face of another, with the gap
/// between them: the pairs the layer and bridge rules judge.
struct Pair<'f> {
    down: &'f HFace,
    up: &'f HFace,
    gap: f64,
}

fn pairs<'f>(faces: &'f [HFace]) -> Vec<Pair<'f>> {
    let mut out = Vec::new();
    for down in faces.iter().filter(|f| !f.up) {
        for up in faces.iter().filter(|f| f.up && f.body != down.body) {
            let gap = down.height - up.height;
            if gap <= 0.0 || gap > REACH || !plan_overlap(down, up) {
                continue;
            }
            out.push(Pair { down, up, gap });
        }
    }
    out
}

/// Where to point at for a pair: between the two faces, over the smaller
/// one's centre.
fn pair_at(frame: &Frame, p: &Pair<'_>) -> Vec3 {
    let small = if area_of(p.down) <= area_of(p.up) { p.down } else { p.up };
    frame.lift(small.centroid, p.up.height + p.gap * 0.5)
}

fn area_of(f: &HFace) -> f64 {
    f.tris.iter().map(|t| ((t[1].0 - t[0].0) * (t[2].1 - t[0].1) - (t[2].0 - t[0].0) * (t[1].1 - t[0].1)).abs() * 0.5).sum()
}

// ---------------------------------------------------------------------------
// layer_gap
// ---------------------------------------------------------------------------

fn layer_gaps(bodies: &[BodyRef<'_>], faces: &[HFace], profile: &Profile, out: &mut Vec<Finding>) {
    let lh = profile.layer_height;
    if lh <= 0.0 {
        return;
    }
    let frame = Frame::new(profile.up());
    for p in pairs(faces) {
        // Below the fuse threshold the `fuse` rule owns the pair; a rounding
        // warning on a gap that will not survive is noise.
        if p.gap < profile.min_gap {
            continue;
        }
        let layers = p.gap / lh;
        let nearest = layers.round().max(1.0);
        if (p.gap - nearest * lh).abs() <= LAYER_TOLERANCE {
            continue;
        }
        let (below, above) = (layers.floor().max(1.0), layers.ceil());
        let (a, b) = (bodies[p.down.body], bodies[p.up.body]);
        let at = pair_at(&frame, &p);
        out.push(Finding {
            body: p.down.body.min(p.up.body),
            other: Some(p.down.body.max(p.up.body)),
            issue: Issue::new(
                Check::LayerGap,
                Severity::Warning,
                format!(
                    "gap of {} mm between {} and {} is {} layers at {} mm: the slicer rounds it to {} or {} mm at its whim, and the smaller may fuse; make it {} mm ({} layers)",
                    num(p.gap),
                    a.face_name(p.down.face),
                    b.face_name(p.up.face),
                    num(layers),
                    num(lh),
                    num(below * lh),
                    num(above * lh),
                    num(nearest * lh),
                    num(nearest),
                ),
                Some(at),
                p.gap,
            ),
        });
    }
}

// ---------------------------------------------------------------------------
// bridge
// ---------------------------------------------------------------------------

fn bridges(bodies: &[BodyRef<'_>], faces: &[HFace], profile: &Profile, fused: &[bool], out: &mut Vec<Finding>) {
    let up = profile.up();
    let lows: Vec<f64> = bodies.iter().map(|b| crate::overhang::lowest(b.mesh, up)).collect();
    let floor = lows.iter().copied().fold(f64::INFINITY, f64::min);
    if !floor.is_finite() {
        return;
    }
    let scale = bodies
        .iter()
        .filter_map(|b| b.mesh.bbox())
        .map(|(lo, hi)| (hi - lo).length())
        .fold(0.0f64, f64::max)
        .max(1e-6);
    let bed_eps = (scale * 1e-4).max(1e-6);
    let all = pairs(faces);
    for (i, b) in bodies.iter().enumerate() {
        let lo = lows[i];
        if fused[i] || !lo.is_finite() || lo - floor <= bed_eps {
            continue; // attached to another body, or on the plate
        }
        // The faces it would stand on: its undersides at its lowest height.
        // None means the underside is curved, and that is not judged.
        let lowest: Vec<&HFace> = faces.iter().filter(|f| f.body == i && !f.up && f.height - lo <= FLAT_EPS).collect();
        if lowest.is_empty() {
            continue;
        }
        let supported = all.iter().any(|p| lowest.iter().any(|f| std::ptr::eq(*f, p.down)));
        if supported {
            // Whether the bridge over that gap prints is its span's business,
            // which is not measured; the fuse and layer rules have already
            // judged the gap itself.
            continue;
        }
        let f = lowest[0];
        let frame = Frame::new(up);
        let at = frame.lift(f.centroid, lo);
        out.push(Finding {
            body: i,
            other: None,
            issue: Issue::new(
                Check::Bridge,
                Severity::Warning,
                format!(
                    "{} does not touch the bed ({} mm up) and nothing lies under {} within {} mm: it prints in the air; the body's own overhang check took that underside for the bed",
                    b.name,
                    num(lo - floor),
                    b.face_name(f.face),
                    num(REACH)
                ),
                Some(at),
                lo - floor,
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn run(bodies: &[(&str, &Mesh)], profile: &Profile) -> Vec<Finding> {
        let refs: Vec<BodyRef<'_>> = bodies.iter().map(|(n, m)| BodyRef { name: n, mesh: m, topo: None }).collect();
        check(&refs, profile, 0.05)
    }

    fn checks(f: &[Finding]) -> Vec<&'static str> {
        f.iter().map(|f| f.issue.check.as_str()).collect()
    }

    #[test]
    fn the_fuse_rule_fires_at_two_tenths_and_not_at_four() {
        let a = box_mesh(10.0, 10.0, 10.0);
        let near = box_between(v(10.2, 0.0, 0.0), v(20.2, 10.0, 10.0));
        let far = box_between(v(10.4, 0.0, 0.0), v(20.4, 10.0, 10.0));
        let p = Profile::default();
        assert_eq!(p.min_gap, 0.3, "the FDM default");
        let f = run(&[("a", &a), ("b", &near)], &p);
        assert_eq!(checks(&f), ["fuse"], "{f:?}");
        assert_eq!(f[0].issue.severity, Severity::Error);
        assert_eq!((f[0].body, f[0].other), (0, Some(1)));
        let m = &f[0].issue.message;
        assert!(m.contains("gap of 0.2 mm between a and b"), "{m}");
        assert!(m.contains("between a/face:") && m.contains(" and b/face:"), "faces named: {m}");
        assert!(m.contains("below the 0.3 mm fuse threshold"), "{m}");
        assert!(f[0].issue.at.is_some());
        assert!((f[0].issue.value - 0.2).abs() < 1e-5);
        let f = run(&[("a", &a), ("b", &far)], &p);
        assert!(f.is_empty(), "0.4 mm is enough: {f:?}");
        // And the threshold is the profile's.
        let strict = Profile { min_gap: 0.5, ..Profile::default() };
        assert_eq!(checks(&run(&[("a", &a), ("b", &far)], &strict)), ["fuse"]);
    }

    #[test]
    fn touching_and_overlapping_bodies_are_fuse_errors_that_say_which() {
        let a = box_mesh(10.0, 10.0, 10.0);
        let touching = box_between(v(10.0, 0.0, 0.0), v(20.0, 10.0, 10.0));
        let overlapping = box_between(v(8.0, 2.0, 2.0), v(18.0, 8.0, 8.0));
        let f = run(&[("a", &a), ("b", &touching)], &Profile::default());
        assert_eq!(checks(&f), ["fuse"]);
        assert!(f[0].issue.message.contains("a and b touch at"), "{}", f[0].issue.message);
        let f = run(&[("a", &a), ("b", &overlapping)], &Profile::default());
        assert_eq!(checks(&f), ["fuse"]);
        assert!(f[0].issue.message.contains("a and b overlap"), "{}", f[0].issue.message);
        assert_eq!(f[0].issue.value, 0.0);
    }

    #[test]
    fn the_layer_rule_fires_for_half_a_layer_and_not_for_a_whole_one() {
        let base = box_mesh(20.0, 20.0, 5.0);
        let half = box_between(v(5.0, 5.0, 5.5), v(15.0, 15.0, 10.0)); // 0.5 mm: 2.5 layers
        let whole = box_between(v(5.0, 5.0, 5.4), v(15.0, 15.0, 10.0)); // 0.4 mm: 2 layers
        let p = Profile::default();
        assert_eq!(p.layer_height, 0.2);
        let f = run(&[("base", &base), ("plate", &half)], &p);
        assert_eq!(checks(&f), ["layer_gap"], "{f:?}");
        assert_eq!(f[0].issue.severity, Severity::Warning);
        let m = &f[0].issue.message;
        assert!(m.contains("gap of 0.5 mm between plate/face:") && m.contains(" and base/face:"), "{m}");
        assert!(m.contains("2.5 layers at 0.2 mm"), "{m}");
        assert!(m.contains("rounds it to 0.4 or 0.6 mm"), "{m}");
        assert!(m.contains("make it 0.6 mm (3 layers)") || m.contains("make it 0.4 mm (2 layers)"), "{m}");
        let at = f[0].issue.at.unwrap();
        assert!((at.z - 5.25).abs() < 1e-9 && (at.x - 10.0).abs() < 1e-9, "between the faces: {at:?}");
        let f = run(&[("base", &base), ("plate", &whole)], &p);
        assert!(f.is_empty(), "two whole layers, bridged: {f:?}");
        // A gap that is not a whole number at one layer height is at another.
        let p3 = Profile { layer_height: 0.25, ..Profile::default() };
        assert!(run(&[("base", &base), ("plate", &half)], &p3).is_empty());
    }

    #[test]
    fn a_gap_below_the_fuse_threshold_is_the_fuse_rules_alone() {
        // 0.25 mm: not whole layers, but it will fuse first, and one finding
        // that says so is the useful one.
        let base = box_mesh(20.0, 20.0, 5.0);
        let plate = box_between(v(5.0, 5.0, 5.25), v(15.0, 15.0, 10.0));
        let f = run(&[("base", &base), ("plate", &plate)], &Profile::default());
        assert_eq!(checks(&f), ["fuse"], "{f:?}");
    }

    #[test]
    fn a_body_in_the_air_is_a_bridge_warning_and_a_body_over_another_is_not() {
        let base = box_mesh(20.0, 20.0, 5.0);
        // Floating well off the base, over nothing: alone it would be "on the
        // bed" by its own lowest point.
        let floating = box_between(v(30.0, 30.0, 5.0), v(40.0, 40.0, 10.0));
        let f = run(&[("base", &base), ("part", &floating)], &Profile::default());
        assert_eq!(checks(&f), ["bridge"], "{f:?}");
        assert_eq!(f[0].issue.severity, Severity::Warning);
        assert_eq!((f[0].body, f[0].other), (1, None));
        let m = &f[0].issue.message;
        assert!(m.contains("part does not touch the bed (5 mm up)"), "{m}");
        assert!(m.contains("prints in the air"), "{m}");
        // Two whole layers above the base: bridged, and its span is not
        // judged, so nothing.
        let bridged = box_between(v(5.0, 5.0, 5.4), v(15.0, 15.0, 10.0));
        assert!(run(&[("base", &base), ("part", &bridged)], &Profile::default()).is_empty());
        // 3 mm above the base: past the 2 mm reach, so nothing is "under" it.
        let high = box_between(v(5.0, 5.0, 8.0), v(15.0, 15.0, 12.0));
        assert_eq!(checks(&run(&[("base", &base), ("part", &high)], &Profile::default())), ["bridge"]);
    }

    #[test]
    fn a_curved_underside_is_not_judged() {
        // A torus floating over a slab has no lowest *face* to pair, and
        // guessing would be wrong either way: no finding.
        let base = box_mesh(60.0, 60.0, 5.0);
        let mut ring = torus_mesh(20.0, 5.0, 48, 24);
        for p in ring.positions.chunks_exact_mut(3) {
            p[0] += 30.0;
            p[1] += 30.0;
            p[2] += 11.0; // lowest point at z = 6, a millimetre up
        }
        let f = run(&[("base", &base), ("ring", &ring)], &Profile::default());
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn one_body_is_nothing_to_check_and_names_come_from_the_topology() {
        let a = box_mesh(10.0, 10.0, 10.0);
        assert!(run(&[("a", &a)], &Profile::default()).is_empty());
        let b = box_between(v(10.2, 0.0, 0.0), v(20.2, 10.0, 10.0));
        let mut topo = Topology::default();
        for k in 0..6 {
            topo.faces.push(cadpole_core::tessellate::FaceInfo { name: format!("side{k}"), ..Default::default() });
        }
        let refs = [
            BodyRef { name: "a", mesh: &a, topo: Some(&topo) },
            BodyRef { name: "b", mesh: &b, topo: Some(&topo) },
        ];
        let f = check(&refs, &Profile::default(), 0.05);
        assert_eq!(checks(&f), ["fuse"]);
        assert!(f[0].issue.message.contains("between side") && f[0].issue.message.contains(" and side"), "{}", f[0].issue.message);
        assert!(f[0].issue.message.contains("±0.05 mm"), "{}", f[0].issue.message);
    }

    #[test]
    fn findings_are_errors_first_then_the_smallest_gap() {
        let base = box_mesh(40.0, 40.0, 5.0);
        let fusing = box_between(v(1.0, 1.0, 5.2), v(9.0, 9.0, 10.0)); // fuse (0.2)
        let odd = box_between(v(11.0, 11.0, 5.5), v(19.0, 19.0, 10.0)); // layer_gap (0.5)
        let air = box_between(v(60.0, 60.0, 9.0), v(70.0, 70.0, 12.0)); // bridge (9 up)
        let f = run(&[("base", &base), ("a", &air), ("b", &odd), ("c", &fusing)], &Profile::default());
        assert_eq!(checks(&f), ["fuse", "layer_gap", "bridge"], "{f:?}");
    }
}
