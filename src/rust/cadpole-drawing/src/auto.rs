//! Automatic dimensioning: the strategies, and the pass that stops them
//! overlapping.
//!
//! A drawing is read from its dimensions, never scaled off, which makes
//! auto-dimensioning the deliverable rather than a convenience, so this module
//! is deliberately opinionated about *which* dimensions a drawing gets:
//!
//! * [`extents_dims`] — the overall width and height of a view. Every
//!   mechanical drawing starts here, and `sheet=three-view` applies it without
//!   being asked.
//! * [`exterior_strings`] — the residential convention: up to three parallel
//!   strings per side of a plan, innermost first, openings then jogs then
//!   overall.
//! * [`interior_chains`] — one chain per axis across each room, run through a
//!   point inside it.
//! * [`dimension_selection`] — everything else, on request: this pair of
//!   points, this circle's diameter, this angle.
//!
//! Then [`place`] runs over the lot. It is the difference between a drawing
//! and a mess: a dimension whose value does not fit between its witness lines
//! is moved outside them, and a value that would land on top of an already
//! placed one is stepped outward along the dimension line's normal until it
//! clears. The walk is in index order with no randomness anywhere, so the same
//! input produces byte-identical output — which is what makes the placement
//! tests meaningful and what keeps a re-export from churning a diff.

use cadpole_core::sketch::{Loop, P2};

use crate::dim::{Dim, DimKind, DimStyle, Rect};
use crate::ir::{Style, Xform2};

/// Clearance kept between two text boxes, in paper millimetres.
const CLEARANCE: f64 = 0.6;

/// How many times [`place`] will step a label outward before giving up and
/// reporting it.
const MAX_STEPS: usize = 10;

/// Overall width and height of a bounding box, as two linear dimensions: the
/// width below and the height to the left, which is where a reader looks.
pub fn extents_dims(style: Style, lo: P2, hi: P2, ds: &DimStyle, offset_mm: f64) -> Vec<Dim> {
    let mut out = Vec::new();
    if hi.x - lo.x > 1e-9 {
        out.push(Dim::linear(
            style.clone(),
            P2::new(lo.x, lo.y),
            P2::new(hi.x, lo.y),
            P2::new(1.0, 0.0),
            -offset_mm,
            ds.clone(),
        ));
    }
    if hi.y - lo.y > 1e-9 {
        out.push(Dim::linear(
            style,
            P2::new(lo.x, lo.y),
            P2::new(lo.x, hi.y),
            P2::new(0.0, 1.0),
            offset_mm,
            ds.clone(),
        ));
    }
    out
}

/// A chain of dimensions between consecutive `stations` measured along `axis`
/// from `base`, all on one line at `offset_mm`.
///
/// Every station lies on the line through `base` along `axis`, so every
/// dimension's offset is measured from the same place and the whole string
/// shares one dimension line — which is what makes it a string rather than a
/// stack of unrelated dimensions.
pub fn chain_dims(
    style: Style,
    base: P2,
    axis: P2,
    stations: &[f64],
    offset_mm: f64,
    ds: &DimStyle,
) -> Vec<Dim> {
    let d = crate::dim::unit(axis);
    if d.x == 0.0 && d.y == 0.0 {
        return Vec::new();
    }
    let mut s: Vec<f64> = stations.iter().copied().filter(|t| t.is_finite()).collect();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    s.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    let at = |t: f64| P2::new(base.x + d.x * t, base.y + d.y * t);
    s.windows(2)
        .map(|w| Dim::linear(style.clone(), at(w[0]), at(w[1]), d, offset_mm, ds.clone()))
        .collect()
}

/// What an exterior string is built from.
#[derive(Clone, Debug)]
pub struct ExteriorInput {
    /// The outer boundary of the plan.
    pub outline: Loop,
    /// Wall-face breaks and major jogs, in the middle string.
    pub jogs: Vec<P2>,
    /// Opening centrelines, in the innermost string.
    pub openings: Vec<P2>,
}

