//! Which way up to print it.
//!
//! **Deliberately simple and explainable, not exhaustive.** A real search over
//! SO(3) is a research problem; what a print needs is a flat
//! face on the bed and as little support as possible, and that is a short list
//! of candidates away.
//!
//! **The candidates** are the six axis directions plus the normals of the
//! largest flat regions of the model (triangles grouped by normal within 5°,
//! biggest eight taken). One of those is almost always the answer, and each
//! one has a name a human recognises: "the 800 mm² flat face".
//!
//! **The score**, for each candidate direction `d` that would face the bed:
//!
//! ```text
//! score = 1.0 * (1 − unsupported_area / total_area)     // supports
//!       + 0.6 * (bed_contact_area / best_contact)       // how stably it sits
//!       + 0.3 * (1 − height / tallest)                  // how tall it is
//!       − 10  if it does not fit the build volume
//! ```
//!
//! Supports dominate because support material is the thing that ruins a
//! surface; footprint comes next because a tall part on a small foot falls
//! over; height last because it is only print time. Ties go to leaving the
//! model as the script built it — a rotation nobody asked for is a cost of its
//! own.
//!
//! What it does **not** model: strength against the layer lines, where the
//! seam will land, whether a bridge is short enough to print unsupported,
//! part-to-part packing, or the bed adhesion of a tall thin footprint beyond
//! its area.

use cadpole_core::geom::{Transform, Vec3};
use cadpole_core::tessellate::Mesh;

use crate::{Profile, num, overhang, scale_of, tri, tri_normal_area};

/// One orientation that was considered.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// The model-space direction that would end up pointing at the bed.
    pub down: Vec3,
    /// Where the candidate came from: `"-Z (as modelled)"`, `"a 800 mm² flat
    /// face"`.
    pub label: String,
    pub unsupported_area: f64,
    pub bed_contact_area: f64,
    pub height: f64,
    /// Footprint across the plate, in mm.
    pub footprint: (f64, f64),
    pub fits_bed: bool,
    pub score: f64,
}

/// The chosen orientation and why.
#[derive(Clone, Debug)]
pub struct Orientation {
    /// Rotation (about the model's bounding-box centre) followed by a drop so
    /// the lowest point sits at height 0 along the build direction. Apply it
    /// to the body; it is rigid, so nothing changes size.
    pub transform: Transform,
    pub chosen: Candidate,
    /// Every candidate, best first.
    pub considered: Vec<Candidate>,
    /// True when the model was actually turned.
    pub changed: bool,
    /// One paragraph a human or an assistant can read out.
    pub reasoning: String,
}

/// Candidate flat regions to consider beyond the six axes.
const MAX_FACE_CANDIDATES: usize = 8;
/// Two normals within this angle are the same flat region.
const FLAT_DEGREES: f64 = 5.0;
/// Ties this close leave the model alone.
const TIE: f64 = 1e-6;

pub fn orient(mesh: &Mesh, profile: &Profile) -> Orientation {
    let up = profile.up();
    let scale = scale_of(mesh);
    let bed_eps = (scale * 1e-4).max(1e-6);
    let total_area = mesh.area().max(1e-12);

    let mut candidates = Vec::new();
    // `-up` first, so a tie leaves the model exactly as the script built it.
    push_unique(&mut candidates, -up, "as modelled".to_owned());
    // Then the big flat faces, which get the descriptive labels, and finally
    // the remaining axes for the shapes that have no flat face to stand on.
    for (n, area) in flat_regions(mesh) {
        push_unique(&mut candidates, n, format!("the {} mm² flat face", num(area)));
    }
    for (d, name) in
        [(up, "upside down"), (Vec3::X, "+X down"), (-Vec3::X, "-X down"), (Vec3::Y, "+Y down"), (-Vec3::Y, "-Y down")]
    {
        push_unique(&mut candidates, d, name.to_owned());
    }

    // Measure each one on the rotated mesh. Rotating the mesh is a few
    // hundred microseconds and keeps every measurement honest.
    let mut measured: Vec<(Candidate, Transform)> = Vec::new();
    for (down, label) in candidates {
        let t = rotation_putting_down(mesh, down, up);
        let m = mesh.transformed(&t);
        let (unsupported, contact) = overhang::unsupported_area(&m, up, profile.max_overhang_degrees, bed_eps);
        let (footprint, height) = footprint_and_height(&m, up);
        let fits_bed = footprint.0 <= profile.bed[0] && footprint.1 <= profile.bed[1] && height <= profile.bed[2]
            || footprint.1 <= profile.bed[0] && footprint.0 <= profile.bed[1] && height <= profile.bed[2];
        measured.push((
            Candidate {
                down,
                label,
                unsupported_area: unsupported,
                bed_contact_area: contact,
                height,
                footprint,
                fits_bed,
                score: 0.0,
            },
            t,
        ));
    }

    let best_contact = measured.iter().map(|(c, _)| c.bed_contact_area).fold(0.0f64, f64::max).max(1e-12);
    let tallest = measured.iter().map(|(c, _)| c.height).fold(0.0f64, f64::max).max(1e-12);
    for (c, _) in measured.iter_mut() {
        c.score = (1.0 - c.unsupported_area / total_area)
            + 0.6 * (c.bed_contact_area / best_contact)
            + 0.3 * (1.0 - c.height / tallest)
            - if c.fits_bed { 0.0 } else { 10.0 };
    }

    // Best score wins; a tie keeps the earlier candidate, and "as modelled" is
    // first in the list.
    let mut best = 0usize;
    for i in 1..measured.len() {
        if measured[i].0.score > measured[best].0.score + TIE {
            best = i;
        }
    }

    let (chosen, rotation) = measured[best].clone();
    // Drop the rotated model onto the bed.
    let rotated = mesh.transformed(&rotation);
    let floor = overhang::lowest(&rotated, up);
    let transform = rotation.then(Transform::translation(up * -floor));
    let changed = best != 0;

    let mut considered: Vec<Candidate> = measured.into_iter().map(|(c, _)| c).collect();
    considered.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    let baseline = considered.iter().find(|c| c.label == "as modelled").cloned();
    let reasoning = explain(&chosen, baseline.as_ref(), changed, profile);
    Orientation { transform, chosen, considered, changed, reasoning }
}

