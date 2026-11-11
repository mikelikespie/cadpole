//! `import_dxf` and the `Drawing2D` value (docs/starlark-api.md):
//! an ASCII DXF — a surveyor's site plan, an architect's floor
//! plan — read once into a drawing the script then questions layer by
//! layer: `.sketch()` for the closed loops (and the open chains as
//! construction paths), `.curves()` for every entity as a planar `Curve`,
//! `.points()`, `.texts()`, `.spot_elevations()` for the elevation labels
//! a survey writes beside its shots, `.contours()` for the contour
//! polylines with their elevations, `.bbox()`.
//!
//! The import is one feature and one `Context::produce_value` op keyed by
//! the asset's content hash, the layer selection, the unit and the
//! importer's version, as the mesh imports are: on a memo hit
//! nothing is parsed and the same `Arc<Drawing>` comes back with its
//! diagnostics replayed; every view is a pure function of it. A
//! `Drawing2D` stands in wherever a `Sketch` is expected — `extrude(
//! import_dxf(...), 10)` — as its `.sketch()` with the defaults.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use allocative::Allocative;
use cadpole_core::context::fmt_num;
use cadpole_core::curve::Curve;
use cadpole_core::geom::Plane;
use cadpole_core::memo::KeyBuilder;
use cadpole_core::sketch::{Loop, P2, Path, Sketch};
use cadpole_core::{CadpoleError, kernel};
use cadpole_import::IMPORTER_VERSION;
use cadpole_import::dxf::{self, DEFAULT_TOLERANCE, Drawing, Unit};
use starlark::environment::{GlobalsBuilder, Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::values::float::UnpackFloat;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::tuple::AllocTuple;
use starlark::values::{Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, UnpackValue, Value};
use starlark::{starlark_module, starlark_simple_value};
use starlark_derive::starlark_value;

use crate::check::{VecArg, num, p2};
use crate::convert::alloc_str_list;
use crate::import3d::{asset_of, budget_ms, raise_recorded_warnings, record_note, source_name};
use crate::store::{call_span, store};
use crate::values::{KCurve, KPlane, KSketch};

/// An imported 2D drawing: the feature that imported it and the parsed
/// drawing, shared with the memo.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KDrawing2D {
    pub feature: String,
    #[allocative(skip)]
    pub drawing: Arc<Drawing>,
}
starlark_simple_value!(KDrawing2D);

impl fmt::Display for KDrawing2D {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "drawing2d({}, {} entities on {} layers, {})",
            self.feature,
            self.drawing.entities.len(),
            self.drawing.layer_names().len(),
            self.drawing.unit.name()
        )
    }
}

const DRAWING2D_ATTRS: &[&str] = &["id", "units", "unit_source", "scale", "entities", "layer_names", "version", "extents"];

/// `layer=`: one layer name (with `*` and `?`), a list of them, or nothing
/// for every layer the import kept.
fn layer_patterns<'v>(v: Option<Value<'v>>, what: &str) -> anyhow::Result<Vec<String>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.is_none() {
        return Ok(Vec::new());
    }
    if let Some(s) = v.unpack_str() {
        return Ok(vec![s.to_owned()]);
    }
    if let Some(items) = <UnpackListOrTuple<Value<'v>>>::unpack_value(v).map_err(|e| e.into_anyhow())? {
        let mut out = Vec::with_capacity(items.items.len());
        for i in items.items {
            match i.unpack_str() {
                Some(s) => out.push(s.to_owned()),
                None => anyhow::bail!(
                    "{what}: layer= takes a layer name (with * and ?) or a list of them, got a {} in the list",
                    i.get_type()
                ),
            }
        }
        return Ok(out);
    }
    anyhow::bail!("{what}: layer= takes a layer name (with * and ?) or a list of them, got {}", v.get_type())
}

fn tolerance_arg(t: Option<UnpackFloat>, what: &str) -> anyhow::Result<f64> {
    match t {
        None => Ok(DEFAULT_TOLERANCE),
        Some(t) => {
            let v = num(&t, &format!("{what}: tolerance"))?;
            if v <= 0.0 {
                anyhow::bail!("{what}: tolerance must be a positive number of millimetres, got {}", fmt_num(v));
            }
            Ok(v)
        }
    }
}

