//! Contact handling: what to do when two boundaries do not just *meet* but
//! lie on top of each other (docs/design.md).
//!
//! [`crate::kernel::disposition`] answers the disjoint cases before any
//! boolean is attempted. What it calls
//! [`crate::kernel::Disposition::Meeting`] cannot simply go to
//! `truck_shapeops`, which implements *transversal* booleans: it intersects
//! the two boundaries, cuts every face along the intersection curves and keeps
//! the pieces on the wanted side. A pair of faces that are **coplanar** hands
//! it a 2-dimensional intersection to cut along, which its algorithm has no
//! representation for, so it returns `None` or panics. Measured, not assumed:
//! every configuration below fails inside truck, and the tests in
//! `kernel::tests` are the three the Gridfinity bin actually builds.
//!
//! So a meeting pair is classified once more here, and three operations
//! answer it:
//!
//! 1. [`Plan::Glue`] — an additive pair whose operands *touch* on coincident
//!    faces and nowhere else. The union is topological: the touching faces are
//!    merged and the two shells are stitched into one. No `shapeops` call, no
//!    surface recomputed, no tolerance in the answer.
//! 2. [`Plan::ExtendTool`] — a subtract or an intersect whose **tool** has a
//!    face coplanar with the body's. The tool is grown along that face's own
//!    normal, and only when the material it gains provably misses the body, so
//!    the answer cannot change. Additive geometry is never moved.
//! 3. [`Plan::Refuse`] — everything else: a tangent pair, a partial
//!    coincidence, a flush additive contact whose operands also share volume,
//!    and a coincident pair with material *between* its two planes — a floor
//!    or a wall thinner than the tolerance, which is a feature the script
//!    asked for and the kernel cannot see. The diagnostic names the face
//!    pair, both boxes, the overlap on each axis and the way out.

use std::collections::HashMap;

use truck_modeling::{Curve, Face, Point3, Shell, Solid, Surface, Vertex, Wire};

use crate::geom::{Transform, Vec3};
use crate::kernel::{self, BoolOp, KSolid};
use crate::tangent::{self, inside_face, point_segment_distance};
use crate::{CadpoleError, Result};

/// Which way two coincident faces look: [`crate::tangent::Sense`], the
/// classifier's vocabulary, which this module shares.
pub use crate::tangent::Sense;

/// How close two planes have to be before they count as the same plane.
///
/// Exactly the tolerance the boolean itself will run at, and for one reason:
/// two planes closer than that are a pair `shapeops` cannot separate, which is
/// precisely the pair we have to answer ourselves. It follows that a feature
/// finer than the document's boolean tolerance reads as a coincidence and is
/// refused rather than guessed at — and that asking for a finer tolerance is
/// what resolves it, which is the behaviour a tolerance ought to have.
fn coincidence_tolerance(tol: f64) -> f64 {
    if tol > 0.0 { tol } else { kernel::DEFAULT_BOOLEAN_TOLERANCE }
}

/// How far a tool is grown past a coplanar face.
///
/// It has to be comfortably more than the tolerance `shapeops` separates
/// surfaces with, or the two faces are still the same plane to it and nothing
/// is gained; and it has to stay small enough that the region the tool gains
/// is easy to prove empty. Four tolerances is the smallest multiple that
/// separated every case measured (0.4 mm at the 0.1 mm default), and the floor
/// keeps it meaningful when a document asks for a very fine tolerance.
fn over_extension(tol: f64) -> f64 {
    (4.0 * tol).max(0.02)
}

/// One coplanar, area-overlapping pair of faces.
#[derive(Clone, Debug)]
pub struct Coincidence {
    /// Index into `a`'s faces, in `KSolid::face_iter` order.
    pub a: usize,
    /// Index into `b`'s faces.
    pub b: usize,
    pub sense: Sense,
    /// `a`'s outward unit normal there.
    pub normal: Vec3,
    /// How much area the two faces actually share, in mm².
    pub overlap: f64,
}

/// What to do with a meeting pair.
#[derive(Clone, Debug)]
pub enum Plan {
    /// No coplanar face pair: `shapeops` is written for exactly this.
    Transversal,
    /// Union by topological merge ([`glue`]).
    Glue(Vec<Coincidence>),
    /// Subtract or intersect after growing the tool ([`extend`]).
    ExtendTool(Vec<Extension>),
    /// Subtract or intersect where the tool only *rests against* the body:
    /// every coincidence is face to face and the two share no volume. There is
    /// nothing to cut, so the answer comes from the operands.
    Touching,
    /// None of the three applies; the string is the whole diagnostic.
    Refuse(String),
}

/// One stretch of the tool along one axis.
///
/// The tool spans `lo..hi` along `axis`; `plus` and `minus` say which ends
/// carry a coplanar face and therefore have to move out by
/// [`over_extension`]. Both ends at once is the through-cutter whose two faces
/// are flush with the body's two faces, and it is one stretch about the middle
/// rather than two, so no end is ever stretched twice.
#[derive(Clone, Copy, Debug)]
pub struct Extension {
    /// Unit direction `lo` and `hi` are measured along.
    pub axis: Vec3,
    pub lo: f64,
    pub hi: f64,
    /// Grow past `hi`.
    pub plus: bool,
    /// Grow past `lo`.
    pub minus: bool,
}

impl Extension {
    /// (the plane along `axis` that stays put, the factor distance from it is
    /// scaled by).
    fn stretch(&self, eps: f64) -> (f64, f64) {
        let span = self.hi - self.lo;
        match (self.plus, self.minus) {
            (true, true) => (0.5 * (self.lo + self.hi), (span + 2.0 * eps) / span),
            (true, false) => (self.lo, (span + eps) / span),
            (false, true) => (self.hi, (span + eps) / span),
            (false, false) => (self.lo, 1.0),
        }
    }
}

// ---------------------------------------------------------------- face facts

/// Everything the contact tests need to know about one face.
struct FaceInfo {
    /// `Some((a point on it, its outward unit normal))` when the face is planar.
    plane: Option<(Vec3, Vec3)>,
    /// Outward-wound triangles, at the boolean tolerance.
    tris: Vec<[Vec3; 3]>,
    lo: Vec3,
    hi: Vec3,
    area: f64,
    /// Length of the face's tessellated boundary: the edges only one of its
    /// triangles has.
    perimeter: f64,
    kind: &'static str,
}

impl FaceInfo {
    fn label(&self, which: &str, idx: usize) -> String {
        format!(
            "{which} face #{idx} ({}, box ({:.3}, {:.3}, {:.3})..({:.3}, {:.3}, {:.3}))",
            self.kind, self.lo.x, self.lo.y, self.lo.z, self.hi.x, self.hi.y, self.hi.z
        )
    }
}

/// Tessellate `s` and hand back one [`FaceInfo`] per B-rep face, in
/// `face_iter` order, with every triangle wound so its normal points *out* of
/// the material.
fn faces_of(s: &KSolid, tol: f64) -> Result<Vec<FaceInfo>> {
    let meshed = kernel::tessellate(s, tol)?;
    let mut out = Vec::new();
    for (fi, face) in s.face_iter().enumerate() {
        let mut tris = Vec::new();
        let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        let mut area = 0.0;
        // The tessellator's triangles are already wound outward.
        for ti in 0..meshed.faces[fi].triangles.len() {
            let p = meshed.triangle_points(fi, ti);
            let t = [kernel::point_to_vec3(p[0]), kernel::point_to_vec3(p[1]), kernel::point_to_vec3(p[2])];
            for p in &t {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            area += (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5;
            tris.push(t);
        }
        // Planar *as geometry*, not as a surface type. Cadpole's own `loft`
        // builds every lateral face with `builder::homotopy`, which hands back
        // a `NurbsSurface` whatever the shape — so the Gridfinity foot's flat
        // flanks, the ones two neighbouring feet share, are not
        // `Surface::Plane` and never would be. What matters here is whether
        // the face *lies* in a plane, and the tessellation says so exactly: a
        // planar face's triangle normals are parallel to the last bit, and a
        // curved one's are not, because the tessellator's chordal sag is
        // orders of magnitude coarser than this test.
        let plane = match face.oriented_surface() {
            // An exact plane gives an exact normal; prefer it when there is one.
            Surface::Plane(p) => {
                let n = p.normal();
                Some((kernel::point_to_vec3(p.origin()), Vec3::new(n.x, n.y, n.z).normalized()))
            }
            _ => planar_fit(&tris, area),
        };
        let perimeter = boundary_length(&tris);
        out.push(FaceInfo {
            plane,
            tris,
            lo,
            hi,
            area,
            perimeter,
            kind: kernel::surface_kind(&face.surface()),
        });
    }
    Ok(out)
}

/// The total length of the edges that belong to one triangle of `tris` only.
fn boundary_length(tris: &[[Vec3; 3]]) -> f64 {
    let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    let mut count: std::collections::HashMap<([u64; 3], [u64; 3]), (usize, f64)> = std::collections::HashMap::new();
    for t in tris {
        for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, b) = (key(p), key(q));
            let e = count.entry(if a <= b { (a, b) } else { (b, a) }).or_insert((0, (q - p).length()));
            e.0 += 1;
        }
    }
    count.values().filter(|(n, _)| *n == 1).map(|(_, l)| l).sum()
}

/// Does this triangle set lie in one plane, and if so which?
///
/// The vector area `Σ ½(p₁-p₀)×(p₂-p₀)` has length equal to the scalar area
/// exactly when every triangle faces the same way, and less otherwise — so one
/// comparison answers it, with no tolerance to pick beyond "the last few bits".
/// The threshold is deliberately far tighter than any tessellation error: a
/// face that is *nearly* flat is curved, and treating it as a plane here would
/// be a guess.
fn planar_fit(tris: &[[Vec3; 3]], area: f64) -> Option<(Vec3, Vec3)> {
    if tris.is_empty() || area <= 0.0 {
        return None;
    }
    let vector_area = tris
        .iter()
        .fold(Vec3::ZERO, |acc, t| acc + (t[1] - t[0]).cross(t[2] - t[0]) * 0.5);
    if vector_area.length() < area * (1.0 - 1e-9) {
        return None;
    }
    let n = vector_area.normalized();
    let mut centroid = Vec3::ZERO;
    let mut count = 0.0;
    for t in tris {
        for p in t {
            centroid = centroid + *p;
            count += 1.0;
        }
    }
    let centroid = centroid * (1.0 / count);
    // Parallel normals still allow a fold; check the points really are on it.
    let scale = tris.iter().flatten().fold(0.0f64, |m, p| m.max((*p - centroid).length()));
    let off = tris
        .iter()
        .flatten()
        .fold(0.0f64, |m, p| m.max(((*p - centroid).dot(n)).abs()));
    if off > 1e-9 * scale.max(1.0) {
        return None;
    }
    Some((centroid, n))
}

