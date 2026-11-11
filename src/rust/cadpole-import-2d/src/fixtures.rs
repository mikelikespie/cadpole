//! Tests against the files under `//examples/assets`.
//!
//! The unit tests in `svg` and `dxf` use string literals, which is the right
//! shape for "does this one command parse". These read the same files the
//! example scripts import, so the fixtures cannot rot: if `badge.svg` stops
//! having an arc in it, or `plate.dxf` stops needing its lines joined, the
//! thing the examples are meant to demonstrate has quietly gone away.

#![cfg(test)]

use cadpole_core::sketch::{Segment, arc_params};
use cadpole_core::{Plane, Sketch};

use crate::{dxf, svg};

fn fixture(name: &str) -> Vec<u8> {
    let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
    let path = std::path::Path::new(&dir).join("assets").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Build a sketch the way `import_svg` / `import_dxf` do, so `regions()` sees
/// exactly what the feature would hand to `extrude`.
fn sketch(im: &crate::Import) -> Sketch {
    im.loops
        .iter()
        .fold(Sketch::new(Plane::XY), |s, (n, l)| s.with_named_loop(l.clone(), n.clone()))
}

#[test]
fn the_badge_fixture_keeps_its_arcs_and_nests_its_holes() {
    let im = svg::import(&fixture("badge.svg"), &svg::Options::default()).unwrap();
    // Five filled loops: the plate, its window, two rivets and the wave.
    assert_eq!(im.loops.len(), 5, "{:?}", im.loops.iter().map(|(n, _)| n).collect::<Vec<_>>());
    // The unfilled guide line is a construction path, not a face.
    assert_eq!(im.paths.len(), 1);
    assert!(im.paths[0].0.starts_with("guide/"), "{}", im.paths[0].0);

    // The plate's two `A` commands stayed circular arcs of radius 16 mm.
    let (_, plate) = im.loops.iter().find(|(n, _)| n.starts_with("badge/plate")).unwrap();
    let radii: Vec<f64> = plate
        .segments
        .iter()
        .filter_map(|s| match s {
            Segment::Arc { a, b, via } => arc_params(*a, *b, *via).map(|(_, r, _, _)| r),
            _ => None,
        })
        .collect();
    assert_eq!(radii.len(), 2, "two arcs survived as arcs");
    for r in radii {
        assert!((r - 16.0).abs() < 1e-6, "arc radius {r}");
    }

    // The rivets came through <use> of a <circle> and are still two arcs each.
    let rivets: Vec<_> = im.loops.iter().filter(|(n, _)| n.contains("/use[")).collect();
    assert_eq!(rivets.len(), 2);
    for (_, l) in &rivets {
        assert_eq!(l.segments.len(), 2);
        assert!(l.segments.iter().all(|s| matches!(s, Segment::Arc { .. })));
    }

    // One region with four holes: the window, both rivets and the wave.
    let regions = sketch(&im).regions().unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes.len(), 4, "nested subpaths and marks are holes");
    assert!(regions[0].outer.signed_area() > 0.0);
    assert!(regions[0].holes.iter().all(|h| h.signed_area() < 0.0));

    // 40 mm wide in the file, 40 mm wide in the model.
    let (lo, hi) = im.bbox().unwrap();
    assert!((hi.x - lo.x - 32.0).abs() < 0.01, "the artwork spans 4..36 mm: {lo:?} {hi:?}");
}

#[test]
fn the_plate_fixture_has_real_fillet_radii_and_needs_joining() {
    let bytes = fixture("plate.dxf");
    let im = dxf::import(&bytes, &dxf::Options::default()).unwrap();

    // The rounded rectangle's four bulges are 8 mm arcs, not polylines.
    let (_, outline) = im.loops.iter().find(|(n, _)| n.starts_with("OUTLINE/")).unwrap();
    let radii: Vec<f64> = outline
        .segments
        .iter()
        .filter_map(|s| match s {
            Segment::Arc { a, b, via } => arc_params(*a, *b, *via).map(|(_, r, _, _)| r),
            _ => None,
        })
        .collect();
    assert_eq!(radii.len(), 4, "four bulged corners");
    for r in &radii {
        assert!((r - 8.0).abs() < 1e-6, "fillet radius {r}");
    }
    // Area of a 60 x 40 rectangle with four r = 8 corners rounded off.
    let a = outline.signed_area().abs();
    let exact = 60.0 * 40.0 - (4.0 - std::f64::consts::PI) * 64.0;
    assert!((a - exact).abs() < 1.0, "outline area {a} vs {exact}");

    // The A-WALL layer is four unconnected LINEs; joining closes them.
    let (_, wall) = im.loops.iter().find(|(n, _)| n.starts_with("A-WALL/loop[")).unwrap();
    assert_eq!(wall.segments.len(), 4);
    assert!(wall.is_closed(1e-9));
    assert!((wall.signed_area().abs() - 600.0).abs() < 1e-6);

    // The lone NOTES line cannot close, so it stays a construction path.
    assert_eq!(im.paths.len(), 1, "{:?}", im.paths.iter().map(|(n, _)| n).collect::<Vec<_>>());
    assert!(im.paths[0].0.starts_with("NOTES/"), "{}", im.paths[0].0);

    // Without joining, the same four lines bound nothing at all.
    let loose = dxf::import(&bytes, &dxf::Options { join: false, ..Default::default() }).unwrap();
    assert!(!loose.loops.iter().any(|(n, _)| n.starts_with("A-WALL/loop[")));
    assert_eq!(loose.paths.len(), 5, "four wall lines plus the note");

    // The circle on HOLES is inside the outline, so it nests as a hole.
    let regions = sketch(&im).regions().unwrap();
    let with_hole = regions.iter().find(|r| !r.holes.is_empty()).unwrap();
    assert_eq!(with_hole.holes.len(), 1);
    // `signed_area` samples arcs at 16 steps, so a 5 mm circle comes out
    // ~0.6 % under the exact area; that is the sampler, not the import.
    let hole = with_hole.holes[0].signed_area().abs();
    assert!((hole - std::f64::consts::PI * 25.0).abs() < 0.6, "hole area {hole}");
    assert_eq!(with_hole.holes[0].segments.len(), 2, "a CIRCLE is two arcs, not a polygon");

    // A layer selector narrows it to just the walls.
    let walls =
        dxf::import(&bytes, &dxf::Options { layers: vec!["A-*".into()], ..Default::default() }).unwrap();
    assert_eq!(walls.loops.len(), 1);
    assert!(walls.paths.is_empty());
}