/// Up to three exterior dimension strings on each side of a plan: openings,
/// then jogs, then overall, stacked outward.
///
/// Stations are assigned to the side they are nearest, projected onto that
/// side's axis, and de-duplicated. A side with nothing but its own corners
/// gets one string, which is the overall.
pub fn exterior_strings(
    style: Style,
    input: &ExteriorInput,
    ds: &DimStyle,
    offset_mm: f64,
) -> Vec<Dim> {
    let pts = input.outline.sample_tolerance(0.5);
    if pts.len() < 3 {
        return Vec::new();
    }
    let (lo, hi) = bbox(&pts);
    let mut out = Vec::new();
    // South, north, west, east: (axis, base point, offset sign that puts the
    // string outside that wall).
    let sides: [(P2, P2, f64); 4] = [
        (P2::new(1.0, 0.0), P2::new(lo.x, lo.y), -1.0),
        (P2::new(1.0, 0.0), P2::new(lo.x, hi.y), 1.0),
        (P2::new(0.0, 1.0), P2::new(lo.x, lo.y), 1.0),
        (P2::new(0.0, 1.0), P2::new(hi.x, lo.y), -1.0),
    ];
    for (s_index, (axis, base, sign)) in sides.iter().enumerate() {
        let extent = if axis.x != 0.0 {
            hi.x - lo.x
        } else {
            hi.y - lo.y
        };
        if extent < 1e-6 {
            continue;
        }
        let along = |p: P2| {
            if axis.x != 0.0 {
                p.x - base.x
            } else {
                p.y - base.y
            }
        };
        // Innermost string: openings. Then jogs. Then the overall, outermost.
        let mut strings: Vec<Vec<f64>> = Vec::new();
        let openings: Vec<f64> = input
            .openings
            .iter()
            .filter(|p| nearest_side(**p, lo, hi) == s_index)
            .map(|p| along(*p))
            .collect();
        if !openings.is_empty() {
            strings.push(with_ends(&openings, extent));
        }
        let jogs: Vec<f64> = input
            .jogs
            .iter()
            .filter(|p| nearest_side(**p, lo, hi) == s_index)
            .map(|p| along(*p))
            .collect();
        if !jogs.is_empty() {
            strings.push(with_ends(&jogs, extent));
        }
        strings.push(vec![0.0, extent]);
        for (i, stations) in strings.iter().enumerate() {
            let off = sign * offset_mm * (i as f64 + 1.0);
            out.extend(chain_dims(style.clone(), *base, *axis, stations, off, ds));
        }
    }
    out
}

/// Which of south, north, west, east a station belongs to: the one whose wall
/// line it is nearest. Ties go to the earlier side, so the answer never
/// depends on floating-point noise.
fn nearest_side(p: P2, lo: P2, hi: P2) -> usize {
    let d = [
        (p.y - lo.y).abs(),
        (p.y - hi.y).abs(),
        (p.x - lo.x).abs(),
        (p.x - hi.x).abs(),
    ];
    let mut best = 0;
    for i in 1..4 {
        if d[i] < d[best] {
            best = i;
        }
    }
    best
}

fn with_ends(stations: &[f64], extent: f64) -> Vec<f64> {
    let mut v = vec![0.0, extent];
    v.extend_from_slice(stations);
    v.retain(|t| *t >= -1e-6 && *t <= extent + 1e-6);
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    v
}

/// One dimension across each axis of each room, run through a point inside it.
pub fn interior_chains(style: Style, rooms: &[Loop], ds: &DimStyle, _offset_mm: f64) -> Vec<Dim> {
    let mut out = Vec::new();
    for room in rooms {
        let pts = room.sample_tolerance(0.5);
        if pts.len() < 3 {
            continue;
        }
        let c = centroid(&pts);
        if let Some((a, b)) = span(&pts, c.y, true) {
            out.push(Dim::linear(
                style.clone(),
                P2::new(a, c.y),
                P2::new(b, c.y),
                P2::new(1.0, 0.0),
                0.0,
                ds.clone(),
            ));
        }
        if let Some((a, b)) = span(&pts, c.x, false) {
            out.push(Dim::linear(
                style.clone(),
                P2::new(c.x, a),
                P2::new(c.x, b),
                P2::new(0.0, 1.0),
                0.0,
                ds.clone(),
            ));
        }
    }
    out
}