/// Every face's triangles as one outward-oriented soup.
///
/// The same thing [`kernel::soup`] builds, but from the tessellation
/// [`faces_of`] has already paid for. Measured on the 2x3x6 Gridfinity bin,
/// calling `kernel::soup` again inside the containment and over-cut proofs
/// tessellated each operand up to three times per boolean and was most of the
/// pre-pass's cost: 64 ms on the biggest pair, against 21 ms once shared.
fn soup_of(faces: &[FaceInfo]) -> Vec<[Vec3; 3]> {
    faces.iter().flat_map(|f| f.tris.iter().copied()).collect()
}

fn boxes_overlap(a: &FaceInfo, b: &FaceInfo, tol: f64) -> bool {
    let gap = |a0: f64, a1: f64, b0: f64, b1: f64| (b0 - a1).max(a0 - b1);
    gap(a.lo.x, a.hi.x, b.lo.x, b.hi.x) <= tol
        && gap(a.lo.y, a.hi.y, b.lo.y, b.hi.y) <= tol
        && gap(a.lo.z, a.hi.z, b.lo.z, b.hi.z) <= tol
}

/// How much area two coplanar faces share, exactly: every triangle of `b`
/// clipped against every triangle of `a` whose box it meets, in `a`'s plane
/// ([`crate::solid::coplanar_overlap`]).
///
/// It used to count a whole triangle at a time by its centroid, which is
/// neither a lower nor an upper bound: a boss standing over the centroids of
/// both of a wall top's two triangles credited the boss with the wall's whole
/// face, [`glue`] took the two faces for the same patch, dropped both and
/// could not close the shell. Clipping also keeps two coplanar faces that
/// merely *abut* at 0 mm² — two triangles meeting along an edge share no
/// area — so faces that share an edge and nothing else stay ordinary
/// neighbours, not contact. `tol` pads the triangle boxes, because the two
/// faces are within it of one plane, not on it.
fn shared_area(a: &FaceInfo, b: &FaceInfo, tol: f64) -> f64 {
    let mut total = 0.0;
    for ta in &a.tris {
        // `coplanar_overlap` measures in `ta`'s own frame, about its own
        // winding normal.
        let (na, area) = crate::solid::tri_normal_area(ta);
        if area == 0.0 {
            continue;
        }
        let (alo, ahi) = tangent::tri_box(ta);
        for tb in &b.tris {
            let (blo, bhi) = tangent::tri_box(tb);
            if !tangent::boxes_within(alo, ahi, blo, bhi, tol) {
                continue;
            }
            if let Some(shared) = crate::solid::coplanar_overlap(ta, na, tb, 1e-9) {
                total += shared;
            }
        }
    }
    total
}

/// Do these two faces lie in the same plane and share area?
fn coincidence(a: &FaceInfo, ai: usize, b: &FaceInfo, bi: usize, tol: f64) -> Option<Coincidence> {
    let ((pa, na), (pb, nb)) = (a.plane?, b.plane?);
    let dot = na.dot(nb);
    if dot.abs() < 1.0 - 1e-6 {
        return None; // not parallel
    }
    if (pb - pa).dot(na).abs() > tol {
        return None; // parallel but not the same plane
    }
    if !boxes_overlap(a, b, tol) {
        return None;
    }
    let overlap = shared_area(a, b, tol);
    // A shared *edge* is not contact: two coplanar faces that merely abut are
    // an ordinary pair of neighbours, and the kernel handles them.
    if overlap <= 1e-9 * a.area.max(b.area).max(1.0) {
        return None;
    }
    // Nor is the sliver two faces that abut along a *curved* edge share: each
    // is tessellated with its own chords, so a disc and the annulus round it
    // overlap by up to the chordal sag along the circle, a few hundredths of
    // a mm². Area no wider than the tolerance along the shorter boundary,
    // and a small part of the smaller face, is that, not contact.
    let sliver = tol * a.perimeter.min(b.perimeter);
    if overlap <= sliver && overlap < 0.5 * a.area.min(b.area) {
        return None;
    }
    Some(Coincidence {
        a: ai,
        b: bi,
        sense: if dot < 0.0 { Sense::Opposed } else { Sense::Aligned },
        normal: na,
        overlap,
    })
}

// -------------------------------------------------------------- the pre-pass

/// Classify a meeting pair and choose one of the three contact operations.
pub fn plan(op: BoolOp, a: &KSolid, b: &KSolid, tol: f64) -> Result<Plan> {
    let _span = kernel::trace::Span::start(format!(
        "contact {} a={}f b={}f tol={tol}",
        op.kind_name(),
        kernel::face_count(a),
        kernel::face_count(b)
    ));
    let ctol = coincidence_tolerance(tol);
    let fa = faces_of(a, tol)?;
    let fb = faces_of(b, tol)?;
    let mut pairs = Vec::new();
    for (i, x) in fa.iter().enumerate() {
        if x.plane.is_none() {
            continue;
        }
        for (j, y) in fb.iter().enumerate() {
            if let Some(c) = coincidence(x, i, y, j, ctol) {
                pairs.push(c);
            }
        }
    }
    if pairs.is_empty() {
        return Ok(Plan::Transversal);
    }
    match op {
        BoolOp::Union => plan_union(a, b, &fa, &fb, pairs, tol),
        BoolOp::Subtract | BoolOp::Intersect => plan_cut(op, a, b, &fa, &fb, pairs, tol),
        // (both build their soups from `fa`/`fb` rather than tessellating again)
    }
}

/// Operation 1: is this a `glue`?
fn plan_union(
    a: &KSolid,
    b: &KSolid,
    fa: &[FaceInfo],
    fb: &[FaceInfo],
    pairs: Vec<Coincidence>,
    tol: f64,
) -> Result<Plan> {
    if let Some(c) = pairs.iter().find(|c| c.sense == Sense::Aligned) {
        return Ok(Plan::Refuse(refusal(
            "union",
            c,
            &fa[c.a],
            &fb[c.b],
            a,
            b,
            "the two faces are flush — both bodies lie on the same side of the plane — so the \
             union has to merge two coplanar faces into one, which is a boolean in the plane \
             itself and not one of the exact kernel's three contact operations. Overlap the bodies by at \
             least 0.5 mm so the contact is transversal, cut the flush part back so the two \
             faces only meet along an edge, or pass kernel=\"mesh\" for this one",
        )));
    }
    // Every coincidence is a touch. `glue` is only sound if the operands touch
    // and do nothing else: two solids that also share volume need a real
    // union, and a merged shell would cross itself.
    if let Some(why) = interiors_overlap(fa, fb, tol) {
        return Ok(Plan::Refuse(format!(
            "union: the bodies touch on {} coincident face{} and also share volume ({why}). A \
             touching union is topological (a glue) and an overlapping one is a \
             kernel boolean; this pair is both, and the exact kernel cannot do the second while \
             the first is in the way. Overlap the bodies by at least 0.5 mm everywhere so the \
             whole contact is transversal, or pass kernel=\"mesh\" for this one",
            pairs.len(),
            if pairs.len() == 1 { "" } else { "s" }
        )));
    }
    Ok(Plan::Glue(pairs))
}

/// Do the two solids share any volume? The reason, if they do.
///
/// Decided from the two tessellations. Two closed surfaces whose interiors
/// overlap either **cross** — and then some edge of one passes through the
/// inside of a triangle of the other, from one side of its plane to the
/// other — or one **holds** part of the other without crossing it, and then
/// a corner of the held part lies strictly inside the holder's material.
/// Those miss one shape: a part whose every corner lies *on* the other
/// surface and whose edges only start and end there, such as a wedge dipping
/// through a slab from one face to the other. So two more probes: the
/// centroid of every triangle near the other body, and the midpoint of every
/// edge whose two ends both lie on the other surface. Each test runs both
/// ways, and each ignores whatever lies *on* the other surface to within
/// float noise: a face resting on a face, an edge ending on a face, a corner
/// sitting on a face are contact, not shared volume, which is the whole
/// point. A touching face's centroid lies on the other surface, so it stays
/// a touch.
///
/// It used to probe one point per triangle, the centroid pushed two
/// tolerances inward. That misses a penetration no centroid falls in — the
/// pointed tip of a tool under a few big triangles, a pocket shallower than
/// the probe depth — and plans it as a touch, which hands the body back
/// unchanged; and it pushes the probe clean through a plate thinner than the
/// probe depth into the other body, which reports shared volume where there
/// is none. "Noise" here is the same float-noise scale [`flatness`] uses, not
/// the boolean tolerance: a tool sunk 0.05 mm into a body shares 0.05 mm of
/// it, and [`plan_cut`] refuses that as a feature finer than the tolerance
/// rather than this function calling it a touch.
fn interiors_overlap(fa: &[FaceInfo], fb: &[FaceInfo], _tol: f64) -> Option<String> {
    let sa = soup_of(fa);
    let sb = soup_of(fb);
    let (alo, ahi) = kernel::soup_bbox(&sa)?;
    let (blo, bhi) = kernel::soup_bbox(&sb)?;
    let noise = 1e-6 * (ahi - alo).length().max((bhi - blo).length()).max(1.0);
    let at = |p: Vec3| format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z);
    if let Some(p) = corner_inside(&sb, &sa, noise) {
        return Some(format!("a corner of the second body at {} is inside the first", at(p)));
    }
    if let Some(p) = corner_inside(&sa, &sb, noise) {
        return Some(format!("a corner of the first body at {} is inside the second", at(p)));
    }
    if let Some(p) = edge_crosses(&sb, &sa, noise).or_else(|| edge_crosses(&sa, &sb, noise)) {
        return Some(format!("their surfaces cross at {}", at(p)));
    }
    if let Some(p) = surface_inside(&sb, &sa, noise) {
        return Some(format!("the second body's surface at {} is inside the first", at(p)));
    }
    if let Some(p) = surface_inside(&sa, &sb, noise) {
        return Some(format!("the first body's surface at {} is inside the second", at(p)));
    }
    None
}

/// A point of `x`'s surface strictly inside `against`'s material, other than
/// a corner: a triangle's centroid, or the midpoint of an edge whose two ends
/// both lie on `against`'s surface.
fn surface_inside(x: &[[Vec3; 3]], against: &[[Vec3; 3]], noise: f64) -> Option<Vec3> {
    let (lo, hi) = kernel::soup_bbox(against)?;
    let boxes = boxes_of(against);
    let on = |p: Vec3| {
        against.iter().zip(&boxes).any(|(t, (tlo, thi))| {
            tangent::boxes_within(p, p, *tlo, *thi, noise) && tangent::point_tri_distance(p, t) <= noise
        })
    };
    let mut seen: std::collections::HashSet<[u64; 6]> = std::collections::HashSet::new();
    for t in x {
        let (tlo, thi) = tangent::tri_box(t);
        if !tangent::boxes_within(tlo, thi, lo, hi, noise) {
            continue;
        }
        let c = (t[0] + t[1] + t[2]) * (1.0 / 3.0);
        if strictly_in(against, &boxes, c, noise) {
            return Some(c);
        }
        for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, b) = if (p.x, p.y, p.z) <= (q.x, q.y, q.z) { (p, q) } else { (q, p) };
            let key = [a.x.to_bits(), a.y.to_bits(), a.z.to_bits(), b.x.to_bits(), b.y.to_bits(), b.z.to_bits()];
            if !seen.insert(key) || !on(p) || !on(q) {
                continue;
            }
            let m = (p + q) * 0.5;
            if strictly_in(against, &boxes, m, noise) {
                return Some(m);
            }
        }
    }
    None
}

