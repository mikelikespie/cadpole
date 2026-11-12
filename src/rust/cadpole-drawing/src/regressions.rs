//! Regression tests for drawing bugs: each one states what
//! the drawing got wrong and checks the number a person would check on paper.

#![cfg(test)]

use cadpole_core::geom::Plane;
use cadpole_core::kernel;
use cadpole_core::sketch::{Loop, Sketch, P2};
use cadpole_core::tessellate::{tessellate, Mesh, Topology};

use crate::ir::{layer, Drawing, Prim};
use crate::model::{build, DrawingOptions};
use crate::view::{append_body, project, BodyRanges, ProjectOptions, ViewBasis};

/// A `w` x `d` x `h` box with its low corner at `at`.
fn box_at(at: [f64; 3], w: f64, d: f64, h: f64) -> (Mesh, Topology) {
    let s = Sketch::new(Plane::XY.offset(at[2]))
        .with_loop(Loop::rect(P2::new(at[0] + w / 2.0, at[1] + d / 2.0), w, d));
    let solids = kernel::extrude(&s, h).expect("extrude");
    tessellate(&solids[0], 0.05).expect("tessellate")
}

/// Several boxes drawn together, as `combined_mesh` hands them to `build`.
fn boxes(list: &[([f64; 3], f64, f64, f64)]) -> (Mesh, Topology) {
    let mut mesh = Mesh::default();
    let mut topo = Topology::default();
    let mut ranges = BodyRanges::default();
    for (at, w, d, h) in list {
        let (m, t) = box_at(*at, *w, *d, *h);
        append_body(&mut mesh, &mut topo, &mut ranges, &m, &t);
    }
    (mesh, topo)
}

