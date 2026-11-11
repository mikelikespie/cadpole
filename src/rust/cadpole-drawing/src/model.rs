//! From a model to a drawing: the vocabulary, and what each word means.
//!
//! Every front end — the `Drawing` rpc, `cadpole export --format svg`, the
//! Starlark `drawing()` builtin — says the same six things: which view, at what
//! scale, on what sheet, dimensioned or not, hidden lines how, in what units.
//! This module is the one place those strings are read and the one place they
//! turn into geometry, so the CLI, MCP, HTTP and a script cannot drift apart
//! and cannot disagree about what `section:z=40` means.

use crate::dim::DimStyle;
use crate::ir::{Drawing, Group, Item, Pattern, Scale, Style, Xform2};
use crate::sheet::{draw_border, three_view, Sheet, SheetSize, ThreeViewOptions, TitleBlock};
use crate::units::{parse_length, LengthUnit, UnitFormat, UNIT_NAMES};
use crate::view::{section_drawing, to_drawing, Hidden, ProjectOptions, ViewBasis};
use crate::{auto, layer};
use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::sketch::P2;
use cadpole_core::tessellate::{Mesh, Topology};
use cadpole_core::Context;

/// Everything the drawing pipeline needs, parsed out of strings.
#[derive(Clone, Debug)]
pub struct DrawingOptions {
    pub view: String,
    pub scale: Option<Scale>,
    pub sheet: SheetSize,
    pub dimensions: bool,
    pub hidden: Hidden,
    pub unit_format: UnitFormat,
    pub title: String,
    pub project: String,
}

impl Default for DrawingOptions {
    fn default() -> Self {
        DrawingOptions {
            view: "three-view".into(),
            scale: None,
            sheet: SheetSize::A3,
            dimensions: true,
            hidden: Hidden::Dashed,
            unit_format: UnitFormat::mm(),
            title: String::new(),
            project: String::new(),
        }
    }
}