/// A corner of `x` that lies strictly inside `against`'s material: more than
/// `noise` from its surface, with winding number one.
fn corner_inside(x: &[[Vec3; 3]], against: &[[Vec3; 3]], noise: f64) -> Option<Vec3> {
    let (lo, hi) = kernel::soup_bbox(against)?;
    let pad = Vec3::new(noise, noise, noise);
    let (lo, hi) = (lo - pad, hi + pad);
    let boxes = boxes_of(against);
    let mut seen: std::collections::HashSet<[u64; 3]> = std::collections::HashSet::new();
    for p in x.iter().flatten() {
        if p.x < lo.x || p.y < lo.y || p.z < lo.z || p.x > hi.x || p.y > hi.y || p.z > hi.z {
            continue;
        }
        if !seen.insert([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]) {
            continue;
        }
        if strictly_in(against, &boxes, *p, noise) {
            return Some(*p);
        }
    }
    None
}

/// Where an edge of `x` passes through a triangle of `against`, its two ends
/// more than `noise` on either side of that triangle's plane.
///
/// A crossing more than `noise` inside the triangle settles it: just past
/// it, on the side the triangle faces away from, the edge — part of `x`'s
/// surface — is inside `against`'s material. A crossing *on* the triangle's
/// border is ambiguous and common: the diagonal of a plate's top passes
/// through the corner of a boss standing on it, and that is a touch. Then
/// the edge is followed into the material from the crossing, and only a
/// point of it found strictly inside `against` (off its surface, winding
/// number one) counts — which still catches an edge that crosses a face
/// exactly along the diagonal its two triangles share.
fn edge_crosses(x: &[[Vec3; 3]], against: &[[Vec3; 3]], noise: f64) -> Option<Vec3> {
    let (lo, hi) = kernel::soup_bbox(against)?;
    let boxes = boxes_of(against);
    let planes: Vec<Option<(Vec3, Vec3, Vec3)>> = against
        .iter()
        .map(|t| {
            let (n, area) = crate::solid::tri_normal_area(t);
            let (tlo, thi) = tangent::tri_box(t);
            (area > 0.0).then_some((n, tlo, thi))
        })
        .collect();
    for t in x {
        for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (elo, ehi) = (p.min(q), p.max(q));
            if !tangent::boxes_within(elo, ehi, lo, hi, noise) {
                continue;
            }
            for (u, plane) in against.iter().zip(&planes) {
                let Some((n, tlo, thi)) = plane else { continue };
                if !tangent::boxes_within(elo, ehi, *tlo, *thi, noise) {
                    continue;
                }
                let (dp, dq) = ((p - u[0]).dot(*n), (q - u[0]).dot(*n));
                if !((dp > noise && dq < -noise) || (dp < -noise && dq > noise)) {
                    continue;
                }
                let hit = p + (q - p) * (dp / (dp - dq));
                if tangent::point_tri_distance(hit, u) > noise {
                    continue;
                }
                let border = [(u[0], u[1]), (u[1], u[2]), (u[2], u[0])]
                    .iter()
                    .map(|(a, b)| point_segment_distance(hit, *a, *b))
                    .fold(f64::MAX, f64::min);
                if border > noise {
                    return Some(hit);
                }
                // On the border: walk into the material, halving the step.
                let inner = if dp < 0.0 { p } else { q };
                let mut step = 0.5;
                for _ in 0..12 {
                    let probe = hit + (inner - hit) * step;
                    step *= 0.5;
                    if strictly_in(against, &boxes, probe, noise) {
                        return Some(probe);
                    }
                }
            }
        }
    }
    None
}

fn boxes_of(s: &[[Vec3; 3]]) -> Vec<(Vec3, Vec3)> {
    s.iter().map(tangent::tri_box).collect()
}

/// Is `p` in `against`'s material and more than `noise` from its surface?
fn strictly_in(against: &[[Vec3; 3]], boxes: &[(Vec3, Vec3)], p: Vec3, noise: f64) -> bool {
    let on_surface = against.iter().zip(boxes).any(|(t, (tlo, thi))| {
        tangent::boxes_within(p, p, *tlo, *thi, noise) && tangent::point_tri_distance(p, t) <= noise
    });
    !on_surface && kernel::encloses(against, p)
}

/// Operation 2: can the tool be grown out of the way?
fn plan_cut(
    op: BoolOp,
    a: &KSolid,
    b: &KSolid,
    fa: &[FaceInfo],
    fb: &[FaceInfo],
    pairs: Vec<Coincidence>,
    tol: f64,
) -> Result<Plan> {
    let eps = over_extension(tol);
    let (sa, tb) = (soup_of(fa), soup_of(fb));
    // A tool that only *rests against* the body — every coincidence face to
    // face, no shared volume — removes nothing and keeps nothing. There is no
    // over-cut to make harmless because there is no cut, so say so here rather
    // than stretching a tool to no purpose.
    if pairs.iter().all(|c| c.sense == Sense::Opposed)
        && interiors_overlap(fa, fb, tol).is_none()
    {
        return Ok(Plan::Touching);
    }
    // A coincidence is two planes within a tolerance of each other, not two
    // equal planes. When the material between them is on the side the cut
    // would see — a pocket floor 0.03 mm thick under a cutter at a 0.05 mm
    // tolerance, a 0.04 mm wall around a cavity — that material is a feature
    // the script asked for, finer than the tolerance, and growing the tool
    // clear of the coincidence would cut it away. This is the refusal
    // `coincidence_tolerance` promises, and it has to come before any
    // extension is planned: a tool grown past all six faces of a thin shell
    // swallows the body, and the second `disposition` then reports "the
    // first body lies entirely inside the second", which is true of the
    // grown tool and false of the operands. Measured before this check
    // existed: a 0.03 mm floor at the 0.05 mm tolerance (and a 0.4 mm floor
    // at 0.5) came back as a through hole — 3000 mm³ instead of 3003, ten
    // faces instead of eleven — with no diagnostic at all.
    for c in &pairs {
        if let Some(thickness) = thin_feature(c, &fa[c.a], &fb[c.b]) {
            let ctol = coincidence_tolerance(tol);
            return Ok(Plan::Refuse(refusal(
                op.kind_name(),
                c,
                &fa[c.a],
                &fb[c.b],
                a,
                b,
                &format!(
                    "the two faces are {} apart, closer than the {ctol} mm boolean tolerance, so \
                     the exact kernel cannot tell them apart — but the material between them is \
                     a real feature {} thick, a floor or a wall finer than the tolerance, and \
                     growing the tool clear of the coincidence would cut it away. The exact kernel \
                     refuses that rather than guess. Use a boolean tolerance below {} \
                     (`--boolean-tolerance`), make the feature at least 0.5 mm thick, or pass \
                     kernel=\"mesh\" for this one",
                    mm(thickness),
                    mm(thickness),
                    mm(thickness)
                ),
            )));
        }
    }
    // Which way would each coplanar tool face have to move? Always *outward*,
    // away from the tool's own material: that is the only direction that can
    // be harmless, because it is more cut and never less, and the extra cut
    // then only has to hit nothing.
    let mut axes: Vec<(Vec3, bool, bool, &Coincidence)> = Vec::new();
    for c in &pairs {
        // `c.normal` is `a`'s outward normal there; the tool's own outward
        // normal is the same for a flush pair and the opposite for a touch.
        let dir = match c.sense {
            Sense::Aligned => c.normal,
            Sense::Opposed => -c.normal,
        };
        match axes.iter_mut().find(|(ax, ..)| ax.dot(dir).abs() > 1.0 - 1e-6) {
            Some(e) => {
                if e.0.dot(dir) > 0.0 {
                    e.1 = true;
                } else {
                    e.2 = true;
                }
            }
            None => axes.push((dir, true, false, c)),
        }
    }
    let mut exts: Vec<Extension> = Vec::new();
    for (axis, plus, minus, c) in axes {
        let (lo, hi) = match prism_extent(fb, axis) {
            Ok(v) => v,
            Err(why) => {
                return Ok(Plan::Refuse(refusal(
                    op.kind_name(),
                    c,
                    &fa[c.a],
                    &fb[c.b],
                    a,
                    b,
                    &format!(
                        "the tool cannot be grown along that face's own normal without changing \
                         its shape somewhere else ({why}), so there is no over-cut that is \
                         provably harmless. Stand the tool proud of the body by at least 0.5 mm \
                         on that face, or pass kernel=\"mesh\" for this one"
                    ),
                )));
            }
        };
        let e = Extension { axis, lo, hi, plus, minus };
        // The proof obligation: what the tool gains has to miss the body.
        if let Some(why) = extension_hits_body(&sa, &tb, &e, eps)? {
            return Ok(Plan::Refuse(refusal(
                op.kind_name(),
                c,
                &fa[c.a],
                &fb[c.b],
                a,
                b,
                &format!(
                    "growing the tool past that face would change the answer: {why}. That is a \
                     partial coincidence — the body both ends on that plane and crosses it — and \
                     the exact kernel refuses it rather than guess. Move the tool's face off the \
                     body's own plane by at least 0.5 mm, make the cut before the material that \
                     crosses the plane is added, or pass kernel=\"mesh\" for this one"
                ),
            )));
        }
        exts.push(e);
    }
    Ok(Plan::ExtendTool(exts))
}

/// How thick the feature between a coincident pair's two planes is, when
/// there is one on the side the cut would see.
///
/// `c.normal` is the body's outward normal; the tool's own outward normal at
/// its coincident face is the same for a flush pair and the opposite for a
/// touch, and an extension always moves that face outward along it
/// ([`plan_cut`]). Measure both planes along that direction. For a flush
/// pair the body's material lies *behind* its face, so a body face beyond
/// the tool's end has material between the two planes: a floor, which the
/// extension would cut through. For a touch the body's material lies *ahead*
/// of its face, so a body face short of the tool's end means the tool already
/// enters it by less than the tolerance: a sliver, which the extension would
/// deepen. Either way the answer is a feature thinner than the tolerance.
/// Planes that really are the same differ by float noise, so the test has a
/// flatness margin far below any tolerance a document can ask for — the same
/// one [`prism_extent`] uses.
fn thin_feature(c: &Coincidence, body: &FaceInfo, tool: &FaceInfo) -> Option<f64> {
    let (pa, _) = body.plane?;
    let (pb, _) = tool.plane?;
    let dir = match c.sense {
        Sense::Aligned => c.normal,
        Sense::Opposed => -c.normal,
    };
    // The body's face relative to the tool's end, measured outward from the
    // tool; positive is beyond the end.
    let d = (pa - pb).dot(dir);
    let thickness = match c.sense {
        Sense::Aligned => d,
        Sense::Opposed => -d,
    };
    (thickness > flatness(body, tool)).then_some(thickness)
}

/// What "the same plane" means once a coincidence has been found: float
/// noise on the size of the faces, not the boolean tolerance.
fn flatness(a: &FaceInfo, b: &FaceInfo) -> f64 {
    1e-6 * (a.hi - a.lo).length().max((b.hi - b.lo).length()).max(1.0)
}

