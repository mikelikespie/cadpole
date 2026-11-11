//! `terrain(...)`, `terrain_from_contours(...)` and the `Terrain` value
//! (docs/buildings.md).
//!
//! A `terrain(...)` call records one feature and makes no body: the surface
//! is a value, queried by `z_at`, `slope_at`, `average_grade` and
//! `contours`, raised by `offset_up`, and turned into a body by `.body()`,
//! which is the kernel op — keyed through `Context::produce` by the
//! points, the method, the extent, the offset and the base, so a re-run
//! that moves nothing rebuilds nothing. A TIN's body is a mesh slab
//! (`<name>/top`, `/bottom`, `/side`); a fit's is an exact slab
//! (`<name>/top`, `/bottom`, `/side[0..4]`).
//!
//! `terrain` is a callable vocabulary: `terrain.tin` is `"tin"` and
//! `terrain(points, method=terrain.tin)` is the call. The native behind it
//! lives here and `vocab::all()` hangs the vocabulary on it through
//! [`crate::emboss::native`].

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, LazyLock};

use allocative::Allocative;
use cadpole_core::context::{fmt_num, Body, NameSource};
use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::memo::{KeyBuilder, Produced, ProducedBody};
use cadpole_core::sketch::{Path, Segment, Sketch, P2};
use cadpole_core::terrain::{Extent, Ground, Method, Terrain};
use cadpole_core::FeatureId;
use starlark::environment::{Globals, GlobalsBuilder, Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::values::float::UnpackFloat;
use starlark::values::list::UnpackList;
use starlark::values::{
    FrozenValue, Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueLike,
};
use starlark::{starlark_module, starlark_simple_value};
use starlark_derive::starlark_value;

use crate::check::{num, opt_num, p2, vec3, VecArg};
use crate::convert::alloc_vec3;
use crate::globals::bodies_value;
use crate::store::{call_span, store};
use crate::values::{KCurve, KSketch};

/// The most spot elevations one terrain takes: a dense LiDAR tile, not a
/// point cloud.
pub const MAX_TERRAIN_POINTS: usize = 200_000;

static NATIVES: LazyLock<Globals> = LazyLock::new(|| GlobalsBuilder::new().with(natives_terrain).build());

/// The native function behind the `terrain` vocabulary.
pub(crate) fn native(name: &str) -> Option<FrozenValue> {
    NATIVES.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
}

/// A length for a terrain's diagnostics and feature args.
///
/// TODO: read the document's display unit off the context
/// (`Context.display_units`, the building library's convention) once it
/// lands, so a site in feet reports its residual and its relief in feet
/// and inches; until then every readout is millimetres, as the kernel is.
fn length_label(_ctx: &cadpole_core::Context, mm: f64) -> String {
    format!("{} mm", fmt_num(mm))
}

// ---------------------------------------------------------------- Terrain

/// A ground surface: what `terrain(...)` returns.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KTerrain {
    /// The `terrain` feature's id.
    pub feature: String,
    #[allocative(skip)]
    pub terrain: Arc<Terrain>,
    /// The points as given, in order: what keys the body op.
    #[allocative(skip)]
    pub points: Arc<Vec<Vec3>>,
    /// The extent the script asked for, if any.
    #[allocative(skip)]
    pub asked_extent: Option<Extent>,
    /// The rectangle `clip` cut the surface down to, if any.
    #[allocative(skip)]
    pub clip: Option<Extent>,
}
starlark_simple_value!(KTerrain);

impl fmt::Display for KTerrain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (lo, hi) = self.terrain.z_range();
        write!(
            f,
            "terrain({}, {}, {} points, z {} .. {}{}{})",
            self.feature,
            self.terrain.method().name(),
            self.points.len(),
            fmt_num(lo),
            fmt_num(hi),
            if self.terrain.offset != 0.0 { format!(", raised {}", fmt_num(self.terrain.offset)) } else { String::new() },
            match &self.clip {
                Some(e) => format!(", clipped to {}", fmt_extent(e)),
                None => String::new(),
            }
        )
    }
}

impl KTerrain {
    fn extent_value<'v>(&self, heap: Heap<'v>) -> Value<'v> {
        let e = self.terrain.extent;
        heap.alloc(((e.x0, e.y0), (e.x1, e.y1)))
    }
}