fn explain(chosen: &Candidate, baseline: Option<&Candidate>, changed: bool, profile: &Profile) -> String {
    let head = if changed {
        format!("Turned to put {} on the bed.", chosen.label)
    } else {
        "Left as modelled: nothing scored better.".to_owned()
    };
    let support = match baseline {
        Some(b) if changed && b.unsupported_area > chosen.unsupported_area => format!(
            " Unsupported area drops from {} mm² to {} mm² at the {}° threshold.",
            num(b.unsupported_area),
            num(chosen.unsupported_area),
            num(profile.max_overhang_degrees)
        ),
        _ => format!(
            " {} mm² needs support at the {}° threshold.",
            num(chosen.unsupported_area),
            num(profile.max_overhang_degrees)
        ),
    };
    format!(
        "{head}{support} It sits on {} mm² of bed, {} x {} mm across and {} mm tall.{}",
        num(chosen.bed_contact_area),
        num(chosen.footprint.0),
        num(chosen.footprint.1),
        num(chosen.height),
        if chosen.fits_bed { "" } else { " It does not fit the build volume in any orientation considered." }
    )
}

fn push_unique(out: &mut Vec<(Vec3, String)>, d: Vec3, label: String) {
    let d = d.normalized();
    if d.length() < 0.5 || out.iter().any(|(e, _)| e.dot(d) > 0.999_999) {
        return;
    }
    out.push((d, label));
}

/// Flat regions of the model: triangles grouped by normal within
/// [`FLAT_DEGREES`], biggest first. The normal returned points *out* of the
/// model, so it is exactly the direction that would face the bed if that face
/// were laid down.
fn flat_regions(mesh: &Mesh) -> Vec<(Vec3, f64)> {
    let cos = FLAT_DEGREES.to_radians().cos();
    let mut groups: Vec<(Vec3, f64)> = Vec::new();
    for t in 0..mesh.triangle_count() {
        let (n, area) = tri_normal_area(&tri(mesh, t));
        if area <= 0.0 {
            continue;
        }
        match groups.iter_mut().find(|(g, _)| g.normalized().dot(n) >= cos) {
            Some(g) => {
                g.0 = g.0 + n * area;
                g.1 += area;
            }
            None => groups.push((n * area, area)),
        }
    }
    groups.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    groups.truncate(MAX_FACE_CANDIDATES);
    groups.into_iter().map(|(n, a)| (n.normalized(), a)).collect()
}

/// The rigid rotation, about the model's bounding-box centre, that turns
/// `down` into `-up`.
fn rotation_putting_down(mesh: &Mesh, down: Vec3, up: Vec3) -> Transform {
    let centre = match mesh.bbox() {
        Some((lo, hi)) => (lo + hi) * 0.5,
        None => Vec3::ZERO,
    };
    let target = -up;
    let d = down.normalized();
    let dot = d.dot(target).clamp(-1.0, 1.0);
    if dot > 1.0 - 1e-12 {
        return Transform::identity();
    }
    if dot < -1.0 + 1e-12 {
        // Antiparallel: any perpendicular axis, half a turn.
        let a = if d.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        return Transform::rotation(centre, d.cross(a).normalized(), std::f64::consts::PI);
    }
    Transform::rotation(centre, d.cross(target).normalized(), dot.acos())
}

