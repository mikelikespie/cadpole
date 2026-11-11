//! Tests that run the whole pipeline on real solids: tessellate, project,
//! section, dimension, lay out.
//!
//! These are the ones with numbers in them. A box seen from a corner has nine
//! visible edges and three hidden ones; a section through a hollow cylinder
//! has the area of an annulus. Both are things you can check on paper, which
//! is the point — a hidden-line renderer that is only tested against its own
//! output is testing nothing.

#![cfg(test)]

use cadpole_core::geom::Plane;
use cadpole_core::kernel;
use cadpole_core::sketch::{Loop, Sketch, P2};
use cadpole_core::tessellate::{tessellate, Mesh, Topology};

use crate::auto;
use crate::dim::DimStyle;
use crate::ir::Style;
use crate::sheet::{three_view, SheetSize, ThreeViewOptions, TitleBlock};
use crate::units::UnitFormat;
use crate::view::{project, section_loops, Hidden, ProjectOptions, ViewBasis};

/// Build a solid from one sketch and tessellate it.
fn solid_of(s: &Sketch, height: f64, tolerance: f64) -> (Mesh, Topology) {
    let solids = kernel::extrude(s, height).expect("extrude");
    assert_eq!(solids.len(), 1, "one body");
    tessellate(&solids[0], tolerance).expect("tessellate")
}

fn box_solid(w: f64, d: f64, h: f64) -> (Mesh, Topology) {
    let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
    solid_of(&s, h, 0.05)
}

#[test]
fn a_box_seen_from_a_corner_has_nine_visible_and_three_hidden_edges() {
    let (mesh, topo) = box_solid(40.0, 20.0, 10.0);
    // A box has twelve edges; from an isometric corner exactly three of them
    // meet at the far corner and are hidden behind the solid.
    assert_eq!(
        topo.edges
            .iter()
            .filter(|e| e.dihedral_degrees >= 15.0)
            .count(),
        12,
        "twelve creases"
    );

    let basis = ViewBasis::preset("iso");
    let p = project(&mesh, &topo, &basis, &ProjectOptions::default());
    assert_eq!(p.visible.len(), 9, "visible edges");
    assert_eq!(p.hidden.len(), 3, "hidden edges");

    // Omitting them leaves the nine.
    let omitted = project(
        &mesh,
        &topo,
        &basis,
        &ProjectOptions {
            hidden: Hidden::Omit,
            ..Default::default()
        },
    );
    assert_eq!(omitted.visible.len(), 9);
    assert!(omitted.hidden.is_empty());
}

#[test]
fn a_box_seen_face_on_shows_its_outline_and_hides_the_back() {
    let (mesh, topo) = box_solid(40.0, 20.0, 10.0);
    let p = project(&mesh, &topo, &topo_front(), &ProjectOptions::default());
    // Straight on, the four front edges are visible, the four back ones are
    // exactly behind them (still "visible" — nothing is nearer), and the four
    // edges running away from the viewer project to points.
    assert!(p.visible.len() >= 4, "{} visible", p.visible.len());
    let (lo, hi) = p.bounds().unwrap();
    assert!((hi.x - lo.x - 40.0).abs() < 1e-6, "width {}", hi.x - lo.x);
    assert!((hi.y - lo.y - 10.0).abs() < 1e-6, "height {}", hi.y - lo.y);
}

fn topo_front() -> ViewBasis {
    ViewBasis::preset("front")
}