/// A length for a diagnostic: millimetres to three places, or in exponent
/// form when three places would print as nothing.
fn mm(x: f64) -> String {
    if x >= 0.0005 { format!("{x:.3} mm") } else { format!("{x:.1e} mm") }
}

/// Is the tool a prism along `axis`, and if so where does it start and end?
///
/// Growing the tool is done by stretching it along `axis`, which moves its end
/// face out and leaves its other end and all its lateral surfaces exactly
/// where they were — but only if the tool really is a prism along that axis:
/// every face either *contains* the axis (its normal is perpendicular to it,
/// so the stretch slides the face along itself) or is a plane square to the
/// axis at one of the two ends (so the stretch translates it). A loft, a cone,
/// a sphere or a stepped tool fails this test and is refused rather than
/// quietly reshaped.
fn prism_extent(faces: &[FaceInfo], axis: Vec3) -> std::result::Result<(f64, f64), String> {
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for f in faces {
        for t in &f.tris {
            for p in t {
                let d = p.dot(axis);
                lo = lo.min(d);
                hi = hi.max(d);
            }
        }
    }
    if hi <= lo {
        return Err("the tool has no extent along it".into());
    }
    let flat = 1e-6 * (hi - lo).max(1.0);
    for (i, f) in faces.iter().enumerate() {
        let mut worst: f64 = 0.0;
        for t in &f.tris {
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            if n.length() >= 1e-12 {
                worst = worst.max(n.normalized().dot(axis).abs());
            }
        }
        if worst <= 1e-6 {
            continue; // lateral: the stretch slides it along itself
        }
        if f.plane.is_none() || worst < 1.0 - 1e-6 {
            return Err(format!(
                "its {} face #{i} is neither square to that direction nor parallel to it",
                f.kind
            ));
        }
        let d = f.tris.first().map(|t| t[0].dot(axis)).unwrap_or(lo);
        if (d - lo).abs() > flat && (d - hi).abs() > flat {
            return Err(format!(
                "it has a step square to that direction {:.3} mm in from its end, so it is not a \
                 prism along it",
                (d - lo).min(hi - d)
            ));
        }
    }
    Ok((lo, hi))
}

/// The proof obligation for operation 2: the material the tool gains must miss
/// the body entirely, or the over-cut is not harmless and we refuse.
///
/// What the tool gains is the slab between its old end face and its new one,
/// inside the tool's own cross-section. Two things are checked, and both have
/// to hold: the body's **boundary** must not enter the slab (so the slab is
/// either all material or all air), and a probe inside it must be outside the
/// body's material (so it is all air). A face sitting exactly *on* the old end
/// plane is not in the slab and does not count — serving that face is the
/// whole point of the operation — and "exactly" means within float noise of
/// it, not within the tolerance: a slab starting a whole tolerance out from
/// the end plane would start precisely where a floor thinner than the
/// tolerance sits, and cut that floor away unseen. [`plan_cut`] refuses
/// the coincident face itself before this runs ([`thin_feature`]); the thin
/// margin here is for everything else the body may have inside that band.
fn extension_hits_body(
    sa: &[[Vec3; 3]],
    tb: &[[Vec3; 3]],
    e: &Extension,
    eps: f64,
) -> Result<Option<String>> {
    let Some((blo, bhi)) = kernel::soup_bbox(tb) else {
        return Ok(Some("the tool has no surface".into()));
    };
    let guard = 1e-6 * (bhi - blo).length().max(1.0);
    // The tool's own cross-section, approximated by its box: over-reporting it
    // only ever refuses more, never less.
    let in_section = |lo: Vec3, hi: Vec3| {
        lo.x <= bhi.x + eps
            && hi.x >= blo.x - eps
            && lo.y <= bhi.y + eps
            && hi.y >= blo.y - eps
            && lo.z <= bhi.z + eps
            && hi.z >= blo.z - eps
    };
    let mut slabs: Vec<(f64, f64)> = Vec::new();
    if e.plus {
        slabs.push((e.hi + guard, e.hi + eps));
    }
    if e.minus {
        slabs.push((e.lo - eps, e.lo - guard));
    }
    for (d0, d1) in slabs {
        // Whole triangles, not their corners: a triangle that spans the slab
        // has no vertex in it, and the Gridfinity trim is exactly that — the
        // feet's flanks run from the body's plane out past it in one facet.
        for t in sa {
            let (mut tlo, mut thi) = (f64::MAX, f64::MIN);
            for p in t {
                let d = p.dot(e.axis);
                tlo = tlo.min(d);
                thi = thi.max(d);
            }
            if thi < d0 || tlo > d1 {
                continue;
            }
            // Its box against the tool's cross-section: over-reporting here
            // only ever refuses more, never less.
            let lo = t[0].min(t[1]).min(t[2]);
            let hi = t[0].max(t[1]).max(t[2]);
            if in_section(lo, hi) {
                let mid = (lo + hi) * 0.5;
                return Ok(Some(format!(
                    "the body's own surface passes through the {eps:.3} mm the tool would gain, \
                     near ({:.3}, {:.3}, {:.3})",
                    mid.x, mid.y, mid.z
                )));
            }
        }
        let centre = (blo + bhi) * 0.5;
        let probe = centre + e.axis * (0.5 * (d0 + d1) - centre.dot(e.axis));
        if kernel::encloses(sa, probe) {
            return Ok(Some(format!(
                "the {eps:.3} mm the tool would gain lies inside the body, at ({:.3}, {:.3}, {:.3})",
                probe.x, probe.y, probe.z
            )));
        }
    }
    Ok(None)
}

/// Operation 3: the diagnostic. It opens with the case's name, then names
/// the offending face pair, both bounding boxes, the overlap along each
/// axis, and the fix.
#[allow(clippy::too_many_arguments)]
fn refusal(
    op: &str,
    c: &Coincidence,
    fa: &FaceInfo,
    fb: &FaceInfo,
    a: &KSolid,
    b: &KSolid,
    fix: &str,
) -> String {
    let overlap = |a0: f64, a1: f64, b0: f64, b1: f64| a1.min(b1) - a0.max(b0);
    let overlaps = [
        ("X", overlap(fa.lo.x, fa.hi.x, fb.lo.x, fb.hi.x)),
        ("Y", overlap(fa.lo.y, fa.hi.y, fb.lo.y, fb.hi.y)),
        ("Z", overlap(fa.lo.z, fa.hi.z, fb.lo.z, fb.hi.z)),
    ]
    .iter()
    .map(|(n, v)| format!("{n} {v:.3}"))
    .collect::<Vec<_>>()
    .join(", ");
    let (case, facing) = match c.sense {
        Sense::Opposed => ("coplanar faces, touching", "facing each other"),
        Sense::Aligned => ("coplanar faces, flush", "facing the same way"),
    };
    let relation = if c.overlap > 0.0 {
        format!(
            "lie in the same plane (normal ({:.3}, {:.3}, {:.3}), {facing}, sharing {:.3} mm²)",
            c.normal.x, c.normal.y, c.normal.z, c.overlap
        )
    } else {
        format!("come together without crossing ({facing})")
    };
    format!(
        "{op}: {case} — {} and {} {relation}; they overlap by {overlaps} mm. The first body has {} \
         faces, the second {}. {fix}.",
        fa.label("the first body's", c.a),
        fb.label("the second body's", c.b),
        kernel::face_count(a),
        kernel::face_count(b),
    )
}

/// Operation 3, after the fact: `shapeops` was handed a pair the pre-pass
/// called transversal, and failed anyway. Say what the pre-pass could not see.
///
/// Coplanar *faces* are cheap to find and are found before the kernel is
/// called. Two surfaces that touch **tangentially** without being coplanar are
/// not: a curved band that grazes a plane, or — the case the Gridfinity bin
/// runs into — two feet one pitch apart, whose 45° chamfers meet along a
/// single line at `z = 0` where the body's floor also sits. Finding those
/// before the fact would mean intersecting the two boundaries, which is the
/// work `shapeops` exists to do. So they are diagnosed here instead, from the
/// same tessellation, by [`crate::tangent::classify`]: tangent cylinders and
/// spheres by surface identity ([`crate::surface_class`]), an edge or a
/// corner resting on a face by triangle geometry, and a near miss — closer
/// than the tolerance without touching — last. The message names the case,
/// the face pair with both boxes, the measured distance and angle, and the
/// fix.
pub fn explain(op: BoolOp, a: &KSolid, b: &KSolid, tol: f64) -> Option<String> {
    let fa = tangent_faces(a, tol, "the first body's").ok()?;
    let fb = tangent_faces(b, tol, "the second body's").ok()?;
    let ctol = coincidence_tolerance(tol);
    let contacts = tangent::classify(&fa, &fb, ctol, tangent::Depth::Full);
    let first = contacts.first()?;
    Some(format!(
        "{}: {}. The exact kernel intersects the two boundaries and cuts every face along the \
         intersection *curves*, so a contact that is a touch rather than a crossing leaves it \
         nothing to cut along — the tangent case, a diagnostic rather than an \
         operation. {}. The first body has {} faces, the second {}",
        op.kind_name(),
        tangent::summary(&contacts, &fa, &fb)?,
        tangent::fix(first, op, &fa, &fb, ctol, tangent::Path::Exact),
        kernel::face_count(a),
        kernel::face_count(b),
    ))
}

/// The faces of a solid as the tangent classifier reads them: the
/// tessellation [`faces_of`] already builds, plus the analytic class of each
/// face's oriented surface. Labelled the way the refusals label faces — by
/// index, kind and box — because a bare solid has no names; `Context` adds
/// them from the realised operands.
fn tangent_faces(s: &KSolid, tol: f64, which: &str) -> Result<Vec<tangent::TangentFace<'static>>> {
    let infos = faces_of(s, tol)?;
    Ok(s.face_iter()
        .zip(infos)
        .enumerate()
        .map(|(i, (face, info))| {
            let class = crate::surface_class::classify(&face.oriented_surface(), crate::surface_class::EXACT_TOL);
            tangent::TangentFace::owned(info.label(which, i), class, info.plane, info.tris)
        })
        .collect())
}

// ----------------------------------------------------------- operation 2: cut

/// Stretch `tool` so the end(s) named by `e` move out by [`over_extension`]
/// and the other end stays put.
///
/// A stretch rather than a translation, because a translation would move the
/// tool's *other* end too, and a cutter's other end is usually the thing that
/// sets the depth of the cut. [`prism_extent`] has already established that
/// every surface of the tool is either carried along itself by this stretch or
/// translated by it, so the tool's shape does not change: only its length
/// along `axis` grows.
pub fn extend(tool: &KSolid, e: &Extension, tol: f64) -> KSolid {
    let (anchor, scale) = e.stretch(over_extension(tol));
    kernel::transformed(tool, &axial_stretch(e.axis, anchor, scale))
}

