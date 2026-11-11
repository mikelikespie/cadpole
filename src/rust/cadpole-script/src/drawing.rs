//! The drawing half of the Starlark API: the `Drawing` value, `drawing(...)`,
//! and `export_svg` / `export_dxf`.
//!
//! Like `export(...)`, the export builtins **record intent**: a script never
//! touches the file system. The difference is that a drawing is cheap to build
//! and expensive to describe, so the bytes are produced at call time and
//! carried out with the evaluation, and the host writes them
//! (`cadpole export --run-exports`, the UI's Export button). That keeps the
//! "scripts are pure" rule while letting a script say exactly what it wants on
//! the page.

use std::fmt;

use allocative::Allocative;
use cadpole_core::sketch::Sketch;
use cadpole_drawing::{Drawing, SketchOptions, UnitFormat};
use starlark::starlark_simple_value;
use starlark::values::{
    Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueLike,
};
use starlark_derive::starlark_value;

/// A file an `export_*` call asked for, already rendered to bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputFile {
    pub path: String,
    pub format: String,
    pub data: Vec<u8>,
}

/// A 2D drawing: line work, dimensions and a sheet, ready to be written as
/// SVG, PDF or DXF.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KDrawing {
    pub name: String,
    #[allocative(skip)]
    pub drawing: Drawing,
}
starlark_simple_value!(KDrawing);

impl fmt::Display for KDrawing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "drawing({}, {} items)",
            self.name,
            self.drawing.flatten().len()
        )
    }
}

#[starlark_value(type = "Drawing")]
impl<'v> StarlarkValue<'v> for KDrawing {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "id" => Some(heap.alloc(self.name.as_str())),
            // How much ink the drawing holds: strokes, filled regions and
            // text runs after groups and dimensions are expanded, which is
            // what "is there anything on this page" means.
            "items" => Some(heap.alloc(self.drawing.flatten().len() as i32)),
            "groups" => Some(heap.alloc(self.drawing.items.len() as i32)),
            "layers" => Some(crate::convert::alloc_str_list(
                heap,
                &self
                    .drawing
                    .layers
                    .iter()
                    .map(|l| l.name.clone())
                    .collect::<Vec<_>>(),
            )),
            "scale" => {
                Some(heap.alloc(self.drawing.scale.label(&self.drawing.unit_format).as_str()))
            }
            "sheet" => Some(
                heap.alloc(
                    self.drawing
                        .sheet
                        .as_ref()
                        .map(|s| s.size.name)
                        .unwrap_or(""),
                ),
            ),
            "notes" => Some(crate::convert::alloc_str_list(heap, &self.drawing.notes)),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["id", "items", "groups", "layers", "scale", "sheet", "notes"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

/// What an `export_svg` / `export_dxf` call was pointed at.
pub enum Target {
    Drawing(Drawing),
    Sketches(Vec<(String, Sketch)>),
}

impl Target {
    /// The drawing to write, building one from sketches when that is what was
    /// given.
    pub fn into_drawing(self, stroke_mm: f64, fill: bool, unit_format: UnitFormat) -> Drawing {
        match self {
            Target::Drawing(d) => d,
            Target::Sketches(s) => cadpole_drawing::from_sketches(
                &s,
                &SketchOptions {
                    stroke_mm,
                    fill,
                    unit_format,
                    ..Default::default()
                },
            ),
        }
    }
}

/// Resolve the first argument of an export builtin: a `Drawing`, a `Sketch`,
/// or a list of either.
pub fn target_of<'v>(value: Value<'v>, heap: Heap<'v>, what: &str) -> anyhow::Result<Target> {
    use crate::values::KSketch;
    if let Some(d) = value.downcast_ref::<KDrawing>() {
        return Ok(Target::Drawing(d.drawing.clone()));
    }
    if let Some(s) = value.downcast_ref::<KSketch>() {
        return Ok(Target::Sketches(vec![(s.feature.clone(), s.sketch())]));
    }
    let mut out = Vec::new();
    let items = value.iterate(heap).map_err(|_| {
        anyhow::anyhow!("{what}: expected a Drawing, a Sketch, or a list of sketches")
    })?;
    for v in items {
        if let Some(s) = v.downcast_ref::<KSketch>() {
            out.push((s.feature.clone(), s.sketch()));
        } else if let Some(d) = v.downcast_ref::<KDrawing>() {
            // A list holding a drawing means the drawing; mixing the two on
            // one page is what `drawing()` is for.
            return Ok(Target::Drawing(d.drawing.clone()));
        } else {
            anyhow::bail!("{what}: expected a Drawing, a Sketch, or a list of sketches");
        }
    }
    if out.is_empty() {
        anyhow::bail!("{what}: nothing to export");
    }
    Ok(Target::Sketches(out))
}

/// The unit format a `units=` keyword asks for.
pub fn unit_format(name: &str, what: &str) -> anyhow::Result<UnitFormat> {
    let u = cadpole_drawing::LengthUnit::parse(name).ok_or_else(|| {
        anyhow::anyhow!(
            "{what}: units {name:?} must be one of {}",
            cadpole_drawing::units::UNIT_NAMES.join(", ")
        )
    })?;
    Ok(if u.imperial() {
        UnitFormat {
            unit: u,
            ..UnitFormat::ft_in()
        }
    } else {
        UnitFormat {
            unit: u,
            ..UnitFormat::mm()
        }
    })
}