fn alloc_p3<'v>(heap: Heap<'v>, p: [f64; 3]) -> Value<'v> {
    heap.alloc((p[0], p[1], p[2]))
}

impl KDrawing2D {
    /// The drawing's loops and paths as a sketch on `plane`, placed by
    /// `align` against `at`, every loop and path named
    /// `<feature>/<layer>/loop[k]`.
    pub(crate) fn to_sketch(
        &self,
        layers: &[String],
        closed: Option<bool>,
        plane: Plane,
        at: P2,
        align: &str,
        tolerance: f64,
    ) -> anyhow::Result<(Sketch, dxf::SketchOut)> {
        let out = self.drawing.sketch(layers, closed, tolerance).map_err(|e| anyhow::anyhow!("Drawing2D.sketch: {e}"))?;
        let mut im = cadpole_import_2d::Import { loops: out.loops.clone(), paths: out.paths.clone(), notes: Vec::new() };
        let (dx, dy) = match align {
            "none" => (at.x, at.y),
            "min" | "center" | "centre" => match im.bbox() {
                None => (at.x, at.y),
                Some((lo, hi)) => {
                    if align == "min" {
                        (at.x - lo.x, at.y - lo.y)
                    } else {
                        (at.x - (lo.x + hi.x) / 2.0, at.y - (lo.y + hi.y) / 2.0)
                    }
                }
            },
            other => anyhow::bail!("Drawing2D.sketch: align {other:?} must be \"none\", \"min\" or \"center\""),
        };
        if dx != 0.0 || dy != 0.0 {
            im.map_points(move |p| P2::new(p.x + dx, p.y + dy));
        }
        let mut sketch = Sketch::new(plane);
        for (n, l) in im.loops {
            sketch.push_named_loop(l, format!("{}/{n}", self.feature));
        }
        for (n, p) in im.paths {
            sketch.push_path(p, format!("{}/{n}", self.feature));
        }
        Ok((sketch, out))
    }
}

#[starlark_value(type = "Drawing2D")]
impl<'v> StarlarkValue<'v> for KDrawing2D {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("drawing2d_methods", drawing2d_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        let d = &self.drawing;
        match attribute {
            "id" => Some(heap.alloc(self.feature.as_str())),
            // The unit the coordinates were scaled from (`"ft"`), and where
            // it came from: `"$INSUNITS"`, `"units="` or `"assumed"`.
            "units" => Some(heap.alloc(d.unit.name())),
            "unit_source" => Some(heap.alloc(d.unit_source.name())),
            // Millimetres per drawing unit.
            "scale" => Some(heap.alloc(d.scale)),
            "entities" => Some(heap.alloc(d.entities.len() as i32)),
            "layer_names" => Some(alloc_str_list(heap, &d.layer_names())),
            "version" => Some(heap.alloc(d.version.as_str())),
            // `$EXTMIN`/`$EXTMAX` as the file declared them, in mm, or None.
            "extents" => Some(match d.extents {
                Some((lo, hi)) => heap.alloc((alloc_p3(heap, lo), alloc_p3(heap, hi))),
                None => Value::new_none(),
            }),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        DRAWING2D_ATTRS.iter().map(|s| s.to_string()).collect()
    }
    fn has_attr(&self, attribute: &str, _heap: Heap<'v>) -> bool {
        DRAWING2D_ATTRS.contains(&attribute)
    }
}

#[starlark_module]
fn drawing2d_methods(builder: &mut MethodsBuilder) {
    /// The layers the import kept, in the file's order, each as
    /// `(name, entity_count, (r, g, b))`: the name is what `layer=` takes
    /// (the file's spelling with the name grammar's reserved characters
    /// replaced), the count is every entity on it in the file, and the
    /// colour is the layer's ACI index as RGB.
    fn layers<'v>(this: &KDrawing2D, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        let rows: Vec<Value<'v>> = this
            .drawing
            .layers
            .iter()
            .filter(|l| l.selected)
            .map(|l| {
                let c = l.color;
                heap.alloc((l.name.as_str(), l.entities as i32, (c[0] as i32, c[1] as i32, c[2] as i32)))
            })
            .collect();
        Ok(heap.alloc(rows))
    }