/// The matrix that fixes the plane `p · n = anchor` and scales distance from
/// it by `s`: `p -> p + (s - 1)((p · n) - anchor) n`.
fn axial_stretch(n: Vec3, anchor: f64, s: f64) -> Transform {
    use truck_modeling::{Matrix4, Vector4};
    let k = s - 1.0;
    let col = |j: usize, nj: f64| {
        let mut e = [0.0, 0.0, 0.0];
        e[j] = 1.0;
        Vector4::new(e[0] + k * n.x * nj, e[1] + k * n.y * nj, e[2] + k * n.z * nj, 0.0)
    };
    let t = -k * anchor;
    Transform(Matrix4::from_cols(
        col(0, n.x),
        col(1, n.y),
        col(2, n.z),
        Vector4::new(t * n.x, t * n.y, t * n.z, 1.0),
    ))
}

// --------------------------------------------------------- operation 1: glue

/// `glue`: the additive union of two solids that touch on
/// coincident faces and nowhere else.
///
/// It is a **topological** union, not a geometric one. Where the two solids
/// touch, one face of each covers the same patch of one plane, back to back;
/// that patch is interior to the result, so both faces go, and the faces that
/// surrounded them become neighbours across the seam. Nothing is intersected,
/// nothing is cut, no surface is recomputed, and no tolerance enters the
/// answer — which is why this is exact exactly where `shapeops` is not even
/// defined.
///
/// The two shapes a touching pair may take, and the only two this accepts:
///
/// * **the same patch** — the two faces have the same boundary, and both are
///   dropped. Two Gridfinity feet one pitch apart meet like this, on the
///   full-pitch band `examples/gridfinity_bin.star` carries above `z = 0`.
/// * **one inside the other** — the smaller face lies strictly inside the
///   larger. The smaller is dropped and its boundary becomes a *hole* in the
///   larger, which is what a boss on a wall or a lip on a wall top leaves.
///
/// A patch that only partly covers the other is refused: merging those two
/// faces means a boolean in the plane, which is a different piece of work.
pub fn glue(a: &KSolid, b: &KSolid, pairs: &[Coincidence], tol: f64) -> Result<KSolid> {
    let _span = kernel::trace::Span::start(format!(
        "glue a={}f b={}f pairs={}",
        kernel::face_count(a),
        kernel::face_count(b),
        pairs.len()
    ));
    if a.boundaries().len() != 1 || b.boundaries().len() != 1 {
        return Err(CadpoleError::kernel(
            "glue needs two solids with one shell each: a body that already has a cavity has to \
             be joined by a kernel boolean, not by merging faces",
        ));
    }
    let fa = faces_of(a, tol)?;
    let fb = faces_of(b, tol)?;
    let ctol = coincidence_tolerance(tol);

    // Decide, per pair, which face survives and which is dropped. `None` means
    // "dropped"; `Some(j)` means "kept, and gains the other face's boundary as
    // a hole".
    let mut fate_a: HashMap<usize, Option<usize>> = HashMap::new();
    let mut fate_b: HashMap<usize, Option<usize>> = HashMap::new();
    for c in pairs {
        if c.sense != Sense::Opposed {
            return Err(CadpoleError::kernel(refusal(
                "glue",
                c,
                &fa[c.a],
                &fb[c.b],
                a,
                b,
                "glue joins two bodies that touch, and these two faces look the same way, so the \
                 bodies are flush rather than touching",
            )));
        }
        let (x, y) = (&fa[c.a], &fb[c.b]);
        let whole = |f: &FaceInfo| c.overlap >= f.area * (1.0 - 1e-6);
        if whole(x) && whole(y) {
            fate_a.insert(c.a, None);
            fate_b.insert(c.b, None);
        } else if whole(y) && strictly_inside(y, x, c.normal, ctol) {
            fate_b.insert(c.b, None);
            if fate_a.insert(c.a, Some(c.b)).is_some() {
                return Err(CadpoleError::kernel(
                    "glue: one face is touched by two different faces of the other body at once, \
                     which would need a boolean in the plane rather than one hole",
                ));
            }
        } else if whole(x) && strictly_inside(x, y, c.normal, ctol) {
            fate_a.insert(c.a, None);
            if fate_b.insert(c.b, Some(c.a)).is_some() {
                return Err(CadpoleError::kernel(
                    "glue: one face is touched by two different faces of the other body at once, \
                     which would need a boolean in the plane rather than one hole",
                ));
            }
        } else {
            return Err(CadpoleError::kernel(refusal(
                "glue",
                c,
                x,
                y,
                a,
                b,
                "the two faces overlap only in part, so merging them means a boolean in the plane \
                 itself, which the exact kernel cannot do. Make the touching faces the same size, \
                 or make one lie wholly inside the other, or overlap the bodies by at least \
                 0.5 mm so the contact is transversal",
            )));
        }
    }

    let a_faces: Vec<Face> = a.face_iter().cloned().collect();
    let b_faces: Vec<Face> = b.face_iter().cloned().collect();
    let mut asm = Assembler::new(ctol);
    let mut shell = Shell::new();
    for (i, f) in a_faces.iter().enumerate() {
        match fate_a.get(&i) {
            Some(None) => continue,
            other => {
                let mut wires: Vec<Wire> = f.boundaries();
                if let Some(Some(j)) = other {
                    // The hole: `b`'s face is oriented the other way round, so
                    // its own outer boundary already runs clockwise seen from
                    // this face's normal, which is what an inner loop wants.
                    wires.extend(b_faces[*j].boundaries());
                }
                shell.push(asm.face(&wires, f.oriented_surface())?);
            }
        }
    }
    for (j, f) in b_faces.iter().enumerate() {
        match fate_b.get(&j) {
            Some(None) => continue,
            other => {
                let mut wires: Vec<Wire> = f.boundaries();
                if let Some(Some(i)) = other {
                    wires.extend(a_faces[*i].boundaries());
                }
                shell.push(asm.face(&wires, f.oriented_surface())?);
            }
        }
    }
    let solid = Solid::try_new(vec![shell]).map_err(|e| {
        // The case by name first: which faces, and how far apart
        // the two planes actually are, because a seam that would not close
        // is most often a touch a few microns short of exact.
        let c = &pairs[0];
        let apart = match (fa[c.a].plane, fb[c.b].plane) {
            (Some((pa, na)), Some((pb, _))) => format!(", their planes {} apart", tangent::um(((pb - pa).dot(na)).abs())),
            _ => String::new(),
        };
        CadpoleError::kernel(format!(
            "glue: coplanar faces, touching — {} and {}{apart}; glue could not close the seam ({e}): \
             the two bodies touch on {} coincident face{}, but what is left after merging them is \
             not one closed surface. That happens when the contact is more than those faces say — \
             an edge or a corner touching as well, a third face in the way, or a gap under the \
             tolerance between the two planes. Overlap the bodies by at least 0.5 mm so the contact \
             is transversal, or pass kernel=\"mesh\" for this one",
            fa[c.a].label("the first body's", c.a),
            fb[c.b].label("the second body's", c.b),
            pairs.len(),
            if pairs.len() == 1 { "" } else { "s" }
        ))
    })?;
    // The seam is the only thing that changed, so the result holds exactly
    // what the two operands held. Checking it costs one tessellation and
    // catches a merge that closed but folded — the one failure the topology
    // cannot see.
    let want = kernel::soup_volume(a, tol)? + kernel::soup_volume(b, tol)?;
    let got = kernel::soup_volume(&solid, tol)?;
    if (got - want).abs() > 1e-6 * want.abs().max(1.0) {
        return Err(CadpoleError::kernel(format!(
            "glue produced a solid of {got:.6} mm³ where the two bodies hold {want:.6} mm³ \
             between them, so the merged surface crosses itself: the contact is more than two \
             touching faces. Overlap the bodies by at least 0.5 mm, or pass kernel=\"mesh\""
        )));
    }
    Ok(kernel::canonical(solid))
}

/// Is every point of `small` strictly inside `large`, in the plane they share?
fn strictly_inside(small: &FaceInfo, large: &FaceInfo, n: Vec3, tol: f64) -> bool {
    if !small
        .tris
        .iter()
        .all(|t| inside_face(large.tris.iter().copied(), (t[0] + t[1] + t[2]) * (1.0 / 3.0), n))
    {
        return false;
    }
    // ... and clear of its border by more than the kernel could confuse them
    // by. `large`'s border is the edges its own triangles do not share.
    let mut border: Vec<(Vec3, Vec3)> = Vec::new();
    for t in &large.tris {
        for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            match border
                .iter()
                .position(|(x, y)| (*x - q).length() < 1e-9 && (*y - p).length() < 1e-9)
            {
                Some(k) => {
                    border.swap_remove(k);
                }
                None => border.push((p, q)),
            }
        }
    }
    !small
        .tris
        .iter()
        .flatten()
        .any(|p| border.iter().any(|(x, y)| point_segment_distance(*p, *x, *y) <= tol))
}

/// Rebuilds faces so that two shells can share their seam.
///
/// `truck` decides whether a shell is closed by counting `EdgeID`s, and an
/// `EdgeID` is the *address* of the edge's curve — so two edges that run
/// between the same two points along the same curve are different edges unless
/// they are literally the same object, and `Solid::try_new` checks
/// connectivity the same way, on `VertexID`. Stitching two shells therefore
/// means rebuilding their faces against one dictionary of vertices and edges
/// keyed on geometry. Faces nowhere near the seam are rebuilt too: it costs
/// nothing and keeps this one path instead of two.
struct Assembler {
    tol: f64,
    verts: Vec<(Vec3, Vertex)>,
    /// (lower vertex, upper vertex) -> the edges between them, each with its
    /// midpoint, stored in the low -> high direction. The midpoint separates
    /// two different edges between the same pair of vertices, such as the two
    /// halves of a circle, and it is matched *within the tolerance*, never by
    /// the cell of a grid: the same edge arrives once from each of the two
    /// faces that share it, in opposite orientations, and the midpoint of a
    /// curve and of its inverse agree only to rounding. A herringbone's lower
    /// half at a 12.6 mm face has its spokes' midpoints at z = 3.15 mm, which
    /// is exactly 31.5 cells of a 0.1 mm grid: one orientation would round
    /// to 31 and the other to 32, the spoke would become two edges and the
    /// shell would not close.
    edges: HashMap<(usize, usize), Vec<(Vec3, truck_modeling::Edge)>>,
}

impl Assembler {
    fn new(tol: f64) -> Assembler {
        Assembler { tol, verts: Vec::new(), edges: HashMap::new() }
    }

    fn vertex(&mut self, p: Point3) -> usize {
        let v = kernel::point_to_vec3(p);
        if let Some(i) = self.verts.iter().position(|(q, _)| (*q - v).length() <= self.tol) {
            return i;
        }
        self.verts.push((v, Vertex::new(p)));
        self.verts.len() - 1
    }

    fn face(&mut self, wires: &[Wire], surface: Surface) -> Result<Face> {
        let mut out: Vec<Wire> = Vec::new();
        for w in wires {
            let mut edges = Vec::new();
            for e in w.edge_iter() {
                edges.push(self.edge(e)?);
            }
            out.push(Wire::from(edges));
        }
        Face::try_new(out, surface)
            .map_err(|e| CadpoleError::kernel(format!("glue could not rebuild a face: {e}")))
    }

