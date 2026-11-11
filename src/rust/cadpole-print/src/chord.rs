//! The chord-error-vs-gap check: does an export's mesh sit close
//! enough to the exact surfaces for the gaps between bodies to survive it?
//!
//! A print-in-place mechanism is made of gaps of 0.15–0.4 mm, and an STL is
//! inscribed in the exact body: every curved flank sits inside the true one
//! by up to the chord tolerance. At the default 0.05 mm that is a third of a
//! per-flank backlash, and two flanks 0.15 mm apart in the model can be
//! anywhere from 0.05 to 0.25 mm apart in the file the slicer reads. The kernel measures the
//! chord error it achieved on every body ([`cadpole_core::kernel::ChordError`]);
//! this module measures the gap between bodies with the clearance query
//! ([`cadpole_core::clearance::distance`]) and says, as a **warning**,
//! when the first is more than [`Profile::chord_gap_fraction`] of the second
//! — naming the body, the error, the gap, the faces and the tolerance to
//! pass to fix it.
//!
//! The clearance is exact between the two *meshes*, and the meshes' distance
//! is within the two chord errors of the exact surfaces' either way — a
//! bore's chords cut into its wall while a pin's cut into the pin. So the
//! rule is judged against the *lower* end of that range,
//! `gap − e_a − e_b`, which errs towards warning, and the message says how
//! wide the range is. Two bodies that touch or overlap are not a gap and are
//! the `fuse` rule's business ([`crate::assembly`]), not this one's.
//!
//! Nothing here knows about Starlark or protobuf. `cadpole-engine` turns the
//! findings into diagnostics on export and into `cadpole check` rows;
//! `cadpole-script`'s `print_check` and `assert_printable` read the same finding
//! for one body, so a model test sees it.

use cadpole_core::clearance::{self, box_distance};
use cadpole_core::context::Context;
use cadpole_core::geom::Vec3;
use cadpole_core::ids::BodyId;
use cadpole_core::tessellate::{Mesh, Topology};

use crate::{Check, Issue, Severity};

/// The default of [`crate::Profile::chord_gap_fraction`]: a third.
///
/// Typical numbers: a per-flank backlash is 0.15–0.2 mm and one
/// extrusion width is 0.42–0.45 mm, so two flanks closer than about a third
/// of that fuse into one wall. A chord error of a third of the gap moves the
/// pair a third of the way to fusing before the slicer has seen it; a
/// quarter is stricter than the 0.05 mm default can
/// meet on a 0.15 mm gap without a warning on every planetary. A third warns
/// on that case only when the gap is under 0.15 mm, which is the case that
/// is actually wrong.
pub const CHORD_GAP_FRACTION: f64 = 1.0 / 3.0;

/// One body whose chord error is more than the fraction of its gap to
/// another.
#[derive(Clone, Debug, PartialEq)]
pub struct ChordFinding {
    pub body: BodyId,
    pub body_name: String,
    /// The chord error measured on the body's mesh, in mm.
    pub chord_error: f64,
    /// The tolerance that mesh was asked for.
    pub tolerance: f64,
    pub other: BodyId,
    pub other_name: String,
    /// The gap as measured between the two meshes.
    pub gap_mesh: f64,
    /// The gap's lower bound on the exact surfaces: `gap_mesh` less both
    /// chord errors, floored at zero. What the rule is judged against.
    pub gap_low: f64,
    /// The sum of the two chord errors: how far either way the exact gap can
    /// be from `gap_mesh`.
    pub uncertainty: f64,
    /// The closest point on this body, and the faces the closest pair lies
    /// on (this body's, the other's), by their generated names.
    pub at: Vec3,
    pub face: String,
    pub other_face: String,
    /// The fraction the rule used.
    pub fraction: f64,
    /// A tolerance that brings both bodies' chord error under the fraction
    /// of the gap, rounded down to two significant figures.
    pub suggested_tolerance: f64,
}

/// Could the rule trip on a pair whose meshes are at least `box_gap` apart?
/// The exact gap may be `e_a + e_b` under the mesh's, and the rule needs
/// the larger error over the fraction of that, so a pair farther apart than
/// this is not measured.
fn could_trip(box_gap: f64, e_a: f64, e_b: f64, fraction: f64) -> bool {
    let e_max = e_a.max(e_b);
    e_max > 0.0 && box_gap <= e_a + e_b + e_max / fraction
}