/// The first and last crossing of a horizontal (or vertical) line through the
/// polygon, which is how wide the room is there.
fn span(pts: &[P2], at: f64, horizontal: bool) -> Option<(f64, f64)> {
    let mut xs = Vec::new();
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        let (pv, qv) = if horizontal { (p.y, q.y) } else { (p.x, q.x) };
        let (po, qo) = if horizontal { (p.x, q.x) } else { (p.y, q.y) };
        let (lo, hi) = if pv <= qv { (pv, qv) } else { (qv, pv) };
        if at < lo || at >= hi || (hi - lo).abs() < 1e-12 {
            continue;
        }
        let t = (at - pv) / (qv - pv);
        xs.push(po + (qo - po) * t);
    }
    if xs.len() < 2 {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some((xs[0], *xs.last().unwrap()))
}

/// Something the user asked to have dimensioned.
#[derive(Clone, Debug)]
pub enum Target {
    /// The distance between two points, measured along the line joining them.
    Between(P2, P2),
    /// The overall width and height of a loop.
    Extent(Loop),
    Radius {
        center: P2,
        radius: f64,
    },
    Diameter {
        center: P2,
        radius: f64,
    },
    Angle {
        center: P2,
        a: P2,
        b: P2,
    },
}

/// Dimension exactly what was selected, and nothing else.
pub fn dimension_selection(
    style: Style,
    targets: &[Target],
    ds: &DimStyle,
    offset_mm: f64,
) -> Vec<Dim> {
    let mut out = Vec::new();
    for t in targets {
        match t {
            Target::Between(a, b) => {
                out.push(Dim::aligned(style.clone(), *a, *b, offset_mm, ds.clone()))
            }
            Target::Extent(l) => {
                let pts = l.sample_tolerance(0.2);
                if pts.len() >= 3 {
                    let (lo, hi) = bbox(&pts);
                    out.extend(extents_dims(style.clone(), lo, hi, ds, offset_mm));
                }
            }
            Target::Radius { center, radius } => out.push(Dim::new(
                style.clone(),
                DimKind::Radial {
                    center: *center,
                    radius: *radius,
                    angle_deg: 45.0,
                    leader_mm: offset_mm,
                },
                ds.clone(),
            )),
            Target::Diameter { center, radius } => out.push(Dim::new(
                style.clone(),
                DimKind::Diameter {
                    center: *center,
                    radius: *radius,
                    angle_deg: 30.0,
                },
                ds.clone(),
            )),
            Target::Angle { center, a, b } => {
                let r = crate::dim::dist(*center, *a).min(crate::dim::dist(*center, *b)) * 0.7;
                out.push(Dim::new(
                    style.clone(),
                    DimKind::Angular {
                        center: *center,
                        a: *a,
                        b: *b,
                        radius: r.max(1e-6),
                    },
                    ds.clone(),
                ));
            }
        }
    }
    out
}

/// Move dimension text until nothing overlaps, in place.
///
/// Returns a note for each dimension that could not be placed cleanly, so the
/// caller can surface it as a diagnostic rather than plot garbage. Walks in
/// index order and only ever steps outward, so the result is a pure function
/// of the input.
pub fn place(dims: &mut [Dim], x: Xform2) -> Vec<String> {
    let mut placed: Vec<Rect> = Vec::new();
    let mut notes = Vec::new();
    for i in 0..dims.len() {
        dims[i].text_shift = 0.0;
        dims[i].text_outside = false;
        let step = dims[i].dim.text_height_mm * 1.6;
        let mut ok = false;
        'search: for outside in [false, true] {
            for k in 0..=MAX_STEPS {
                dims[i].text_outside = outside;
                dims[i].text_shift = step * k as f64;
                let Some(r) = dims[i].text_box(x) else {
                    ok = true;
                    break 'search;
                };
                let grown = r.grown(CLEARANCE);
                if !placed.iter().any(|p| p.overlaps(&grown)) {
                    placed.push(r);
                    ok = true;
                    break 'search;
                }
            }
        }
        if !ok {
            dims[i].text_outside = false;
            dims[i].text_shift = 0.0;
            if let Some(r) = dims[i].text_box(x) {
                placed.push(r);
            }
            notes.push(format!(
                "dimension {:?} could not be placed without overlapping; move it by hand",
                dims[i].text()
            ));
        }
    }
    notes
}

