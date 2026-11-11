//! Cadpole's 2D drawing pipeline: the IR, views computed from a model, automatic
//! dimensioning and the SVG writer.
//!
//! *Mechanical* drawings need it as much as floor plans do:
//! a hidden-line three-view of a printed
//! part and a dimensioned plan of a house are the same pipeline with
//! different defaults.
//!
//! ```text
//!   solid ──tessellate──► Mesh + Topology
//!                              │
//!            view::project ────┤────► view::section_loops
//!         (hidden lines)       │       (cut faces)
//!                              ▼
//!                          Drawing (ir)
//!        layers · strokes · hatches · text · dimensions · sheet
//!                              │
//!         auto::place ─────────┤  (dimension text that does not collide)
//!                              ▼
//!                      Drawing::flatten  →  [Prim]   paper millimetres
//!                              │
//!              ┌───────────────┼───────────────┐
//!            svg            pdf (out)       dxf (out)
//! ```
//!
//! Three rules run through all of it.
//!
//! **Geometry is model units; ink is paper units.** A dimension states a true
//! length whatever scale its view is placed at, and a 2.5 mm text height is
//! 2.5 mm on the paper at 1:1 and at 1:48. [`ir::Drawing::flatten`] is the one
//! place the two meet.
//!
//! **Arcs stay arcs.** Group transforms are uniform scale plus translation, so
//! a circle is still a circle when it reaches the writers, and an exported
//! drawing re-imports through `cadpole-import-2d` as the same loops.
//!
//! **One flattening, three writers.** SVG, PDF and DXF all consume the same
//! [`ir::Prim`] list, so they cannot disagree about where a dimension sits.
//!
//! And one consequence worth stating: **a drawing is a function of the
//! geometry**, not of the order the triangles arrived in. Chains are rotated
//! and oriented canonically, welded points keep their lowest member, and the
//! writers stamp no clock and no random id — so the same mesh exports to the
//! same bytes. (The *mesh* is another matter: a model routed through the mesh
//! kernel does not come back identical from one process to the next, which
//! `cadpole export -o x.stl` shows on its own.)

pub mod auto;
pub mod dim;
pub mod hatch;
pub mod ir;
pub mod model;
pub mod painted;
pub mod plate;
pub mod sheet;
pub mod sheetset;
pub mod stdfont;
pub mod svg;
pub mod text;
pub mod units;
pub mod view;
pub mod views;

#[cfg(test)]
mod integration;
#[cfg(test)]
mod regressions;

pub use dim::{Arrow, Dim, DimKind, DimStyle};
pub use ir::{
    layer, Anchor, ClipRect, Dash, Drawing, FontRef, Group, HAlign, HatchItem, Image, ImageItem,
    Item, Layer, LayerId, Pattern, Prim, Resolved, Rgb, Scale, Stroke, Style, TextItem, VAlign,
    Xform2,
};
pub use model::DrawingOptions;
pub use sheet::{three_view, ProjectionAngle, Sheet, SheetSize, ThreeViewOptions, TitleBlock};
pub use sheetset::{SheetPage, SheetSet, Swatch, TitleTemplate};
pub use stdfont::StdFont;
pub use units::{format_length, parse_length, LengthUnit, UnitFormat};
pub use view::{
    section_drawing, section_loops, BodyRanges, HalfSpace, Hidden, ProjectOptions, Projection,
    ViewBasis,
};
pub use views::{Look, ViewBody, ViewGeometry, ViewOptions};

use cadpole_core::sketch::Sketch;

/// How a bare sketch becomes a drawing, for `export_svg` / `export_dxf`.
#[derive(Clone, Debug)]
pub struct SketchOptions {
    /// Plotted width of the geometry, in paper millimetres.
    pub stroke_mm: f64,
    /// Fill the sketch's regions instead of outlining its loops. A filled
    /// drawing re-imports as *loops*; a stroked one re-imports as construction
    /// paths, because an unfilled shape bounds nothing.
    /// Both keep the geometry exactly.
    pub fill: bool,
    pub unit_format: UnitFormat,
    /// Layer for sketches that have no feature name of their own.
    pub default_layer: String,
}

impl Default for SketchOptions {
    fn default() -> Self {
        SketchOptions {
            stroke_mm: 0.25,
            fill: false,
            unit_format: UnitFormat::mm(),
            default_layer: "SKETCH".to_owned(),
        }
    }
}