/// The chord-error-vs-gap rule over every visible body of an evaluated
/// context: one finding per body that fails it, against the nearest body it
/// fails it with. Bodies are read as they stand
/// ([`Context::body_mesh_cached`]), so an export that just meshed them at
/// its own tolerances is judged on those meshes. Pairs whose bounding boxes
/// are farther apart than the rule could ever trip on are not measured.
pub fn chord_findings(ctx: &mut Context, fraction: f64) -> cadpole_core::Result<Vec<ChordFinding>> {
    let ids: Vec<BodyId> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
    if ids.len() < 2 || !(fraction > 0.0) {
        return Ok(Vec::new());
    }
    for &id in &ids {
        ctx.body_mesh_cached(id)?;
    }
    struct Side<'a> {
        id: BodyId,
        name: String,
        mesh: &'a Mesh,
        topo: &'a Topology,
        bbox: (Vec3, Vec3),
        chord: Option<f64>,
        tolerance: f64,
    }
    let mut sides: Vec<Side<'_>> = Vec::with_capacity(ids.len());
    for &id in &ids {
        let Some((mesh, topo)) = ctx.cached_mesh(id)? else { continue };
        let Some(bbox) = mesh.bbox() else { continue };
        let quality = ctx.mesh_quality(id)?;
        let tolerance = match &quality {
            Some(q) => q.options.tolerance,
            None => ctx.tessellation_for(id, None)?.0.tolerance,
        };
        sides.push(Side {
            id,
            name: ctx.body(id)?.name.clone(),
            mesh,
            topo,
            bbox,
            chord: quality.as_ref().and_then(|q| q.chord_error()),
            tolerance,
        });
    }
    let face_name = |topo: &Topology, f: u32| topo.faces.get(f as usize).map(|f| f.name.clone()).unwrap_or_default();
    let mut worst: Vec<Option<ChordFinding>> = vec![None; sides.len()];
    for i in 0..sides.len() {
        for j in i + 1..sides.len() {
            let (ei, ej) = (sides[i].chord.unwrap_or(0.0), sides[j].chord.unwrap_or(0.0));
            if !could_trip(box_distance(sides[i].bbox, sides[j].bbox), ei, ej, fraction) {
                continue;
            }
            let tol = sides[i].tolerance.max(sides[j].tolerance);
            let Some(c) = clearance::distance(sides[i].mesh, sides[j].mesh, tol) else { continue };
            if c.overlap || c.touching() {
                continue;
            }
            let gap_low = (c.distance - ei - ej).max(0.0);
            for (k, e) in [(i, ei), (j, ej)] {
                if !(e > fraction * gap_low) {
                    continue;
                }
                let o = if k == i { j } else { i };
                let (at, face, other_face) = if k == i {
                    (c.a_point, face_name(sides[i].topo, c.face_a), face_name(sides[j].topo, c.face_b))
                } else {
                    (c.b_point, face_name(sides[j].topo, c.face_b), face_name(sides[i].topo, c.face_a))
                };
                let finding = ChordFinding {
                    body: sides[k].id,
                    body_name: sides[k].name.clone(),
                    chord_error: e,
                    tolerance: sides[k].tolerance,
                    other: sides[o].id,
                    other_name: sides[o].name.clone(),
                    gap_mesh: c.distance,
                    gap_low,
                    uncertainty: ei + ej,
                    at,
                    face,
                    other_face,
                    fraction,
                    suggested_tolerance: suggested_tolerance(c.distance, fraction),
                };
                if worst[k].as_ref().map_or(true, |w| finding.gap_low < w.gap_low) {
                    worst[k] = Some(finding);
                }
            }
        }
    }
    Ok(worst.into_iter().flatten().collect())
}

/// [`chord_findings`] as print-check issues, one per failing body.
pub fn chord_issues(ctx: &mut Context, fraction: f64) -> cadpole_core::Result<Vec<(BodyId, Issue)>> {
    Ok(chord_findings(ctx, fraction)?.into_iter().map(|f| (f.body, f.issue())).collect())
}