    /// The drawing as a `Sketch`: every closed entity (a closed polyline,
    /// a circle, a full ellipse, a hatch boundary) is a loop, and the open
    /// ones — the bag of `LINE`s a real plan is — are joined end to end
    /// within `tolerance` mm; a chain that closes is a loop, one that does
    /// not a construction path (reported, never extruded). `closed=True`
    /// keeps only the loops, `closed=False` only the chains. `layer` is a
    /// name or list of names (`*` and `?` allowed); `plane`, `at` and
    /// `align` (`"none"`, the default — a plan's own coordinates mean
    /// something — `"min"` or `"center"`) place it. Loops are named
    /// `<layer>/loop[k]` under the import's feature, so a face extruded
    /// from one traces back to the layer.
    fn sketch<'v>(
        this: &KDrawing2D,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] closed: Option<bool>,
        #[starlark(require = named)] plane: Option<&KPlane>,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] align: Option<&str>,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let layers = layer_patterns(layer, "Drawing2D.sketch")?;
        let tol = tolerance_arg(tolerance, "Drawing2D.sketch")?;
        let at = at.as_ref().map(|a| p2(a, "Drawing2D.sketch: at")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        let (sketch, out) =
            this.to_sketch(&layers, closed, plane.map(|p| p.0).unwrap_or(Plane::XY), at, align.unwrap_or("none"), tol)?;
        // What the sketch is made of, on the feature, and a note for the
        // chains that did not close.
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let fid = cadpole_core::FeatureId(this.feature.clone());
        // The k-th `.sketch()` call on this drawing.
        let k = ctx
            .feature(&fid)
            .map(|f| f.args.keys().filter(|k| k.starts_with("sketch[") && k.ends_with("].layers")).count())
            .unwrap_or(0);
        let prefix = format!("sketch[{k}].");
        ctx.set_feature_arg(&fid, &format!("{prefix}layers"), if layers.is_empty() { "*".to_owned() } else { layers.join(", ") });
        ctx.set_feature_arg(&fid, &format!("{prefix}loops"), sketch.loops.len());
        ctx.set_feature_arg(&fid, &format!("{prefix}paths"), sketch.paths.len());
        if !sketch.paths.is_empty() || out.open_dropped > 0 {
            let open = sketch.paths.len() + out.open_dropped;
            let span = call_span(eval);
            ctx.info(
                format!(
                    "Drawing2D.sketch: {open} chain{} did not close within {} mm and {} (raise tolerance= or fix the drawing)",
                    if open == 1 { "" } else { "s" },
                    fmt_num(tol),
                    if closed == Some(true) { "were left out" } else { "became construction paths, which cannot be extruded" }
                ),
                &fid,
                span,
            );
        }
        Ok(KSketch::new(this.feature.clone(), sketch))
    }

    /// Every geometry entity on the layers as a planar `Curve` in world
    /// space, in file order, each named `<layer>/<kind>[i]` — `PL/line[3]`,
    /// `CONTOUR/lwpolyline[12]` — under the import's feature. An entity
    /// whose points all lie at one elevation is drawn on the XY plane
    /// lifted to that z, so a contour polyline is a curve at its height.
    /// Ellipses and splines are Béziers within `tolerance` mm; arcs and
    /// bulges are exact.
    fn curves<'v>(
        this: &KDrawing2D,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let layers = layer_patterns(layer, "Drawing2D.curves")?;
        let tol = tolerance_arg(tolerance, "Drawing2D.curves")?;
        let out = this.drawing.curves(&layers, tol).map_err(|e| anyhow::anyhow!("Drawing2D.curves: {e}"))?;
        let mut curves: Vec<KCurve> = Vec::with_capacity(out.len());
        for c in out {
            let plane = match c.z {
                Some(z) if z != 0.0 => Plane::XY.offset(z),
                _ => Plane::XY,
            };
            let curve = if c.closed {
                Curve::from_loop(&Loop { segments: c.segments }, &plane)
            } else {
                let Some(first) = c.segments.first() else { continue };
                Curve::from_path(&Path { start: first.start(), segments: c.segments }, &plane)
            };
            curves.push(KCurve { feature: format!("{}/{}", this.feature, c.name), curve });
        }
        Ok(heap.alloc(curves))
    }

    /// `POINT` entities, and the vertices of the lines and polylines that
    /// carry an elevation (any vertex off z = 0), as `(x, y, z)` in mm:
    /// a survey's spot shots and 3D breaklines, in file order.
    fn points<'v>(
        this: &KDrawing2D,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let layers = layer_patterns(layer, "Drawing2D.points")?;
        let pts = this.drawing.points(&layers).map_err(|e| anyhow::anyhow!("Drawing2D.points: {e}"))?;
        let rows: Vec<Value<'v>> = pts.into_iter().map(|p| alloc_p3(heap, p)).collect();
        Ok(heap.alloc(rows))
    }

    /// Every label — `TEXT`, `MTEXT` with its formatting codes stripped,
    /// and the `ATTRIB` values a block's tags carry — as
    /// `(x, y, text, height, rotation)`: the placement point in mm (the
    /// alignment point of a justified `TEXT`), the plain text, the text
    /// height in mm and the rotation in degrees.
    fn texts<'v>(
        this: &KDrawing2D,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let layers = layer_patterns(layer, "Drawing2D.texts")?;
        let out = this.drawing.texts(&layers).map_err(|e| anyhow::anyhow!("Drawing2D.texts: {e}"))?;
        let rows: Vec<Value<'v>> = out
            .into_iter()
            .map(|t| {
                heap.alloc(AllocTuple([
                    heap.alloc(t.at[0]),
                    heap.alloc(t.at[1]),
                    heap.alloc(t.text.as_str()),
                    heap.alloc(t.height),
                    heap.alloc(t.rotation),
                ]))
            })
            .collect();
        Ok(heap.alloc(rows))
    }

    /// Spot elevations: every label that reads as one, as `(x, y, z)` —
    /// the label's placement point and the number it carries, scaled to
    /// mm as the coordinates were. **The convention** (no `pattern`): the
    /// label's words hold exactly one number, with a fractional part
    /// (`158.14`, never a lot number `12`), and at most two short
    /// alphabetic codes beside it — the `BK`, `TC`, `FL`, `EG`, `TW` a
    /// survey writes next to a shot — so `158.14 BK`, `TC 157.99` and
    /// `158.14` all read as 158.14 and `LOT 12` or `SEE NOTE 4.1 BELOW` do
    /// not; a trailing foot mark is allowed. Keep it to the survey's
    /// spot-elevation layer with `layer=`. `pattern` is a template for
    /// anything else: `#` for a number (the first `#` is the elevation),
    /// `*` for any run of characters, `?` for one, whitespace for a run of
    /// whitespace, anything else itself, case-insensitively — `"EL #"`,
    /// `"# BK"`, `"*=#"`.
    fn spot_elevations<'v>(
        this: &KDrawing2D,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] pattern: Option<&str>,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let layers = layer_patterns(layer, "Drawing2D.spot_elevations")?;
        let pts = this.drawing.spot_elevations(&layers, pattern).map_err(|e| anyhow::anyhow!("Drawing2D.spot_elevations: {e}"))?;
        let rows: Vec<Value<'v>> = pts.into_iter().map(|p| alloc_p3(heap, p)).collect();
        Ok(heap.alloc(rows))
    }

    /// Contour lines as `(elevation, [(x, y), ...])`, in file order: every
    /// polyline, line or spline on the layers, its elevation in mm from
    /// the entity's own z when every vertex shares one off zero, else from
    /// the last number in its layer's name (`CONTOUR-150`, `C-TOPO-152.5`,
    /// scaled like the coordinates); an entity with neither is skipped.
    /// Arcs and splines are flattened within `tolerance` mm; a closed
    /// contour repeats its first point at the end.
    fn contours<'v>(
        this: &KDrawing2D,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let layers = layer_patterns(layer, "Drawing2D.contours")?;
        let tol = tolerance_arg(tolerance, "Drawing2D.contours")?;
        let out = this.drawing.contours(&layers, tol).map_err(|e| anyhow::anyhow!("Drawing2D.contours: {e}"))?;
        let rows: Vec<Value<'v>> = out
            .into_iter()
            .map(|(z, pts)| {
                let pts: Vec<Value<'v>> = pts.into_iter().map(|p| heap.alloc((p.x, p.y))).collect();
                heap.alloc((z, heap.alloc(pts)))
            })
            .collect();
        Ok(heap.alloc(rows))
    }

    /// The bounding box of everything imported, `((x0, y0, z0), (x1, y1,
    /// z1))` in mm with curves sampled, or `None` for an empty drawing.
    /// `.extents` is what the file's header declared instead.
    fn bbox<'v>(this: &KDrawing2D, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(match this.drawing.bbox() {
            Some((lo, hi)) => heap.alloc((alloc_p3(heap, lo), alloc_p3(heap, hi))),
            None => Value::new_none(),
        })
    }
}