    fn edge(&mut self, e: &truck_modeling::Edge) -> Result<truck_modeling::Edge> {
        use truck_modeling::{BoundedCurve, Invertible, ParametricCurve};
        let curve: Curve = e.oriented_curve();
        let fi = self.vertex(e.front().point());
        let bi = self.vertex(e.back().point());
        if fi == bi {
            return Err(CadpoleError::kernel(
                "glue met an edge that starts and ends at the same point, which it cannot key by \
                 its ends; overlap the bodies by at least 0.5 mm instead",
            ));
        }
        let (t0, t1) = curve.range_tuple();
        let mid = kernel::point_to_vec3(curve.subs(0.5 * (t0 + t1)));
        let key = (fi.min(bi), fi.max(bi));
        let forward = fi < bi;
        let tol = self.tol;
        let bucket = self.edges.entry(key).or_default();
        if let Some((_, existing)) = bucket.iter().find(|(m, _)| (*m - mid).length() <= tol) {
            return Ok(if forward { existing.clone() } else { existing.inverse() });
        }
        let (lo, hi, c) = if forward { (fi, bi, curve) } else { (bi, fi, curve.inverse()) };
        let (v0, v1) = (self.verts[lo].1.clone(), self.verts[hi].1.clone());
        let edge = truck_modeling::Edge::new(&v0, &v1, c);
        bucket.push((mid, edge.clone()));
        Ok(if forward { edge } else { edge.inverse() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmath;
    use crate::kernel::{boolean, extrude, tessellate};
    use crate::sketch::{Loop, P2, Sketch};

    fn brick(size: (f64, f64, f64), at: (f64, f64, f64)) -> KSolid {
        let s = Sketch::new(crate::geom::Plane::XY.offset(at.2)).with_loop(Loop::rect(
            P2::new(at.0 + size.0 / 2.0, at.1 + size.1 / 2.0),
            size.0,
            size.1,
        ));
        extrude(&s, size.2).unwrap().remove(0)
    }

    fn volume(s: &KSolid) -> f64 {
        crate::tessellate::tessellate(s, 0.05).unwrap().0.volume()
    }

    fn watertight(s: &KSolid) -> bool {
        crate::tessellate::tessellate(s, 0.05).unwrap().0.welded(1e-4).is_watertight()
    }

    /// Every refusal opens with the case's name and carries the measured
    /// numbers and the fix (stage 2): the tangent boss the exact
    /// kernel cannot cut (`double_projection` is singular where the normals
    /// are parallel), two boxes meeting along one edge, a flush pair, a
    /// partial coincidence, and a glue across planes 30 µm apart.
    #[test]
    fn a_refusal_names_its_case_its_numbers_and_its_fix() {
        // A boss whose side is tangent to the wall of a hole, from inside.
        let plate = {
            let s = Sketch::new(crate::geom::Plane::XY)
                .with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0))
                .with_loop(Loop::circle(P2::new(0.0, 0.0), 4.0));
            extrude(&s, 5.0).unwrap().remove(0)
        };
        let boss = {
            let s = Sketch::new(crate::geom::Plane::XY.offset(-1.0)).with_loop(Loop::circle(P2::new(2.0, 0.0), 2.0));
            extrude(&s, 8.0).unwrap().remove(0)
        };
        let e = boolean(BoolOp::Union, &plate, &boss, 0.1).unwrap_err().to_string();
        assert!(e.starts_with("kernel operation failed: union: tangent cylinders, one inside the other: the first body's face #"), "{e}");
        assert!(e.contains(" (nurbs, box "), "the faces are labelled by index, kind and box: {e}");
        assert!(
            e.contains("touch along a line (axes 2.000 mm apart, the difference of the radii 4.000 and 2.000; 0.00 µm from exact tangency, axes 0.0 µrad apart); and 3 more contacts (tangent along a line ×3)"),
            "{e}"
        );
        assert!(e.contains("the tangent case, a diagnostic rather than an operation. Give the second body's face #"), "{e}");
        assert!(e.contains("at least 0.200 mm of interference (radius 2.200 instead of 2.000) so the contact becomes a crossing (0.5 mm is the usual overlap), or as much clearance so the two do not meet. The first body has 8 faces, the second 4"), "{e}");

        // Two boxes meeting along one edge: an edge lying along the boundary
        // of a face, and the census of every other corner that touches.
        let a = brick((10.0, 10.0, 10.0), (0.0, 0.0, 0.0));
        let b = brick((10.0, 10.0, 10.0), (10.0, 0.0, 10.0));
        let e = boolean(BoolOp::Union, &a, &b, 0.1).unwrap_err().to_string();
        assert!(
            e.starts_with(
                "kernel operation failed: union: edge-on-edge contact: an edge of the second body's face #0 (plane, box \
                 (10.000, 0.000, 10.000)..(20.000, 10.000, 10.000)) lies along the boundary of the first body's face #2 \
                 (plane, box (10.000, 0.000, 0.000)..(10.000, 10.000, 10.000)) within 0.00 µm; and 19 more contacts \
                 (edge-on-edge ×3, corner-on-edge ×16). The exact kernel intersects"
            ),
            "{e}"
        );
        assert!(e.contains("A contact along an edge or at a corner encloses nothing, so the result is pinched to nothing there: overlap the bodies by at least 0.5 mm where they touch, or keep them as two bodies. The first body has 6 faces, the second 6"), "{e}");

        // Flush: the pre-pass refusal opens with the case too.
        let beside = brick((4.0, 4.0, 4.0), (0.0, 2.0, 2.0));
        match plan(BoolOp::Union, &a, &beside, 0.1).unwrap() {
            Plan::Refuse(why) => assert!(why.starts_with("union: coplanar faces, flush — the first body's face #"), "{why}"),
            other => panic!("{other:?}"),
        }

        // A box 30 µm above the other and offset 3 mm each way: within the
        // band, so a coincidence, and the two faces share 7 x 7 = 49 mm² of
        // their 100 — a partial overlap, refused by name with the exact
        // shared area. (Counted a triangle at a time by centroid, the shared
        // area used to read as the whole 100 mm², glue took the faces for the
        // same patch, dropped both and failed to close the seam instead.)
        let near = brick((10.0, 10.0, 10.0), (3.0, 3.0, 10.03));
        let e = boolean(BoolOp::Union, &a, &near, 0.1).unwrap_err().to_string();
        assert!(e.starts_with("kernel operation failed: glue: coplanar faces, touching — the first body's face #5 (plane, box (0.000, 0.000, 10.000)..(10.000, 10.000, 10.000)) and the second body's face #0 (plane, box (3.000, 3.000, 10.030)..(13.000, 13.000, 10.030)) lie in the same plane (normal (0.000, 0.000, 1.000), facing each other, sharing 49.000 mm²)"), "{e}");
        assert!(e.contains("the two faces overlap only in part"), "{e}");

        // The same footprint 30 µm above: both faces whole, so glue takes
        // them for the same patch and stitches the two shells across the
        // gap. The seam closes, but the result holds the 3 mm³ of the gap
        // as well, and glue's own volume check refuses it, naming both
        // volumes.
        let above = brick((10.0, 10.0, 10.0), (0.0, 0.0, 10.03));
        let e = boolean(BoolOp::Union, &a, &above, 0.1).unwrap_err().to_string();
        assert!(e.contains("glue produced a solid of 2003.000000 mm³ where the two bodies hold 2000.000000 mm³"), "{e}");

        // A cylinder lying on a plate is tangent along a line, and truck
        // happens to accept the pair (it works or fails by
        // luck); it is named on the mesh path, not refused here.
        let slab = brick((20.0, 20.0, 5.0), (0.0, 0.0, 0.0));
        let lying = {
            let s = Sketch::new(crate::geom::Plane::YZ.offset(-10.0)).with_loop(Loop::circle(P2::new(10.0, 8.0), 3.0));
            extrude(&s, 20.0).unwrap().remove(0)
        };
        assert!(boolean(BoolOp::Union, &slab, &lying, 0.1).is_ok());
        let explained = explain(BoolOp::Union, &slab, &lying, 0.1).unwrap();
        assert!(explained.starts_with("union: tangent a cylinder on a plane: the first body's face #5 (plane, box (0.000, 0.000, 5.000)..(20.000, 20.000, 5.000)) and the second body's face #"), "{explained}");
        assert!(explained.contains("touch along a line (axis 3.000 mm from the plane, radius 3.000; 0.00 µm from exact tangency, axes 0.0 µrad apart); and 2 more contacts (vertex-on-face ×1, edge-on-edge ×1)"), "{explained}");
        assert!(explained.contains("Sink the second body's face #"), "{explained}");
        assert!(explained.contains("at least 0.200 mm into the first body's face #5 (plane, box (0.000, 0.000, 5.000)..(20.000, 20.000, 5.000)) so the contact becomes a crossing (0.5 mm is the usual overlap), or lift it as far clear so the two do not meet. The first body has 6 faces, the second 4"), "{explained}");
    }

    #[test]
    fn a_flush_pair_and_a_touching_pair_are_told_apart() {
        let a = brick((10.0, 10.0, 10.0), (0.0, 0.0, 0.0));
        // Touching: the two are on opposite sides of z = 10.
        let on_top = brick((10.0, 10.0, 10.0), (0.0, 0.0, 10.0));
        match plan(BoolOp::Union, &a, &on_top, 0.1).unwrap() {
            Plan::Glue(p) => {
                assert_eq!(p.len(), 1);
                assert_eq!(p[0].sense, Sense::Opposed);
                assert!((p[0].overlap - 100.0).abs() < 1e-6, "the whole face: {}", p[0].overlap);
            }
            other => panic!("touching pair planned as {other:?}"),
        }
        // Flush: both are on the same side of x = 0, level with each other.
        let beside = brick((4.0, 4.0, 4.0), (0.0, 2.0, 2.0));
        match plan(BoolOp::Union, &a, &beside, 0.1).unwrap() {
            Plan::Refuse(why) => assert!(why.contains("flush"), "{why}"),
            other => panic!("flush pair planned as {other:?}"),
        }
    }

    #[test]
    fn two_faces_that_only_share_an_edge_are_not_contact() {
        // Side by side on the x = 10 plane, but one sits above the other, so
        // the two coplanar faces meet along a line and share no area. That is
        // an ordinary pair of neighbours and must go to the kernel untouched.
        let a = brick((10.0, 10.0, 10.0), (0.0, 0.0, 0.0));
        let b = brick((10.0, 10.0, 10.0), (10.0, 0.0, 10.0));
        assert!(matches!(
            plan(BoolOp::Union, &a, &b, 0.1).unwrap(),
            Plan::Transversal | Plan::Refuse(_)
        ));
        let pairs = match plan(BoolOp::Union, &a, &b, 0.1).unwrap() {
            Plan::Glue(p) => p.len(),
            _ => 0,
        };
        assert_eq!(pairs, 0, "an edge is not a coincident face");
    }

    /// A smooth loft of a turned outline, in two halves glued on the plane
    /// they share, at every height from 12 to 16 mm in 0.2 mm steps. Each
    /// half's spokes are NURBS curves whose parametric midpoint sits at a
    /// quarter of the height. Keyed by that midpoint rounded to the
    /// tolerance's grid, an edge splits in two at 12.6, 13.4, 14.2, 15.0 and
    /// 15.8, where the midpoint is an exact half-cell (`h / 4 / 0.1 = 31.5`,
    /// ...): the two faces sharing a spoke hand it in opposite orientations
    /// and their midpoints agree only to rounding. The assembler matches the
    /// midpoint within the tolerance instead.
    #[test]
    fn glue_survives_a_spoke_midpoint_on_a_grid_half_cell() {
        use crate::loft::{LoftPlan, Section, SectionMatch};
        let outline = |turn_deg: f64| -> Loop {
            let t = turn_deg.to_radians();
            let pts: Vec<P2> = (0..24)
                .map(|k| {
                    let a = t + k as f64 * std::f64::consts::TAU / 24.0;
                    let r = if k % 2 == 0 { 12.0 } else { 10.0 };
                    let (s, c) = fmath::sin_cos(a);
                    P2::new(r * c, r * s)
                })
                .collect();
            Loop::polygon(&pts)
        };
        let half = |z0: f64, h: f64, turn0: f64, turn1: f64| -> KSolid {
            let n = 4;
            let sections: Vec<Section> = (0..=n)
                .map(|k| {
                    let f = k as f64 / n as f64;
                    let plane = crate::geom::Plane::XY.offset(z0 + h * f);
                    Section::Profile(Sketch::new(plane).with_loop(outline(turn0 + (turn1 - turn0) * f)))
                })
                .collect();
            let plan = LoftPlan::new(&sections, false, SectionMatch::Index).unwrap();
            crate::kernel::loft(&plan).unwrap().remove(0)
        };
        let mut failed = Vec::new();
        for i in 0..=20 {
            let w = 12.0 + 0.2 * i as f64;
            let lower = half(0.0, w / 2.0, 0.0, 15.0);
            let upper = half(w / 2.0, w / 2.0, 15.0, 0.0);
            match crate::kernel::glue(&lower, &upper, 0.1) {
                Ok(r) => {
                    assert_eq!(r.boundaries().len(), 1, "one shell at {w}");
                    let want = volume(&lower) + volume(&upper);
                    assert!((volume(&r) - want).abs() < want * 1e-4, "{} vs {want} at {w}", volume(&r));
                }
                Err(e) => failed.push(format!("{w}: {e}")),
            }
        }
        assert!(failed.is_empty(), "glue refused {} of 21 heights:\n{}", failed.len(), failed.join("\n"));
    }

    #[test]
    fn glue_puts_a_smaller_face_inside_a_larger_one_as_a_hole() {
        // A 4 mm boss standing on the middle of a 20 mm plate: the boss's
        // bottom face lies strictly inside the plate's top face, so the plate
        // keeps its face and gains the boss's outline as an inner loop.
        let plate = brick((20.0, 20.0, 5.0), (0.0, 0.0, 0.0));
        let boss = brick((4.0, 4.0, 6.0), (8.0, 8.0, 5.0));
        let r = boolean(BoolOp::Union, &plate, &boss, 0.1).unwrap();
        assert_eq!(r.boundaries().len(), 1);
        let want = 20.0 * 20.0 * 5.0 + 4.0 * 4.0 * 6.0;
        assert!((volume(&r) - want).abs() < want * 1e-6, "{} vs {want}", volume(&r));
        assert!(watertight(&r));
        // 6 plate faces + 6 boss faces, minus the boss's bottom, which became
        // a hole in the plate's top.
        assert_eq!(r.face_iter().count(), 11);
        assert!(tessellate(&r, 0.05).is_ok());
    }

    #[test]
    fn a_tool_that_only_rests_against_the_body_cuts_nothing() {
        let body = brick((10.0, 10.0, 10.0), (0.0, 0.0, 0.0));
        let resting = brick((4.0, 4.0, 4.0), (3.0, 3.0, 10.0));
        let r = boolean(BoolOp::Subtract, &body, &resting, 0.1).unwrap();
        assert!((volume(&r) - 1000.0).abs() < 1e-6, "nothing removed: {}", volume(&r));
        let e = boolean(BoolOp::Intersect, &body, &resting, 0.1).unwrap_err().to_string();
        assert!(e.contains("only touch"), "{e}");
    }

    /// An L-shaped body (a 2 mm slab with a tower whose wall is x = 10) and a
    /// tool resting flush on that wall whose V-shaped tip digs 1 mm into the
    /// slab. The tool's few big triangles put no centroid in the 2 mm³ the
    /// two share, and the overlap test used to probe centroids only: the pair
    /// was planned as merely touching, so subtract handed the body back
    /// unchanged (2400 for 2398) and union glued two overlapping shells into
    /// one that crosses itself (2480 for 2478).
    /// A pin in its own hole: the plate's bottom annulus and the pin's bottom
    /// disc abut along a circle, each tessellated with its own chords, so
    /// they share a sliver of a few hundredths of a mm². That is not a
    /// coincidence, and the pin, which fills the hole exactly and stands on
    /// the plate, only touches it.
    #[test]
    fn a_pin_in_its_own_hole_is_not_a_coincidence_along_the_hole() {
        let plate = {
            let s = Sketch::new(crate::geom::Plane::XY)
                .with_loop(Loop::rect(P2::new(10.0, 10.0), 20.0, 20.0))
                .with_loop(Loop::circle(P2::new(10.0, 10.0), 3.0));
            extrude(&s, 5.0).unwrap().remove(0)
        };
        let pin = {
            let shank = extrude(&Sketch::new(crate::geom::Plane::XY).with_loop(Loop::circle(P2::new(10.0, 10.0), 3.0)), 5.0)
                .unwrap()
                .remove(0);
            let head = extrude(
                &Sketch::new(crate::geom::Plane::XY.offset(5.0)).with_loop(Loop::circle(P2::new(10.0, 10.0), 5.0)),
                2.0,
            )
            .unwrap()
            .remove(0);
            boolean(BoolOp::Union, &shank, &head, 0.1).expect("the pin")
        };
        let (fa, fb) = (faces_of(&plate, 0.1).unwrap(), faces_of(&pin, 0.1).unwrap());
        for (i, x) in fa.iter().enumerate() {
            for (j, y) in fb.iter().enumerate() {
                if let Some(c) = coincidence(x, i, y, j, 0.1) {
                    assert!(c.sense != Sense::Aligned, "the plate's face #{i} and the pin's #{j} share {} mm²", c.overlap);
                }
            }
        }
        match plan(BoolOp::Subtract, &plate, &pin, 0.1).unwrap() {
            Plan::Touching => {}
            other => panic!("{other:?}"),
        }
    }

    /// The same contact where the wedge's every corner lies on the slab's
    /// surface and its edges run through the slab from one face to the
    /// other: no corner is inside and no edge crosses a face, so only the
    /// surface probes see the 2 mm³ the two share.
    #[test]
    fn a_wedge_through_a_slab_with_its_corners_on_the_surface_is_not_touching() {
        let body = {
            let pts = [(0.0, 0.0), (20.0, 0.0), (20.0, 2.0), (10.0, 2.0), (10.0, 10.0), (0.0, 10.0)];
            let pts: Vec<P2> = pts.iter().map(|&(x, y)| P2::new(x, y)).collect();
            extrude(&Sketch::new(crate::geom::Plane::XY).with_loop(Loop::polygon(&pts)), 20.0).unwrap().remove(0)
        };
        let tool = {
            let pts = [(10.0, 8.0), (14.0, 8.0), (14.0, 2.0), (13.0, 0.0), (12.0, 2.0), (10.0, 3.0)];
            let pts: Vec<P2> = pts.iter().map(|&(x, y)| P2::new(x, y)).collect();
            let s = Sketch::new(crate::geom::Plane::XY.offset(2.0)).with_loop(Loop::polygon(&pts));
            extrude(&s, 1.0).unwrap().remove(0)
        };
        match plan(BoolOp::Subtract, &body, &tool, 0.1).unwrap() {
            Plan::Touching => panic!("a wedge that cuts 2 mm³ out of the slab was planned as only touching it"),
            Plan::Refuse(_) | Plan::ExtendTool(_) | Plan::Transversal | Plan::Glue(_) => {}
        }
        if let Plan::Glue(_) = plan(BoolOp::Union, &body, &tool, 0.1).unwrap() {
            panic!("two bodies that share 2 mm³ were planned as a glue");
        }
        if let Ok(r) = boolean(BoolOp::Subtract, &body, &tool, 0.1) {
            assert!((volume(&r) - 2398.0).abs() < 1e-3, "subtract gave {}", volume(&r));
        }
    }

    #[test]
    fn a_tool_resting_on_one_face_and_digging_into_another_is_not_touching() {
        let body = {
            let pts = [(0.0, 0.0), (20.0, 0.0), (20.0, 2.0), (10.0, 2.0), (10.0, 10.0), (0.0, 10.0)];
            let pts: Vec<P2> = pts.iter().map(|&(x, y)| P2::new(x, y)).collect();
            extrude(&Sketch::new(crate::geom::Plane::XY).with_loop(Loop::polygon(&pts)), 20.0).unwrap().remove(0)
        };
        let tool = {
            let pts = [(10.0, 8.0), (14.0, 8.0), (14.0, 5.0), (12.0, 1.0), (10.0, 5.0)];
            let pts: Vec<P2> = pts.iter().map(|&(x, y)| P2::new(x, y)).collect();
            let s = Sketch::new(crate::geom::Plane::XY.offset(2.0)).with_loop(Loop::polygon(&pts));
            extrude(&s, 4.0).unwrap().remove(0)
        };
        assert!((volume(&body) - 2400.0).abs() < 1e-6);
        match plan(BoolOp::Subtract, &body, &tool, 0.1).unwrap() {
            Plan::Touching => panic!("a tool that cuts 2 mm³ out of the body was planned as only touching it"),
            Plan::Refuse(_) | Plan::ExtendTool(_) | Plan::Transversal | Plan::Glue(_) => {}
        }
        match plan(BoolOp::Union, &body, &tool, 0.1).unwrap() {
            Plan::Glue(_) => panic!("two bodies that share 2 mm³ were planned as a glue"),
            Plan::Refuse(why) => assert!(why.contains("also share volume"), "{why}"),
            other => panic!("{other:?}"),
        }
        // Whatever the exact kernel does with it, it must not be the old
        // silent wrong answer.
        if let Ok(r) = boolean(BoolOp::Subtract, &body, &tool, 0.1) {
            assert!((volume(&r) - 2398.0).abs() < 1e-3, "subtract gave {}", volume(&r));
        }
        if let Ok(r) = boolean(BoolOp::Union, &body, &tool, 0.1) {
            assert!((volume(&r) - 2478.0).abs() < 1e-3, "union gave {}", volume(&r));
        }
    }

    /// A pocket one FDM layer deep or shallower: the tool's floor is within
    /// the 0.1 mm boolean tolerance of the body's top, so the two planes are
    /// a coincidence, and the 0.2 mm the overlap test used to probe inward
    /// stepped clean over the shared slab. The cut was planned as a tool
    /// resting on the body and the body came back unchanged with no word.
    /// The slab is a feature finer than the tolerance, which is refused (and
    /// `kernel="auto"` then cuts it on the mesh kernel).
    #[test]
    fn a_pocket_shallower_than_the_tolerance_is_refused_not_ignored() {
        let body = brick((10.0, 10.0, 10.0), (0.0, 0.0, 0.0));
        for depth in [0.1, 0.09, 0.05, 0.02] {
            let tool = brick((6.0, 6.0, 5.0), (2.0, 2.0, 10.0 - depth));
            match plan(BoolOp::Subtract, &body, &tool, 0.1).unwrap() {
                Plan::Refuse(why) => {
                    assert!(why.contains("finer than the tolerance"), "{depth}: {why}");
                    assert!(why.contains(&format!("{depth:.3} mm thick")), "{depth}: {why}");
                }
                other => panic!("a {depth} mm pocket was planned as {other:?}"),
            }
            assert!(boolean(BoolOp::Subtract, &body, &tool, 0.1).is_err(), "{depth}");
        }
    }

    /// A plate thinner than two tolerances lying on a body shares nothing
    /// with it, but the overlap test pushed its probe 0.2 mm in from the
    /// plate's far face, straight through the plate and into the body, and
    /// called that shared volume.
    #[test]
    fn a_plate_thinner_than_two_tolerances_resting_on_a_body_glues() {
        let body = brick((10.0, 10.0, 10.0), (0.0, 0.0, 0.0));
        for th in [0.19, 0.15, 0.11] {
            for plate in [brick((10.0, 10.0, th), (0.0, 0.0, 10.0)), brick((th, 10.0, 10.0), (10.0, 0.0, 0.0))] {
                match plan(BoolOp::Union, &body, &plate, 0.1).unwrap() {
                    Plan::Glue(_) => {}
                    other => panic!("a {th} mm plate was planned as {other:?}"),
                }
                let r = boolean(BoolOp::Union, &body, &plate, 0.1).unwrap();
                let want = 1000.0 + 100.0 * th;
                assert!((volume(&r) - want).abs() < 1e-3, "{th}: {} vs {want}", volume(&r));
            }
        }
    }

    /// A boss standing on the middle of a wall top covers the centroids of
    /// both of the wall top's triangles. Counting covered area a whole
    /// triangle at a time by its centroid credited the boss with the wall's
    /// whole face, so glue took the two faces for the same patch, dropped
    /// both and could not close the shell.
    #[test]
    fn glue_holes_a_wall_top_under_a_centred_boss() {
        for (plate, boss) in [
            (brick((10.0, 10.0, 10.0), (0.0, 0.0, 0.0)), brick((6.0, 6.0, 3.0), (2.0, 2.0, 10.0))),
            (brick((10.0, 10.0, 10.0), (0.0, 0.0, 0.0)), brick((4.0, 4.0, 3.0), (3.0, 3.0, 10.0))),
            (brick((20.0, 20.0, 10.0), (0.0, 0.0, 0.0)), brick((10.0, 10.0, 10.0), (5.0, 5.0, 10.0))),
            (brick((20.0, 20.0, 10.0), (0.0, 0.0, 0.0)), brick((11.0, 11.0, 10.0), (3.0, 3.0, 10.0))),
        ] {
            let want = volume(&plate) + volume(&boss);
            match plan(BoolOp::Union, &plate, &boss, 0.1).unwrap() {
                Plan::Glue(p) => {
                    assert_eq!(p.len(), 1);
                    let (lo, hi) = (fa_box(&boss).0, fa_box(&boss).1);
                    let footprint = (hi.x - lo.x) * (hi.y - lo.y);
                    assert!((p[0].overlap - footprint).abs() < 1e-6, "the boss's footprint: {}", p[0].overlap);
                }
                other => panic!("{other:?}"),
            }
            for (x, y) in [(&plate, &boss), (&boss, &plate)] {
                let r = boolean(BoolOp::Union, x, y, 0.1).unwrap();
                assert!((volume(&r) - want).abs() < want * 1e-9, "{} vs {want}", volume(&r));
                assert_eq!(r.face_iter().count(), 11);
                assert!(watertight(&r));
            }
        }
    }

    fn fa_box(s: &KSolid) -> (Vec3, Vec3) {
        let m = tessellate(s, 0.05).unwrap();
        let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        for f in 0..m.faces.len() {
            for t in 0..m.faces[f].triangles.len() {
                for p in m.triangle_points(f, t) {
                    lo = lo.min(kernel::point_to_vec3(p));
                    hi = hi.max(kernel::point_to_vec3(p));
                }
            }
        }
        (lo, hi)
    }

    #[test]
    fn a_tool_that_is_not_a_prism_along_the_face_normal_is_refused() {
        // A cone resting flush on a body face cannot be stretched along that
        // normal without changing its angle, so there is no harmless over-cut.
        let body = brick((20.0, 20.0, 10.0), (0.0, 0.0, 0.0));
        let cone = crate::kernel::cone(Vec3::new(10.0, 10.0, 4.0), 4.0, 1.0, 6.0).unwrap();
        let e = boolean(BoolOp::Subtract, &body, &cone, 0.1).unwrap_err().to_string();
        assert!(e.contains("without changing"), "{e}");
        assert!(e.contains("0.5 mm"), "the fix has to be a number: {e}");
    }

    #[test]
    fn a_floor_thinner_than_the_tolerance_is_refused_not_cut_through() {
        // A pocket whose floor is thinner than the boolean tolerance: the
        // pocket's own floor and the body's are the same plane as far as the
        // kernel can tell, and the over-cut that would clear the coincidence
        // would cut the floor away. Refusing is the only honest answer -- the
        // feature is finer than the tolerance the document asked for -- and
        // the refusal has to say so.
        //
        // It used not to. `extension_hits_body` ignored everything within a
        // tolerance of the tool's end plane, which is exactly where such a
        // floor sits, so a 0.03 mm floor at a 0.05 mm tolerance (and a 0.4 mm
        // one at 0.5) came back as a through hole -- 3000 mm³ for 3003, ten
        // faces for eleven -- with no diagnostic. The one case this test used
        // to cover, a floor of *exactly* one tolerance, was caught only because
        // it landed on the closed edge of that band, and was reported as a
        // "partial coincidence", which it is not.
        let body = brick((20.0, 20.0, 10.0), (0.0, 0.0, 0.0));
        for (floor, tol) in [(0.1, 0.1), (0.03, 0.05), (0.4, 0.5), (0.05, 0.1), (0.004, 0.05)] {
            let pocket = brick((10.0, 10.0, 12.0), (5.0, 5.0, floor));
            let e = boolean(BoolOp::Subtract, &body, &pocket, tol).unwrap_err().to_string();
            assert!(e.contains("finer than the tolerance"), "floor {floor} at tolerance {tol}: {e}");
            assert!(e.contains(&format!("{floor:.3} mm thick")), "the thickness is named: {e}");
            assert!(e.contains("kernel=\"mesh\""), "the way out is named: {e}");
            assert!(!e.contains("partial coincidence"), "{e}");
        }
        // Thicker than the tolerance, the two planes are distinct, the pair is
        // transversal and the pocket cuts with its floor intact.
        for (floor, tol) in [(0.06, 0.05), (0.3, 0.1), (0.1, 0.01)] {
            let pocket = brick((10.0, 10.0, 12.0), (5.0, 5.0, floor));
            let r = boolean(BoolOp::Subtract, &body, &pocket, tol).unwrap();
            let want = 4000.0 - 100.0 * (10.0 - floor);
            assert!((volume(&r) - want).abs() < 1e-3, "floor {floor} at {tol}: {} vs {want}", volume(&r));
            assert_eq!(r.face_iter().count(), 11, "floor {floor} at {tol}");
            assert!(watertight(&r), "floor {floor} at {tol}");
        }
        // Exactly flush, there is nothing between the planes: the tool is
        // grown and the pocket goes through, as before.
        let through = brick((10.0, 10.0, 12.0), (5.0, 5.0, 0.0));
        let r = boolean(BoolOp::Subtract, &body, &through, 0.1).unwrap();
        assert!((volume(&r) - 3000.0).abs() < 1e-3, "{}", volume(&r));
        assert_eq!(r.face_iter().count(), 10);
        assert!(watertight(&r));
        // And a tool that pokes *out* of the body by less than the tolerance
        // is the flush case with air between the planes, not a feature: the
        // cut already goes through and the over-cut is harmless.
        let proud = brick((10.0, 10.0, 12.0), (5.0, 5.0, -0.03));
        let r = boolean(BoolOp::Subtract, &body, &proud, 0.1).unwrap();
        assert!((volume(&r) - 3000.0).abs() < 1e-3, "{}", volume(&r));
        assert_eq!(r.face_iter().count(), 10);
    }

    #[test]
    fn a_shell_thinner_than_the_tolerance_is_refused_as_a_thin_feature() {
        // A cavity 0.04 mm inside every face of a 20 mm cube, at a 0.1 mm
        // tolerance: six coincident pairs, each with a wall between its two
        // planes. Growing the tool past all six swallows the body, so a
        // second disposition would report "the first body lies entirely
        // inside the second" -- true of the grown tool, false of the operands
        // the script wrote. The wall is refused for what it is.
        let outer = brick((20.0, 20.0, 20.0), (0.0, 0.0, 0.0));
        let inner = brick((19.92, 19.92, 19.92), (0.04, 0.04, 0.04));
        let e = boolean(BoolOp::Subtract, &outer, &inner, 0.1).unwrap_err().to_string();
        assert!(e.contains("finer than the tolerance"), "{e}");
        assert!(e.contains("0.040 mm thick"), "{e}");
        assert!(!e.contains("entirely inside"), "{e}");
        // A wall thicker than the tolerance is a void, as before.
        let inner = brick((19.6, 19.6, 19.6), (0.2, 0.2, 0.2));
        let r = boolean(BoolOp::Subtract, &outer, &inner, 0.1).unwrap();
        assert_eq!(r.boundaries().len(), 2);
        let want = 8000.0 - fmath::powi(19.6f64, 3);
        assert!((volume(&r) - want).abs() < 1e-3, "{} vs {want}", volume(&r));
    }

    #[test]
    fn the_over_extension_leaves_the_tools_other_end_where_it_was() {
        // A 6 mm pocket flush with a 10 mm body's top: the floor of the pocket
        // must not move, or the part changes.
        let body = brick((20.0, 20.0, 10.0), (0.0, 0.0, 0.0));
        for depth in [1.0, 3.0, 6.0, 9.0] {
            let pocket = brick((5.0, 5.0, depth), (5.0, 5.0, 10.0 - depth));
            let r = boolean(BoolOp::Subtract, &body, &pocket, 0.1).unwrap();
            let want = 4000.0 - 25.0 * depth;
            assert!(
                (volume(&r) - want).abs() < 1e-3,
                "a {depth} mm pocket leaves {}, want {want}",
                volume(&r)
            );
            assert!(watertight(&r), "{depth} mm pocket");
        }
    }
}