/// The tolerance at which both bodies' chord error stays under `fraction`
/// of the gap even after the gap has closed by both chord errors:
/// `t = f·(g − 2t)`, so `t = f·g / (1 + 2f)` — a fifth of the mesh gap at a
/// third. Rounded down to two significant figures, because it is typed.
pub fn suggested_tolerance(gap_mesh: f64, fraction: f64) -> f64 {
    let t = fraction * gap_mesh / (1.0 + 2.0 * fraction);
    round_down_2sf(t)
}

fn round_down_2sf(x: f64) -> f64 {
    if !(x > 0.0) || !x.is_finite() {
        return 0.0;
    }
    let e = 10f64.powi(x.log10().floor() as i32 - 1);
    (x / e).floor() * e
}

/// `fraction` as the message says it: "a third", "a quarter", "half", or
/// the number.
pub fn fraction_words(fraction: f64) -> String {
    let near = |v: f64| (fraction - v).abs() < 1e-9;
    if near(1.0 / 3.0) {
        "a third".to_owned()
    } else if near(0.25) {
        "a quarter".to_owned()
    } else if near(0.5) {
        "half".to_owned()
    } else {
        format!("{fraction:.2}×")
    }
}

impl ChordFinding {
    /// The finding as one warning line with its numbers, in the form every
    /// other print finding takes.
    pub fn issue(&self) -> Issue {
        Issue {
            check: Check::ChordError,
            severity: Severity::Warning,
            message: self.message(),
            at: Some(self.at),
            value: self.chord_error,
        }
    }

    /// The warning's text: the error, the gap and the range the chord
    /// errors leave it in, and the tolerance to pass, spelled three ways.
    pub fn message(&self) -> String {
        let f = fraction_words(self.fraction);
        let range = if self.gap_low > 0.0 {
            format!("anywhere from {:.3} to {:.3} mm apart", self.gap_low, self.gap_mesh + self.uncertainty)
        } else {
            format!("anywhere from touching to {:.3} mm apart", self.gap_mesh + self.uncertainty)
        };
        let faces = if self.face.is_empty() || self.other_face.is_empty() {
            String::new()
        } else {
            format!(" between {} and {}", self.face, self.other_face)
        };
        format!(
            "chord error {:.3} mm at tolerance {} mm is more than {f} of the gap to \"{}\": the export \
             meshes are {:.3} mm apart{faces} at ({:.2}, {:.2}, {:.2}), and with the two chord errors the \
             exact surfaces could be {range} -- a slicer reads the gap from the triangles. Tessellate \
             finer -- `{}.tessellation(tolerance={})`, `export(..., tolerance={})` or `--tolerance {}` -- \
             to keep the error under {f} of it",
            self.chord_error,
            self.tolerance,
            self.other_name,
            self.gap_mesh,
            self.at.x,
            self.at.y,
            self.at.z,
            self.body_name,
            self.suggested_tolerance,
            self.suggested_tolerance,
            self.suggested_tolerance,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_suggested_tolerance_is_a_fifth_of_the_gap_at_a_third_rounded_down() {
        assert_eq!(suggested_tolerance(0.3, 1.0 / 3.0), 0.06);
        assert_eq!(suggested_tolerance(0.31, 1.0 / 3.0), 0.062);
        assert_eq!(suggested_tolerance(1.0, 1.0 / 3.0), 0.2);
        assert_eq!(suggested_tolerance(0.0, 1.0 / 3.0), 0.0);
        assert_eq!(round_down_2sf(0.0499), 0.049);
        assert_eq!(fraction_words(1.0 / 3.0), "a third");
        assert_eq!(fraction_words(0.25), "a quarter");
    }

    /// A pair is measured only when the rule could trip on it: two 0.05 mm
    /// errors need the boxes within 0.25 mm at a third; a pair with no
    /// measured error (two mesh bodies) is never measured.
    #[test]
    fn pairs_too_far_apart_to_matter_are_not_measured() {
        let f = 1.0 / 3.0;
        assert!(could_trip(0.0, 0.05, 0.05, f));
        assert!(could_trip(0.25, 0.05, 0.05, f));
        assert!(!could_trip(0.26, 0.05, 0.05, f));
        assert!(could_trip(0.2, 0.05, 0.0, f), "one measured error is enough");
        assert!(!could_trip(0.0, 0.0, 0.0, f), "two mesh bodies have nothing to judge");
    }
}
