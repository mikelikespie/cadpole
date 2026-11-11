//! Parameter sweeps: evaluate the cartesian product of a
//! parameter grid, render every cell with one shared camera and compose a
//! labelled contact sheet.
//!
//! The shared camera is the whole point. Each variant has its own bounding
//! box, so per-scene fitting would zoom every cell differently and the sheet
//! would be useless for "which of these is right?". Here every variant is
//! evaluated first, the camera is fitted once to the union of the bounding
//! boxes, and only then is anything rasterized.

use std::collections::HashMap;

use cadpole_core::ParamValue;
use cadpole_core::Vec3;
use cadpole_core::render::{EdgeMode, GridMode, Image, RenderOptions, contact_sheet, contact_sheet_clear};
use cadpole_proto::v1 as pb;

use crate::{UNDECLARED, apply_document_defaults, available_threads, camera_from_proto_aspect, document_camera, render_scene, sweep_grid, sweep_scenes};

/// Default and maximum number of cells. Sweeps are meant to be quick; a
/// bigger study is a job, not a request.
const DEFAULT_MAX_CELLS: usize = 400;

/// The parameters of a sweep, most significant first (the last one listed is
/// the one that varies fastest, as `sweep_grid` builds the product).
///
/// proto3 maps are unordered, so `SweepRequest.param_order` decides which
/// parameter becomes the columns of the sheet; anything it does not mention
/// follows in sorted order, which keeps the result deterministic whatever the
/// caller's map iteration does.
fn ordered_params(req: &pb::SweepRequest) -> Vec<(String, Vec<ParamValue>)> {
    let mut names: Vec<String> = Vec::new();
    for n in &req.param_order {
        if req.params.contains_key(n) && !names.contains(n) {
            names.push(n.clone());
        }
    }
    let mut rest: Vec<&String> = req.params.keys().filter(|k| !names.contains(k)).collect();
    rest.sort();
    names.extend(rest.into_iter().cloned());
    names
        .into_iter()
        .map(|n| {
            let values = req.params[&n].values.iter().filter_map(ParamValue::from_proto).collect();
            (n, values)
        })
        .collect()
}

pub(crate) fn options(req: &pb::SweepRequest) -> RenderOptions {
    let (w, h) = (if req.width > 0 { req.width } else { 320 }, if req.height > 0 { req.height } else { 240 });
    // Sweeps trade quality for speed unless the caller asks for samples.
    let draft = req.draft || req.samples == 0;
    let mut o = if draft { RenderOptions::draft(w, h) } else { RenderOptions { width: w, height: h, ..Default::default() } };
    if req.samples > 0 {
        o.samples = req.samples;
    }
    o.threads = req.threads;
    o.draw_edges = !req.no_edges;
    o.draw_grid = !req.no_grid;
    if req.no_ambient_occlusion {
        o.ambient_occlusion = false;
    }
    if req.no_contact_shadow {
        o.contact_shadow = false;
    }
    if req.no_silhouettes {
        o.draw_silhouettes = false;
    }
    if req.exposure > 0.0 {
        o.exposure = req.exposure as f32;
    }
    if let Some(bg) = &req.background {
        o.background = [bg.r as f32, bg.g as f32, bg.b as f32];
    }
    o.path.show_environment = req.show_environment;
    if req.edge_crease_degrees > 0.0 {
        o.edge_crease_degrees = req.edge_crease_degrees as f32;
    }
    if req.edge_width_px > 0.0 {
        o.edge_width_px = (req.edge_width_px as f32).clamp(0.2, 16.0);
    }
    o.edges = match req.edge_mode() {
        pb::EdgeMode::Off => EdgeMode::Off,
        pb::EdgeMode::Silhouette => EdgeMode::Silhouette,
        pb::EdgeMode::All => EdgeMode::All,
        pb::EdgeMode::Creases | pb::EdgeMode::Unspecified => EdgeMode::Creases,
    };
    o.grid = match req.grid_mode() {
        pb::GridMode::Off => GridMode::Off,
        pb::GridMode::Finite => GridMode::Finite,
        pb::GridMode::Infinite | pb::GridMode::Unspecified => GridMode::Infinite,
    };
    o.xray = req.xray;
    o.transparent = req.transparent;
    o.shadow_catcher = req.shadow_catcher;
    o.supersample = req.supersample.clamp(1, 4);
    crate::Look::from_sweep(req).unwrap_or_default().apply(&mut o, req.background.is_some());
    o
}