#[starlark_value(type = "Terrain")]
impl<'v> StarlarkValue<'v> for KTerrain {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("terrain_methods", terrain_methods);
        Some(RES.methods())
    }

    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        let (lo, hi) = self.terrain.z_range();
        match attribute {
            "id" => Some(heap.alloc(self.feature.as_str())),
            "method" => Some(heap.alloc(self.terrain.method().name())),
            "points" => Some(heap.alloc(self.points.len() as i32)),
            "extent" => Some(self.extent_value(heap)),
            "z_min" => Some(heap.alloc(lo)),
            "z_max" => Some(heap.alloc(hi)),
            "offset" => Some(heap.alloc(self.terrain.offset)),
            // The TIN's size; `None` for a fit.
            "triangles" => Some(match &self.terrain.ground {
                Ground::Tin(t) => heap.alloc(t.triangles.len() as i32),
                Ground::Fit(_) => Value::new_none(),
            }),
            // The fit's net: control points and knot spans, `(along x,
            // along y)`; `None` for a TIN.
            "control" | "spans" => Some(match &self.terrain.ground {
                Ground::Fit(f) => {
                    let (a, b) = if attribute == "control" { f.control() } else { f.spans() };
                    heap.alloc((a as i32, b as i32))
                }
                Ground::Tin(_) => Value::new_none(),
            }),
            // The fit's residual over its points, mm: rms and the largest.
            "residual" | "max_residual" => Some(match self.terrain.residual() {
                Some((rms, max)) => heap.alloc(if attribute == "residual" { rms } else { max }),
                None => Value::new_none(),
            }),
            _ => None,
        }
    }

    fn dir_attr(&self) -> Vec<String> {
        ["id", "method", "points", "extent", "z_min", "z_max", "offset", "triangles", "control", "spans", "residual", "max_residual"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

/// A point in the plane from `(x, y)` (a third coordinate is ignored).
fn xy(v: &VecArg, what: &str) -> anyhow::Result<(f64, f64)> {
    let p = p2(v, what)?;
    Ok((p.x, p.y))
}

/// `((x0, y0), (x1, y1))` or `(x0, y0, x1, y1)`.
fn extent_arg<'v>(v: Option<Value<'v>>, heap: Heap<'v>, what: &str) -> anyhow::Result<Option<Extent>> {
    let Some(v) = v.filter(|v| !v.is_none()) else { return Ok(None) };
    let items: Vec<Value<'_>> = v.iterate(heap).map_err(|e| e.into_anyhow())?.collect();
    let number = |n: Value<'_>| -> anyhow::Result<f64> {
        let f: Option<UnpackFloat> = starlark::values::UnpackValue::unpack_value(n).map_err(|e| e.into_anyhow())?;
        f.map(|f| f.0).ok_or_else(|| anyhow::anyhow!("{what}: extent must be numbers, got {}", n.get_type()))
    };
    let nums: Vec<f64> = match items.len() {
        2 => {
            let mut out = Vec::with_capacity(4);
            for it in &items {
                let pair: Vec<Value<'_>> = it.iterate(heap).map_err(|e| e.into_anyhow())?.collect();
                if pair.len() != 2 {
                    anyhow::bail!("{what}: extent is ((x0, y0), (x1, y1)), got a corner with {} numbers", pair.len());
                }
                for n in pair {
                    out.push(number(n)?);
                }
            }
            out
        }
        4 => items.iter().map(|n| number(*n)).collect::<anyhow::Result<_>>()?,
        n => anyhow::bail!("{what}: extent is ((x0, y0), (x1, y1)), got {n} items"),
    };
    Ok(Some(Extent::new(nums[0], nums[1], nums[2], nums[3]).map_err(|e| anyhow::anyhow!("{what}: {e}"))?))
}

fn fmt_extent(e: &Extent) -> String {
    format!("({}, {}) .. ({}, {})", fmt_num(e.x0), fmt_num(e.y0), fmt_num(e.x1), fmt_num(e.y1))
}

