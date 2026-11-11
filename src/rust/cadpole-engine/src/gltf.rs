//! The `Export` rpc's glTF arm and what the whole-document export
//! knows about display layers.
//!
//! The writer is `cadpole_core::export::gltf`, pure and testable alone; this
//! module turns an evaluated `Context` into its input -- which bodies, on
//! which layer, hidden or not, at what scale, with which display units --
//! and is where `model_unit` and `units` are read.

use cadpole_core::Context;
use cadpole_core::export::gltf::{self as writer, GltfBody, GltfLayer, GltfOptions};
use cadpole_core::tessellate::Mesh;
use cadpole_proto::v1 as pb;

/// A unit name as millimetres per unit: the `--model-unit` vocabulary
/// `cadpole bake` and `cadpole export` share.
pub fn mm_per_unit(name: &str) -> Result<f64, String> {
    Ok(match name.trim().to_ascii_lowercase().as_str() {
        "" | "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "in" | "inch" => 25.4,
        "ft" | "foot" | "feet" => 304.8,
        other => return Err(format!("unknown unit {other:?}: mm, cm, m, in or ft")),
    })
}

/// A `--units` name as a display convention: the unit and, for the
/// imperial ones, the fraction of an inch a length rounds to.
pub fn display_units(name: &str) -> Result<(String, u32), String> {
    let unit = match name.trim().to_ascii_lowercase().as_str() {
        u @ ("mm" | "cm" | "m" | "in" | "ft" | "ft-in") => u.to_owned(),
        "ft_in" | "ftin" => "ft-in".to_owned(),
        other => return Err(format!("unknown display unit {other:?}: mm, cm, m, in, ft or ft-in")),
    };
    let fraction = if unit == "ft-in" || unit == "in" { 16 } else { 0 };
    Ok((unit, fraction))
}

/// Whether a body on `layer` is hidden by the document's own declaration
/// (`layer(..., visible=False)`). A whole-document export leaves such
/// a body out unless asked otherwise, as every viewer does.
pub fn layer_hidden(ctx: &Context, layer: &str) -> bool {
    !cadpole_core::layers::effective(layer, &ctx.layers, &[]).0
}

/// The display convention a glTF records: the request's `units`, else the
/// script's `units(...)`, else feet and inches for a script drawn in feet or
/// inches, else millimetres.
fn recorded_units(ctx: &Context, req: &pb::ExportRequest, mm: f64) -> anyhow::Result<(String, u32)> {
    if !req.units.trim().is_empty() {
        return display_units(&req.units).map_err(|e| anyhow::anyhow!("export: units: {e}"));
    }
    if let Some(d) = &ctx.display_units {
        return Ok((d.unit.clone(), d.fraction));
    }
    if mm == 25.4 || mm == 304.8 {
        return Ok(("ft-in".to_owned(), 16));
    }
    Ok(("mm".to_owned(), 0))
}

/// Write `bodies` (each a body id, its mesh as the export made it) as a GLB
/// or a `.gltf`.
pub fn write(
    ctx: &Context,
    bodies: &[(cadpole_core::BodyId, Mesh)],
    title: &str,
    req: &pb::ExportRequest,
    binary: bool,
) -> anyhow::Result<Vec<u8>> {
    let mm = mm_per_unit(&req.model_unit).map_err(|e| anyhow::anyhow!("export: model_unit: {e}"))?;
    let (display_unit, display_fraction) = recorded_units(ctx, req, mm)?;
    let mut records = Vec::with_capacity(bodies.len());
    for (id, _) in bodies {
        let b = ctx.body(*id)?;
        records.push((b.name.clone(), b.id_string(), b.layer.clone(), layer_hidden(ctx, &b.layer), b.material.clone()));
    }
    let input: Vec<GltfBody<'_>> = records
        .iter()
        .zip(bodies)
        .map(|((name, id, layer, hidden, material), (_, mesh))| GltfBody {
            name,
            body_id: id,
            layer,
            hidden: *hidden,
            material,
            mesh,
        })
        .collect();
    let layers: Vec<GltfLayer> = ctx
        .layers
        .iter()
        .map(|l| GltfLayer { name: l.name.clone(), visible: l.visible, opacity: l.opacity, color: l.color })
        .collect();
    let opts = GltfOptions {
        title: title.to_owned(),
        generator: format!("cadpole {}", crate::VERSION),
        mm_per_unit: mm,
        model_unit: if req.model_unit.trim().is_empty() { "mm".to_owned() } else { req.model_unit.trim().to_ascii_lowercase() },
        display_unit,
        display_fraction,
        binary,
    };
    Ok(writer::gltf(&input, &layers, &opts))
}

#[cfg(test)]
#[path = "gltf_tests.rs"]
mod tests;