/// The dimension texts of a drawing.
fn dim_texts(d: &Drawing) -> Vec<String> {
    d.flatten()
        .iter()
        .filter_map(|p| match p {
            Prim::Text { text, style, .. } if style.layer == layer::DIMS => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_section_does_not_draw_what_the_cut_removed() {
    // A 60 x 40 x 20 block with a 6 x 6 x 15 knob on top. `section:x=30`
    // looks from +x, so what is kept is x <= 30: a knob at x = 5 is beyond
    // the cut and stands 35 tall, a knob at x = 50 was cut away and the
    // section is 20 tall.
    let opts = DrawingOptions::parse("section:x=30", "1:1", "", "auto", "", "").unwrap();
    let height = |knob_x: f64| {
        let (mesh, topo) = boxes(&[
            ([0.0, 0.0, 0.0], 60.0, 40.0, 20.0),
            ([knob_x, 5.0, 20.0], 6.0, 6.0, 15.0),
        ]);
        let d = build(&mesh, &topo, &opts).unwrap();
        dim_texts(&d)
    };
    let beyond = height(5.0);
    assert!(beyond.contains(&"35".to_owned()), "knob beyond the cut: {beyond:?}");
    let removed = height(50.0);
    assert!(
        removed.contains(&"20".to_owned()) && !removed.contains(&"35".to_owned()),
        "knob in front of the cut is not drawn: {removed:?}"
    );
}

fn dist(a: P2, b: P2) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn polyline_length(l: &[P2]) -> f64 {
    l.windows(2).map(|w| dist(w[0], w[1])).sum()
}

fn distance_to_polylines(p: P2, ls: &[Vec<P2>]) -> f64 {
    let mut best = f64::MAX;
    for l in ls {
        for w in l.windows(2) {
            let (a, b) = (w[0], w[1]);
            let d = P2::new(b.x - a.x, b.y - a.y);
            let len2 = d.x * d.x + d.y * d.y;
            let t = if len2 > 0.0 {
                (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = P2::new(a.x + d.x * t, a.y + d.y * t);
            best = best.min(dist(p, q));
        }
    }
    best
}

#[test]
fn a_box_seen_square_on_has_no_hidden_lines() {
    // The back edges project exactly onto the front ones: the outline is
    // drawn once, visible, and not again dashed on top of itself.
    let (mesh, topo) = box_at([0.0, 0.0, 0.0], 40.0, 30.0, 10.0);
    let p = project(&mesh, &topo, &ViewBasis::preset("front"), &ProjectOptions::default());
    assert!(p.hidden.is_empty(), "hidden: {:?}", p.hidden);
    let total: f64 = p.visible.iter().map(|l| polyline_length(l)).sum();
    assert!((total - 100.0).abs() < 1e-3, "the outline once: {total}");
}

#[test]
fn hidden_lines_are_drawn_once_and_never_over_a_visible_line() {
    // A low block in front of a tall post: from the front, the post's two
    // vertical silhouettes are hidden for the block's 10 mm height. Each is
    // two 3D edges (front and back corner) on one 2D line, and the post's
    // bottom edges lie on the block's visible bottom edge.
    let (mesh, topo) = boxes(&[
        ([0.0, 0.0, 0.0], 40.0, 30.0, 10.0),
        ([15.0, 40.0, 0.0], 10.0, 10.0, 30.0),
    ]);
    let p = project(&mesh, &topo, &ViewBasis::preset("front"), &ProjectOptions::default());
    for l in &p.hidden {
        for w in l.windows(2) {
            let m = P2::new((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0);
            assert!(
                distance_to_polylines(m, &p.visible) > 1e-3,
                "hidden segment {w:?} lies on a visible line"
            );
        }
    }
    let hidden: f64 = p.hidden.iter().map(|l| polyline_length(l)).sum();
    assert!((hidden - 20.0).abs() < 0.05, "two 10 mm hidden lines, once each: {hidden} in {:?}", p.hidden);
    let visible: f64 = p.visible.iter().map(|l| polyline_length(l)).sum();
    // The block's outline (100) plus the post above it: 2 x 20 up, 10 across.
    assert!((visible - 150.0).abs() < 0.05, "visible {visible} in {:?}", p.visible);
}

#[test]
fn an_isometric_view_is_not_dimensioned_with_screen_lengths() {
    // The projected extents of an axonometric view are no length of the
    // model: a 40 x 30 x 10 box came back dimensioned 49.5 and 33.07.
    let (mesh, topo) = box_at([0.0, 0.0, 0.0], 40.0, 30.0, 10.0);
    for view in ["iso", "iso-back", "ISO"] {
        let opts = DrawingOptions::parse(view, "", "", "auto", "", "").unwrap();
        let d = build(&mesh, &topo, &opts).unwrap();
        assert!(dim_texts(&d).is_empty(), "{view}: {:?}", dim_texts(&d));
    }
    // An orthographic view still reads true lengths.
    let opts = DrawingOptions::parse("front", "", "", "auto", "", "").unwrap();
    let d = build(&mesh, &topo, &opts).unwrap();
    assert_eq!(dim_texts(&d), vec!["40", "10"]);
}

#[test]
fn an_explicit_scale_that_does_not_fit_the_sheet_says_so() {
    // A 40 x 30 x 10 box at 20:1 drew its three views to x = 827 on a
    // 420 mm sheet, with nothing in the response to say so.
    let (mesh, topo) = box_at([0.0, 0.0, 0.0], 40.0, 30.0, 10.0);
    let fits = |n: &String| n.contains("does not fit");
    for view in ["three-view", "front", "section:z=5"] {
        let big = DrawingOptions::parse(view, "20:1", "", "", "", "").unwrap();
        let d = build(&mesh, &topo, &big).unwrap();
        let note = d.notes.iter().find(|n| fits(n)).unwrap_or_else(|| panic!("{view}: {:?}", d.notes));
        assert!(note.contains("20:1") && note.contains("A3"), "{view}: {note}");
        // It names a scale that does fit, and that scale draws no such note.
        let largest = note.rsplit("largest standard scale that fits is ").next().unwrap();
        let largest = largest.trim_end_matches('.').to_owned();
        let ok = DrawingOptions::parse(view, &largest, "", "", "", "").unwrap();
        let d = build(&mesh, &topo, &ok).unwrap();
        assert!(!d.notes.iter().any(fits), "{view} at {largest}: {:?}", d.notes);
        // And the fitted default never does.
        let auto = DrawingOptions::parse(view, "", "", "", "", "").unwrap();
        let d = build(&mesh, &topo, &auto).unwrap();
        assert!(!d.notes.iter().any(fits), "{view}: {:?}", d.notes);
    }
    // Just past the room the layout has, though inside the whole drawing
    // area: the views still run off the paper, and the note says so.
    for (view, scale) in [("front", "10:1"), ("three-view", "5.5:1")] {
        let o = DrawingOptions::parse(view, scale, "", "", "", "").unwrap();
        let d = build(&mesh, &topo, &o).unwrap();
        assert!(d.notes.iter().any(fits), "{view} at {scale}: {:?}", d.notes);
    }
}

/// A section through one body: the cut face is solid material, so the
/// edges of the body behind it are hidden, not drawn as visible lines across
/// the hatch. A body 60 deep with a tab sticking out at the back, cut at
/// x = 40 and seen from +x: the tab's edges lie behind the 40 x 20 cut face.
#[test]
fn a_section_hides_what_lies_behind_the_cut_face() {
    let outline = Loop::polygon(&[
        P2::new(20.0, 0.0),
        P2::new(60.0, 0.0),
        P2::new(60.0, 40.0),
        P2::new(20.0, 40.0),
        P2::new(20.0, 30.0),
        P2::new(0.0, 30.0),
        P2::new(0.0, 10.0),
        P2::new(20.0, 10.0),
    ]);
    let solids = kernel::extrude(&Sketch::new(Plane::XY).with_loop(outline), 20.0).expect("extrude");
    let (mesh, topo) = tessellate(&solids[0], 0.05).expect("tessellate");
    let plane = Plane::YZ.offset(40.0);
    let d = crate::view::section_drawing(&mesh, &topo, &plane, 0.05, crate::ir::Pattern::Solid, Some(&ProjectOptions::default()));
    let prims = d.flatten();
    let points_on = |want: &str| -> Vec<P2> {
        prims
            .iter()
            .filter_map(|p| match p {
                Prim::Path { style, path, .. } if style.layer == want => {
                    // The ends and the midpoint of every segment, so a line
                    // running across the face from edge to edge is seen.
                    let ends: Vec<P2> = std::iter::once(path.start).chain(path.segments.iter().map(|g| g.end())).collect();
                    let mids: Vec<P2> = ends.windows(2).map(|w| P2::new((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0)).collect();
                    Some(ends.into_iter().chain(mids).collect::<Vec<P2>>())
                }
                _ => None,
            })
            .flatten()
            .collect()
    };
    let cut = points_on(layer::SECTION);
    assert!(!cut.is_empty(), "the cut outline is drawn");
    let (lo, hi) = cut.iter().fold((P2::new(f64::MAX, f64::MAX), P2::new(f64::MIN, f64::MIN)), |(lo, hi), p| {
        (P2::new(lo.x.min(p.x), lo.y.min(p.y)), P2::new(hi.x.max(p.x), hi.y.max(p.y)))
    });
    let inside: Vec<P2> = points_on(layer::VISIBLE)
        .into_iter()
        .filter(|p| p.x > lo.x + 0.5 && p.x < hi.x - 0.5 && p.y > lo.y + 0.5 && p.y < hi.y - 0.5)
        .collect();
    assert!(inside.is_empty(), "visible lines inside the cut face ({lo:?}..{hi:?}): {inside:?}");
}

/// Two collinear lines that meet end to end share their endpoint: the
/// hidden-line pass used to count a touching neighbour as covering the last
/// `tol` of a line, so two flush boxes' top and bottom lines stopped short of
/// x = 40 and left a gap in the DXF and SVG output.
#[test]
fn collinear_lines_that_meet_end_to_end_keep_their_shared_point() {
    let (mesh, topo) = boxes(&[([0.0, 0.0, 0.0], 40.0, 40.0, 10.0), ([40.0, 0.0, 0.0], 40.0, 40.0, 10.0)]);
    let p = project(&mesh, &topo, &ViewBasis::preset("front"), &ProjectOptions::default());
    let ends: Vec<f64> = p.visible.iter().flat_map(|l| [l[0].x, l[l.len() - 1].x]).collect();
    // Every end near x = ±40 (whichever way the view mirrors x) is exactly
    // there.
    for x in ends.iter().filter(|x| (x.abs() - 40.0).abs() < 0.01) {
        assert!((x.abs() - 40.0).abs() < 1e-9, "a line ends at x = {x}, not 40: {:?}", p.visible);
    }
    assert!(ends.iter().any(|x| (x.abs() - 40.0).abs() < 1e-9), "no line ends at the shared corner: {:?}", p.visible);
}

#[test]
fn a_through_hole_seen_down_its_bore_is_one_path_per_rim_arc() {
    // A 60 x 30 x 6 plate with a 5.5 mm through hole, from above. The top
    // rim is two arcs and the bottom rim projects exactly onto it; looking
    // straight down the bore, the bottom rim flickers between visible and
    // hidden along the grazing wall. The hole must still come out as the two
    // arcs of its top rim, drawn once, with no slivers: a cutter following
    // the file makes one pass round the hole, not a dozen.
    let r = 2.75;
    let s = Sketch::new(Plane::XY)
        .with_loop(Loop::rect(P2::new(0.0, 0.0), 60.0, 30.0))
        .with_loop(Loop::circle(P2::new(-22.0, 0.0), r));
    let solids = kernel::extrude(&s, 6.0).expect("extrude");
    let (mesh, topo) = tessellate(&solids[0], 0.05).expect("tessellate");
    let p = project(&mesh, &topo, &ViewBasis::preset("top"), &ProjectOptions::default());
    let near_hole = |poly: &Vec<P2>| poly.iter().all(|q| ((q.x + 22.0).powi(2) + q.y.powi(2)).sqrt() < r + 0.1);
    let hole: Vec<&Vec<P2>> = p.visible.iter().filter(|l| near_hole(l)).collect();
    assert!(
        (1..=2).contains(&hole.len()),
        "the hole is its two rim arcs, not {} pieces",
        hole.len()
    );
    let ink: f64 = hole
        .iter()
        .flat_map(|l| l.windows(2))
        .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
        .sum();
    let round = 2.0 * std::f64::consts::PI * r;
    assert!((ink - round).abs() < 0.01 * round, "inked once round: {ink} vs {round}");
    assert!(p.hidden.iter().all(|l| !near_hole(l)), "no hidden rim drawn under the visible one");
    for l in p.visible.iter().chain(&p.hidden) {
        for w in l.windows(2) {
            let len = ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
            assert!(len > 1e-3, "a {len} mm segment in {l:?}");
        }
    }
}