/// Build the terrain, record its feature and say what it is.
fn make_terrain<'v>(
    points: Vec<Vec3>,
    method: Option<&str>,
    extent: Option<Extent>,
    name: Option<&str>,
    what: &str,
    mut args: BTreeMap<String, String>,
    eval: &mut Evaluator<'v, '_, '_>,
) -> anyhow::Result<KTerrain> {
    let method = Method::parse(method.unwrap_or("tin")).map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
    if points.len() > MAX_TERRAIN_POINTS {
        anyhow::bail!("{what}: {} points, over the {MAX_TERRAIN_POINTS} limit", points.len());
    }
    let terrain = Terrain::new(&points, method, extent).map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
    let span = call_span(eval);
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    args.insert("method".into(), method.name().to_owned());
    args.insert("points".into(), points.len().to_string());
    args.insert("extent".into(), fmt_extent(&terrain.extent));
    let message = match &terrain.ground {
        Ground::Tin(t) => {
            args.insert("triangles".into(), t.triangles.len().to_string());
            if t.duplicates > 0 {
                args.insert("duplicates".into(), t.duplicates.to_string());
            }
            if t.outside > 0 {
                args.insert("outside".into(), t.outside.to_string());
            }
            format!(
                "{what}: a TIN of {} point{} in {} triangles over {}{}{}",
                t.points.len(),
                if t.points.len() == 1 { "" } else { "s" },
                t.triangles.len(),
                fmt_extent(&terrain.extent),
                if t.duplicates > 0 {
                    format!("; {} point{} at the same plane position as an earlier one dropped", t.duplicates, if t.duplicates == 1 { "" } else { "s" })
                } else {
                    String::new()
                },
                if t.outside > 0 {
                    format!("; {} outside the extent left out", t.outside)
                } else {
                    String::new()
                },
            )
        }
        Ground::Fit(f) => {
            let ((cu, cv), (su, sv)) = (f.control(), f.spans());
            args.insert("control".into(), format!("{cu}×{cv}"));
            args.insert("spans".into(), format!("{su}×{sv}"));
            args.insert("rms_residual".into(), fmt_num(f.rms));
            args.insert("max_residual".into(), fmt_num(f.max_residual));
            if f.outside > 0 {
                args.insert("outside".into(), f.outside.to_string());
            }
            format!(
                "{what}: a bicubic B-spline through {} point{} on a {cu}×{cv} control net ({su}×{sv} knot spans) over {}: \
                 rms residual {}, largest {}{}",
                f.fitted,
                if f.fitted == 1 { "" } else { "s" },
                fmt_extent(&terrain.extent),
                length_label(&ctx, f.rms),
                length_label(&ctx, f.max_residual),
                if f.outside > 0 { format!("; {} outside the extent left out", f.outside) } else { String::new() },
            )
        }
    };
    let fid = ctx.new_feature("terrain", name.map(str::to_owned), vec![], args, span.clone())?;
    ctx.info(message, &fid, span);
    Ok(KTerrain { feature: fid.0, terrain: Arc::new(terrain), points: Arc::new(points), asked_extent: extent, clip: None })
}

#[starlark_module]
fn natives_terrain(builder: &mut GlobalsBuilder) {
    /// A ground surface from spot elevations: `points` is a list of
    /// `(x, y, z)` in millimetres, a survey's spot shots or a LiDAR
    /// footprint's ground samples. `method=terrain.tin` (the default)
    /// triangulates them — the surface passes through every point and its
    /// body is a mesh; `method=terrain.fit` fits a least-squares bicubic
    /// B-spline — smooth, and its body is an exact B-rep whose top face is
    /// the patch, so booleans with an envelope stay exact and export to
    /// STEP. The feature records the triangle count or the net size and
    /// the residual; `cadpole inspect` prints them as an info line.
    ///
    /// `extent=((x0, y0), (x1, y1))` is the rectangle the surface covers.
    /// Without it a TIN covers the points' convex hull and a fit their
    /// bounding rectangle. With it a TIN is clipped to the rectangle, its
    /// corners interpolated from the points (or, outside them, continued
    /// from the plane of the twelve nearest), and a fit is defined over
    /// it, continuing its least-squares plane where no point reaches.
    ///
    /// The value answers `z_at(x, y)`, `slope_at(x, y)`,
    /// `average_grade(polygon)`, `contours(interval)`, `offset_up(h)` and
    /// `body(base=, name=)`; see the `Terrain` type.
    fn terrain<'v>(
        #[starlark(require = pos)] points: UnpackList<VecArg>,
        #[starlark(require = named)] method: Option<&str>,
        #[starlark(require = named)] extent: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KTerrain> {
        let pts: Vec<Vec3> = points.items.iter().map(|p| vec3(p, "terrain: point")).collect::<anyhow::Result<_>>()?;
        let extent = extent_arg(extent, eval.heap(), "terrain")?;
        make_terrain(pts, method, extent, name, "terrain", BTreeMap::new(), eval)
    }
}