/// Render a labelled contact sheet over a parameter grid.
///
/// Parameters with a single value are applied to every cell rather than
/// becoming a dimension of the grid; at least one parameter must have several
/// values or there is nothing to compare.
pub fn sweep(req: &pb::SweepRequest) -> anyhow::Result<pb::SweepResponse> {
    // Clamp before anything is evaluated: a sweep is one evaluation *and* one
    // render per cell, so it is the cheapest request to make expensive
    // (docs/security.md, "Render and sweep limits").
    let mut clamped = req.clone();
    let warnings = crate::limits::clamp_sweep(&mut clamped);
    let req = &clamped;
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("sweep: no document"))?;
    crate::Look::from_sweep(req)?;
    let all = ordered_params(req);
    if all.is_empty() {
        anyhow::bail!("sweep needs at least one parameter with values");
    }
    let mut specs: Vec<(String, Vec<ParamValue>)> = Vec::new();
    let mut fixed: HashMap<String, ParamValue> = HashMap::new();
    for (name, values) in all {
        match values.len() {
            0 => {}
            1 => {
                fixed.insert(name, values.into_iter().next().unwrap());
            }
            _ => specs.push((name, values)),
        }
    }
    if specs.is_empty() {
        anyhow::bail!("sweep needs at least one parameter with several values (got only fixed values)");
    }
    // The cell count comes from the value counts, checked, before the grid
    // exists: the grid is one allocation per cell, so building it first let
    // a small request spend gigabytes before being refused, and an
    // unchecked product could wrap to an empty grid that "succeeded".
    let max = if req.max_cells == 0 { DEFAULT_MAX_CELLS } else { req.max_cells as usize };
    match crate::sweep_cell_count(&specs) {
        Some(n) if n <= max => {}
        Some(n) => anyhow::bail!("sweep would render {n} cells; keep it under {max}"),
        None => anyhow::bail!("sweep would render more than {} cells; keep it under {max}", usize::MAX),
    }
    let grid = sweep_grid(&specs);
    let opts = options(req);
    let look = crate::Look::from_sweep(req)?;

    // Pass 1: evaluate every variant, in parallel. Evaluation dominates a
    // sweep's cost and the variants are independent, so this is the
    // difference between a sweep an assistant waits on and one it does not.
    let threads = if req.threads > 0 { req.threads as usize } else { available_threads() };
    let layers = crate::layer_overrides_from_proto(&req.layers);
    let variants = sweep_scenes(doc, &grid, &fixed, req.tolerance, req.boolean_tolerance, threads, &layers);
    let mut cells = Vec::with_capacity(variants.len());
    let mut union: Option<(Vec3, Vec3)> = None;
    let mut warnings = warnings;
    for (combo, v) in grid.iter().zip(&variants) {
        let mut diagnostics: Vec<pb::Diagnostic> = v.diagnostics.iter().map(cadpole_core::context::Diagnostic::to_proto).collect();
        for w in &v.point_warnings {
            let d = cadpole_core::context::Diagnostic::warning(w.clone()).to_proto();
            // Once on the sheet; a value substitution also on the cell it
            // is true of. A name the document does not declare is true of
            // every cell, so it is said on the sheet only.
            if !warnings.iter().any(|o| o.message == d.message) {
                warnings.push(d.clone());
            }
            if !w.starts_with(UNDECLARED) {
                diagnostics.push(d);
            }
        }
        let ok = !diagnostics.iter().any(|d| d.severity() == pb::Severity::Error);
        if let Some((lo, hi)) = v.scene.bbox {
            union = Some(match union {
                None => (lo, hi),
                Some((a, b)) => (a.min(lo), b.max(hi)),
            });
        }
        cells.push(pb::SweepCell {
            params: combo.iter().map(|(k, v)| (k.clone(), v.to_proto())).collect(),
            label: v.label.clone(),
            bbox: v.scene.bbox.map(|(lo, hi)| pb::BBox { min: Some(lo.to_proto()), max: Some(hi.to_proto()) }),
            diagnostics,
            ok,
        });
    }

    // Pass 2: one camera fitted to the union of the variants, then rasterize.
    // A per-cell camera would zoom every cell differently and the sheet would
    // no longer answer "which of these is right?".
    let (lo, hi) = union.unwrap_or((Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 10.0)));
    let aspect = opts.width.max(1) as f64 / opts.height.max(1) as f64;
    // The document's free camera if the request names none: the
    // first variant's, since a camera is not a thing a sweep varies.
    let cam_req = document_camera(&req.camera, variants.first().and_then(|v| v.render_defaults.as_ref()));
    let shared = camera_from_proto_aspect(&cam_req, lo, hi, aspect);
    let tiles: Vec<(String, Image)> = variants
        .iter()
        .map(|v| {
            // Each cell's own sun: a sweep over an hour parameter is
            // a shadow study.
            let mut cell_opts = opts.clone();
            apply_document_defaults(&mut cell_opts, &look, v.render_defaults.as_ref());
            (v.label.clone(), render_scene(&v.scene, &shared, &cell_opts))
        })
        .collect();
    let cols = if req.cols > 0 { req.cols } else { specs[0].1.len() as u32 }.max(1);
    let sheet = if req.transparent {
        contact_sheet_clear(&tiles, cols, 1, [230, 232, 236, 255])
    } else {
        contact_sheet(&tiles, cols, [0.09, 0.09, 0.1])
    };
    Ok(pb::SweepResponse {
        png: sheet.to_png(),
        width: sheet.width,
        height: sheet.height,
        cols,
        cells,
        diagnostics: warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(vs: &[f64]) -> pb::ParamValues {
        pb::ParamValues { values: vs.iter().map(|v| ParamValue::Number(*v).to_proto()).collect() }
    }

    #[test]
    fn sweep_renders_one_cell_per_combination() {
        let doc = crate::document_from_scripts(
            "t",
            vec![("main.star".into(), "w = param(\"w\", 10)\nh = param(\"h\", 5)\nb = box(w, 10, h)".into())],
        );
        let r = sweep(&pb::SweepRequest {
            document: Some(doc),
            params: [("w".to_owned(), values(&[10.0, 20.0])), ("h".to_owned(), values(&[3.0, 6.0, 9.0]))].into_iter().collect(),
            param_order: vec!["w".into(), "h".into()],
            width: 40,
            height: 30,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(r.cells.len(), 6);
        assert_eq!(r.cols, 2, "columns default to the first parameter's value count");
        assert_eq!(&r.png[1..4], b"PNG");
        assert!(r.cells.iter().all(|c| c.ok), "{:?}", r.cells);
        // The first parameter varies fastest, so with `cols = 2` the six cells
        // fill a two-wide sheet row by row: w across, h down.
        let labels: Vec<&str> = r.cells.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["w=10 h=3", "w=20 h=3", "w=10 h=6", "w=20 h=6", "w=10 h=9", "w=20 h=9"]);
    }

    #[test]
    fn single_valued_parameters_are_fixed_not_dimensions() {
        let doc = crate::document_from_scripts(
            "t",
            vec![("main.star".into(), "w = param(\"w\", 10)\nh = param(\"h\", 5)\nb = box(w, 10, h)".into())],
        );
        let r = sweep(&pb::SweepRequest {
            document: Some(doc.clone()),
            params: [("w".to_owned(), values(&[10.0, 20.0])), ("h".to_owned(), values(&[7.0]))].into_iter().collect(),
            width: 32,
            height: 24,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(r.cells.len(), 2);
        assert_eq!(r.cells[0].label, "w=10");

        let err = sweep(&pb::SweepRequest {
            document: Some(doc),
            params: [("h".to_owned(), values(&[7.0]))].into_iter().collect(),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("several values"), "{err}");
    }

    /// A sweep value the document did not evaluate at (clamped into the
    /// param's min/max, or not one of its choices) or a name the document
    /// does not declare is said, not silently drawn under the requested
    /// label: the cells of `w=100` and `w=1000` were the same `w=20` model.
    #[test]
    fn values_the_document_did_not_take_are_reported() {
        let doc = crate::document_from_scripts(
            "t",
            vec![(
                "main.star".into(),
                "w = param(\"w\", 10, min=5, max=20)\ns = param(\"s\", \"a\", choices=[\"a\", \"b\"])\nb = box(w, 10, 5)".into(),
            )],
        );
        let text = |vs: &[&str]| pb::ParamValues { values: vs.iter().map(|v| ParamValue::Text((*v).into()).to_proto()).collect() };
        let r = sweep(&pb::SweepRequest {
            document: Some(doc),
            params: [
                ("w".to_owned(), values(&[10.0, 100.0])),
                ("nosuch".to_owned(), values(&[1.0, 2.0])),
                ("s".to_owned(), text(&["c"])),
            ]
            .into_iter()
            .collect(),
            param_order: vec!["w".into(), "nosuch".into()],
            width: 32,
            height: 24,
            ..Default::default()
        })
        .unwrap();
        let said: Vec<&str> = r.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let has = |want: &str| said.iter().any(|m| m.contains(want));
        assert!(has("w=100 was evaluated as w=20"), "{said:?}");
        assert!(!has("w=10 was"), "a value the document took is not reported: {said:?}");
        assert!(has("\"nosuch\" is not a param of this document"), "{said:?}");
        assert!(has("s=c was evaluated as s=a"), "{said:?}");
        assert_eq!(said.iter().filter(|m| m.contains("nosuch")).count(), 1, "said once, not per cell: {said:?}");
        let cell = r.cells.iter().find(|c| c.label == "w=100 nosuch=1").unwrap();
        assert!(cell.diagnostics.iter().any(|d| d.message.contains("w=100 was evaluated as w=20")), "{:?}", cell.diagnostics);
        // The undeclared name is said on the sheet only, not on every cell.
        assert!(
            r.cells.iter().all(|c| !c.diagnostics.iter().any(|d| d.message.contains("is not a param of this document"))),
            "{:?}",
            r.cells
        );
    }

    #[test]
    fn cells_are_capped() {
        let doc = crate::document_from_scripts("t", vec![("main.star".into(), "b = box(1, 1, 1)".into())]);
        let err = sweep(&pb::SweepRequest {
            document: Some(doc),
            params: [("w".to_owned(), values(&[1.0, 2.0, 3.0]))].into_iter().collect(),
            max_cells: 2,
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("keep it under 2"), "{err}");
    }

    /// 16 parameters of 16 values is 2^64 cells: the product used to wrap to
    /// 0 in `usize`, the grid came out empty and the sweep "succeeded" with
    /// an empty sheet. It has to be refused from the value counts.
    #[test]
    fn a_cell_count_that_overflows_is_refused_not_wrapped() {
        let doc = crate::document_from_scripts("t", vec![("main.star".into(), "b = box(1, 1, 1)".into())]);
        let sixteen: Vec<f64> = (0..16).map(f64::from).collect();
        let params = (0..16).map(|i| (format!("p{i:02}"), values(&sixteen))).collect();
        let err = sweep(&pb::SweepRequest { document: Some(doc), params, max_cells: 64, ..Default::default() }).unwrap_err();
        assert!(err.to_string().contains("keep it under 64"), "{err}");
        assert!(err.to_string().contains("more than"), "an overflowing count is named as such: {err}");
    }

    /// The count is computed from the value counts and checked before any
    /// grid exists, so a refused sweep allocates nothing per cell.
    #[test]
    fn the_cell_count_is_checked_before_the_grid_is_built() {
        let hundred: Vec<ParamValue> = (0..100).map(|i| ParamValue::Number(f64::from(i))).collect();
        let specs: Vec<(String, Vec<ParamValue>)> = (0..3).map(|i| (format!("p{i}"), hundred.clone())).collect();
        assert_eq!(crate::sweep_cell_count(&specs), Some(1_000_000));
        let specs: Vec<(String, Vec<ParamValue>)> = (0..16).map(|i| (format!("p{i}"), hundred[..16].to_vec())).collect();
        assert_eq!(crate::sweep_cell_count(&specs), None, "2^64 does not fit");
    }
}