/// The text boxes of a placed set, for callers (and tests) that want to check
/// the result rather than trust it.
pub fn text_boxes(dims: &[Dim], x: Xform2) -> Vec<Rect> {
    dims.iter().filter_map(|d| d.text_box(x)).collect()
}

fn bbox(pts: &[P2]) -> (P2, P2) {
    let mut lo = P2::new(f64::MAX, f64::MAX);
    let mut hi = P2::new(f64::MIN, f64::MIN);
    for p in pts {
        lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo, hi)
}

fn centroid(pts: &[P2]) -> P2 {
    let n = pts.len().max(1) as f64;
    P2::new(
        pts.iter().map(|p| p.x).sum::<f64>() / n,
        pts.iter().map(|p| p.y).sum::<f64>() / n,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::UnitFormat;

    fn no_overlaps(boxes: &[Rect]) -> Option<(usize, usize)> {
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                if boxes[i].overlaps(&boxes[j]) {
                    return Some((i, j));
                }
            }
        }
        None
    }

    #[test]
    fn a_dimensioned_rectangle_reads_its_two_sides() {
        let ds = DimStyle {
            format: UnitFormat::ft_in(),
            ..DimStyle::architectural()
        };
        let dims = extents_dims(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(3810.0, 2438.4),
            &ds,
            8.0,
        );
        assert_eq!(dims.len(), 2);
        assert_eq!(dims[0].text(), "12'-6\"");
        assert_eq!(dims[1].text(), "8'-0\"");

        let mm = DimStyle::mechanical();
        let dims = extents_dims(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(3810.0, 2438.4),
            &mm,
            8.0,
        );
        assert_eq!(dims[0].text(), "3810");
        assert_eq!(dims[1].text(), "2438.4");
    }

    #[test]
    fn a_dense_chain_ends_up_with_no_overlapping_text() {
        // Twenty stations 1.5 mm apart at 1:1: each label is more than twice
        // as wide as the space it has, so even pushing the text outside the
        // witness lines is not enough and placement has to stack it.
        let stations: Vec<f64> = (0..=20).map(|i| i as f64 * 1.5).collect();
        let mut dims = chain_dims(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(1.0, 0.0),
            &stations,
            10.0,
            &DimStyle::mechanical(),
        );
        assert_eq!(dims.len(), 20);
        let before = text_boxes(&dims, Xform2::IDENTITY);
        assert!(
            no_overlaps(&before).is_some(),
            "the unplaced case really does collide"
        );
        let notes = place(&mut dims, Xform2::IDENTITY);
        let after = text_boxes(&dims, Xform2::IDENTITY);
        assert_eq!(after.len(), dims.len());
        assert_eq!(no_overlaps(&after), None, "{notes:?}");
    }

    #[test]
    fn placement_is_deterministic() {
        let stations: Vec<f64> = (0..=14).map(|i| i as f64 * 7.5).collect();
        let run = || {
            let mut d = chain_dims(
                Style::default(),
                P2::new(0.0, 0.0),
                P2::new(1.0, 0.0),
                &stations,
                10.0,
                &DimStyle::mechanical(),
            );
            place(&mut d, Xform2::IDENTITY);
            d.iter()
                .map(|x| (x.text_shift.to_bits(), x.text_outside))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
        // And the shifts are all outward, never negative.
        assert!(run().iter().all(|(bits, _)| f64::from_bits(*bits) >= 0.0));
    }

    #[test]
    fn a_wide_string_needs_no_nudging_at_all() {
        let stations: Vec<f64> = (0..=4).map(|i| i as f64 * 120.0).collect();
        let mut dims = chain_dims(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(1.0, 0.0),
            &stations,
            10.0,
            &DimStyle::mechanical(),
        );
        let notes = place(&mut dims, Xform2::IDENTITY);
        assert!(notes.is_empty());
        assert!(dims.iter().all(|d| d.text_shift == 0.0 && !d.text_outside));
    }

    #[test]
    fn a_chain_shares_one_dimension_line() {
        let stations = [0.0, 40.0, 100.0];
        let dims = chain_dims(
            Style::default(),
            P2::new(0.0, 0.0),
            P2::new(1.0, 0.0),
            &stations,
            10.0,
            &DimStyle::mechanical(),
        );
        assert_eq!(dims.len(), 2);
        assert_eq!(dims[0].text(), "40");
        assert_eq!(dims[1].text(), "60");
        // Both dimension lines sit at the same height.
        let boxes = text_boxes(&dims, Xform2::IDENTITY);
        assert!((boxes[0].lo.y - boxes[1].lo.y).abs() < 1e-9, "{boxes:?}");
    }

    #[test]
    fn exterior_strings_stack_outward_without_colliding() {
        // A 12 x 8 m rectangle with two openings and one jog on the south.
        let outline = Loop::polygon(&[
            P2::new(0.0, 0.0),
            P2::new(12000.0, 0.0),
            P2::new(12000.0, 8000.0),
            P2::new(0.0, 8000.0),
        ]);
        let input = ExteriorInput {
            outline,
            jogs: vec![P2::new(5000.0, 0.0)],
            openings: vec![P2::new(1800.0, 0.0), P2::new(9000.0, 0.0)],
        };
        let ds = DimStyle::architectural();
        let mut dims = exterior_strings(Style::default(), &input, &ds, 8.0);
        assert!(
            dims.len() >= 8,
            "four sides, up to three strings each: {}",
            dims.len()
        );
        // At 1:100 the plan is 120 mm wide and the labels are real.
        let x = Xform2::new(0.01, 0.0, 0.0);
        let notes = place(&mut dims, x);
        let boxes = text_boxes(&dims, x);
        assert_eq!(no_overlaps(&boxes), None, "{notes:?}");
        // The overall south string reads the full width: 12 m to 1/16".
        assert!(
            dims.iter().any(|d| d.text() == "39'-4 7/16\""),
            "{:?}",
            dims.iter().map(|d| d.text()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn interior_chains_measure_the_room_both_ways() {
        let room = Loop::rect(P2::new(0.0, 0.0), 4000.0, 3000.0);
        let dims = interior_chains(Style::default(), &[room], &DimStyle::architectural(), 0.0);
        assert_eq!(dims.len(), 2);
        let texts: Vec<String> = dims.iter().map(|d| d.text()).collect();
        assert!(texts.contains(&"13'-1 1/2\"".to_owned()), "{texts:?}");
        assert!(texts.contains(&"9'-10 1/8\"".to_owned()), "{texts:?}");
    }

    #[test]
    fn a_selection_is_dimensioned_on_request_and_nothing_more() {
        let targets = vec![
            Target::Between(P2::new(0.0, 0.0), P2::new(30.0, 40.0)),
            Target::Diameter {
                center: P2::new(0.0, 0.0),
                radius: 12.5,
            },
            Target::Radius {
                center: P2::new(0.0, 0.0),
                radius: 8.0,
            },
            Target::Angle {
                center: P2::new(0.0, 0.0),
                a: P2::new(10.0, 0.0),
                b: P2::new(0.0, 10.0),
            },
        ];
        let dims = dimension_selection(Style::default(), &targets, &DimStyle::mechanical(), 6.0);
        let texts: Vec<String> = dims.iter().map(|d| d.text()).collect();
        assert_eq!(texts, vec!["50", "\u{d8}25", "R8", "90\u{b0}"]);
    }
}
