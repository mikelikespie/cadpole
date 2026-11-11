//! Round trips: write a drawing, read it back with Cadpole's own importer, and
//! check the geometry survived.
//!
//! This is the strongest end-to-end check in the drawing pipeline, because it
//! closes the loop with code that was written without knowing about the writer.
//! `cadpole-import-2d` parses SVG `A` commands and DXF polyline bulges into real
//! [`cadpole_core::sketch::Segment::Arc`]s; if the writers had quietly turned a
//! circle into chords, or lost the y flip, or got the arc sweep flag backwards,
//! the areas and the radii would not come back.
//!
//! Geometry is compared by shape, not by structure: the importer is free to
//! split a half-turn arc, start a loop at a different vertex or reverse its
//! winding — all of which describe the same curve. So the comparison is the
//! symmetric maximum deviation between the two sampled outlines, which is zero
//! only when they really are the same curve.

#![cfg(test)]

use cadpole_core::geom::Plane;
use cadpole_core::sketch::{Loop, Path, Segment, Sketch, P2};
use cadpole_drawing::{from_sketches, SketchOptions};

/// A sketch with everything the writers have to get right: straight lines, a
/// full circle (two half arcs), a rounded rectangle (quarter arcs), a hole,
/// and an open construction path.
fn specimen() -> Sketch {
    Sketch::new(Plane::XY)
        .with_named_loop(Loop::rect(P2::new(60.0, 40.0), 120.0, 80.0), "plate")
        .with_named_loop(Loop::circle(P2::new(30.0, 40.0), 12.0), "bore")
        .with_named_loop(
            Loop::rounded_rect(P2::new(90.0, 40.0), 40.0, 30.0, 6.0),
            "window",
        )
        .with_path(
            Path::new(P2::new(0.0, 0.0)).line_to(P2::new(120.0, 0.0)),
            "datum",
        )
}

/// Points along a loop, densely enough that a deviation test means something.
fn outline(l: &Loop) -> Vec<P2> {
    l.sample_tolerance(0.002)
}

/// Closest approach from `p` to the polyline `pts` (treated as closed).
fn distance_to(pts: &[P2], p: P2) -> f64 {
    let mut best = f64::MAX;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 < 1e-18 {
            0.0
        } else {
            (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
        };
        let (qx, qy) = (a.x + dx * t, a.y + dy * t);
        best = best.min(((p.x - qx).powi(2) + (p.y - qy).powi(2)).sqrt());
    }
    best
}

/// Symmetric maximum deviation between two closed outlines.
fn deviation(a: &Loop, b: &Loop) -> f64 {
    let (pa, pb) = (outline(a), outline(b));
    let mut worst: f64 = 0.0;
    for p in &pa {
        worst = worst.max(distance_to(&pb, *p));
    }
    for p in &pb {
        worst = worst.max(distance_to(&pa, *p));
    }
    worst
}

/// Match each original loop to the imported loop nearest to it, and report the
/// worst deviation over the lot. Fails loudly if the counts differ.
fn compare(original: &[Loop], imported: &[Loop], shift: P2) -> f64 {
    assert_eq!(imported.len(), original.len(), "loop count");
    let moved: Vec<Loop> = imported
        .iter()
        .map(|l| Loop {
            segments: l
                .segments
                .iter()
                .map(|s| map_segment(s, |p| P2::new(p.x + shift.x, p.y + shift.y)))
                .collect(),
        })
        .collect();
    let mut worst: f64 = 0.0;
    for o in original {
        let best = moved
            .iter()
            .map(|m| deviation(o, m))
            .fold(f64::MAX, f64::min);
        worst = worst.max(best);
    }
    worst
}

fn map_segment(s: &Segment, f: impl Fn(P2) -> P2) -> Segment {
    match s {
        Segment::Line { a, b } => Segment::Line { a: f(*a), b: f(*b) },
        Segment::Arc { a, b, via } => Segment::Arc {
            a: f(*a),
            b: f(*b),
            via: f(*via),
        },
        Segment::Bezier { a, b, ctrl } => Segment::Bezier {
            a: f(*a),
            b: f(*b),
            ctrl: ctrl.iter().map(|c| f(*c)).collect(),
        },
    }
}

/// The offset the writers apply: content is placed with its bounding box at the
/// page origin plus the margin, so an import comes back shifted by that much.
fn page_shift(d: &cadpole_drawing::Drawing, margin: f64) -> P2 {
    let (lo, _) = cadpole_drawing::svg::page(d, margin);
    P2::new(lo.x, lo.y)
}