impl DrawingOptions {
    /// Read the request's strings, rejecting anything unknown with a message
    /// that lists what is allowed. Empty strings mean "the default".
    pub fn parse(
        view: &str,
        scale: &str,
        sheet: &str,
        dimensions: &str,
        hidden: &str,
        units: &str,
    ) -> Result<DrawingOptions, String> {
        let mut o = DrawingOptions::default();
        if !view.trim().is_empty() {
            o.view = view.trim().to_owned();
            check_view(&o.view)?;
        }
        if !scale.trim().is_empty() {
            o.scale = Some(
                Scale::parse(scale)
                    .ok_or_else(|| format!("scale {scale:?} is not a ratio like \"1:2\""))?,
            );
        }
        if !sheet.trim().is_empty() {
            o.sheet = SheetSize::parse(sheet).ok_or_else(|| {
                format!(
                    "sheet {sheet:?} is not a paper size (one of {})",
                    SheetSize::ALL
                        .iter()
                        .map(|s| s.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        }
        match dimensions.trim().to_ascii_lowercase().as_str() {
            "" | "auto" | "on" | "true" => o.dimensions = true,
            "none" | "off" | "false" => o.dimensions = false,
            other => return Err(format!("dimensions {other:?} must be \"auto\" or \"none\"")),
        }
        if !hidden.trim().is_empty() {
            o.hidden = Hidden::parse(hidden).ok_or_else(|| {
                format!("hidden {hidden:?} must be \"dashed\", \"omit\" or \"solid\"")
            })?;
        }
        if !units.trim().is_empty() {
            let u = LengthUnit::parse(units).ok_or_else(|| {
                format!("units {units:?} must be one of {}", UNIT_NAMES.join(", "))
            })?;
            o.unit_format = if u.imperial() {
                UnitFormat {
                    unit: u,
                    ..UnitFormat::ft_in()
                }
            } else {
                UnitFormat {
                    unit: u,
                    ..UnitFormat::mm()
                }
            };
        }
        Ok(o)
    }

    fn dim_style(&self) -> DimStyle {
        let base = if self.unit_format.unit.imperial() {
            DimStyle::architectural()
        } else {
            DimStyle::mechanical()
        };
        DimStyle {
            format: self.unit_format,
            ..base
        }
    }
}

/// Every visible body of `ctx` as one mesh and one topology.
///
/// A drawing of an assembly is a drawing of everything in it, so the bodies are
/// concatenated rather than drawn one at a time: hidden-line removal then works
/// *between* them, which is what makes a part behind another part come out
/// dashed.
pub fn combined_mesh(ctx: &mut Context, body_ids: &[String]) -> Result<(Mesh, Topology), String> {
    let ids: Vec<_> = ctx
        .bodies
        .iter()
        .filter(|b| b.visible && (body_ids.is_empty() || body_ids.contains(&b.id_string())))
        .map(|b| b.id)
        .collect();
    if ids.is_empty() {
        return Err("no visible bodies to draw".to_owned());
    }
    let mut mesh = Mesh::default();
    let mut topo = Topology::default();
    for id in ids {
        let (m, t) = ctx.body_mesh(id).map_err(|e| e.to_string())?;
        let base = (mesh.positions.len() / 3) as u32;
        mesh.positions.extend_from_slice(&m.positions);
        mesh.normals.extend_from_slice(&m.normals);
        mesh.indices.extend(m.indices.iter().map(|i| i + base));
        mesh.face_ids.extend_from_slice(&m.face_ids);
        topo.faces.extend(t.faces.iter().cloned());
        topo.edges.extend(t.edges.iter().cloned());
        topo.vertices.extend(t.vertices.iter().cloned());
    }
    Ok((mesh, topo))
}

/// Build the drawing `opts.view` asks for.
pub fn build(mesh: &Mesh, topo: &Topology, opts: &DrawingOptions) -> Result<Drawing, String> {
    let project = ProjectOptions {
        hidden: opts.hidden,
        ..Default::default()
    };
    let title = TitleBlock {
        title: opts.title.clone(),
        project: opts.project.clone(),
        ..Default::default()
    };

    if let Some(spec) = opts.view.strip_prefix("section:") {
        let plane = section_plane(spec, mesh)?;
        let mut d = section_drawing(mesh, topo, &plane, 0.01, Pattern::ANSI31, Some(&project));
        d.unit_format = opts.unit_format;
        finish_single(&mut d, opts, title);
        return Ok(d);
    }

    if opts.view.eq_ignore_ascii_case("three-view") || opts.view.eq_ignore_ascii_case("sheet") {
        return Ok(three_view(
            mesh,
            topo,
            &ThreeViewOptions {
                sheet: opts.sheet,
                scale: opts.scale,
                dimensions: opts.dimensions,
                hidden: opts.hidden,
                iso: true,
                angle: Default::default(),
                title,
                unit_format: opts.unit_format,
                dim_style: opts.dim_style(),
                project,
            },
        ));
    }

    check_view(&opts.view)?;
    let basis = ViewBasis::preset(&opts.view);
    let mut d = to_drawing(mesh, topo, &basis, &project);
    d.unit_format = opts.unit_format;
    finish_single(&mut d, opts, title);
    Ok(d)
}

/// The views [`build`] knows how to draw.
const VIEWS: &[&str] = &[
    "front", "back", "top", "bottom", "left", "right", "iso", "iso-back", "plan",
];

/// Reject a view name early, so a typo comes back as a typo rather than as
/// whatever the next step happens to complain about.
fn check_view(view: &str) -> Result<(), String> {
    let lower = view.to_ascii_lowercase();
    if lower == "three-view" || lower == "sheet" || lower.starts_with("section:") {
        return Ok(());
    }
    if VIEWS.contains(&lower.as_str()) {
        return Ok(());
    }
    Err(format!(
        "view {view:?} is not \"three-view\", \"section:<axis>=<value>\" or one of {}",
        VIEWS.join(", ")
    ))
}

/// Put a single view on a sheet, dimension its extents and draw the border.
fn finish_single(d: &mut Drawing, opts: &DrawingOptions, title: TitleBlock) {
    let Some((lo, hi)) = d.bounds() else { return };
    let mut sheet = Sheet::new(opts.sheet);
    sheet.title_block = title;
    let (area_lo, area_hi) = sheet.drawing_area();
    let ds = opts.dim_style();
    let pad = if opts.dimensions {
        ds.text_height_mm * 7.0
    } else {
        6.0
    };
    let avail_w = (area_hi.x - area_lo.x - 2.0 * pad).max(10.0);
    let avail_h = (area_hi.y - area_lo.y - 2.0 * pad).max(10.0);
    let fit = (avail_w / (hi.x - lo.x).max(1e-6)).min(avail_h / (hi.y - lo.y).max(1e-6));
    let scale = opts.scale.unwrap_or_else(|| snap(fit));
    let s = scale.ratio();
    if opts.scale.is_some() {
        d.notes.extend(crate::sheet::overflow_note(
            scale,
            fit,
            (hi.x - lo.x, hi.y - lo.y),
            (avail_w, avail_h),
            &opts.sheet,
            &opts.unit_format,
        ));
    }

    let at = P2::new(
        area_lo.x + pad + (avail_w - (hi.x - lo.x) * s).max(0.0) / 2.0,
        area_lo.y + pad + (avail_h - (hi.y - lo.y) * s).max(0.0) / 2.0,
    );
    let mut items = std::mem::take(&mut d.items);
    // An axonometric view foreshortens every axis, so the extents of its
    // projection are no length of the model; like the isometric on a
    // three-view sheet, it is there to be looked at, not measured.
    if opts.dimensions && !is_axonometric(&opts.view) {
        let dl = d.layer(layer::DIMS);
        let mut dims = auto::extents_dims(Style::on(dl), lo, hi, &ds, ds.text_height_mm * 2.4);
        let notes = auto::place(&mut dims, Xform2::new(s, 0.0, 0.0));
        d.notes.extend(notes);
        items.extend(dims.into_iter().map(Item::Dim));
    }
    d.items = vec![Item::Group(Group {
        name: opts.view.clone(),
        source: None,
        transform: Xform2::new(s, at.x - lo.x * s, at.y - lo.y * s),
        items,
        clip: None,
    })];
    d.scale = scale;
    sheet.title_block.scale = scale.label(&opts.unit_format);
    d.sheet = Some(sheet);
    draw_border(d);
}

/// The single views whose projection is not true-length along the page.
fn is_axonometric(view: &str) -> bool {
    matches!(view.trim().to_ascii_lowercase().as_str(), "iso" | "iso-back")
}

/// Standard drawing ratios, largest first.
pub(crate) fn snap(fit: f64) -> Scale {
    const NICE: &[f64] = &[
        10.0, 5.0, 4.0, 2.5, 2.0, 1.0, 0.5, 0.4, 0.25, 0.2, 0.1, 0.05, 0.04, 0.025, 0.02, 0.01,
        0.005, 0.002, 0.001,
    ];
    let s = NICE.iter().copied().find(|n| *n <= fit).unwrap_or(0.001);
    if s >= 1.0 {
        Scale::new(s, 1.0)
    } else {
        Scale::new(1.0, (1.0 / s).round())
    }
}

/// `section:z=40`, `section:x=0`, or `section:z` for the middle of the model.
fn section_plane(spec: &str, mesh: &Mesh) -> Result<Plane, String> {
    let (axis, value) = match spec.split_once('=') {
        Some((a, v)) => {
            let v: f64 =
                parse_length(v).ok_or_else(|| format!("section: {v:?} is not a length"))?;
            (a.trim().to_ascii_lowercase(), Some(v))
        }
        None => (spec.trim().to_ascii_lowercase(), None),
    };
    let (lo, hi) = mesh
        .bbox()
        .unwrap_or((Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)));
    let mid = (lo + hi) * 0.5;
    // The in-plane x axis is chosen so the cut is drawn the way it is read:
    // looking along the plane's normal, with the world's up axis up the page.
    let (normal, x_dir, default) = match axis.as_str() {
        "x" => (Vec3::X, Vec3::Y, mid.x),
        "y" => (Vec3::Y, Vec3::new(-1.0, 0.0, 0.0), mid.y),
        "z" => (Vec3::Z, Vec3::X, mid.z),
        other => return Err(format!("section: {other:?} must be x, y or z")),
    };
    let at = value.unwrap_or(default);
    let origin = Vec3::new(normal.x * at, normal.y * at, normal.z * at);
    Plane::new(origin, normal, x_dir).map_err(|e| format!("section plane: {e}"))
}

/// The number of drawable pieces in a drawing, for the response's sanity
/// check.
pub fn item_count(d: &Drawing) -> u32 {
    d.flatten().len() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_strings_are_parsed_or_refused_with_help() {
        let o = DrawingOptions::parse("front", "1:2", "a4", "none", "omit", "ft-in").unwrap();
        assert_eq!(o.view, "front");
        assert_eq!(o.scale.unwrap().ratio(), 0.5);
        assert_eq!(o.sheet.name, "A4");
        assert!(!o.dimensions);
        assert_eq!(o.hidden, Hidden::Omit);
        assert_eq!(o.unit_format.unit, LengthUnit::FtIn);

        let d = DrawingOptions::parse("", "", "", "", "", "").unwrap();
        assert_eq!(d.view, "three-view");
        assert!(d.scale.is_none());
        assert!(d.dimensions);

        let e = DrawingOptions::parse("", "", "legal", "", "", "").unwrap_err();
        assert!(e.contains("ARCH_D"), "{e}");
        let e = DrawingOptions::parse("", "", "", "", "", "furlong").unwrap_err();
        assert!(e.contains("ft-in"), "{e}");
        let e = DrawingOptions::parse("", "half", "", "", "", "").unwrap_err();
        assert!(e.contains("1:2"), "{e}");
        // Rust's float parser takes `inf` and `nan`; a scale that reached the
        // writers would produce a PDF no reader can open and an SVG in which
        // every path has collapsed to a point.
        for bad in ["inf", "nan", "0", "-1", "1:0", "inf:1", "-1:2"] {
            let r = DrawingOptions::parse("", bad, "", "", "", "");
            let e = r.expect_err(bad);
            assert!(e.contains("ratio"), "{bad:?}: {e}");
        }
    }

    #[test]
    fn a_section_spec_names_a_plane() {
        let mut mesh = Mesh::default();
        mesh.positions = vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 20.0];
        mesh.indices = vec![0, 1, 2];
        let p = section_plane("z=40", &mesh).unwrap();
        assert_eq!(p.origin.z, 40.0);
        // No value: the middle of the model.
        let p = section_plane("z", &mesh).unwrap();
        assert_eq!(p.origin.z, 10.0);
        // Imperial lengths work here too.
        let p = section_plane("x=12'-6\"", &mesh).unwrap();
        assert_eq!(p.origin.x, 3810.0);
        assert!(section_plane("w=1", &mesh).is_err());
    }

    #[test]
    fn an_unknown_view_lists_the_ones_that_exist() {
        // Caught when the options are parsed, so a typo never reaches the
        // geometry and never comes back as some later step's complaint.
        let e = DrawingOptions::parse("sideways", "", "", "", "", "").unwrap_err();
        assert!(e.contains("three-view") && e.contains("iso"), "{e}");
        // And again at the door of `build`, for callers that built the options
        // by hand.
        let o = DrawingOptions {
            view: "sideways".into(),
            ..DrawingOptions::default()
        };
        let e = build(&Mesh::default(), &Topology::default(), &o).unwrap_err();
        assert!(e.contains("three-view"), "{e}");
        // The ones that do exist are accepted, including a section.
        for good in ["three-view", "front", "ISO", "section:z=40"] {
            DrawingOptions::parse(good, "", "", "", "", "")
                .unwrap_or_else(|e| panic!("{good}: {e}"));
        }
    }
}