/// The plain globals: `terrain_from_contours` (the vocabularies carry
/// `terrain` and `units` themselves).
#[starlark_module]
pub(crate) fn terrain_globals(builder: &mut GlobalsBuilder) {
    /// A ground surface from contour lines: `polylines` is a list of
    /// polylines, each a list of `(x, y, z)` with `z` the line's elevation,
    /// as a contour drawing or a DXF import gives them. Every vertex is a
    /// point, and any segment longer than `spacing` (default: the median
    /// segment length) is sampled at that spacing so the surface follows
    /// the line between its vertices; then the points go through
    /// `terrain(...)` with the same `method`, `extent` and `name`. A TIN of
    /// contour points has the usual flat triangles where a ridge or a
    /// valley crosses between lines; `method=terrain.fit` smooths them.
    fn terrain_from_contours<'v>(
        #[starlark(require = pos)] polylines: UnpackList<UnpackList<VecArg>>,
        #[starlark(require = named)] method: Option<&str>,
        #[starlark(require = named)] extent: Option<Value<'v>>,
        #[starlark(require = named)] spacing: Option<UnpackFloat>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KTerrain> {
        let what = "terrain_from_contours";
        let mut lines: Vec<Vec<Vec3>> = Vec::with_capacity(polylines.items.len());
        for (k, line) in polylines.items.iter().enumerate() {
            let pts: Vec<Vec3> = line
                .items
                .iter()
                .map(|p| vec3(p, &format!("{what}: polyline {k}")))
                .collect::<anyhow::Result<_>>()?;
            if pts.is_empty() {
                anyhow::bail!("{what}: polyline {k} has no points");
            }
            lines.push(pts);
        }
        if lines.is_empty() {
            anyhow::bail!("{what}: no polylines");
        }
        let spacing = match opt_num(&spacing, &format!("{what}: spacing"))? {
            Some(s) => s,
            None => cadpole_core::terrain::default_contour_spacing(&lines),
        };
        let pts = cadpole_core::terrain::sample_contours(&lines, spacing).map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
        let extent = extent_arg(extent, eval.heap(), what)?;
        let mut args = BTreeMap::new();
        args.insert("polylines".into(), lines.len().to_string());
        args.insert("spacing".into(), fmt_num(spacing));
        make_terrain(pts, method, extent, name, what, args, eval)
    }
}