#[test]
fn an_svg_of_filled_regions_reimports_as_the_same_loops() {
    let s = specimen();
    let d = from_sketches(
        &[("plate".to_owned(), s.clone())],
        &SketchOptions {
            fill: true,
            ..Default::default()
        },
    );
    let svg = cadpole_drawing::svg::write(&d, &cadpole_drawing::svg::SvgOptions::default());
    let back =
        cadpole_import_2d::svg::import(svg.as_bytes(), &cadpole_import_2d::svg::Options::default())
            .expect("our own SVG parses");

    let shift = page_shift(&d, cadpole_drawing::svg::SvgOptions::default().margin_mm);
    let loops: Vec<Loop> = back.loops.iter().map(|(_, l)| l.clone()).collect();
    let worst = compare(&s.loops, &loops, shift);
    assert!(worst < 1e-6, "worst deviation {worst} mm");

    // The arcs came back as arcs, not as chords. Compare against the source
    // loop's own area rather than against pi*r^2, because `signed_area` samples
    // curves and both sides sample them the same way — what is being tested is
    // that the *curve* survived, not how it is integrated.
    let want = s.loops[1].signed_area().abs();
    let areas: Vec<f64> = loops.iter().map(|l| l.signed_area().abs()).collect();
    assert!(
        areas.iter().any(|a| (a - want).abs() < 1e-6),
        "no circular bore of area {want} in {areas:?}"
    );
    let arcs = loops
        .iter()
        .flat_map(|l| l.segments.iter())
        .filter(|sg| matches!(sg, Segment::Arc { .. }))
        .count();
    assert!(
        arcs >= 6,
        "the bore and the fillets are real arcs, not chords: {arcs}"
    );
    // A filled export names the *region*, since that is the element the
    // writer emits; the layer name is the sketch feature either way.
    assert!(
        back.loops
            .iter()
            .all(|(n, _)| n.starts_with("plate/plate/region")),
        "region names survive: {:?}",
        back.loops
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
    );

    // And the construction path is still an open path, not a face.
    assert_eq!(back.paths.len(), 1, "one construction path");
    assert!(back.paths[0].0.contains("datum"), "{}", back.paths[0].0);
}

#[test]
fn a_stroked_svg_reimports_as_construction_outlines_of_the_same_shape() {
    // The drawing default is `fill=None`: an unfilled closed path bounds
    // nothing, so the importer hands it back as a construction outline rather
    // than a face. The *geometry* must
    // still be identical.
    let s = specimen();
    let d = from_sketches(
        &[("plate".to_owned(), s.clone())],
        &SketchOptions::default(),
    );
    let svg = cadpole_drawing::svg::write(&d, &cadpole_drawing::svg::SvgOptions::default());
    let back =
        cadpole_import_2d::svg::import(svg.as_bytes(), &cadpole_import_2d::svg::Options::default())
            .expect("our own SVG parses");
    assert!(back.loops.is_empty(), "nothing is filled");

    let shift = page_shift(&d, cadpole_drawing::svg::SvgOptions::default().margin_mm);
    let closed: Vec<Loop> = back
        .paths
        .iter()
        .filter(|(n, _)| !n.contains("datum"))
        .map(|(_, p)| Loop {
            segments: p.segments.clone(),
        })
        .collect();
    let worst = compare(&s.loops, &closed, shift);
    assert!(worst < 1e-6, "worst deviation {worst} mm");

    // Ids equal Cadpole names, so a round trip re-imports under the same names.
    let names: Vec<&str> = back.paths.iter().map(|(n, _)| n.as_str()).collect();
    for want in ["plate", "bore", "window", "datum"] {
        assert!(
            names.iter().any(|n| n.contains(want)),
            "{want} missing from {names:?}"
        );
    }
}

#[test]
fn a_dxf_reimports_as_the_same_loops() {
    let s = specimen();
    let d = from_sketches(
        &[("plate".to_owned(), s.clone())],
        &SketchOptions::default(),
    );
    let bytes = crate::dxf::write(&d, &crate::dxf::DxfOptions::default()).expect("write");
    let back = cadpole_import_2d::dxf::import(
        &bytes,
        &cadpole_import_2d::dxf::Options {
            units: "mm".into(),
            ..Default::default()
        },
    )
    .expect("our own DXF parses");

    // DXF is written in the drawing's own coordinates, so there is no shift.
    let loops: Vec<Loop> = back.loops.iter().map(|(_, l)| l.clone()).collect();
    let worst = compare(&s.loops, &loops, P2::new(0.0, 0.0));
    assert!(worst < 1e-6, "worst deviation {worst} mm");

    // The bore is still a real arc pair, which is the whole point of writing
    // bulges rather than chords.
    let arcs = loops
        .iter()
        .flat_map(|l| l.segments.iter())
        .filter(|s| matches!(s, Segment::Arc { .. }))
        .count();
    assert!(arcs >= 6, "two half arcs and four fillets: {arcs}");

    // The layer name came through, which is what makes an NCS-layered plan
    // open into an architect's template.
    assert!(String::from_utf8_lossy(&bytes).contains("plate"));
}

#[test]
fn a_dxf_of_a_dimensioned_drawing_carries_its_text() {
    use cadpole_drawing::{layer, Dim, DimStyle, Drawing, Item, Style};
    let mut d = Drawing::new();
    let dl = d.layer(layer::DIMS);
    let ds = DimStyle {
        format: cadpole_drawing::UnitFormat::ft_in(),
        ..DimStyle::architectural()
    };
    d.push(Item::Dim(Dim::linear(
        Style::on(dl),
        P2::new(0.0, 0.0),
        P2::new(3810.0, 0.0),
        P2::new(1.0, 0.0),
        -20.0,
        ds,
    )));
    let bytes = crate::dxf::write(&d, &crate::dxf::DxfOptions::default()).expect("write");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("12'-6\""),
        "the figured dimension is in the file"
    );
    assert!(s.contains("A-ANNO-DIMS"), "on the NCS annotation layer");
}