#[test]
fn a_section_through_a_hollow_cylinder_is_an_annulus_of_the_right_area() {
    let (r_out, r_in, h) = (20.0, 12.0, 30.0);
    let s = Sketch::new(Plane::XY)
        .with_loop(Loop::circle(P2::new(0.0, 0.0), r_out))
        .with_loop(Loop::circle(P2::new(0.0, 0.0), r_in));
    let (mesh, _topo) = solid_of(&s, h, 0.02);

    let plane = Plane::XY.offset(h / 2.0);
    let loops = section_loops(&mesh, &plane, 0.01);
    assert_eq!(loops.len(), 2, "an outer circle and an inner one");

    let sketch = loops
        .iter()
        .fold(Sketch::new(plane), |sk, l| sk.with_loop(l.clone()));
    let regions = sketch.regions().expect("nesting");
    assert_eq!(regions.len(), 1, "one region");
    assert_eq!(regions[0].holes.len(), 1, "with one hole");

    let area = regions[0].outer.signed_area().abs()
        - regions[0]
            .holes
            .iter()
            .map(|l| l.signed_area().abs())
            .sum::<f64>();
    let exact = std::f64::consts::PI * (r_out * r_out - r_in * r_in);
    assert!(
        (area - exact).abs() / exact < 0.01,
        "annulus area {area:.3} against pi(R^2 - r^2) = {exact:.3}"
    );

    // And the cut is where it was asked for: both loops are centred on the
    // axis with the right radii.
    for (l, r) in [(&regions[0].outer, r_out), (&regions[0].holes[0], r_in)] {
        for p in l.sample() {
            let d = (p.x * p.x + p.y * p.y).sqrt();
            assert!((d - r).abs() < 0.2, "radius {d} against {r}");
        }
    }
}

#[test]
fn a_section_drawing_hatches_the_cut_face() {
    let s = Sketch::new(Plane::XY)
        .with_loop(Loop::circle(P2::new(0.0, 0.0), 20.0))
        .with_loop(Loop::circle(P2::new(0.0, 0.0), 12.0));
    let (mesh, topo) = solid_of(&s, 30.0, 0.05);
    let d = crate::view::section_drawing(
        &mesh,
        &topo,
        &Plane::XY.offset(15.0),
        0.01,
        crate::ir::Pattern::ANSI31,
        None,
    );
    let prims = d.flatten();
    let hatch_lines = prims
        .iter()
        .filter(|p| matches!(p, crate::ir::Prim::Path { style, .. } if style.layer == crate::ir::layer::SECTION_HATCH))
        .count();
    assert!(
        hatch_lines > 20,
        "the annulus is hatched: {hatch_lines} lines"
    );
    let cut = prims
        .iter()
        .filter(|p| matches!(p, crate::ir::Prim::Path { style, .. } if style.layer == crate::ir::layer::SECTION))
        .count();
    assert_eq!(
        cut, 2,
        "both boundaries of the annulus are outlined heavily"
    );
}