#[starlark_module]
fn terrain_methods(builder: &mut MethodsBuilder) {
    /// The elevation at `(x, y)`, mm. An error off the surface.
    fn z_at<'v>(this: &KTerrain, #[starlark(require = pos)] x: UnpackFloat, #[starlark(require = pos)] y: UnpackFloat) -> anyhow::Result<f64> {
        let (x, y) = (num(&x, "z_at: x")?, num(&y, "z_at: y")?);
        this.terrain.z_at(x, y).ok_or_else(|| off_surface(this, x, y, "z_at"))
    }

    /// The grade at `(x, y)`: rise over run, `0.25` for a 25 % slope. An
    /// error off the surface.
    fn slope_at<'v>(this: &KTerrain, #[starlark(require = pos)] x: UnpackFloat, #[starlark(require = pos)] y: UnpackFloat) -> anyhow::Result<f64> {
        let (x, y) = (num(&x, "slope_at: x")?, num(&y, "slope_at: y")?);
        this.terrain.slope_at(x, y).ok_or_else(|| off_surface(this, x, y, "slope_at"))
    }

    /// The direction of steepest descent at `(x, y)` as a unit vector
    /// `(dx, dy, 0)`, or `None` where the ground is flat.
    fn downhill_at<'v>(
        this: &KTerrain,
        #[starlark(require = pos)] x: UnpackFloat,
        #[starlark(require = pos)] y: UnpackFloat,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let (x, y) = (num(&x, "downhill_at: x")?, num(&y, "downhill_at: y")?);
        if this.terrain.z_at(x, y).is_none() {
            return Err(off_surface(this, x, y, "downhill_at"));
        }
        Ok(match this.terrain.downhill_at(x, y) {
            Some(d) => alloc_vec3(heap, d),
            None => Value::new_none(),
        })
    }

    /// The average grade over `polygon` — a list of `(x, y)`, or a Sketch
    /// whose first loop is the polygon — as rise over run: the slope of the
    /// least-squares plane through the surface sampled on a grid of about
    /// 400 points inside it. What "a 25 % lot" means.
    fn average_grade<'v>(this: &KTerrain, #[starlark(require = pos)] polygon: Value<'v>, heap: Heap<'v>) -> anyhow::Result<f64> {
        let poly = polygon_arg(polygon, heap, "average_grade")?;
        this.terrain
            .average_grade(&poly)
            .ok_or_else(|| anyhow::anyhow!("average_grade: no sample of the polygon lands on the surface"))
    }

    /// Contour lines at every multiple of `interval` (mm) within the
    /// surface's range, as a list of `Curve`s on the surface: polylines
    /// marched over the TIN's triangles (exact on its edges), or over a
    /// 96 × 96 sampling of a fit. Each carries this terrain's feature id.
    fn contours<'v>(
        this: &KTerrain,
        #[starlark(require = pos)] interval: UnpackFloat,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let interval = num(&interval, "contours: interval")?;
        let curves: Vec<KCurve> = this
            .terrain
            .contours(interval)
            .map_err(|e| anyhow::anyhow!("contours: {e}"))?
            .into_iter()
            .map(|curve| KCurve { feature: this.feature.clone(), curve })
            .collect();
        Ok(heap.alloc(curves))
    }

    /// The same contours as a `Sketch` on XY for a site plan: one
    /// construction path per line, named `contour[<z>]` (and `#k` for the
    /// k-th line at one elevation), drawn as open strokes by `export_svg`
    /// and `export_dxf` and never extruded. Records a `sketch` feature.
    fn contour_sketch<'v>(
        this: &KTerrain,
        #[starlark(require = pos)] interval: UnpackFloat,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let interval = num(&interval, "contour_sketch: interval")?;
        let curves = this.terrain.contours(interval).map_err(|e| anyhow::anyhow!("contour_sketch: {e}"))?;
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut args = BTreeMap::new();
        args.insert("plane".into(), "XY".into());
        args.insert("terrain".into(), this.feature.clone());
        args.insert("interval".into(), fmt_num(interval));
        args.insert("contours".into(), curves.len().to_string());
        args.insert("loops".into(), "0".into());
        let fid = ctx.new_feature("sketch", name.map(str::to_owned), vec![FeatureId(this.feature.clone())], args, span)?;
        let mut sketch = Sketch::new(Plane::XY);
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for c in &curves {
            let mut path = Path::new(P2::new(c.start.x, c.start.y));
            for s in &c.segments {
                let b = s.end();
                path.segments.push(Segment::Line { a: path.current(), b: P2::new(b.x, b.y) });
            }
            let level = fmt_num(c.start.z);
            let k = seen.entry(level.clone()).or_insert(0);
            let pname = if *k == 0 { format!("{}/contour[{level}]", fid) } else { format!("{}/contour[{level}]#{k}", fid) };
            *k += 1;
            sketch = sketch.with_path(path, pname);
        }
        Ok(KSketch::new(fid.0, sketch))
    }

    /// The same surface raised by `h` mm: a TIN with every vertex moved, a
    /// fit with every control point moved — exact in both cases, and what
    /// "30 ft above grade at every point" is. A value; nothing recorded
    /// until `.body()`.
    fn offset_up<'v>(this: &KTerrain, #[starlark(require = pos)] h: UnpackFloat) -> anyhow::Result<KTerrain> {
        let h = num(&h, "offset_up: h")?;
        Ok(KTerrain {
            feature: this.feature.clone(),
            terrain: Arc::new(this.terrain.offset_up(h)),
            points: this.points.clone(),
            asked_extent: this.asked_extent,
            clip: this.clip,
        })
    }

    /// The same surface over a smaller rectangle `extent` (`((x0, y0),
    /// (x1, y1))`, mm, inside this one's): a fitted patch is split there
    /// by knot insertion and a TIN's triangles are clipped, so `z_at` is
    /// unchanged wherever both are defined and `.body()` is a slab over
    /// exactly that rectangle — a lot's envelope out of a site's surface
    /// with no boolean, which matters for a fit: the exact kernel's
    /// intersection of the patch with a prism leaves a trimmed face the
    /// tessellator cannot always close. A value; nothing recorded.
    fn clip<'v>(this: &KTerrain, #[starlark(require = pos)] extent: Value<'v>, heap: Heap<'v>) -> anyhow::Result<KTerrain> {
        let Some(e) = extent_arg(Some(extent), heap, "clip")? else {
            anyhow::bail!("clip: extent is ((x0, y0), (x1, y1))");
        };
        let terrain = this.terrain.clip(e).map_err(|e| anyhow::anyhow!("clip: {e}"))?;
        Ok(KTerrain {
            feature: this.feature.clone(),
            terrain: Arc::new(terrain),
            points: this.points.clone(),
            asked_extent: this.asked_extent,
            clip: Some(e),
        })
    }

    /// The lowest and highest elevation over `polygon` — a list of `(x, y)`,
    /// or a Sketch whose first loop is the polygon — as `(lo, hi)` mm, from
    /// the surface sampled on a grid of about 400 points inside it and at
    /// its vertices (a sampling: a peak between samples can sit a little
    /// outside). What decides whether a cap or a pad level touches the
    /// ground at all.
    fn z_range<'v>(this: &KTerrain, #[starlark(require = pos)] polygon: Value<'v>, heap: Heap<'v>) -> anyhow::Result<(f64, f64)> {
        let poly = polygon_arg(polygon, heap, "z_range")?;
        this.terrain.z_range_over(&poly).ok_or_else(|| anyhow::anyhow!("z_range: no sample of the polygon lands on the surface"))
    }

    /// The surface as a body: a slab from the surface down to the plane
    /// `z = base` (default: below the lowest point by a tenth of the
    /// relief, half a metre at least). A TIN gives a mesh body with the
    /// faces `<name>/top`, `<name>/bottom` and `<name>/side`; a fit gives
    /// an exact B-rep with `<name>/top` (the patch), `<name>/bottom` and
    /// `<name>/side[0]`…`[3]` (south, east, north, west). One kernel op,
    /// keyed by the points, the method, the extent, the clip, the offset
    /// and the base.
    ///
    /// `tolerance=` is the fitted slab's own chord tolerance in mm, as
    /// `Body.tessellation(tolerance=)` sets it: a site is metres, and the
    /// document's 0.05 mm makes a few hundred thousand triangles of a
    /// hillside's patch where 5 mm makes a thousand. A TIN's triangles are
    /// the surface, so it ignores this.
    fn body<'v>(
        this: &KTerrain,
        #[starlark(require = named)] base: Option<UnpackFloat>,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let base = match opt_num(&base, "body: base")? {
            Some(b) => b,
            None => this.terrain.default_base(),
        };
        let tolerance = opt_num(&tolerance, "body: tolerance")?;
        if let Some(t) = tolerance {
            if !(t > 0.0) {
                anyhow::bail!("body: tolerance must be positive, got {t}");
            }
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let method = this.terrain.method();
        let mut args = BTreeMap::new();
        args.insert("terrain".into(), this.feature.clone());
        args.insert("method".into(), method.name().to_owned());
        args.insert("base".into(), fmt_num(base));
        args.insert("offset".into(), fmt_num(this.terrain.offset));
        if let Some(e) = &this.clip {
            args.insert("clip".into(), fmt_extent(e));
        }
        let fid = ctx.new_feature("terrain_body", name.map(str::to_owned), vec![FeatureId(this.feature.clone())], args, span.clone())?;
        let mut key = KeyBuilder::new("terrain_body");
        key.put(&fid).str(method.name()).put(this.points.as_ref()).f64(this.terrain.offset).f64(base);
        for e in [this.asked_extent, this.clip] {
            match e {
                Some(e) => {
                    key.bool(true).f64(e.x0).f64(e.y0).f64(e.x1).f64(e.y1);
                }
                None => {
                    key.bool(false);
                }
            }
        }
        let terrain = this.terrain.clone();
        let fid_run = fid.clone();
        let ids = ctx.produce("terrain_body", &fid, span, key.finish(), name.map(str::to_owned), move |_ctx| {
            let prefix = fid_run.0.clone();
            match terrain.method() {
                Method::Tin => {
                    let mb = terrain.mesh_body(base, &prefix)?;
                    Ok(Produced::one(Body::Mesh(mb), NameSource::Intrinsic))
                }
                Method::Fit => {
                    let solid = terrain.brep_body(base)?;
                    let names = Terrain::brep_face_names(&prefix);
                    Ok(Produced::bodies(vec![ProducedBody::new(
                        Body::Brep(solid),
                        NameSource::Listed { names: Arc::new(names) },
                    )]))
                }
            }
        })?;
        if let (Some(t), Method::Fit) = (tolerance, method) {
            for id in &ids {
                ctx.body_mut(*id)?.tessellation.tolerance = Some(t);
            }
        }
        bodies_value(eval.heap(), &ctx, ids)
    }
}