#[starlark_module]
pub(crate) fn import2d_globals(builder: &mut GlobalsBuilder) {
    /// Import an ASCII DXF (R12 through 2018) as a `Drawing2D`: layers,
    /// entities in millimetres, labels and points, read once and then
    /// questioned — `.sketch()`, `.curves()`, `.points()`, `.texts()`,
    /// `.spot_elevations()`, `.contours()`, `.bbox()`, `.layers()`.
    /// `source` is an `asset("site.dxf")` or the asset's name. `layers`
    /// keeps only the named layers (`*` and `?` allowed, case-insensitive;
    /// a name that matches nothing is an error listing what there is).
    /// Units come from `$INSUNITS`; a file that declares none is read as
    /// millimetres with a warning, and `units=dxf_units.ft` (or `inch`, `m`,
    /// `yd`, `survey_ft`, …; `in` is a Starlark keyword) settles it either
    /// way. Every coordinate,
    /// elevation and text height comes back in mm.
    ///
    /// What is read: `LINE`, `LWPOLYLINE` and `POLYLINE` (bulges stay
    /// arcs), `ARC`, `CIRCLE`, `ELLIPSE`, `SPLINE`, `POINT`, `TEXT`,
    /// `MTEXT`, `ATTRIB`, `SOLID`, `HATCH` boundaries, and `INSERT`s of
    /// blocks expanded with their scale, rotation and arrays (eight deep,
    /// never through a cycle). What is not, each counted and said once:
    /// `DIMENSION` and `LEADER` (explode them), `3DSOLID`/`REGION`/`BODY`
    /// (ACIS data; export STEP), `ACAD_PROXY_ENTITY`, images and
    /// viewports. DWG is proprietary and is not read at all: export a DXF.
    /// A `Drawing2D` is accepted wherever a `Sketch` is, as its
    /// `.sketch()` with the defaults.
    fn import_dxf<'v>(
        #[starlark(require = pos)] source: Value<'v>,
        #[starlark(require = named)] layers: Option<Value<'v>>,
        #[starlark(require = named)] units: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KDrawing2D> {
        let asset = source_name(source, "import_dxf")?;
        let layers = layer_patterns(layers, "import_dxf")?;
        let unit = Unit::parse(units.unwrap_or("")).map_err(|e| anyhow::anyhow!("import_dxf: {e}"))?;
        let span = call_span(eval);
        let st = store(eval)?;
        let (record, bytes) = asset_of(st, &asset, "import_dxf", &[cadpole_assets::AssetKind::Dxf])?;
        let budget = budget_ms(st);
        let mut ctx = st.ctx.borrow_mut();
        let mut args = BTreeMap::new();
        args.insert("asset".into(), asset.clone());
        args.insert("sha256".into(), record.sha256.clone());
        if !layers.is_empty() {
            args.insert("layers".into(), layers.join(", "));
        }
        if unit != Unit::Auto {
            args.insert("units".into(), unit.name().to_owned());
        }
        args.insert("importer_version".into(), IMPORTER_VERSION.to_string());
        let fid = ctx.new_feature("import_dxf", name.map(str::to_owned), vec![], args, span.clone())?;
        let mut key = KeyBuilder::new("import_dxf");
        key.put(&fid).str(&record.sha256).str(&layers.join("\u{1f}")).str(unit.name()).u64(IMPORTER_VERSION as u64);
        let fid_run = fid.clone();
        let asset_run = asset.clone();
        let drawing = ctx.produce_value::<Drawing, _>("import_dxf", &fid, span.clone(), key.finish(), move |ctx| {
            let opts = dxf::Options { layers, units: unit, tolerance: DEFAULT_TOLERANCE, budget_ms: budget };
            let d = dxf::import(&bytes, &opts).map_err(|e| CadpoleError::invalid(format!("import_dxf({asset_run:?}): {e}")))?;
            let mut warned = 0;
            for n in &d.notes {
                record_note(ctx, &fid_run, n.level, &n.message, &mut warned);
            }
            let texts = d.entities.iter().filter(|e| e.kind.is_text()).count();
            let points = d.entities.iter().filter(|e| e.kind == dxf::Kind::Point).count();
            let layer_names = d.layer_names();
            ctx.set_feature_arg(&fid_run, "entities", d.entities.len());
            ctx.set_feature_arg(&fid_run, "unit", d.unit.name());
            ctx.set_feature_arg(&fid_run, "unit_source", d.unit_source.name());
            ctx.set_feature_arg(&fid_run, "layers_found", layer_names.len());
            ctx.set_feature_arg(&fid_run, "layer_names", layer_names.join(", "));
            ctx.set_feature_arg(&fid_run, "texts", texts);
            ctx.set_feature_arg(&fid_run, "points", points);
            ctx.set_feature_arg(&fid_run, "blocks", d.blocks);
            ctx.set_feature_arg(&fid_run, "inserts", d.inserts);
            if !d.version.is_empty() {
                ctx.set_feature_arg(&fid_run, "version", &d.version);
            }
            if !d.skipped.is_empty() {
                let s: Vec<String> = d.skipped.iter().map(|(k, n)| format!("{k}:{n}")).collect();
                ctx.set_feature_arg(&fid_run, "skipped", s.join(", "));
            }
            kernel::notes::push(format!(
                "import_dxf: {} entit{} on {} layer{} ({}); unit {} ({}); {} text{}, {} point{}, {} block{} in {} INSERT{}{}",
                d.entities.len(),
                if d.entities.len() == 1 { "y" } else { "ies" },
                layer_names.len(),
                if layer_names.len() == 1 { "" } else { "s" },
                layer_names.join(", "),
                d.unit.name(),
                d.unit_source.name(),
                texts,
                if texts == 1 { "" } else { "s" },
                points,
                if points == 1 { "" } else { "s" },
                d.blocks,
                if d.blocks == 1 { "" } else { "s" },
                d.inserts,
                if d.inserts == 1 { "" } else { "s" },
                if d.version.is_empty() { String::new() } else { format!("; {}", d.version) },
            ));
            let bytes = d.bytes();
            Ok((d, bytes))
        })?;
        raise_recorded_warnings(&mut ctx, &fid, span);
        Ok(KDrawing2D { feature: fid.0, drawing })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::tests::errors;
    use crate::{EvalInput, ScriptFile, evaluate};

    /// A DXF of `n` unconnected LINEs on one layer: every one an open path.
    fn loose_lines(n: usize) -> Vec<u8> {
        let mut s = String::from("0\nSECTION\n2\nENTITIES\n");
        for i in 0..n {
            s.push_str(&format!("0\nLINE\n8\nL\n10\n{i}\n20\n0\n11\n{i}\n21\n1\n"));
        }
        s.push_str("0\nENDSEC\n0\nEOF\n");
        s.into_bytes()
    }

    /// `Drawing2D.sketch()` used to add each open path by copying the whole
    /// sketch built so far, so a plan with tens of thousands of loose lines
    /// took minutes (32 000 lines: 52 s, 4x per doubling). Building it is
    /// linear: the 24 000 paths here come out in about the time the import
    /// takes, and all of them are there, named.
    #[test]
    fn a_drawing_with_many_open_paths_becomes_a_sketch_in_linear_time() {
        let n = 24_000;
        let mut store = cadpole_assets::MemoryAssetStore::new();
        store.insert("lines.dxf", loose_lines(n));
        let src = format!(
            "d = import_dxf(asset(\"lines.dxf\"), name=\"d\")\n\
             s = d.sketch()\n\
             assert_eq(s.paths, {n})\n\
             assert_eq(s.loops, 0)\n"
        );
        let t0 = std::time::Instant::now();
        let ctx = evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src }],
            assets: Some(Arc::new(store)),
            ..Default::default()
        });
        let took = t0.elapsed().as_secs_f64();
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(took < 5.0, "{n} open paths took {took:.1} s");
    }
}