#[test]
fn a_three_view_sheet_lays_out_four_views_and_dimensions_the_part() {
    let (mesh, topo) = box_solid(80.0, 40.0, 25.0);
    let opts = ThreeViewOptions {
        sheet: SheetSize::A3,
        title: TitleBlock {
            title: "Block".into(),
            number: "1".into(),
            ..Default::default()
        },
        unit_format: UnitFormat::mm(),
        ..Default::default()
    };
    let d = three_view(&mesh, &topo, &opts);

    let groups: Vec<&str> = d
        .items
        .iter()
        .filter_map(|i| match i {
            crate::ir::Item::Group(g) => Some(g.name.as_str()),
            _ => None,
        })
        .collect();
    for want in ["front", "top", "right", "iso"] {
        assert!(
            groups.contains(&want),
            "{want} view missing from {groups:?}"
        );
    }

    // The dimensions read the real part, not the paper.
    let texts: Vec<String> = d
        .flatten()
        .iter()
        .filter_map(|p| match p {
            crate::ir::Prim::Text { text, style, .. } if style.layer == crate::ir::layer::DIMS => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect();
    assert!(texts.contains(&"80".to_owned()), "width: {texts:?}");
    assert!(texts.contains(&"25".to_owned()), "height: {texts:?}");
    assert!(texts.contains(&"40".to_owned()), "depth: {texts:?}");

    // Everything lands on the sheet.
    let (lo, hi) = d.bounds().expect("bounds");
    assert!(lo.x > -25.0 && lo.y > -25.0, "{lo:?}");
    assert!(
        hi.x < SheetSize::A3.w_mm + 25.0 && hi.y < SheetSize::A3.h_mm + 25.0,
        "{hi:?}"
    );
    // And the title block picked up the fitted scale.
    let sheet = d.sheet.as_ref().unwrap();
    assert!(!sheet.title_block.scale.is_empty(), "scale is filled in");
    assert_eq!(sheet.title_block.scale, d.scale.label(&UnitFormat::mm()));
}

#[test]
fn a_dimensioned_rectangle_reads_the_same_number_in_both_unit_systems() {
    // 12'-6" by 8'-0" — a room, as a sketch.
    let (w, h) = (3810.0, 2438.4);
    let outline = Loop::rect(P2::new(w / 2.0, h / 2.0), w, h);
    let pts = outline.sample();
    let (lo, hi) = (P2::new(0.0, 0.0), P2::new(w, h));
    assert_eq!(pts.len(), 4);

    let mm = auto::extents_dims(Style::default(), lo, hi, &DimStyle::mechanical(), 10.0);
    assert_eq!(
        mm.iter().map(|d| d.text()).collect::<Vec<_>>(),
        vec!["3810", "2438.4"]
    );

    let ft = auto::extents_dims(Style::default(), lo, hi, &DimStyle::architectural(), 10.0);
    assert_eq!(
        ft.iter().map(|d| d.text()).collect::<Vec<_>>(),
        vec!["12'-6\"", "8'-0\""]
    );
}

#[test]
fn text_placement_is_deterministic_across_a_whole_sheet() {
    let (mesh, topo) = box_solid(63.5, 41.25, 18.0);
    let opts = ThreeViewOptions {
        sheet: SheetSize::A4,
        ..Default::default()
    };
    let render = || {
        let d = three_view(&mesh, &topo, &opts);
        crate::svg::write(&d, &crate::svg::SvgOptions::default())
    };
    assert_eq!(
        render(),
        render(),
        "the same model gives byte-identical SVG"
    );
}

/// The same mesh with its triangles shuffled: same geometry, different arrival
/// order.
fn shuffled(mesh: &Mesh) -> Mesh {
    let n = mesh.triangle_count();
    let mut out = Mesh {
        positions: mesh.positions.clone(),
        normals: mesh.normals.clone(),
        ..Default::default()
    };
    // Reversal: a real reordering, always a bijection (a stride would only be
    // one when it is coprime with the count, and silently drop triangles when
    // it is not), and deterministic.
    for i in (0..n).rev() {
        out.indices
            .extend_from_slice(&mesh.indices[i * 3..i * 3 + 3]);
        out.face_ids
            .push(mesh.face_ids.get(i).copied().unwrap_or(0));
    }
    out
}

#[test]
fn a_drawing_is_a_function_of_the_geometry_not_of_the_triangle_order() {
    // A mesh boolean is under no obligation to hand back its triangles in the
    // same order twice, and manifold does not. If the drawing depended on that
    // order, two exports of one model would differ — so it must not.
    let s = Sketch::new(Plane::XY)
        .with_loop(Loop::circle(P2::new(0.0, 0.0), 20.0))
        .with_loop(Loop::circle(P2::new(0.0, 0.0), 12.0));
    let (mesh, topo) = solid_of(&s, 30.0, 0.05);
    let mixed = shuffled(&mesh);
    assert_eq!(mixed.triangle_count(), mesh.triangle_count());
    assert_ne!(
        mixed.indices, mesh.indices,
        "the permutation really permuted"
    );
    // A permutation, not a resampling: exactly the same triangles.
    let key = |m: &Mesh| {
        let mut v: Vec<[u32; 3]> = m
            .indices
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(key(&mesh), key(&mixed), "same triangles, different order");

    // Hidden-line projection, including the smooth silhouettes that are found
    // from the mesh rather than from the topology.
    let basis = ViewBasis::preset("iso");
    let a = project(&mesh, &topo, &basis, &ProjectOptions::default());
    let b = project(&mixed, &topo, &basis, &ProjectOptions::default());
    assert_eq!(a.visible, b.visible, "visible edges");
    assert_eq!(a.hidden, b.hidden, "hidden edges");

    // And the section: same loops, in the same order, starting at the same
    // vertex.
    let plane = Plane::XY.offset(15.0);
    assert_eq!(
        section_loops(&mesh, &plane, 0.01),
        section_loops(&mixed, &plane, 0.01)
    );

    // Which means the written file is the same bytes.
    let opts = ThreeViewOptions {
        sheet: SheetSize::A4,
        ..Default::default()
    };
    let svg = |m: &Mesh| {
        crate::svg::write(
            &three_view(m, &topo, &opts),
            &crate::svg::SvgOptions::default(),
        )
    };
    assert_eq!(svg(&mesh), svg(&mixed));
}