fn off_surface(t: &KTerrain, x: f64, y: f64, what: &str) -> anyhow::Error {
    let e = t.terrain.extent;
    anyhow::anyhow!(
        "{what}: ({}, {}) is off the surface {} ({}{})",
        fmt_num(x),
        fmt_num(y),
        t.feature,
        match t.terrain.method() {
            Method::Tin => "the convex hull of its points within ",
            Method::Fit => "its extent ",
        },
        fmt_extent(&e)
    )
}

/// A polygon argument: a list of `(x, y)`, or a Sketch's first loop.
fn polygon_arg<'v>(v: Value<'v>, heap: Heap<'v>, what: &str) -> anyhow::Result<Vec<P2>> {
    if let Some(s) = v.downcast_ref::<KSketch>() {
        let sk = s.sketch();
        let Some(l) = sk.loops.first() else {
            anyhow::bail!("{what}: the sketch has no loop");
        };
        return Ok(l.sample());
    }
    let items: Vec<Value<'_>> = v.iterate(heap).map_err(|e| e.into_anyhow())?.collect();
    if items.len() < 3 {
        anyhow::bail!("{what}: a polygon needs at least 3 points, got {}", items.len());
    }
    items
        .iter()
        .map(|it| {
            let arg: VecArg = starlark::values::UnpackValue::unpack_value(*it)
                .map_err(|e| e.into_anyhow())?
                .ok_or_else(|| anyhow::anyhow!("{what}: a polygon is a list of (x, y) points, got {}", it.get_type()))?;
            xy(&arg, &format!("{what}: point")).map(|(x, y)| P2::new(x, y))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    const SLOPE: &str = r#"
pts = []
for i in range(6):
    for j in range(8):
        x = i * 2000.0 + (j % 2) * 300.0
        y = j * 2500.0
        pts.append((x, y, 50000.0 - 0.31 * y + 0.02 * x))
"#;

    #[test]
    fn a_terrain_answers_its_queries_and_records_its_feature() {
        let ctx = run(&format!(
            "{SLOPE}
t = terrain(pts, method=terrain.tin, name=\"grade\")
assert_eq(t.method, \"tin\")
assert_eq(t.points, 48)
assert_true(t.triangles > 40, msg=\"triangles %d\" % t.triangles)
z = t.z_at(5000.0, 10000.0)
assert_true(z - (50000.0 - 3100.0 + 100.0) < 0.001 and (50000.0 - 3100.0 + 100.0) - z < 0.001, msg=\"z %f\" % z)
s = t.slope_at(5000.0, 10000.0)
assert_true(s > 0.31 and s < 0.312, msg=\"slope %f\" % s)
g = t.average_grade([(0, 0), (10000, 0), (10000, 17000), (0, 17000)])
assert_true(g > 0.31 and g < 0.312, msg=\"grade %f\" % g)
d = t.downhill_at(5000.0, 10000.0)
assert_true(d[1] > 0.99, msg=\"downhill\")
cs = t.contours(1000.0)
assert_true(len(cs) > 3, msg=\"%d contours\" % len(cs))
up = t.offset_up(9144.0)
assert_true(up.z_at(5000.0, 10000.0) - z - 9144.0 < 1e-6, msg=\"offset\")
assert_eq(up.offset, 9144.0)
b = t.body(base=30000.0, name=\"ground\")
assert_eq(b.kind, \"mesh\")
assert_solid(b)
e = up.body(base=30000.0, name=\"envelope\")
assert_true(e.volume() > b.volume(), msg=\"envelope volume\")
f = terrain(pts, method=terrain.fit, name=\"smooth\")
assert_eq(f.method, \"fit\")
assert_eq(f.control, (4, 4))
assert_eq(f.spans, (1, 1))
assert_true(f.residual < 1.0, msg=\"residual %f\" % f.residual)
fb = f.body(name=\"exact\")
assert_eq(fb.kind, \"brep\")
assert_solid(fb)
# The data is a plane, so the fitted patch is one too and is recognised as one: six planes, not five.
assert_true(len(fb.faces(kind=kind.plane)) == 6, msg=\"planes %d\" % len(fb.faces(kind=kind.plane)))
assert_true(len(fb.faces(kind=kind.nurbs)) == 1, msg=\"nurbs %d\" % len(fb.faces(kind=kind.nurbs)))
plan = t.contour_sketch(1000.0, name=\"plan\")
assert_true(plan.paths > 3, msg=\"paths %d\" % plan.paths)
# A clip is the same surface over a smaller rectangle, for both constructions,
# and its slab is exact for the fit with no boolean.
lot = ((1000, 2000), (8000, 15000))
lo, hi = t.z_range([(1000, 2000), (8000, 2000), (8000, 15000), (1000, 15000)])
assert_true(lo < z and z < hi, msg=\"z_range %f .. %f\" % (lo, hi))
fc = f.clip(lot)
assert_eq(fc.extent, ((1000.0, 2000.0), (8000.0, 15000.0)))
assert_close(fc.z_at(5000.0, 10000.0), f.z_at(5000.0, 10000.0), tol=1e-6)
lot_exact = fc.body(tolerance=5.0, name=\"lot_exact\")
assert_eq(lot_exact.kind, \"brep\")
assert_solid(lot_exact)
blo, bhi = lot_exact.bbox()
assert_close(blo[0], 1000.0, tol=1e-6)
assert_close(bhi[1], 15000.0, tol=1e-6)
tc = t.offset_up(9144.0).clip(lot)
assert_close(tc.z_at(5000.0, 10000.0), z + 9144.0, tol=1e-6)
lot_mesh = tc.body(base=30000.0, name=\"lot_mesh\")
assert_solid(lot_mesh)
assert_true(lot_mesh.volume() < e.volume(), msg=\"a clip is smaller\")
"
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.feature(&"grade".into()).unwrap();
        assert_eq!(f.kind, "terrain");
        assert_eq!(f.args["method"], "tin");
        assert_eq!(f.args["points"], "48");
        assert!(f.outputs.is_empty(), "a terrain is a value; its bodies are features of their own");
        let f = ctx.feature(&"smooth".into()).unwrap();
        assert_eq!(f.args["control"], "4×4");
        assert!(f.args.contains_key("rms_residual"));
        let body = ctx.feature(&"ground".into()).unwrap();
        assert_eq!(body.kind, "terrain_body");
        assert_eq!(body.inputs, vec!["grade".into()]);
        assert_eq!(body.outputs.len(), 1);
        let names: Vec<String> = ctx.bodies.iter().map(|b| b.name.clone()).collect();
        assert!(names.contains(&"ground".to_owned()) && names.contains(&"exact".to_owned()), "{names:?}");
        let infos: Vec<&str> = ctx.diagnostics.iter().map(|d| d.message.as_str()).filter(|m| m.starts_with("terrain:")).collect();
        assert!(infos.iter().any(|m| m.contains("a TIN of 48 points")), "{infos:?}");
        assert!(infos.iter().any(|m| m.contains("4×4 control net") && m.contains("rms residual")), "{infos:?}");
        let sk = ctx.feature(&"plan".into()).unwrap();
        assert_eq!(sk.kind, "sketch");
        assert_eq!(sk.inputs, vec!["grade".into()]);
        // The clipped slab records its clip and carries its own tolerance.
        let lot = ctx.feature(&"lot_exact".into()).unwrap();
        assert_eq!(lot.args["clip"], "(1000, 2000) .. (8000, 15000)");
        let id = ctx.bodies.iter().find(|b| b.name == "lot_exact").unwrap().id;
        assert_eq!(ctx.body(id).unwrap().tessellation.tolerance, Some(5.0));
        let id = ctx.bodies.iter().find(|b| b.name == "lot_mesh").unwrap().id;
        assert_eq!(ctx.body(id).unwrap().tessellation.tolerance, None, "a TIN's triangles are the surface");
    }

    #[test]
    fn the_faces_are_named_and_the_terrain_vocabulary_is_the_function() {
        let mut ctx = run(&format!(
            "{SLOPE}
t = terrain(pts, name=\"grade\")
g = t.body(name=\"ground\")
assert_eq(sorted([f.name for f in g.faces()]), [\"ground/bottom\", \"ground/side\", \"ground/top\"])
f = terrain(pts, method=terrain.fit, extent=((0, 0), (10000, 17500)), name=\"smooth\").body(name=\"exact\")
assert_eq(sorted([x.name for x in f.faces()]), [\"exact/bottom\", \"exact/side[0]\", \"exact/side[1]\", \"exact/side[2]\", \"exact/side[3]\", \"exact/top\"])
assert_eq(terrain.tin, \"tin\")
assert_eq(terrain.fit, \"fit\")
assert_eq(type(terrain), \"vocabulary\")
"
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let id = ctx.bodies.iter().find(|b| b.name == "exact").unwrap().id;
        let (_, topo) = ctx.body_mesh(id).unwrap();
        let top = topo.faces.iter().find(|f| f.name == "exact/top").unwrap();
        assert_eq!(top.surface_kind, "nurbs");
        assert_eq!(topo.faces.iter().filter(|f| f.surface_kind == "plane").count(), 5);
    }

    #[test]
    fn contours_and_refusals() {
        let ctx = run(&format!(
            "{SLOPE}
t = terrain(pts, name=\"grade\")
t.z_at(-5000.0, -5000.0)
"
        ));
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("off the surface grade"), "{}", e[0]);
        let ctx = run("t = terrain([(0, 0, 1), (1, 0, 1)])\n");
        assert!(errors(&ctx)[0].contains("at least 3 points"), "{:?}", errors(&ctx));
        let ctx = run(&format!("{SLOPE}\nt = terrain(pts, method=\"spline\")\n"));
        assert!(errors(&ctx)[0].contains("valid: tin, fit"), "{:?}", errors(&ctx));
        let ctx = run(&format!("{SLOPE}\nt = terrain(pts, method=terrain.tni)\n"));
        assert!(errors(&ctx)[0].contains("not a member of terrain"), "{:?}", errors(&ctx));
        let ctx = run(&format!("{SLOPE}\nt = terrain(pts, name=\"grade\").clip(((-1, 0), (5000, 5000)))\n"));
        assert!(errors(&ctx)[0].contains("clip: terrain: the clip rectangle") && errors(&ctx)[0].contains("must lie within"), "{:?}", errors(&ctx));
        let ctx = run(&format!("{SLOPE}\nt = terrain(pts, method=terrain.fit, name=\"grade\").body(tolerance=0)\n"));
        assert!(errors(&ctx)[0].contains("tolerance must be positive"), "{:?}", errors(&ctx));
        // From contours: three lines at three levels, sampled.
        let ctx = run(
            "lines = [[(0, 0, 100), (20000, 0, 100)], [(0, 5000, 90), (20000, 5000, 90)], [(0, 10000, 70), (20000, 10000, 70)]]
t = terrain_from_contours(lines, spacing=2000, name=\"c\")
assert_true(t.points > 6)
z = t.z_at(10000.0, 2500.0)
assert_true(z > 94.9 and z < 95.1, msg=\"z %f\" % z)
",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.feature(&"c".into()).unwrap();
        assert_eq!(f.args["polylines"], "3");
    }

}