/// Turn named sketches into a 1:1 drawing: one layer per sketch (so a DXF
/// opens with one layer per feature), one named path per loop, construction
/// paths kept as open strokes.
///
/// Geometry is in the *first* sketch's 2D coordinates, and the drawing scale
/// is 1:1, so model millimetres and paper millimetres are the same number and
/// the round trip through `cadpole-import-2d` is exact. A later sketch on
/// another plane is carried through world space onto the first one's, so
/// text set on a plane turned in the XY plane comes out turned rather than
/// in its own plane's coordinates. A sketch on a plane that is not parallel
/// is projected along the first plane's normal.
pub fn from_sketches(sketches: &[(String, Sketch)], opts: &SketchOptions) -> Drawing {
    let mut d = Drawing::new();
    d.unit_format = opts.unit_format;
    let reference = sketches.first().map(|(_, s)| s.plane);
    for (name, s) in sketches {
        let s = match reference {
            Some(r) if s.plane != r => &onto_plane(s, &r),
            _ => s,
        };
        let layer_name = if name.is_empty() {
            opts.default_layer.clone()
        } else {
            name.clone()
        };
        let l = d.layer(&layer_name);
        if let Some(layer) = d.layers.get_mut(l) {
            layer.weight_mm = opts.stroke_mm;
        }
        if opts.fill {
            match s.regions() {
                Ok(regions) => {
                    for (i, r) in regions.into_iter().enumerate() {
                        d.push(Item::Hatch(HatchItem {
                            style: Style::on(l),
                            name: format!("{layer_name}/region[{i}]"),
                            outer: r.outer,
                            holes: r.holes,
                            pattern: Pattern::Solid,
                            scale: 1.0,
                        }));
                    }
                }
                // Filling needs closed, nested loops. Saying so beats writing
                // a valid-looking file with nothing in it.
                Err(e) => d.notes.push(format!(
                    "{layer_name}: cannot fill ({e}); its loops are written as outlines instead"
                )),
            }
            if !d.notes.is_empty() {
                for (i, lp) in s.loops.iter().enumerate() {
                    let id = match s.loop_name(i) {
                        "" => format!("{layer_name}/loop[{i}]"),
                        n => n.to_owned(),
                    };
                    d.named_loop_stroke(l, lp, id);
                }
            }
        } else {
            for (i, lp) in s.loops.iter().enumerate() {
                let id = match s.loop_name(i) {
                    "" => format!("{layer_name}/loop[{i}]"),
                    n => n.to_owned(),
                };
                d.named_loop_stroke(l, lp, id);
            }
        }
        for (i, p) in s.paths.iter().enumerate() {
            let id = match s.path_name(i) {
                "" => format!("{layer_name}/path[{i}]"),
                n => n.to_owned(),
            };
            d.push(Item::Stroke(Stroke {
                style: Style::on(l),
                name: id,
                path: p.clone(),
                closed: false,
            }));
        }
    }
    d
}

/// `s` re-expressed in `r`'s 2D coordinates: every point through world
/// space and back along `r`'s frame.
fn onto_plane(s: &Sketch, r: &cadpole_core::geom::Plane) -> Sketch {
    let (rx, ry) = (r.x_dir.normalized(), r.y_dir().normalized());
    let map = |p: cadpole_core::sketch::P2| {
        let w = s.plane.to_world(p.x, p.y) - r.origin;
        cadpole_core::sketch::P2::new(w.dot(rx), w.dot(ry))
    };
    let mut out = Sketch::new(*r);
    out.loop_names = s.loop_names.clone();
    out.path_names = s.path_names.clone();
    out.loops = s
        .loops
        .iter()
        .map(|l| cadpole_core::sketch::Loop {
            segments: l.segments.iter().map(|sg| ir::xform_segment_with(sg, map)).collect(),
        })
        .collect();
    out.paths = s
        .paths
        .iter()
        .map(|p| cadpole_core::sketch::Path {
            start: map(p.start),
            segments: p.segments.iter().map(|sg| ir::xform_segment_with(sg, map)).collect(),
        })
        .collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_core::geom::Plane;
    use cadpole_core::sketch::{Loop, P2};

    /// A sketch on a plane turned 90 degrees about z comes out turned, not in
    /// its own coordinates.
    #[test]
    fn a_sketch_on_a_turned_plane_is_turned_in_the_export() {
        use cadpole_core::geom::Vec3;
        let turned = Plane::new(Vec3::ZERO, Vec3::Z, Vec3::Y).unwrap();
        let base = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        // A 20 x 4 rectangle along the turned plane's x, which is world y.
        let label = Sketch::new(turned).with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 4.0));
        let d = from_sketches(&[("base".to_owned(), base), ("label".to_owned(), label)], &SketchOptions::default());
        let Item::Stroke(st) = &d.items[1] else { panic!() };
        let pts = ir::sample_path(&st.path, 0.1);
        let (xs, ys): (Vec<f64>, Vec<f64>) = pts.iter().map(|p| (p.x, p.y)).unzip();
        let span = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min);
        assert!((span(&xs) - 4.0).abs() < 1e-9 && (span(&ys) - 20.0).abs() < 1e-9, "turned: {} x {}", span(&xs), span(&ys));
    }

    #[test]
    fn a_sketch_becomes_one_layer_with_one_named_path_per_loop() {
        let s = Sketch::new(Plane::XY)
            .with_named_loop(Loop::rect(P2::new(0.0, 0.0), 40.0, 20.0), "outline")
            .with_named_loop(Loop::circle(P2::new(0.0, 0.0), 5.0), "hole");
        let d = from_sketches(&[("plate".to_owned(), s)], &SketchOptions::default());
        assert_eq!(d.layers.len(), 1);
        assert_eq!(d.layers[0].name, "plate");
        assert_eq!(d.layers[0].weight_mm, 0.25);
        let names: Vec<&str> = d
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Stroke(s) => Some(s.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["outline", "hole"]);
    }

    #[test]
    fn an_unnamed_sketch_still_gets_stable_ids() {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let d = from_sketches(&[(String::new(), s)], &SketchOptions::default());
        assert_eq!(d.layers[0].name, "SKETCH");
        let Item::Stroke(st) = &d.items[0] else {
            panic!()
        };
        assert_eq!(st.name, "SKETCH/loop[0]");
    }
}