fn footprint_and_height(mesh: &Mesh, up: Vec3) -> ((f64, f64), f64) {
    let (e1, e2) = if (up - Vec3::Z).length() < 1e-9 {
        (Vec3::X, Vec3::Y)
    } else {
        let a = if up.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let e1 = up.cross(a).normalized();
        (e1, up.cross(e1).normalized())
    };
    let ext = |axis: Vec3| {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in mesh.positions.chunks_exact(3) {
            let h = Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64).dot(axis);
            lo = lo.min(h);
            hi = hi.max(h);
        }
        if lo.is_finite() { hi - lo } else { 0.0 }
    };
    ((ext(e1), ext(e2)), ext(up))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    /// Area of the mesh's lowest flat face, for checking what ends up down.
    fn contact_area(mesh: &Mesh, up: Vec3) -> f64 {
        let floor = overhang::lowest(mesh, up);
        let eps = scale_of(mesh) * 1e-4;
        (0..mesh.triangle_count())
            .filter_map(|t| {
                let v = tri(mesh, t);
                let (n, a) = tri_normal_area(&v);
                (a > 0.0 && n.dot(-up) > 0.9 && v.iter().all(|p| p.dot(up) - floor <= eps)).then_some(a)
            })
            .sum()
    }

    #[test]
    fn a_tilted_l_bracket_is_put_back_flat_on_a_face() {
        // 40 mm arms, 4 mm thick, 20 mm deep, then tipped 30 degrees so no
        // face is on the bed and the whole thing overhangs.
        let flat = l_bracket(40.0, 4.0, 20.0);
        let tilted = flat.transformed(&Transform::rotation(Vec3::ZERO, Vec3::Y, 30f64.to_radians()));
        let before = overhang::unsupported_area(&tilted, Vec3::Z, 45.0, 1e-3);
        assert!(before.0 > 100.0, "the tilted bracket really does need support: {before:?}");

        let o = orient(&tilted, &Profile::default());
        assert!(o.changed, "it must not be left tilted");
        assert_eq!(o.chosen.unsupported_area, 0.0, "{}", o.reasoning);
        // The two 40 x 20 faces are the biggest flats; either is a fine
        // answer and both are 800 mm2.
        assert!((o.chosen.bed_contact_area - 800.0).abs() < 1e-3, "{}", o.reasoning);

        let out = tilted.transformed(&o.transform);
        assert!((contact_area(&out, Vec3::Z) - 800.0).abs() < 1e-3, "the flat face really is down");
        assert!(overhang::lowest(&out, Vec3::Z).abs() < 1e-6, "and it is dropped onto the bed");
    }

    #[test]
    fn a_bracket_already_flat_is_left_alone() {
        let flat = l_bracket(40.0, 4.0, 20.0);
        let o = orient(&flat, &Profile::default());
        assert!(!o.changed, "{}", o.reasoning);
        assert!(o.reasoning.contains("Left as modelled"), "{}", o.reasoning);
        assert_eq!(o.chosen.unsupported_area, 0.0);
    }

    #[test]
    fn a_box_on_its_corner_is_straightened() {
        let m = box_mesh(20.0, 20.0, 20.0)
            .transformed(&Transform::rotation(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0), 45f64.to_radians()));
        let o = orient(&m, &Profile::default());
        assert!(o.changed);
        assert_eq!(o.chosen.unsupported_area, 0.0);
        assert!((o.chosen.bed_contact_area - 400.0).abs() < 1e-3, "a whole face down: {}", o.reasoning);
        assert!((o.chosen.height - 20.0).abs() < 1e-3);
    }

    #[test]
    fn the_overhanging_side_of_a_leaning_prism_is_turned_away_from_the_bed() {
        // 70 degrees off vertical: 274 mm2 of overhang as modelled.
        let m = leaning_prism(10.0, 10.0, 70.0, 10.0);
        let base = overhang::unsupported_area(&m, Vec3::Z, 45.0, 1e-3).0;
        assert!(base > 200.0, "as modelled it needs support: {base}");
        let o = orient(&m, &Profile::default());
        assert!(o.chosen.unsupported_area < base, "{}", o.reasoning);
        assert!(o.reasoning.contains("Unsupported area drops from"), "{}", o.reasoning);
    }

    #[test]
    fn an_oversize_part_is_penalised_for_not_fitting() {
        // A 20 x 20 x 100 post on a 50 x 120 x 30 machine: it only fits lying
        // down, so the bed penalty has to beat the footprint and height terms
        // that would otherwise stand it up.
        let p = Profile { bed: [50.0, 120.0, 30.0], ..Profile::default() };
        let m = box_mesh(20.0, 20.0, 100.0);
        let o = orient(&m, &p);
        assert!(o.changed, "standing up does not fit a 30 mm gantry");
        assert!(o.chosen.fits_bed, "{}", o.reasoning);
        assert!((o.chosen.height - 20.0).abs() < 1e-6, "{}", o.reasoning);
    }

    #[test]
    fn every_candidate_is_reported_with_its_score() {
        let o = orient(&box_mesh(10.0, 10.0, 10.0), &Profile::default());
        assert!(o.considered.len() >= 6, "six directions at least");
        for w in o.considered.windows(2) {
            assert!(w[0].score >= w[1].score, "best first");
        }
        // The faces are named by their area, so the reasoning can say which
        // face it means.
        assert!(
            o.considered.iter().any(|c| c.label == "the 100 mm² flat face"),
            "{:?}",
            o.considered.iter().map(|c| c.label.clone()).collect::<Vec<_>>()
        );
    }
}
