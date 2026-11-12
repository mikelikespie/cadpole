//! Starlark value types: Plane, Sketch, Body, Face, Edge, Ref.
//!
//! All of these are immutable snapshots. Bodies and sketches refer to state
//! in the [`Store`](crate::store::Store) by id; methods that "modify" return
//! a new value and record a feature.

use std::collections::BTreeMap;
use std::fmt;

use allocative::Allocative;
use cadpole_core::context::{Diagnostic, fmt_num, fmt_vec};
use cadpole_core::curve::{Curve, Segment3};
use cadpole_core::geom::{Plane, Transform, Vec3};
use cadpole_core::ids::{BodyId, FeatureId};
use cadpole_core::naming::{self, Fingerprint, Kind, Named, Resolution};
use cadpole_core::select::{self, Candidate, Filter};
use cadpole_core::sketch::{Loop, P2, Path, Sketch};
use cadpole_core::sketch2d::BoolOp;
use starlark::environment::{Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::values::float::UnpackFloat;
use starlark::values::list::UnpackList;
use starlark::values::{
    Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueLike,
};
use starlark::{starlark_module, starlark_simple_value};
use starlark_derive::starlark_value;

// Points and numbers come through `check`: NaN and infinity are refused with
// a diagnostic naming the argument, never built into a body.
use crate::check::{VecArg, num, opt_num, p2, vec3};
use crate::convert::{alloc_str_list, alloc_vec3, color};
use crate::selection::{self, KSelection};
use crate::store::{call_span, store};
use cadpole_core::fmath;

// ---------------------------------------------------------------- Plane

#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KPlane(#[allocative(skip)] pub Plane);
starlark_simple_value!(KPlane);

impl fmt::Display for KPlane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = &self.0;
        write!(
            f,
            "plane(origin=({}, {}, {}), normal=({}, {}, {}))",
            fmt_num(p.origin.x),
            fmt_num(p.origin.y),
            fmt_num(p.origin.z),
            fmt_num(p.normal.x),
            fmt_num(p.normal.y),
            fmt_num(p.normal.z)
        )
    }
}

#[starlark_value(type = "Plane")]
impl<'v> StarlarkValue<'v> for KPlane {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("plane_methods", plane_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "origin" => Some(alloc_vec3(heap, self.0.origin)),
            "normal" => Some(alloc_vec3(heap, self.0.normal)),
            "x_dir" => Some(alloc_vec3(heap, self.0.x_dir)),
            "y_dir" => Some(alloc_vec3(heap, self.0.y_dir())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["origin", "normal", "x_dir", "y_dir"].iter().map(|s| s.to_string()).collect()
    }
}

#[starlark_module]
fn plane_methods(builder: &mut MethodsBuilder) {
    /// A parallel plane moved `distance` along this plane's normal.
    fn offset(this: &KPlane, #[starlark(require = pos)] distance: UnpackFloat) -> anyhow::Result<KPlane> {
        Ok(KPlane(this.0.offset(num(&distance, "offset: distance")?)))
    }

    /// The point on this plane at sketch coordinates (x, y), in world space.
    fn to_world<'v>(
        this: &KPlane,
        #[starlark(require = pos)] x: UnpackFloat,
        #[starlark(require = pos)] y: UnpackFloat,
        heap: Heap<'v>,
    ) -> anyhow::Result<Value<'v>> {
        Ok(alloc_vec3(heap, this.0.to_world(num(&x, "to_world: x")?, num(&y, "to_world: y")?)))
    }
}

// ---------------------------------------------------------------- Asset

/// An external file the document carries, named by the user and pinned by
/// content hash. Scripts never see a path: `asset("plan.dxf")` resolves a
/// *name* in the document's asset table, and the importers take it from there.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KAsset {
    pub name: String,
    pub sha256: String,
    pub mime: String,
    pub kind: String,
    pub size: u64,
}
starlark_simple_value!(KAsset);

impl fmt::Display for KAsset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let short: String = self.sha256.chars().take(12).collect();
        write!(f, "asset({:?}, {}, {} bytes, sha256 {}…)", self.name, self.kind, self.size, short)
    }
}

#[starlark_value(type = "Asset")]
impl<'v> StarlarkValue<'v> for KAsset {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "name" => Some(heap.alloc(self.name.as_str())),
            "sha256" => Some(heap.alloc(self.sha256.as_str())),
            "mime" => Some(heap.alloc(self.mime.as_str())),
            "kind" => Some(heap.alloc(self.kind.as_str())),
            "size" => Some(heap.alloc(self.size as i32)),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["name", "sha256", "mime", "kind", "size"].iter().map(|s| s.to_string()).collect()
    }
}

// ---------------------------------------------------------------- Sketch

/// The loops and paths a chain of sketch values shares.
///
/// `s.polygon(a).polygon(b)...` returns a new value at every step; if each
/// value carried its own copy of every loop so far, a chain of 6,000
/// polygons would copy eighteen million loops and keep them all alive on the
/// Starlark heap. Here every value in a chain points at one log and sees the
/// first `n_loops` entries of it; appending to a value that sees the whole
/// log is a push, so a linear chain costs one push per step. Appending to a
/// value that sees only a prefix (a branch: two children of one parent)
/// copies that prefix into a log of its own, once, and continues from there.
/// A value never sees an entry it did not have when it was made, so the
/// sharing is invisible to a script.
#[derive(Debug)]
pub struct SketchLog {
    plane: Plane,
    loops: std::sync::Mutex<Vec<(Loop, String)>>,
    paths: std::sync::Mutex<Vec<(Path, String)>>,
}

#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KSketch {
    pub feature: String,
    #[allocative(skip)]
    log: std::sync::Arc<SketchLog>,
    n_loops: usize,
    n_paths: usize,
}
starlark_simple_value!(KSketch);

impl fmt::Display for KSketch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sketch({}, {} loops)", self.feature, self.n_loops)
    }
}

impl KSketch {
    /// A value holding all of `sketch`, in a log of its own.
    pub fn new(feature: String, sketch: Sketch) -> KSketch {
        let n_loops = sketch.loops.len();
        let n_paths = sketch.paths.len();
        let loops = sketch
            .loops
            .into_iter()
            .enumerate()
            .map(|(i, l)| (l, sketch.loop_names.get(i).cloned().unwrap_or_default()))
            .collect();
        let paths = sketch
            .paths
            .into_iter()
            .enumerate()
            .map(|(i, p)| (p, sketch.path_names.get(i).cloned().unwrap_or_default()))
            .collect();
        KSketch {
            feature,
            log: std::sync::Arc::new(SketchLog {
                plane: sketch.plane,
                loops: std::sync::Mutex::new(loops),
                paths: std::sync::Mutex::new(paths),
            }),
            n_loops,
            n_paths,
        }
    }

    /// The sketch this value stands for: its prefix of the shared log, as
    /// an owned `Sketch` for the kernel and the exporters.
    pub fn sketch(&self) -> Sketch {
        let loops = self.log.loops.lock().unwrap_or_else(|e| e.into_inner());
        let paths = self.log.paths.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = Sketch::new(self.log.plane);
        for (l, name) in loops.iter().take(self.n_loops) {
            s.loops.push(l.clone());
            s.loop_names.push(name.clone());
        }
        for (p, name) in paths.iter().take(self.n_paths) {
            s.paths.push(p.clone());
            s.path_names.push(name.clone());
        }
        s
    }

    pub fn plane(&self) -> Plane {
        self.log.plane
    }

    pub fn loop_count(&self) -> usize {
        self.n_loops
    }

    pub fn path_count(&self) -> usize {
        self.n_paths
    }

    pub fn loop_names(&self) -> Vec<String> {
        let loops = self.log.loops.lock().unwrap_or_else(|e| e.into_inner());
        loops.iter().take(self.n_loops).map(|(_, n)| n.clone()).collect()
    }

    pub fn path_names(&self) -> Vec<String> {
        let paths = self.log.paths.lock().unwrap_or_else(|e| e.into_inner());
        paths.iter().take(self.n_paths).map(|(_, n)| n.clone()).collect()
    }

    /// True when `other` continues the same log: the test that a chain
    /// shares rather than copies.
    #[cfg(test)]
    pub(crate) fn shares_log_with(&self, other: &KSketch) -> bool {
        std::sync::Arc::ptr_eq(&self.log, &other.log)
    }

    /// [`Self::appended`] without a feature to record on, for the tests.
    #[cfg(test)]
    pub(crate) fn appended_for_test(&self, l: Loop) -> KSketch {
        self.appended(l)
    }

    /// This value plus one loop: a push onto the shared log when this value
    /// sees all of it, a copy of the prefix otherwise.
    fn appended(&self, l: Loop) -> KSketch {
        {
            let mut loops = self.log.loops.lock().unwrap_or_else(|e| e.into_inner());
            if loops.len() == self.n_loops {
                loops.push((l, String::new()));
                return KSketch {
                    feature: self.feature.clone(),
                    log: self.log.clone(),
                    n_loops: self.n_loops + 1,
                    n_paths: self.n_paths,
                };
            }
        }
        let mut s = self.sketch();
        s = s.with_loop(l);
        KSketch::new(self.feature.clone(), s)
    }

    /// This value plus one open path, by the same rule as [`Self::appended`].
    fn appended_path(&self, p: Path) -> KSketch {
        {
            let mut paths = self.log.paths.lock().unwrap_or_else(|e| e.into_inner());
            if paths.len() == self.n_paths {
                paths.push((p, String::new()));
                return KSketch {
                    feature: self.feature.clone(),
                    log: self.log.clone(),
                    n_loops: self.n_loops,
                    n_paths: self.n_paths + 1,
                };
            }
        }
        let s = self.sketch().with_path(p, String::new());
        KSketch::new(self.feature.clone(), s)
    }

    /// Record an open path on the feature, as `with_loop` records a loop.
    fn with_path(&self, p: Path, eval: &Evaluator<'_, '_, '_>, desc: String) -> anyhow::Result<KSketch> {
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let fid = cadpole_core::FeatureId(self.feature.clone());
        let n = self.n_paths;
        ctx.set_feature_arg(&fid, &format!("path{n}"), desc);
        ctx.set_feature_arg(&fid, "paths", n + 1);
        Ok(self.appended_path(p))
    }

    fn with_loop(&self, l: Loop, eval: &Evaluator<'_, '_, '_>, desc: String) -> anyhow::Result<KSketch> {
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let fid = cadpole_core::FeatureId(self.feature.clone());
        let n = self.n_loops;
        ctx.set_feature_arg(&fid, &format!("loop{n}"), desc);
        ctx.set_feature_arg(&fid, "loops", n + 1);
        Ok(self.appended(l))
    }
}

#[starlark_value(type = "Sketch")]
impl<'v> StarlarkValue<'v> for KSketch {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("sketch_methods", sketch_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "id" => Some(heap.alloc(self.feature.as_str())),
            "plane" => Some(heap.alloc(KPlane(self.plane()))),
            "loops" => Some(heap.alloc(self.n_loops as i32)),
            // Open chains an import brought in. They bound nothing, so they
            // never extrude; they are here to be read and reported.
            "paths" => Some(heap.alloc(self.n_paths as i32)),
            "names" => Some(crate::convert::alloc_str_list(heap, &self.loop_names())),
            "path_names" => Some(crate::convert::alloc_str_list(heap, &self.path_names())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["id", "plane", "loops", "paths", "names", "path_names"].iter().map(|s| s.to_string()).collect()
    }
}

#[starlark_module]
fn sketch_methods(builder: &mut MethodsBuilder) {
    /// Add an axis-aligned rectangle of width `w` and height `h` centred at `center`.
    fn rect<'v>(
        this: &KSketch,
        #[starlark(require = pos)] w: UnpackFloat,
        #[starlark(require = pos)] h: UnpackFloat,
        #[starlark(require = named)] center: Option<VecArg>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        num(&w, "rect: w")?;
        num(&h, "rect: h")?;
        if w.0 <= 0.0 || h.0 <= 0.0 {
            anyhow::bail!("rect: w and h must be positive (got {}, {})", w.0, h.0);
        }
        let c = center.as_ref().map(|c| p2(c, "rect: center")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        this.with_loop(Loop::rect(c, w.0, h.0), eval, format!("rect({}, {})", fmt_num(w.0), fmt_num(h.0)))
    }

    /// Add a circle of radius `r` centred at `center`.
    fn circle<'v>(
        this: &KSketch,
        #[starlark(require = pos)] r: UnpackFloat,
        #[starlark(require = named)] center: Option<VecArg>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        num(&r, "circle: r")?;
        if r.0 <= 0.0 {
            anyhow::bail!("circle: r must be positive (got {})", r.0);
        }
        let c = center.as_ref().map(|c| p2(c, "circle: center")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        this.with_loop(Loop::circle(c, r.0), eval, format!("circle({})", fmt_num(r.0)))
    }

    /// Add a closed polygon through `points` (a list of (x, y)).
    fn polygon<'v>(
        this: &KSketch,
        #[starlark(require = pos)] points: UnpackList<VecArg>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        if points.items.len() < 3 {
            anyhow::bail!("polygon needs at least 3 points");
        }
        let pts: Vec<P2> = points.items.iter().map(|p| p2(p, "polygon: point")).collect::<anyhow::Result<_>>()?;
        let n = pts.len();
        this.with_loop(Loop::polygon(&pts), eval, format!("polygon({n} points)"))
    }

    /// Add a rectangle with corner radius `r` (a plain rect when r = 0).
    fn rounded_rect<'v>(
        this: &KSketch,
        #[starlark(require = pos)] w: UnpackFloat,
        #[starlark(require = pos)] h: UnpackFloat,
        #[starlark(require = pos)] r: UnpackFloat,
        #[starlark(require = named)] center: Option<VecArg>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        num(&w, "rounded_rect: w")?;
        num(&h, "rounded_rect: h")?;
        num(&r, "rounded_rect: r")?;
        if w.0 <= 0.0 || h.0 <= 0.0 || r.0 < 0.0 {
            anyhow::bail!("rounded_rect: w and h must be positive and r >= 0");
        }
        let c = center.as_ref().map(|c| p2(c, "rounded_rect: center")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        this.with_loop(Loop::rounded_rect(c, w.0, h.0, r.0), eval, format!("rounded_rect({}, {}, {})", fmt_num(w.0), fmt_num(h.0), fmt_num(r.0)))
    }

    /// Add a stadium slot of overall `length` along x and `width` along y.
    fn slot<'v>(
        this: &KSketch,
        #[starlark(require = pos)] length: UnpackFloat,
        #[starlark(require = pos)] width: UnpackFloat,
        #[starlark(require = named)] center: Option<VecArg>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        num(&length, "slot: length")?;
        num(&width, "slot: width")?;
        if length.0 <= width.0 || width.0 <= 0.0 {
            anyhow::bail!("slot: need length > width > 0 (got {}, {})", length.0, width.0);
        }
        let c = center.as_ref().map(|c| p2(c, "slot: center")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        this.with_loop(Loop::slot(c, length.0, width.0), eval, format!("slot({}, {})", fmt_num(length.0), fmt_num(width.0)))
    }

    /// Add a loop built with `path(...)`; the path is closed with a straight
    /// segment back to its start if needed. `close=False` adds it as an
    /// open path instead: a stroke that
    /// bounds nothing, never extruded, written by `export_svg` /
    /// `export_pdf` / `export_dxf` as the open line it is, arcs kept.
    fn add<'v>(
        this: &KSketch,
        #[starlark(require = pos)] path: &KPath,
        #[starlark(require = named, default = true)] close: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let n = path.0.segments.len();
        if !close {
            if n == 0 {
                anyhow::bail!("add: an open path needs at least one segment");
            }
            return this.with_path(path.0.clone(), eval, format!("open path({n} segments)"));
        }
        let l = path.0.close()?;
        let n = l.segments.len();
        this.with_loop(l, eval, format!("path({n} segments)"))
    }

    /// Add an open polyline through `points` (a list of (x, y)), which
    /// bounds nothing and is never extruded: a contour, a grade
    /// line, a leader, a property line. Exported as the open stroke it is.
    /// `closed=True` adds it as a closed polygon, the same as `polygon`.
    fn polyline<'v>(
        this: &KSketch,
        #[starlark(require = pos)] points: UnpackList<VecArg>,
        #[starlark(require = named, default = false)] closed: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let pts: Vec<P2> = points.items.iter().map(|p| p2(p, "polyline: point")).collect::<anyhow::Result<_>>()?;
        let n = pts.len();
        if closed {
            if n < 3 {
                anyhow::bail!("polyline: a closed polyline needs at least 3 points");
            }
            return this.with_loop(Loop::polygon(&pts), eval, format!("polygon({n} points)"));
        }
        if n < 2 {
            anyhow::bail!("polyline needs at least 2 points");
        }
        let mut p = Path::new(pts[0]);
        for q in &pts[1..] {
            p = p.line_to(*q);
        }
        this.with_path(p, eval, format!("polyline({n} points)"))
    }

    /// Add a closed smooth spline through `points` (Catmull-Rom; `tension`
    /// 0 = loose, 1 = tight). Good for organic outlines.
    fn spline<'v>(
        this: &KSketch,
        #[starlark(require = pos)] points: UnpackList<VecArg>,
        #[starlark(require = named, default = UnpackFloat(0.5))] tension: UnpackFloat,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let pts: Vec<P2> = points.items.iter().map(|c| p2(c, "spline: point")).collect::<anyhow::Result<_>>()?;
        let n = pts.len();
        // `clamp` passes NaN through, so the check comes first.
        let tension = num(&tension, "spline: tension")?.clamp(0.0, 1.0);
        this.with_loop(Loop::spline(&pts, tension)?, eval, format!("spline({n} points)"))
    }

    /// Every loop of the sketch as a closed `Curve` in world space, as drawn,
    /// in loop order — the bridge from a sketch to `loft`'s sections in
    /// space: `loft([a.curves()[0], b.curves()[0]])` lofts two sketches on
    /// planes that need not be parallel. Each curve carries the sketch's
    /// feature id.
    fn curves<'v>(this: &KSketch, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        let sketch = this.sketch();
        let curves: Vec<KCurve> = sketch
            .loops
            .iter()
            .map(|l| KCurve { feature: this.feature.clone(), curve: Curve::from_loop(l, &sketch.plane) })
            .collect();
        Ok(heap.alloc(curves))
    }

    /// Add a regular polygon with `n` sides inscribed in a circle of radius `r`.
    fn ngon<'v>(
        this: &KSketch,
        #[starlark(require = pos)] n: UnpackFloat,
        #[starlark(require = pos)] r: UnpackFloat,
        #[starlark(require = named)] center: Option<VecArg>,
        #[starlark(require = named, default = UnpackFloat(0.0))] rotate: UnpackFloat,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        // Capped before the point list is sized by it (see `check`).
        let n = crate::check::count(&n, "ngon: n", 3, crate::limits::Limits::MAX_NGON_SIDES)?;
        let r = num(&r, "ngon: r")?;
        let rotate = num(&rotate, "ngon: rotate")?;
        let c = center.as_ref().map(|c| p2(c, "ngon: center")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        let pts: Vec<P2> = (0..n)
            .map(|i| {
                let t = rotate.to_radians() + std::f64::consts::TAU * i as f64 / n as f64;
                P2::new(c.x + r * fmath::cos(t), c.y + r * fmath::sin(t))
            })
            .collect();
        this.with_loop(Loop::polygon(&pts), eval, format!("ngon({n}, {})", fmt_num(r)))
    }

    // ---- the 2D toolkit; the globals in `crate::sketch2d` are the same features.

    /// Offset every loop by `d`: positive grows the material (outer loops
    /// outward, holes inward), negative shrinks it. Lines stay lines and
    /// arcs stay concentric arcs, so an offset circle still extrudes to a
    /// cylinder; a spline is re-fitted to the document tolerance. Where the
    /// offset opens a corner, `join` fills it: `join.round` (the true
    /// offset, default), `join.miter` (sharp, bevelled past four offset
    /// distances) or `join.bevel`. Self-intersections are removed: a thin
    /// section offset inward pinches off cleanly, a loop that collapses is
    /// dropped with a note naming it, and a loop the offset splits comes
    /// back as two (`#0`, `#1`). Loops are named `<loop>/offset`.
    /// Feature: `offset2d`, args `distance`, `join`.
    fn offset<'v>(
        this: &KSketch,
        #[starlark(require = pos)] d: UnpackFloat,
        #[starlark(require = named)] join: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let d = num(&d, "offset: d")?;
        crate::sketch2d::offset_op((this.feature.clone(), this.sketch()), d, join, name, eval)
    }

    /// Unite this sketch with `other`, on the same plane: the same feature
    /// as the global `union2d(a, b)`.
    fn union<'v>(
        this: &KSketch,
        #[starlark(require = pos)] other: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let b = crate::sketch2d::source(other, "union", eval)?;
        crate::sketch2d::boolean_op((this.feature.clone(), this.sketch()), b, BoolOp::Union, name, eval)
    }

    /// Cut `other` out of this sketch: the same feature as `subtract2d(a, b)`.
    fn subtract<'v>(
        this: &KSketch,
        #[starlark(require = pos)] other: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let b = crate::sketch2d::source(other, "subtract", eval)?;
        crate::sketch2d::boolean_op((this.feature.clone(), this.sketch()), b, BoolOp::Subtract, name, eval)
    }

    /// Keep what this sketch and `other` both cover: the same feature as
    /// `intersect2d(a, b)`. No overlap is an empty sketch with a note.
    fn intersect<'v>(
        this: &KSketch,
        #[starlark(require = pos)] other: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let b = crate::sketch2d::source(other, "intersect", eval)?;
        crate::sketch2d::boolean_op((this.feature.clone(), this.sketch()), b, BoolOp::Intersect, name, eval)
    }

    /// Round corners with arcs of radius `r` tangent to both segments; the
    /// global `fillet2d(sketch, r, corners=)` says how `corners=` selects.
    fn fillet2d<'v>(
        this: &KSketch,
        #[starlark(require = pos)] r: UnpackFloat,
        #[starlark(require = named)] corners: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let r = num(&r, "fillet2d: r")?;
        crate::sketch2d::corner_op((this.feature.clone(), this.sketch()), r, corners, false, name, eval)
    }

    /// Cut corners back by `d` along both segments; the global
    /// `chamfer2d(sketch, d, corners=)` says how `corners=` selects.
    fn chamfer2d<'v>(
        this: &KSketch,
        #[starlark(require = pos)] d: UnpackFloat,
        #[starlark(require = named)] corners: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let d = num(&d, "chamfer2d: d")?;
        crate::sketch2d::corner_op((this.feature.clone(), this.sketch()), d, corners, true, name, eval)
    }

    /// Mirror every loop across the line through `origin` along `axis` (the
    /// sketch's y axis by default); the global `mirror2d` has the details of
    /// `keep=` and `merge=`.
    fn mirror2d<'v>(
        this: &KSketch,
        #[starlark(require = named)] axis: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named, default = true)] keep: bool,
        #[starlark(require = named, default = true)] merge: bool,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        crate::sketch2d::mirror_op((this.feature.clone(), this.sketch()), axis, origin, keep, merge, name, eval)
    }

    /// `count` copies of every loop `spacing` apart; the global
    /// `pattern2d_linear` has the details of `merge=`.
    fn pattern2d_linear<'v>(
        this: &KSketch,
        #[starlark(require = pos)] count: UnpackFloat,
        #[starlark(require = pos)] spacing: VecArg,
        #[starlark(require = named, default = false)] merge: bool,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let count = crate::check::count(&count, "pattern2d_linear: count", 1, crate::limits::Limits::MAX_PATTERN_COUNT)?;
        let spacing = p2(&spacing, "pattern2d_linear: spacing")?;
        crate::sketch2d::pattern_linear_op((this.feature.clone(), this.sketch()), count, spacing, merge, name, eval)
    }

    /// `count` copies of every loop rotated about `center` over `angle`
    /// degrees; the global `pattern2d_circular` has the details.
    fn pattern2d_circular<'v>(
        this: &KSketch,
        #[starlark(require = pos)] count: UnpackFloat,
        #[starlark(require = named)] center: Option<VecArg>,
        #[starlark(require = named, default = UnpackFloat(360.0))] angle: UnpackFloat,
        #[starlark(require = named, default = false)] merge: bool,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let count = crate::check::count(&count, "pattern2d_circular: count", 1, crate::limits::Limits::MAX_PATTERN_COUNT)?;
        let center = center.as_ref().map(|c| p2(c, "pattern2d_circular: center")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        let angle = num(&angle, "pattern2d_circular: angle")?;
        crate::sketch2d::pattern_circular_op((this.feature.clone(), this.sketch()), count, center, angle, merge, name, eval)
    }

    /// The names of the sketch's corners, `<loop>/corner[k]`, in loop order:
    /// what `fillet2d` and `chamfer2d` select from. A tangent-continuous
    /// joint (a rounded rectangle's, a circle's) is not a corner.
    fn corners<'v>(this: &KSketch, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        Ok(alloc_str_list(heap, &crate::sketch2d::corner_names(&(this.feature.clone(), this.sketch()))))
    }

    /// Every loop as a list of `(x, y)` points in the sketch's own
    /// coordinates, in loop order: a polygon's corners as drawn, an arc or
    /// a spline sampled to `tolerance` (the document's by default). For
    /// reading geometry back — a section's outline, a footprint's corners —
    /// where a `Curve` would be the wrong shape.
    fn points<'v>(
        this: &KSketch,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let tol = match tolerance {
            Some(t) => {
                let t = num(&t, "points: tolerance")?;
                if t <= 0.0 {
                    anyhow::bail!("points: tolerance must be positive, got {t}");
                }
                t
            }
            None => store(eval)?.ctx.borrow().tolerance,
        };
        let loops: Vec<Vec<(f64, f64)>> = this
            .sketch()
            .loops
            .iter()
            .map(|l| l.sample_tolerance(tol).into_iter().map(|p| (p.x, p.y)).collect())
            .collect();
        Ok(heap.alloc(loops))
    }

    /// The area the sketch's loops enclose, in mm²: the material of every
    /// region less its holes, by nesting. Exact for lines and arcs (`∮ (x dy
    /// − y dx) / 2` piece by piece, `exact_area`); a spline is
    /// sampled to the document tolerance. An empty sketch is 0. Loops that
    /// do not nest — one crossing another — are refused by name.
    fn area<'v>(this: &KSketch, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<f64> {
        let sketch = this.sketch();
        if sketch.loops.is_empty() {
            return Ok(0.0);
        }
        if let Some((i, j)) = sketch.crossing_pair() {
            let name = |k: usize| match sketch.loop_name(k) {
                "" => format!("loop {k}"),
                n => n.to_owned(),
            };
            anyhow::bail!(
                "area: {}: {} and {} cross, so what they enclose together is not the sum of the two; unite them with union2d first",
                this.feature,
                name(i),
                name(j)
            );
        }
        let tolerance = store(eval)?.ctx.borrow().tolerance;
        let regions = sketch.regions().map_err(|e| anyhow::anyhow!("area: {}: {e}", this.feature))?;
        Ok(regions
            .iter()
            .map(|r| {
                cadpole_core::sketch2d::exact_area(&r.outer, tolerance).abs()
                    - r.holes.iter().map(|h| cadpole_core::sketch2d::exact_area(h, tolerance).abs()).sum::<f64>()
            })
            .sum())
    }
}

// ---------------------------------------------------------------- Path

/// An open line/arc chain. Immutable: every method returns a new path.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KPath(#[allocative(skip)] pub Path);
starlark_simple_value!(KPath);

impl fmt::Display for KPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.0.current();
        write!(f, "path({} segments, at ({}, {}))", self.0.segments.len(), fmt_num(c.x), fmt_num(c.y))
    }
}

#[starlark_value(type = "Path")]
impl<'v> StarlarkValue<'v> for KPath {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("path_methods", path_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "start" => Some(heap.alloc((self.0.start.x, self.0.start.y))),
            "current" => {
                let c = self.0.current();
                Some(heap.alloc((c.x, c.y)))
            }
            "segments" => Some(heap.alloc(self.0.segments.len() as i32)),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["start", "current", "segments"].iter().map(|s| s.to_string()).collect()
    }
}

#[starlark_module]
fn path_methods(builder: &mut MethodsBuilder) {
    /// Straight segment to `to`.
    fn line(this: &KPath, #[starlark(require = pos)] to: VecArg) -> anyhow::Result<KPath> {
        Ok(KPath(this.0.line_to(p2(&to, "line: to")?)))
    }

    /// Straight segment by a relative offset `(dx, dy)`.
    fn line_by(this: &KPath, #[starlark(require = pos)] delta: VecArg) -> anyhow::Result<KPath> {
        let d = p2(&delta, "line_by: delta")?;
        let c = this.0.current();
        Ok(KPath(this.0.line_to(P2::new(c.x + d.x, c.y + d.y))))
    }

    /// Horizontal segment to x = `x`.
    fn h(this: &KPath, #[starlark(require = pos)] x: UnpackFloat) -> anyhow::Result<KPath> {
        let c = this.0.current();
        Ok(KPath(this.0.line_to(P2::new(num(&x, "h: x")?, c.y))))
    }

    /// Vertical segment to y = `y`.
    fn v(this: &KPath, #[starlark(require = pos)] y: UnpackFloat) -> anyhow::Result<KPath> {
        let c = this.0.current();
        Ok(KPath(this.0.line_to(P2::new(c.x, num(&y, "v: y")?))))
    }

    /// Circular arc to `to` passing through `via`.
    fn arc(this: &KPath, #[starlark(require = pos)] to: VecArg, #[starlark(require = named)] via: VecArg) -> anyhow::Result<KPath> {
        Ok(KPath(this.0.arc_to(p2(&to, "arc: to")?, p2(&via, "arc: via")?)))
    }

    /// Circular arc to `to` that leaves tangent to the previous segment.
    fn tangent_arc(this: &KPath, #[starlark(require = pos)] to: VecArg) -> anyhow::Result<KPath> {
        Ok(KPath(this.0.tangent_arc_to(p2(&to, "tangent_arc: to")?)?))
    }

    /// Bézier curve to `to` with one (quadratic) or two (cubic) control points.
    fn bezier(
        this: &KPath,
        #[starlark(require = pos)] to: VecArg,
        #[starlark(require = named)] controls: UnpackList<VecArg>,
    ) -> anyhow::Result<KPath> {
        let ctrl: Vec<P2> = controls.items.iter().map(|c| p2(c, "bezier: control")).collect::<anyhow::Result<_>>()?;
        Ok(KPath(this.0.bezier_to(p2(&to, "bezier: to")?, ctrl)?))
    }

    /// Smooth spline from the current point through `points` (Catmull-Rom;
    /// `tension` 0 = loose, 1 = tight).
    fn spline(
        this: &KPath,
        #[starlark(require = pos)] points: UnpackList<VecArg>,
        #[starlark(require = named, default = UnpackFloat(0.5))] tension: UnpackFloat,
    ) -> anyhow::Result<KPath> {
        let pts: Vec<P2> = points.items.iter().map(|c| p2(c, "spline: point")).collect::<anyhow::Result<_>>()?;
        Ok(KPath(this.0.spline_through(&pts, tension.0.clamp(0.0, 1.0))?))
    }

    /// The path lifted onto `plane` as a `Curve` in world space: every
    /// `(u, v)` becomes `plane.to_world(u, v)`, the same mapping the kernel
    /// applies to a sketch, so the curve is the sketch's geometry exactly.
    /// Open, as the path is; `.close()` it to use it as a loft section, or
    /// keep drawing in space from where the path ended. Records a feature
    /// of kind `curve`.
    fn on<'v>(
        this: &KPath,
        #[starlark(require = pos)] plane: &KPlane,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KCurve> {
        let curve = Curve::from_path(&this.0, &plane.0);
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut args = BTreeMap::new();
        args.insert("from".into(), format!("path({} segments)", this.0.segments.len()));
        args.insert("plane".into(), format!("{}", plane));
        args.insert("start".into(), fmt_vec(curve.start));
        args.insert("segments".into(), curve.segments.len().to_string());
        args.insert("kinds".into(), curve_kinds(&curve));
        args.insert("closed".into(), "false".into());
        let fid = ctx.new_feature("curve", name.map(str::to_owned), vec![], args, span)?;
        Ok(KCurve { feature: fid.0, curve })
    }
}

// ---------------------------------------------------------------- Curve

/// A chain of segments in world space: what `curve(start)` builds, what
/// `Path.on(plane)` lifts a sketch path into, and what `Sketch.curves()`
/// returns one of per loop. Immutable: every method returns a new
/// value with the same feature id, and each segment added is recorded on the
/// feature, as a sketch records its loops.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KCurve {
    /// The feature the curve belongs to: its own `curve` feature, or the
    /// sketch it was lifted out of by `Sketch.curves()`.
    pub feature: String,
    #[allocative(skip)]
    pub curve: Curve,
}
starlark_simple_value!(KCurve);

impl fmt::Display for KCurve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.curve.current();
        write!(
            f,
            "curve({}, {} segments, {}, at ({}, {}, {}))",
            self.feature,
            self.curve.segments.len(),
            if self.curve.closed { "closed" } else { "open" },
            fmt_num(c.x),
            fmt_num(c.y),
            fmt_num(c.z)
        )
    }
}

/// What a segment is, for the feature tree.
fn segment3_desc(seg: &Segment3) -> String {
    match seg {
        Segment3::Line { .. } => "line".into(),
        Segment3::Arc { .. } => "arc".into(),
        Segment3::Bezier { ctrl, .. } => format!("bezier({})", ctrl.len()),
        Segment3::Helix { angle, .. } => format!("helix({:.3} turns)", angle.abs() / std::f64::consts::TAU),
    }
}

/// The segments of a curve for the feature tree, run-length encoded:
/// `line, bezier(2) x47, arc, bezier(2) x47, line` — a blade section in one
/// line rather than a hundred.
pub(crate) fn curve_kinds(curve: &Curve) -> String {
    let mut runs: Vec<(String, usize)> = Vec::new();
    for seg in &curve.segments {
        let d = segment3_desc(seg);
        match runs.last_mut() {
            Some((k, n)) if *k == d => *n += 1,
            _ => runs.push((d, 1)),
        }
    }
    runs.iter()
        .map(|(k, n)| if *n == 1 { k.clone() } else { format!("{k} x{n}") })
        .collect::<Vec<_>>()
        .join(", ")
}

impl KCurve {
    /// The curve after a builder method, with what it now holds recorded on
    /// the feature: the segment count, the run-length list of kinds, and
    /// whether it is closed.
    fn with(&self, curve: Curve, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<KCurve> {
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let fid = FeatureId(self.feature.clone());
        ctx.set_feature_arg(&fid, "segments", curve.segments.len());
        ctx.set_feature_arg(&fid, "kinds", curve_kinds(&curve));
        ctx.set_feature_arg(&fid, "closed", curve.closed);
        Ok(KCurve { feature: self.feature.clone(), curve })
    }
}

#[starlark_value(type = "Curve")]
impl<'v> StarlarkValue<'v> for KCurve {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("curve_methods", curve_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "id" => Some(heap.alloc(self.feature.as_str())),
            "start" => Some(alloc_vec3(heap, self.curve.start)),
            "end" => Some(alloc_vec3(heap, self.curve.end())),
            "segments" => Some(heap.alloc(self.curve.segments.len() as i32)),
            "closed" => Some(heap.alloc(self.curve.closed)),
            "length" => Some(heap.alloc(self.curve.length())),
            // The best-fit plane when the curve lies in one (within the
            // kernel's tolerance, so it is exactly the test `cap="plane"`
            // applies); `None` for a curve that is genuinely 3D.
            "plane" => Some(match self.curve.plane() {
                Some(p) => heap.alloc(KPlane(p)),
                None => Value::new_none(),
            }),
            // The planes perpendicular to the curve at its two ends, under
            // the rotation-minimising frame: where a profile for
            // `sweep_along` is drawn, and where it arrives. `None` for a
            // curve with no segments yet.
            "start_plane" | "end_plane" => Some(match cadpole_core::sweep::end_planes(&self.curve, None) {
                Ok((s, e)) => heap.alloc(KPlane(if attribute == "start_plane" { s } else { e })),
                Err(_) => Value::new_none(),
            }),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["id", "start", "end", "segments", "closed", "length", "plane", "start_plane", "end_plane"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

#[starlark_module]
fn curve_methods(builder: &mut MethodsBuilder) {
    /// Straight segment to `to`, a point `(x, y, z)`.
    fn line<'v>(
        this: &KCurve,
        #[starlark(require = pos)] to: VecArg,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KCurve> {
        this.with(this.curve.line_to(vec3(&to, "line: to")?)?, eval)
    }

    /// Straight segment by a relative offset `(dx, dy, dz)`.
    fn line_by<'v>(
        this: &KCurve,
        #[starlark(require = pos)] delta: VecArg,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KCurve> {
        let d = vec3(&delta, "line_by: delta")?;
        this.with(this.curve.line_to(this.curve.current() + d)?, eval)
    }

    /// Circular arc to `to` passing through `via`: the circle through the
    /// three points, which need not lie in any sketch plane.
    fn arc<'v>(
        this: &KCurve,
        #[starlark(require = pos)] to: VecArg,
        #[starlark(require = named)] via: VecArg,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KCurve> {
        this.with(this.curve.arc_to(vec3(&to, "arc: to")?, vec3(&via, "arc: via")?)?, eval)
    }

    /// Bézier curve to `to` with one (quadratic) or two (cubic) control
    /// points, each `(x, y, z)`. An exact NURBS edge in the kernel.
    fn bezier<'v>(
        this: &KCurve,
        #[starlark(require = pos)] to: VecArg,
        #[starlark(require = named)] controls: UnpackList<VecArg>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KCurve> {
        let ctrl: Vec<Vec3> = controls.items.iter().map(|c| vec3(c, "bezier: control")).collect::<anyhow::Result<_>>()?;
        this.with(this.curve.bezier_to(vec3(&to, "bezier: to")?, ctrl)?, eval)
    }

    /// Smooth spline from the current point through `points` (Catmull-Rom,
    /// one cubic Bézier per span, `tension` 0 = loose, 1 = tight), exactly
    /// as `Path.spline` draws one in a sketch — so a section built from the
    /// same station list at the hub and at the tip pairs station for station.
    fn spline<'v>(
        this: &KCurve,
        #[starlark(require = pos)] points: UnpackList<VecArg>,
        #[starlark(require = named, default = UnpackFloat(0.5))] tension: UnpackFloat,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KCurve> {
        let pts: Vec<Vec3> = points.items.iter().map(|c| vec3(c, "spline: point")).collect::<anyhow::Result<_>>()?;
        // `clamp` passes NaN through, so the check comes first.
        let tension = num(&tension, "spline: tension")?.clamp(0.0, 1.0);
        this.with(this.curve.spline_through(&pts, tension)?, eval)
    }

    /// The closed curve: a straight segment back to the start is added only
    /// if the end is not already there. A closed curve is what `loft` takes
    /// as a section; nothing can be added to it afterwards.
    fn close<'v>(this: &KCurve, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<KCurve> {
        this.with(this.curve.close()?, eval)
    }

    /// The same curve run the other way. A closed curve keeps its seam and
    /// runs round the other way — the fix when a loft warns that one section
    /// is drawn the opposite way round from its neighbour.
    fn reverse(this: &KCurve) -> anyhow::Result<KCurve> {
        Ok(KCurve { feature: this.feature.clone(), curve: this.curve.reversed() })
    }
}

// ---------------------------------------------------------------- Body

#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KBody {
    pub id: u32,
    pub feature: String,
    pub name: String,
    /// `"brep"` or `"mesh"` — which representation the body is in.
    pub kind: String,
}
starlark_simple_value!(KBody);

impl fmt::Display for KBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "body({}#{})", self.feature, self.id)
    }
}

impl KBody {
    pub fn body_id(&self) -> BodyId {
        BodyId(self.id)
    }

    pub fn from_ctx(ctx: &cadpole_core::Context, id: BodyId) -> anyhow::Result<KBody> {
        let b = ctx.body(id)?;
        Ok(KBody {
            id: id.0,
            feature: b.feature.0.clone(),
            name: b.name.clone(),
            kind: b.kind().to_owned(),
        })
    }

    pub fn faces(&self, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<Vec<KFace>> {
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let (_, topo) = ctx.body_mesh(self.body_id())?;
        Ok(topo
            .faces
            .iter()
            .map(|f| KFace {
                body: self.id,
                name: f.name.clone(),
                aliases: f.aliases.clone(),
                kind: f.kind().to_owned(),
                surface_kind: f.surface_kind.to_owned(),
                normal: f.normal,
                centroid: f.centroid,
                area: f.area,
                edges: f.edges.clone(),
            })
            .collect())
    }

    pub fn edges(&self, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<Vec<KEdge>> {
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let (_, topo) = ctx.body_mesh(self.body_id())?;
        Ok(topo.edges.iter().map(|e| KEdge::from_topology(self.id, e)).collect())
    }
}

#[starlark_value(type = "Body")]
impl<'v> StarlarkValue<'v> for KBody {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("body_methods", body_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "id" => Some(heap.alloc(format!("{}#{}", self.feature, self.id))),
            "feature" => Some(heap.alloc(self.feature.as_str())),
            "name" => Some(heap.alloc(self.name.as_str())),
            // "brep" (exact surfaces, STEP-able) or "mesh" (triangles).
            "kind" => Some(heap.alloc(self.kind.as_str())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["id", "feature", "name", "kind"].iter().map(|s| s.to_string()).collect()
    }
}

fn transform_body(
    this: &KBody,
    t: Transform,
    kind: &str,
    args: BTreeMap<String, String>,
    name: Option<&str>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KBody> {
    let span = call_span(eval);
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let id = ctx.transform(this.body_id(), &t, kind, args, name.map(str::to_owned), span)?;
    KBody::from_ctx(&ctx, id)
}

/// The closed face vocabulary: the five analytic classes
/// `cadpole_core::surface_class` recognises, then truck's own surface types,
/// which still select a face whatever it was recognised as (`kind="nurbs"`
/// includes a cylinder that truck holds as a NURBS patch). The `kind`
/// namespace (`crate::vocab`) is the union of this list and
/// [`EDGE_KINDS`], so a spelling added here is a member there.
pub(crate) const FACE_KINDS: &[&str] = &["plane", "cylinder", "cone", "sphere", "torus", "bspline", "nurbs", "revolved"];
/// Curve classes, plus the two this crate measures for itself by fitting.
pub(crate) const EDGE_KINDS: &[&str] = &["line", "bspline", "nurbs", "intersection", "circle", "arc"];

/// Builds the filter list for `faces()` and `edges()` and turns the result
/// into a `Selection`, so the two methods differ only in the keywords they
/// accept.
struct Query {
    which: select::Which,
    filters: Vec<Filter>,
    described: Vec<String>,
    tol: select::Tol,
    has_length_filter: bool,
    tolerance_given: bool,
    within_given: bool,
    directional: bool,
}

impl Query {
    fn new(
        which: select::Which,
        within: &Option<UnpackFloat>,
        tolerance: &Option<UnpackFloat>,
        directional: bool,
    ) -> Query {
        let mut tol = select::Tol::default();
        if let Some(w) = within {
            tol.within_deg = w.0;
        }
        if let Some(t) = tolerance {
            tol.mm = t.0;
        }
        Query {
            which,
            filters: Vec::new(),
            described: Vec::new(),
            tol,
            has_length_filter: false,
            tolerance_given: tolerance.is_some(),
            within_given: within.is_some(),
            directional,
        }
    }

    fn push(&mut self, f: Filter, desc: String) {
        if matches!(f, Filter::OnPlane(_) | Filter::Above(_) | Filter::Below(_) | Filter::Inside { .. }) {
            self.has_length_filter = true;
        }
        self.filters.push(f);
        self.described.push(desc);
    }

    /// The keywords `faces()` and `edges()` share.
    #[allow(clippy::too_many_arguments)]
    fn common<'v>(
        &mut self,
        kind: Option<&str>,
        name: Option<&str>,
        on_plane: Option<&KPlane>,
        above: Option<&KPlane>,
        below: Option<&KPlane>,
        inside: Option<Value<'v>>,
        heap: Heap<'v>,
    ) -> anyhow::Result<()> {
        if let Some(k) = kind {
            let k = if k == "planar" { "plane" } else { k };
            let valid = if self.which == select::Which::Faces { FACE_KINDS } else { EDGE_KINDS };
            if !valid.contains(&k) {
                // The same shape as a `kind.<typo>` miss (`crate::vocab`),
                // so the two ways of misspelling a class read alike.
                anyhow::bail!(
                    "{}",
                    crate::vocab::not_one_of(format!("kind={k:?} is not a {} class", self.which.singular()), valid)
                );
            }
            self.push(Filter::Class(k.to_owned()), format!("kind={k:?}"));
        }
        if let Some(n) = name {
            self.push(Filter::Name(n.to_owned()), format!("name={n:?}"));
        }
        if let Some(p) = on_plane {
            self.push(Filter::OnPlane(p.0), format!("on_plane={p}"));
        }
        if let Some(p) = above {
            self.push(Filter::Above(p.0), format!("above={p}"));
        }
        if let Some(p) = below {
            self.push(Filter::Below(p.0), format!("below={p}"));
        }
        if let Some(b) = inside {
            let (lo, hi) = box_arg(b, heap)?;
            self.push(Filter::Inside { lo, hi }, format!("inside=({}, {})", fmt_vec(lo), fmt_vec(hi)));
        }
        Ok(())
    }

    /// Run the filters and wrap the survivors, warning when a non-empty body
    /// matched nothing.
    #[allow(clippy::too_many_arguments)]
    fn finish<T: Clone + selection::Selectable>(
        mut self,
        body: &KBody,
        what: &str,
        items: &[T],
        cands: &[Candidate],
        rank: &[u32],
        nearest: Option<VecArg>,
        allow_empty: bool,
        eval: &Evaluator<'_, '_, '_>,
    ) -> anyhow::Result<KSelection> {
        // `within=` is degrees, `tolerance=` is millimetres. A script
        // that passes `tolerance=` for an angle is told, never reinterpreted.
        if self.tolerance_given && !self.within_given && self.directional && !self.has_length_filter {
            anyhow::bail!(
                "{what}s(tolerance=) is a length in millimetres; the angular tolerance \
                 for normal=/direction= is within=. Write within={} for the old meaning",
                fmt_num(self.tol.mm)
            );
        }
        let mut out = select::apply(cands, &self.filters, &self.tol);
        if let Some(p) = nearest {
            let point = vec3(&p, "nearest")?;
            self.described.push(format!("nearest={}", fmt_vec(point)));
            out.picked = select::nearest(cands, &out.picked, point).into_iter().collect();
        }
        let query = format!("{what}s({})", self.described.join(", "));
        if out.picked.is_empty() && !allow_empty {
            let why = match &out.reason {
                Some(r) => r.clone(),
                None => format!("the body has no {what}s"),
            };
            let d = Diagnostic::warning(format!(
                "{}.{query} matched nothing: {why}. Pass allow_empty=True if that is the answer \
                 you wanted",
                body.feature
            ))
            .with_span(call_span(eval))
            .with_feature(FeatureId(body.feature.clone()));
            let st = store(eval)?;
            let mut ctx = st.ctx.borrow_mut();
            ctx.diagnostics.push(d.clone());
            if let Ok(f) = ctx.feature_mut(&FeatureId(body.feature.clone())) {
                f.diagnostics.push(d);
            }
        }
        Ok(selection::from_picked(body, what, items, rank, &out.picked, &query))
    }
}

/// `((x, y, z), (x, y, z))` -- the shape `bbox()` returns, so a box filter can
/// be written `a.faces(inside=b.bbox())`.
fn box_arg<'v>(v: Value<'v>, heap: Heap<'v>) -> anyhow::Result<(Vec3, Vec3)> {
    let items: Vec<Value<'v>> = v.iterate(heap).map_err(|e| e.into_anyhow())?.collect();
    if items.len() != 2 {
        anyhow::bail!("inside= must be two corner points ((x, y, z), (x, y, z))");
    }
    let corner = |k: usize| -> anyhow::Result<Vec3> {
        let arg: VecArg = starlark::values::UnpackValue::unpack_value(items[k])
            .map_err(|e| e.into_anyhow())?
            .ok_or_else(|| anyhow::anyhow!("inside=: corner {k} must be (x, y, z)"))?;
        vec3(&arg, "inside")
    };
    let (a, b) = (corner(0)?, corner(1)?);
    Ok((a.min(b), a.max(b)))
}

#[starlark_module]
fn body_methods(builder: &mut MethodsBuilder) {
    /// Faces of this body that match every filter given, as a `Selection`
    /// (docs/design.md). With no filters, every face.
    ///
    /// Geometry: `normal=(x,y,z)` (the outward normal, within `within`
    /// degrees — signed, so `(0,0,1)` is the top and not the bottom),
    /// `on_plane=`, `above=`, `below=` (a whole face, on or to one side of a
    /// plane), `inside=(lo, hi)` (a whole face inside a box — `bbox()`'s
    /// shape, so `a.faces(inside=b.bbox())` reads well), `nearest=(x,y,z)`
    /// (the single closest face).
    ///
    /// Identity: `kind=` is the surface class ("plane"/"planar", "bspline",
    /// "nurbs", "revolved"); `name=` is a `*`/`?` glob over the generated name,
    /// its aliases or its local part, so `name="side[*]"` needs no feature id.
    ///
    /// Measurement: `min_area=`, `max_area=` in mm².
    ///
    /// Tolerances: `within=` is **degrees** and `tolerance=` is
    /// **millimetres**, here and everywhere.
    ///
    /// An empty result is not an error — `len(sel) == 0` is a fair thing to
    /// assert — but it does raise a warning naming the filter that rejected
    /// everything and the closest miss. Pass `allow_empty=True` when nothing
    /// is the answer you wanted.
    #[allow(clippy::too_many_arguments)]
    fn faces<'v>(
        this: &KBody,
        #[starlark(require = named)] normal: Option<VecArg>,
        #[starlark(require = named)] kind: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] on_plane: Option<&KPlane>,
        #[starlark(require = named)] above: Option<&KPlane>,
        #[starlark(require = named)] below: Option<&KPlane>,
        #[starlark(require = named)] inside: Option<Value<'v>>,
        #[starlark(require = named)] nearest: Option<VecArg>,
        #[starlark(require = named)] min_area: Option<UnpackFloat>,
        #[starlark(require = named)] max_area: Option<UnpackFloat>,
        #[starlark(require = named)] within: Option<UnpackFloat>,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        #[starlark(require = named, default = false)] allow_empty: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSelection> {
        let heap = eval.heap();
        for (v, what) in [(&min_area, "min_area"), (&max_area, "max_area"), (&within, "within"), (&tolerance, "tolerance")] {
            opt_num(v, &format!("faces: {what}"))?;
        }
        let mut q = Query::new(select::Which::Faces, &within, &tolerance, normal.is_some());
        if let Some(n) = normal {
            let d = vec3(&n, "faces: normal")?;
            q.push(Filter::Direction { dir: d, signed: true }, format!("normal={}", fmt_vec(d)));
        }
        q.common(kind, name, on_plane, above, below, inside, heap)?;
        if let Some(a) = min_area {
            q.push(Filter::MinArea(a.0), format!("min_area={}", fmt_num(a.0)));
        }
        if let Some(a) = max_area {
            q.push(Filter::MaxArea(a.0), format!("max_area={}", fmt_num(a.0)));
        }
        let view = selection::face_view(this, eval)?;
        q.finish(this, "face", &view.items, &view.cands, &view.rank, nearest, allow_empty, eval)
    }

    /// The face with the given topological name: the full generated name
    /// (`"lid/end"`), a local name (`"end"`, `"top"`: this body's own
    /// feature's first, else the generator's, so it keeps working after a
    /// move or a boolean), an alias, or the `"face:3"` compatibility form.
    /// An error if there is none, or if the name matches several.
    fn face<'v>(
        this: &KBody,
        #[starlark(require = pos)] name: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KFace> {
        let faces = this.faces(eval)?;
        pick(&faces, Kind::Face, &this.feature, name, "face")
            .map_err(|e| anyhow::anyhow!("{this}: {e}"))
    }

    /// Edges of this body that match every filter given, as a `Selection`
    /// (docs/design.md). With no filters, every edge.
    ///
    /// Geometry: `direction=(x,y,z)` (the tangent of a *straight* edge, within
    /// `within` degrees — unsigned, because an edge has no preferred
    /// direction; a curved edge never matches), `on_plane=`, `above=`,
    /// `below=`, `inside=(lo, hi)`, `nearest=(x,y,z)`, `of_face=` (a Face or a
    /// face name: the edges that bound it).
    ///
    /// Identity: `kind=` is the curve class ("line", "bspline", "nurbs",
    /// "intersection", plus "circle"/"arc", which are measured by fitting);
    /// `name=` is a `*`/`?` glob.
    ///
    /// Measurement: `min_length=`, `max_length=`, `min_radius=`,
    /// `max_radius=` in mm; `convex=True|False` and `min_dihedral=` /
    /// `max_dihedral=` in degrees (0 is a tangent seam, 90 a box corner) —
    /// which is how you ask for the edges worth filleting.
    ///
    /// Tolerances: `within=` is **degrees**, `tolerance=` is **millimetres**.
    /// `allow_empty=True` silences the warning an empty result
    /// raises.
    #[allow(clippy::too_many_arguments)]
    fn edges<'v>(
        this: &KBody,
        #[starlark(require = named)] direction: Option<VecArg>,
        #[starlark(require = named)] kind: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] on_plane: Option<&KPlane>,
        #[starlark(require = named)] above: Option<&KPlane>,
        #[starlark(require = named)] below: Option<&KPlane>,
        #[starlark(require = named)] inside: Option<Value<'v>>,
        #[starlark(require = named)] nearest: Option<VecArg>,
        #[starlark(require = named)] of_face: Option<Value<'v>>,
        #[starlark(require = named)] min_length: Option<UnpackFloat>,
        #[starlark(require = named)] max_length: Option<UnpackFloat>,
        #[starlark(require = named)] min_radius: Option<UnpackFloat>,
        #[starlark(require = named)] max_radius: Option<UnpackFloat>,
        #[starlark(require = named)] convex: Option<bool>,
        #[starlark(require = named)] min_dihedral: Option<UnpackFloat>,
        #[starlark(require = named)] max_dihedral: Option<UnpackFloat>,
        #[starlark(require = named)] within: Option<UnpackFloat>,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        #[starlark(require = named, default = false)] allow_empty: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSelection> {
        let heap = eval.heap();
        for (v, what) in [
            (&min_length, "min_length"),
            (&max_length, "max_length"),
            (&min_radius, "min_radius"),
            (&max_radius, "max_radius"),
            (&min_dihedral, "min_dihedral"),
            (&max_dihedral, "max_dihedral"),
            (&within, "within"),
            (&tolerance, "tolerance"),
        ] {
            opt_num(v, &format!("edges: {what}"))?;
        }
        let mut q = Query::new(select::Which::Edges, &within, &tolerance, direction.is_some());
        if let Some(d) = direction {
            let d = vec3(&d, "edges: direction")?;
            q.push(Filter::Direction { dir: d, signed: false }, format!("direction={}", fmt_vec(d)));
        }
        q.common(kind, name, on_plane, above, below, inside, heap)?;
        if let Some(l) = min_length {
            q.push(Filter::MinLength(l.0), format!("min_length={}", fmt_num(l.0)));
        }
        if let Some(l) = max_length {
            q.push(Filter::MaxLength(l.0), format!("max_length={}", fmt_num(l.0)));
        }
        if let Some(r) = min_radius {
            q.push(Filter::MinRadius(r.0), format!("min_radius={}", fmt_num(r.0)));
        }
        if let Some(r) = max_radius {
            q.push(Filter::MaxRadius(r.0), format!("max_radius={}", fmt_num(r.0)));
        }
        if let Some(c) = convex {
            q.push(Filter::Convex(c), format!("convex={}", if c { "True" } else { "False" }));
        }
        if let Some(d) = min_dihedral {
            q.push(Filter::MinDihedral(d.0), format!("min_dihedral={}", fmt_num(d.0)));
        }
        if let Some(d) = max_dihedral {
            q.push(Filter::MaxDihedral(d.0), format!("max_dihedral={}", fmt_num(d.0)));
        }
        if let Some(f) = of_face {
            let want = if let Some(face) = f.downcast_ref::<KFace>() {
                face.name.clone()
            } else if let Some(s) = f.unpack_str() {
                s.to_owned()
            } else {
                anyhow::bail!("edges: of_face must be a Face or a face name, got {}", f.get_type());
            };
            let faces = selection::face_view(this, eval)?;
            let i = selection::face_index(&faces.cands, &this.feature, &want)?;
            q.push(Filter::Neighbour(i), format!("of_face={want:?}"));
        }
        let view = selection::edge_view(this, eval)?;
        q.finish(this, "edge", &view.items, &view.cands, &view.rank, nearest, allow_empty, eval)
    }

    /// The edge with the given topological name: the pair of faces it bounds
    /// (`"lid/end|side[0.2]"`) or the `"edge:3"` compatibility form.
    fn edge<'v>(
        this: &KBody,
        #[starlark(require = pos)] name: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KEdge> {
        let edges = this.edges(eval)?;
        pick(&edges, Kind::Edge, &this.feature, name, "edge")
            .map_err(|e| anyhow::anyhow!("{this}: {e}"))
    }

    /// Axis-aligned bounding box as ((min_x, min_y, min_z), (max_x, max_y, max_z)).
    fn bbox<'v>(this: &KBody, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let (mesh, _) = ctx.body_mesh(this.body_id())?;
        let (lo, hi) = mesh.bbox().ok_or_else(|| anyhow::anyhow!("{} is empty", this))?;
        Ok(heap.alloc((alloc_vec3(heap, lo), alloc_vec3(heap, hi))))
    }

    /// Volume in mm³, measured on the body's tessellation.
    ///
    /// The divergence theorem over the triangles, so it is exact for a
    /// polyhedron and carries the tessellation's chord error for anything
    /// curved: a cylinder's mesh is inscribed in the true surface, so its
    /// volume comes out slightly low, and halving `tolerance` roughly
    /// quarters that error. Use it to *compare* two bodies built the same way
    /// — that a cut removed nothing, that a pattern really made N copies —
    /// rather than as a certified mass property.
    fn volume<'v>(this: &KBody, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<f64> {
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let (mesh, _) = ctx.body_mesh(this.body_id())?;
        Ok(mesh.volume())
    }

    /// Surface area in mm², measured on the body's tessellation.
    ///
    /// Same caveat as `volume()`: chord error, not a certified mass property.
    fn area<'v>(this: &KBody, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<f64> {
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let (mesh, _) = ctx.body_mesh(this.body_id())?;
        Ok(mesh.area())
    }

    /// The display layer this body is on, by the name `layer(...)`
    /// declared, or `None` for a body on no layer. Read when called, so it
    /// follows `Layer.add` and `paint(layer=)` made after this value was:
    /// `[b for b in bodies() if b.layer() == "walls"]`.
    fn layer<'v>(this: &KBody, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let st = store(eval)?;
        let ctx = st.ctx.borrow();
        let layer = &ctx.body(this.body_id())?.layer;
        Ok(if layer.is_empty() { Value::new_none() } else { heap.alloc(layer.as_str()) })
    }

    /// Whether this body is part of the model as a viewer shows it and a
    /// whole-document export writes it: `False` once a boolean or a split
    /// consumed it (it is still usable as an operand), and `False` while its
    /// layer is declared hidden (`layer(..., visible=False)`). Read when
    /// called, like `layer()`.
    fn visible<'v>(this: &KBody, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<bool> {
        let st = store(eval)?;
        let ctx = st.ctx.borrow();
        let b = ctx.body(this.body_id())?;
        Ok(b.visible && cadpole_core::layers::shown(&b.layer, &ctx.layers))
    }

    /// The body's base colour as `"#RRGGBB"` (sRGB), as `material(color=)`
    /// takes it: the material's own, or the default grey for a body nothing
    /// coloured. A colour `paint(...)` gave some faces is not this.
    fn color<'v>(this: &KBody, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<String> {
        let st = store(eval)?;
        let ctx = st.ctx.borrow();
        Ok(cadpole_core::paint::hex(ctx.body(this.body_id())?.material.base_color))
    }

    /// Cut this body with `plane` and return the cut as a `Sketch` on that
    /// plane: the closed loops where the plane crosses the body's
    /// tessellation, in the plane's own coordinates, nested as material and
    /// holes. What a floor plan at a height *is*, and what `area()` on the
    /// result measures — the plan area of a body at that level. A plane
    /// that misses the body gives an empty sketch. The body is not consumed.
    ///
    /// `on=` lays the cut on another plane instead, coordinates unchanged:
    /// `mass.section(XY.offset(z), on=XY)` is the plan at height `z` drawn
    /// on the ground plane, which is what lets two cuts at different heights
    /// meet in `Sketch.intersect` (the 2D booleans want one plane).
    ///
    /// Exact for a polyhedron; a curved face contributes its chords, as
    /// `volume()` does. Feature: `section`, args `plane`, `body`, `loops`.
    fn section<'v>(
        this: &KBody,
        #[starlark(require = pos)] plane: &KPlane,
        #[starlark(require = named)] on: Option<&KPlane>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let body_id = ctx.body(this.body_id())?.id_string();
        let loops = {
            let (mesh, _) = ctx.body_mesh(this.body_id())?;
            cadpole_drawing::section_loops(mesh, &plane.0, 0.01)
        };
        let mut args = BTreeMap::new();
        args.insert("plane".into(), format!("{}", KPlane(plane.0)));
        if let Some(on) = on {
            args.insert("on".into(), format!("{}", KPlane(on.0)));
        }
        args.insert("body".into(), body_id);
        args.insert("loops".into(), loops.len().to_string());
        let fid = ctx.new_feature("section", name.map(str::to_owned), vec![FeatureId(this.feature.clone())], args, span)?;
        let sketch = loops.into_iter().fold(Sketch::new(on.map(|p| p.0).unwrap_or(plane.0)), |s, l| s.with_loop(l));
        Ok(KSketch::new(fid.0, sketch))
    }

    /// Move this body by `offset`. Returns the moved body; the original is consumed.
    fn translate<'v>(
        this: &KBody,
        #[starlark(require = pos)] offset: VecArg,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let v = vec3(&offset, "translate: offset")?;
        let mut args = BTreeMap::new();
        args.insert("offset".into(), cadpole_core::context::fmt_vec(v));
        let b = transform_body(this, Transform::translation(v), "translate", args, name, eval)?;
        crate::layers::tag_body(eval, layer, &b, "translate")?;
        Ok(b)
    }

    /// Rotate this body by `angle` degrees about `axis` through `origin`.
    fn rotate<'v>(
        this: &KBody,
        #[starlark(require = pos)] angle: UnpackFloat,
        #[starlark(require = named)] axis: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let axis = axis.as_ref().map(|a| vec3(a, "rotate: axis")).transpose()?.unwrap_or(Vec3::Z);
        let origin = origin.as_ref().map(|o| vec3(o, "rotate: origin")).transpose()?.unwrap_or(Vec3::ZERO);
        let angle = num(&angle, "rotate: angle")?;
        if axis.length() == 0.0 {
            anyhow::bail!("rotate: axis must be non-zero");
        }
        let mut args = BTreeMap::new();
        args.insert("angle_deg".into(), fmt_num(angle));
        args.insert("axis".into(), cadpole_core::context::fmt_vec(axis));
        args.insert("origin".into(), cadpole_core::context::fmt_vec(origin));
        let b = transform_body(this, Transform::rotation(origin, axis, angle.to_radians()), "rotate", args, name, eval)?;
        crate::layers::tag_body(eval, layer, &b, "rotate")?;
        Ok(b)
    }

    /// Set how finely this body is tessellated, for everything that reads
    /// its triangles: export (STL/OBJ/3MF; STEP is exact and ignores it),
    /// render, `print_check`, `volume()`. Returns the same body.
    ///
    /// `tolerance=` is the chordal sag in mm no triangle exceeds — the
    /// document's is `--tolerance`, 0.05 by default — and `angle=` the most
    /// the surface normal may turn across one cell of a spline patch, in
    /// degrees (8 by default; 0 switches that bound off). A body's own
    /// setting beats `export(..., tolerance=)`, which beats the document's:
    /// a gear's flanks at 0.01 mm next to a housing at 0.05.
    ///
    /// A rigid copy (a pattern member, a `copy`) shares its source's
    /// tessellation, and a non-uniform `scale` remaps its source's
    /// triangles, so set this on the source; on either it is a warning and
    /// does nothing.
    fn tessellation<'v>(
        this: &KBody,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        #[starlark(require = named)] angle: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let tolerance = opt_num(&tolerance, "tessellation: tolerance")?;
        let angle = opt_num(&angle, "tessellation: angle")?;
        if let Some(t) = tolerance {
            if !(t > 0.0) {
                anyhow::bail!("tessellation: tolerance must be positive, got {t}");
            }
        }
        if let Some(a) = angle {
            if a < 0.0 {
                anyhow::bail!("tessellation: angle must be zero (off) or positive degrees, got {a}");
            }
        }
        if tolerance.is_none() && angle.is_none() {
            anyhow::bail!("tessellation: pass tolerance= (mm) and/or angle= (degrees)");
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = this.body_id();
        let b = ctx.body(id)?;
        if b.is_copy() || b.has_given_mesh() {
            let (name, how) = (b.name.clone(), if b.is_copy() { "a copy shares" } else { "a scaled body remaps" });
            ctx.diagnostics.push(
                Diagnostic::warning(format!(
                    "tessellation({name}): {how} its source's tessellation, so this does nothing; \
                     call tessellation() on the body it was made from"
                ))
                .with_span(span),
            );
            return Ok(this.clone());
        }
        let b = ctx.body_mut(id)?;
        if let Some(t) = tolerance {
            b.tessellation.tolerance = Some(t);
        }
        if let Some(a) = angle {
            b.tessellation.max_normal_degrees = Some(a);
        }
        Ok(this.clone())
    }

    /// Set the appearance used for rendering. Returns the same body.
    ///
    /// `preset=` (see `render::presets`) is applied first and the other
    /// keywords override it. Unknown preset or texture names are errors that
    /// list what is available; the body is left untouched.
    ///
    /// `transmission=`, `attenuation=`, `scatter=`, `scatter_g=`,
    /// `anisotropy=` and `edge_break=` are the print-material terms only the
    /// path tracer reads (`cadpole render --backend path`); see
    /// docs/starlark-api.md.
    ///
    /// `shadow=False` makes the body guide geometry (a zoning envelope, a
    /// setback plane, a lot line): the camera sees it, light does not, so
    /// it neither shadows nor tints nor blocks light in either renderer,
    /// while its own surface is still lit and shadowed by the real bodies.
    /// The flag travels with the material through transforms, booleans and
    /// copies. `shadow_study()` and the sightline queries take explicit body
    /// lists, and a body passed to them is an obstacle whatever this says.
    fn material<'v>(
        this: &KBody,
        #[starlark(require = named)] preset: Option<&str>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named)] roughness: Option<UnpackFloat>,
        #[starlark(require = named)] metallic: Option<UnpackFloat>,
        #[starlark(require = named)] texture: Option<&str>,
        #[starlark(require = named)] scale: Option<UnpackFloat>,
        #[starlark(require = named)] opacity: Option<UnpackFloat>,
        #[starlark(require = named)] ior: Option<UnpackFloat>,
        #[starlark(require = named)] emissive: Option<Value<'v>>,
        #[starlark(require = named)] transmission: Option<UnpackFloat>,
        #[starlark(require = named)] attenuation: Option<UnpackFloat>,
        #[starlark(require = named)] scatter: Option<UnpackFloat>,
        #[starlark(require = named)] scatter_g: Option<UnpackFloat>,
        #[starlark(require = named)] anisotropy: Option<UnpackFloat>,
        #[starlark(require = named)] edge_break: Option<UnpackFloat>,
        #[starlark(require = named)] depth: Option<UnpackFloat>,
        #[starlark(require = named)] label: Option<&str>,
        #[starlark(require = named)] shadow: Option<bool>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let edit = MaterialEdit::parse(
            preset, color, roughness, metallic, texture, scale, opacity, ior, emissive, transmission, attenuation, scatter,
            scatter_g, anisotropy, edge_break, depth, label, shadow,
        )?;
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        edit.apply(&mut ctx.body_mut(this.body_id())?.material);
        Ok(this.clone())
    }
}


/// The keyword arguments of `Body.material(...)` and `Instance.material(...)`,
/// validated before anything is touched, so a bad name never leaves a
/// half-applied material behind. One parser, two callers.
pub(crate) struct MaterialEdit {
    preset: Option<cadpole_core::context::Material>,
    color: Option<[f64; 3]>,
    roughness: Option<f64>,
    metallic: Option<f64>,
    texture: Option<String>,
    scale: Option<f64>,
    opacity: Option<f64>,
    ior: Option<f64>,
    emissive: Option<[f64; 3]>,
    transmission: Option<f64>,
    attenuation: Option<f64>,
    scatter: Option<f64>,
    scatter_g: Option<f64>,
    anisotropy: Option<f64>,
    edge_break: Option<f64>,
    depth: Option<f64>,
    label: Option<String>,
    /// `shadow=`: false makes the body guide geometry.
    shadow: Option<bool>,
}

impl MaterialEdit {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn parse<'v>(
        preset: Option<&str>,
        color: Option<Value<'v>>,
        roughness: Option<UnpackFloat>,
        metallic: Option<UnpackFloat>,
        texture: Option<&str>,
        scale: Option<UnpackFloat>,
        opacity: Option<UnpackFloat>,
        ior: Option<UnpackFloat>,
        emissive: Option<Value<'v>>,
        transmission: Option<UnpackFloat>,
        attenuation: Option<UnpackFloat>,
        scatter: Option<UnpackFloat>,
        scatter_g: Option<UnpackFloat>,
        anisotropy: Option<UnpackFloat>,
        edge_break: Option<UnpackFloat>,
        depth: Option<UnpackFloat>,
        label: Option<&str>,
        shadow: Option<bool>,
    ) -> anyhow::Result<MaterialEdit> {
        use cadpole_core::render::{material_preset, material_preset_names, texture_kind, texture_names};
        let from_preset = match preset {
            Some(p) => Some(material_preset(p).ok_or_else(|| {
                anyhow::anyhow!("unknown material preset {p:?}; expected one of {}", quoted(material_preset_names()))
            })?),
            None => None,
        };
        if let Some(t) = texture {
            if texture_kind(t).is_none() {
                anyhow::bail!("unknown texture {t:?}; expected \"\" or one of {}", quoted(texture_names()));
            }
        }
        let emissive = emissive.map(crate::convert::color).transpose()?.map(|e| e.map(|c| c.max(0.0)));
        let color = color.map(crate::convert::color).transpose()?;
        // `clamp` and `max` pass NaN through, so every number is checked
        // before the body is touched, like the names above.
        Ok(MaterialEdit {
            preset: from_preset,
            color,
            roughness: opt_num(&roughness, "material: roughness")?,
            metallic: opt_num(&metallic, "material: metallic")?,
            texture: texture.map(str::to_owned),
            scale: opt_num(&scale, "material: scale")?,
            opacity: opt_num(&opacity, "material: opacity")?,
            ior: opt_num(&ior, "material: ior")?,
            emissive,
            transmission: opt_num(&transmission, "material: transmission")?,
            attenuation: opt_num(&attenuation, "material: attenuation")?,
            scatter: opt_num(&scatter, "material: scatter")?,
            scatter_g: opt_num(&scatter_g, "material: scatter_g")?,
            anisotropy: opt_num(&anisotropy, "material: anisotropy")?,
            edge_break: opt_num(&edge_break, "material: edge_break")?,
            depth: opt_num(&depth, "material: depth")?,
            label: label.map(str::to_owned),
            shadow,
        })
    }

    /// A preset name alone, as `instance(material="brass")` takes it.
    pub(crate) fn preset_only(preset: &str) -> anyhow::Result<MaterialEdit> {
        MaterialEdit::parse(
            Some(preset), None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
            None,
        )
    }

    /// `shadow=` alone, as `paint(body, shadow=False)` takes it.
    pub(crate) fn shadow_only(shadow: bool) -> MaterialEdit {
        MaterialEdit {
            preset: None,
            color: None,
            roughness: None,
            metallic: None,
            texture: None,
            scale: None,
            opacity: None,
            ior: None,
            emissive: None,
            transmission: None,
            attenuation: None,
            scatter: None,
            scatter_g: None,
            anisotropy: None,
            edge_break: None,
            depth: None,
            label: None,
            shadow: Some(shadow),
        }
    }

    pub(crate) fn apply(&self, m: &mut cadpole_core::context::Material) {
        use cadpole_core::render::{texture_default_scale, texture_kind};
        if let Some(p) = &self.preset {
            *m = p.clone();
        }
        if let Some(c) = self.color {
            m.base_color = c;
        }
        if let Some(r) = self.roughness {
            m.roughness = r.clamp(0.0, 1.0);
        }
        if let Some(x) = self.metallic {
            m.metallic = x.clamp(0.0, 1.0);
        }
        if let Some(t) = &self.texture {
            m.texture = t.clone();
            // A machined finish's repeat is a groove pitch, a fraction of a
            // millimetre; naming the finish without `scale=` takes the
            // pitch of a real one rather than the patterns' 10 mm. The
            // patterns keep whatever scale the material had, as before.
            if self.scale.is_none() {
                if let Some(k) = texture_kind(t).filter(|k| k.is_relief()) {
                    m.texture_scale = f64::from(texture_default_scale(k));
                }
            }
        }
        if let Some(s) = self.scale {
            m.texture_scale = s;
        }
        if let Some(d) = self.depth {
            m.texture_depth = d.max(0.0);
        }
        if let Some(o) = self.opacity {
            m.opacity = o.clamp(0.0, 1.0);
        }
        if let Some(i) = self.ior {
            m.ior = i.clamp(1.0, 2.5);
        }
        if let Some(e) = self.emissive {
            m.emissive = e;
        }
        if let Some(t) = self.transmission {
            m.transmission = t.clamp(0.0, 1.0);
        }
        if let Some(a) = self.attenuation {
            m.attenuation_distance = a.max(0.0);
        }
        if let Some(s) = self.scatter {
            m.scatter_distance = s.max(0.0);
        }
        if let Some(g) = self.scatter_g {
            m.scatter_g = g.clamp(-1.0, 1.0);
        }
        if let Some(a) = self.anisotropy {
            m.anisotropy = a.clamp(0.0, 1.0);
        }
        if let Some(e) = self.edge_break {
            m.edge_break = e.max(0.0);
        }
        if let Some(n) = &self.label {
            m.name = n.clone();
        }
        if let Some(s) = self.shadow {
            m.shadow = s;
        }
    }
}


// ---------------------------------------------------------------- Printer

/// A named machine from `cadpole_print::PRINTERS`, as a script value.
///
/// It is a *snapshot of the numbers*, not a handle: `printer("bambu_a1").bed`
/// is a tuple, and passing the value to `print_check(printer=...)` passes
/// those numbers. Anything spelled out at the call site wins over the profile,
/// so `print_check(b, printer="bambu_a1", nozzle=0.6)` is the A1's bed with a
/// 0.6 mm nozzle.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KPrinter {
    pub id: String,
    pub display_name: String,
    pub bed: [f64; 3],
    pub bed_shape: String,
    pub nozzle: f64,
    pub layer_height: f64,
    /// The gap below which two bodies printed together fuse.
    pub min_gap: f64,
    pub max_overhang: f64,
    pub material: String,
    pub density: f64,
    pub source: String,
    /// The support rules (`cadpole_print::SupportRules`) the machine is
    /// judged by.
    #[allocative(skip)]
    pub support: cadpole_print::SupportRules,
}
starlark_simple_value!(KPrinter);

impl fmt::Display for KPrinter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "printer({:?}: {} x {} x {} mm, {} mm nozzle, {} mm layers, {})",
            self.id,
            fmt_num(self.bed[0]),
            fmt_num(self.bed[1]),
            fmt_num(self.bed[2]),
            fmt_num(self.nozzle),
            fmt_num(self.layer_height),
            self.material
        )
    }
}

#[starlark_value(type = "Printer")]
impl<'v> StarlarkValue<'v> for KPrinter {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "name" => Some(heap.alloc(self.id.as_str())),
            "display_name" => Some(heap.alloc(self.display_name.as_str())),
            "bed" => Some(heap.alloc((self.bed[0], self.bed[1], self.bed[2]))),
            "bed_shape" => Some(heap.alloc(self.bed_shape.as_str())),
            "nozzle" => Some(heap.alloc(self.nozzle)),
            "layer_height" => Some(heap.alloc(self.layer_height)),
            "min_gap" => Some(heap.alloc(self.min_gap)),
            "max_overhang" => Some(heap.alloc(self.max_overhang)),
            "material" => Some(heap.alloc(self.material.as_str())),
            "density" => Some(heap.alloc(self.density)),
            "max_overhang_area" => Some(heap.alloc(self.support.max_overhang_area)),
            "max_cantilever" => Some(heap.alloc(self.support.max_cantilever)),
            "max_horizontal_hole" => Some(heap.alloc(self.support.max_horizontal_hole)),
            "min_island" => Some(heap.alloc(self.support.min_island)),
            "max_slenderness" => Some(heap.alloc(self.support.max_slenderness)),
            "slender_width" => Some(heap.alloc(self.support.slender_width)),
            "max_height_to_base" => Some(heap.alloc(self.support.max_height_to_base)),
            // Where the numbers came from, so a script can print its own
            // provenance.
            "source" => Some(heap.alloc(self.source.as_str())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        [
            "name",
            "display_name",
            "bed",
            "bed_shape",
            "nozzle",
            "layer_height",
            "min_gap",
            "max_overhang",
            "material",
            "density",
            "max_overhang_area",
            "max_cantilever",
            "max_horizontal_hole",
            "min_island",
            "max_slenderness",
            "slender_width",
            "max_height_to_base",
            "source",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }
}

impl KPrinter {
    pub fn from_profile(p: &cadpole_print::Profile, display_name: &str, source: &str) -> KPrinter {
        KPrinter {
            id: p.name.clone(),
            display_name: display_name.to_owned(),
            bed: p.bed,
            bed_shape: p.bed_shape.as_str().to_owned(),
            nozzle: p.nozzle,
            layer_height: p.layer_height,
            min_gap: p.min_gap,
            max_overhang: p.max_overhang_degrees,
            material: p.material.name.to_owned(),
            density: p.material.density_g_cm3,
            source: source.to_owned(),
            support: p.support,
        }
    }

    /// The settings the checks read.
    pub fn profile(&self) -> cadpole_print::Profile {
        let d = cadpole_print::Profile::default();
        cadpole_print::Profile {
            name: self.id.clone(),
            nozzle: self.nozzle,
            perimeters: d.perimeters,
            min_wall: self.nozzle * d.perimeters,
            layer_height: self.layer_height,
            min_gap: self.min_gap,
            max_overhang_degrees: self.max_overhang,
            bed: self.bed,
            bed_shape: cadpole_print::BedShape::parse(&self.bed_shape).unwrap_or_default(),
            material: cadpole_print::Material::parse(&self.material).unwrap_or_default(),
            build_direction: d.build_direction,
            chord_gap_fraction: d.chord_gap_fraction,
            support: self.support,
        }
    }
}

// ---------------------------------------------------------------- PrintReport

/// What `print_check(...)` hands back: every number the checks measured plus
/// the findings as text, so a script can assert on any of them.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KPrintReport {
    pub body: String,
    pub ok: bool,
    /// Closed, consistently oriented, and free of crossings and doubled
    /// faces: `cadpole_core::solid`'s verdict.
    pub solid: bool,
    pub watertight: bool,
    pub triangles: u32,
    pub volume: f64,
    pub area: f64,
    pub holes: u32,
    pub boundary_edges: u32,
    pub non_manifold_edges: u32,
    pub flipped_edges: u32,
    pub self_intersections: u32,
    /// Pairs of coplanar triangles lying on top of each other.
    pub internal_faces: u32,
    pub degenerate: u32,
    pub inverted_normals: bool,
    /// Separate closed shells: 2 for a part with a cavity.
    pub components: u32,
    /// Genus of the surface, or -1 when it is not closed.
    pub genus: i32,
    pub min_wall: f64,
    pub thin_area: f64,
    pub unsupported_area: f64,
    pub max_overhang: f64,
    pub bed_contact_area: f64,
    /// Flat downward faces held up at both ends within `max_bridge`:
    /// their area, the longest span, how many regions.
    pub bridge_area: f64,
    pub longest_bridge: f64,
    pub bridges: u32,
    /// The support rules' measurements (`cadpole_print::support`): what
    /// still needs support after the allowances, the largest connected
    /// overhang patch, ledges, islands, round horizontal holes, slender
    /// features and the first layer.
    pub support_area: f64,
    pub largest_overhang: f64,
    pub ledge_area: f64,
    pub longest_cantilever: f64,
    pub islands: u32,
    pub deepest_island: f64,
    pub horizontal_holes: u32,
    pub wide_holes: u32,
    pub slender: u32,
    pub slenderness: f64,
    pub first_layer_area: f64,
    pub height_to_base: f64,
    /// The findings that say it needs support, as text; empty when it
    /// prints without.
    pub support: Vec<String>,
    pub fits_bed: bool,
    pub fits_bed_rotated: bool,
    #[allocative(skip)]
    pub size: Vec3,
    #[allocative(skip)]
    pub bed_excess: Vec3,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub summary: String,
    /// The printer profile the checks judged this against ("default" when no
    /// `printer=` was named).
    pub profile: String,
    /// Solid mass in grams at the profile's material density: an upper bound,
    /// because infill makes the real print lighter.
    pub mass: f64,
    /// Layers tall at the profile's layer height.
    pub layers: u32,
    /// When the body does not fit the bed: the `split()` call that would make
    /// it fit, and where. Empty when it fits.
    pub split_advice: String,
    pub split_axis: String,
    pub split_pieces: u32,
    pub split_at: Vec<f64>,
    /// Empty unless `orient=True` was passed.
    pub orientation: String,
    pub orientation_reasoning: String,
    pub orientation_changed: bool,
}
starlark_simple_value!(KPrintReport);

impl fmt::Display for KPrintReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "print_check({}: {})", self.body, self.summary)
    }
}

#[starlark_value(type = "PrintReport")]
impl<'v> StarlarkValue<'v> for KPrintReport {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "body" => Some(heap.alloc(self.body.as_str())),
            // False when anything was found that would stop the print.
            "ok" => Some(heap.alloc(self.ok)),
            "solid" => Some(heap.alloc(self.solid)),
            "watertight" => Some(heap.alloc(self.watertight)),
            "triangles" => Some(heap.alloc(self.triangles as i32)),
            "volume" => Some(heap.alloc(self.volume)),
            "area" => Some(heap.alloc(self.area)),
            "holes" => Some(heap.alloc(self.holes as i32)),
            "boundary_edges" => Some(heap.alloc(self.boundary_edges as i32)),
            "non_manifold_edges" => Some(heap.alloc(self.non_manifold_edges as i32)),
            "flipped_edges" => Some(heap.alloc(self.flipped_edges as i32)),
            "self_intersections" => Some(heap.alloc(self.self_intersections as i32)),
            "internal_faces" => Some(heap.alloc(self.internal_faces as i32)),
            "degenerate" => Some(heap.alloc(self.degenerate as i32)),
            "inverted_normals" => Some(heap.alloc(self.inverted_normals)),
            "components" => Some(heap.alloc(self.components as i32)),
            "genus" => Some(heap.alloc(self.genus)),
            "min_wall" => Some(heap.alloc(self.min_wall)),
            "thin_area" => Some(heap.alloc(self.thin_area)),
            "unsupported_area" => Some(heap.alloc(self.unsupported_area)),
            "max_overhang" => Some(heap.alloc(self.max_overhang)),
            "bed_contact_area" => Some(heap.alloc(self.bed_contact_area)),
            "bridge_area" => Some(heap.alloc(self.bridge_area)),
            "longest_bridge" => Some(heap.alloc(self.longest_bridge)),
            "bridges" => Some(heap.alloc(self.bridges as i32)),
            "support_area" => Some(heap.alloc(self.support_area)),
            "largest_overhang" => Some(heap.alloc(self.largest_overhang)),
            "ledge_area" => Some(heap.alloc(self.ledge_area)),
            "longest_cantilever" => Some(heap.alloc(self.longest_cantilever)),
            "islands" => Some(heap.alloc(self.islands as i32)),
            "deepest_island" => Some(heap.alloc(self.deepest_island)),
            "horizontal_holes" => Some(heap.alloc(self.horizontal_holes as i32)),
            "wide_holes" => Some(heap.alloc(self.wide_holes as i32)),
            "slender" => Some(heap.alloc(self.slender as i32)),
            "slenderness" => Some(heap.alloc(self.slenderness)),
            "first_layer_area" => Some(heap.alloc(self.first_layer_area)),
            "height_to_base" => Some(heap.alloc(self.height_to_base)),
            "support" => Some(alloc_str_list(heap, &self.support)),
            "support_free" => Some(heap.alloc(self.support.is_empty())),
            "fits_bed" => Some(heap.alloc(self.fits_bed)),
            "fits_bed_rotated" => Some(heap.alloc(self.fits_bed_rotated)),
            "size" => Some(alloc_vec3(heap, self.size)),
            "bed_excess" => Some(alloc_vec3(heap, self.bed_excess)),
            "errors" => Some(alloc_str_list(heap, &self.errors)),
            "warnings" => Some(alloc_str_list(heap, &self.warnings)),
            "summary" => Some(heap.alloc(self.summary.as_str())),
            "profile" => Some(heap.alloc(self.profile.as_str())),
            "mass" => Some(heap.alloc(self.mass)),
            "layers" => Some(heap.alloc(self.layers as i32)),
            "split_advice" => Some(heap.alloc(self.split_advice.as_str())),
            "split_axis" => Some(heap.alloc(self.split_axis.as_str())),
            "split_pieces" => Some(heap.alloc(self.split_pieces as i32)),
            "split_at" => Some(heap.alloc(self.split_at.clone())),
            "orientation" => Some(heap.alloc(self.orientation.as_str())),
            "orientation_reasoning" => Some(heap.alloc(self.orientation_reasoning.as_str())),
            "orientation_changed" => Some(heap.alloc(self.orientation_changed)),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        [
            "body",
            "ok",
            "solid",
            "watertight",
            "triangles",
            "volume",
            "area",
            "holes",
            "boundary_edges",
            "non_manifold_edges",
            "flipped_edges",
            "self_intersections",
            "internal_faces",
            "degenerate",
            "inverted_normals",
            "components",
            "genus",
            "min_wall",
            "thin_area",
            "unsupported_area",
            "max_overhang",
            "bed_contact_area",
            "bridge_area",
            "longest_bridge",
            "bridges",
            "support_area",
            "largest_overhang",
            "ledge_area",
            "longest_cantilever",
            "islands",
            "deepest_island",
            "horizontal_holes",
            "wide_holes",
            "slender",
            "slenderness",
            "first_layer_area",
            "height_to_base",
            "support",
            "support_free",
            "fits_bed",
            "fits_bed_rotated",
            "size",
            "bed_excess",
            "errors",
            "warnings",
            "summary",
            "profile",
            "mass",
            "layers",
            "split_advice",
            "split_axis",
            "split_pieces",
            "split_at",
            "orientation",
            "orientation_reasoning",
            "orientation_changed",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }
}

impl KPrintReport {
    /// Build the script value from a [`cadpole_print::Report`].
    pub fn from_report(
        body: &str,
        r: &cadpole_print::Report,
        orientation: Option<&cadpole_print::Orientation>,
    ) -> KPrintReport {
        KPrintReport {
            body: body.to_owned(),
            ok: r.ok(),
            solid: r.mesh.solid,
            watertight: r.mesh.watertight,
            triangles: r.triangles as u32,
            volume: r.volume_mm3,
            area: r.area_mm2,
            holes: r.mesh.holes.len() as u32,
            boundary_edges: r.mesh.boundary_edges as u32,
            non_manifold_edges: r.mesh.validator.non_manifold.len() as u32,
            flipped_edges: r.mesh.validator.flipped.len() as u32,
            self_intersections: r.mesh.validator.crossings.len() as u32,
            internal_faces: r.mesh.validator.overlaps.len() as u32,
            degenerate: (r.mesh.validator.degenerate.len() + r.mesh.collapsed) as u32,
            inverted_normals: r.mesh.inverted,
            components: r.mesh.components as u32,
            genus: r.mesh.genus.map(|g| g as i32).unwrap_or(-1),
            min_wall: if r.thickness.min.is_finite() { r.thickness.min } else { 0.0 },
            thin_area: r.thickness.thin_area,
            unsupported_area: r.overhang.unsupported_area,
            max_overhang: r.overhang.max_degrees,
            bed_contact_area: r.overhang.bed_contact_area,
            bridge_area: r.overhang.bridge_area,
            longest_bridge: r.overhang.longest_bridge,
            bridges: r.overhang.bridges as u32,
            support_area: r.overhang.support_area,
            largest_overhang: r.overhang.largest_patch,
            ledge_area: r.overhang.ledge_area,
            longest_cantilever: r.overhang.longest_cantilever,
            islands: r.support.islands as u32,
            deepest_island: r.support.deepest_island,
            horizontal_holes: r.support.horizontal_holes as u32,
            wide_holes: r.support.wide_holes as u32,
            slender: r.support.slender as u32,
            slenderness: r.support.slenderness,
            first_layer_area: r.support.first_layer_area,
            // A part with no first layer (nothing within half a layer of
            // its lowest point) reports a large number rather than inf.
            height_to_base: if r.support.height_to_base.is_finite() { r.support.height_to_base } else { 1e9 },
            support: r.support_findings().map(|i| format!("[{}] {}", i.check.as_str(), i.message)).collect(),
            fits_bed: r.bed.fits,
            fits_bed_rotated: r.bed.fits_rotated,
            size: r.bed.size,
            bed_excess: r.bed.excess,
            errors: r.errors().map(|i| i.message.clone()).collect(),
            warnings: r.warnings().map(|i| i.message.clone()).collect(),
            summary: r.summary(),
            profile: r.profile_name.clone(),
            mass: r.mass_g,
            layers: r.layers as u32,
            split_advice: r.bed.split.as_ref().map(|s| s.call.clone()).unwrap_or_default(),
            split_axis: r.bed.split.as_ref().map(|s| s.axis.to_owned()).unwrap_or_default(),
            split_pieces: r.bed.split.as_ref().map(|s| s.pieces as u32).unwrap_or(0),
            split_at: r.bed.split.as_ref().map(|s| s.cuts.clone()).unwrap_or_default(),
            orientation: orientation.map(|o| o.chosen.label.clone()).unwrap_or_default(),
            orientation_reasoning: orientation.map(|o| o.reasoning.clone()).unwrap_or_default(),
            orientation_changed: orientation.map(|o| o.changed).unwrap_or(false),
        }
    }
}

/// `"a", "b", "c"` — the valid names in a diagnostic.
fn quoted(names: &[&str]) -> String {
    names.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ")
}

// ---------------------------------------------------------------- Face / Edge

#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KFace {
    pub body: u32,
    pub name: String,
    /// Other names that resolve to this face: the generic form of a
    /// primitive's face, the group name of a split piece, or the other sources
    /// of a face a boolean merged.
    pub aliases: Vec<String>,
    /// What the script sees: the recognised analytic class (`cylinder`, …)
    /// when there is one, else truck's own type (`FaceInfo::kind`).
    pub kind: String,
    /// truck's own type, which the fingerprint keeps so a stored reference
    /// still matches the topology it was taken from.
    pub surface_kind: String,
    #[allocative(skip)]
    pub normal: Vec3,
    #[allocative(skip)]
    pub centroid: Vec3,
    pub area: f64,
    pub edges: Vec<String>,
}
starlark_simple_value!(KFace);

impl fmt::Display for KFace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "face(body#{} {} {})", self.body, self.name, self.kind)
    }
}

#[starlark_value(type = "Face")]
impl<'v> StarlarkValue<'v> for KFace {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "name" => Some(heap.alloc(self.name.as_str())),
            "aliases" => Some(alloc_str_list(heap, &self.aliases)),
            "kind" => Some(heap.alloc(self.kind.as_str())),
            "normal" => Some(alloc_vec3(heap, self.normal)),
            "centroid" => Some(alloc_vec3(heap, self.centroid)),
            "area" => Some(heap.alloc(self.area)),
            "edges" => Some(alloc_str_list(heap, &self.edges)),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["name", "aliases", "kind", "normal", "centroid", "area", "edges"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

impl Named for KFace {
    fn name(&self) -> &str {
        &self.name
    }
    fn aliases(&self) -> &[String] {
        &self.aliases
    }
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            surface_kind: self.surface_kind.clone(),
            centroid: self.centroid,
            normal: self.normal,
            area: self.area,
            length: 0.0,
        }
    }
}

impl Named for KEdge {
    fn name(&self) -> &str {
        &self.name
    }
    fn aliases(&self) -> &[String] {
        &[]
    }
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            surface_kind: self.kind.clone(),
            centroid: (self.start + self.end) * 0.5,
            normal: Vec3::ZERO,
            area: 0.0,
            length: self.length,
        }
    }
}

/// Look one element up by name, the ways a script may address
/// it: the full generated name, an alias, a local name (this body's feature's
/// first, else the one generator that has it), or the `face:N` / `edge:N`
/// compatibility form. Several matches is
/// an error naming them, never a silent pick.
pub(crate) fn pick<T: Named + Clone>(
    items: &[T],
    kind: Kind,
    feature: &str,
    query: &str,
    what: &str,
) -> anyhow::Result<T> {
    let listing = || {
        let mut names: Vec<&str> = items.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        if names.len() > 12 {
            names.truncate(12);
            format!("{}, ...", names.join(", "))
        } else {
            names.join(", ")
        }
    };
    match naming::resolve(items, kind, feature, query, None, 0.0) {
        Resolution::Ok(i) => Ok(items[i].clone()),
        Resolution::Ambiguous(all) => anyhow::bail!(
            "{query:?} names {} {what}s ({}); it is a group, so ask for one of them by name",
            all.len(),
            all.iter().map(|i| items[*i].name()).collect::<Vec<_>>().join(", ")
        ),
        _ => anyhow::bail!("no {what} named {query:?}; this body has {}", listing()),
    }
}

#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KEdge {
    pub body: u32,
    pub name: String,
    pub kind: String,
    #[allocative(skip)]
    pub start: Vec3,
    #[allocative(skip)]
    pub end: Vec3,
    pub length: f64,
    pub faces: Vec<String>,
    /// The circle the edge's polyline fits (`select::circle_fit_frame`, the
    /// fit `frame(edge)` uses): its radius and centre, `None` for an
    /// edge that is not an arc. Measured on the polyline, so a circle a
    /// boolean broke into a `bspline` still answers.
    pub radius: Option<f64>,
    #[allocative(skip)]
    pub center: Option<Vec3>,
}
starlark_simple_value!(KEdge);

impl KEdge {
    /// One edge of a body's topology as the script sees it, the circle fit
    /// included.
    pub fn from_topology(body: u32, e: &cadpole_core::tessellate::EdgeInfo) -> KEdge {
        let fit = cadpole_core::select::circle_fit_frame(&e.polyline);
        KEdge {
            body,
            name: e.name.clone(),
            kind: e.curve_kind.to_owned(),
            start: e.start,
            end: e.end,
            length: e.length,
            faces: e.faces.clone(),
            radius: fit.map(|(_, _, r)| r),
            center: fit.map(|(c, _, _)| c),
        }
    }
}

impl fmt::Display for KEdge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "edge(body#{} {} {})", self.body, self.name, self.kind)
    }
}

#[starlark_value(type = "Edge")]
impl<'v> StarlarkValue<'v> for KEdge {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "name" => Some(heap.alloc(self.name.as_str())),
            "kind" => Some(heap.alloc(self.kind.as_str())),
            "start" => Some(alloc_vec3(heap, self.start)),
            "end" => Some(alloc_vec3(heap, self.end)),
            "length" => Some(heap.alloc(self.length)),
            "faces" => Some(alloc_str_list(heap, &self.faces)),
            "radius" => Some(match self.radius {
                Some(r) => heap.alloc(r),
                None => Value::new_none(),
            }),
            "center" => Some(match self.center {
                Some(c) => alloc_vec3(heap, c),
                None => Value::new_none(),
            }),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["name", "kind", "start", "end", "length", "faces", "radius", "center"].iter().map(|s| s.to_string()).collect()
    }
}

// ---------------------------------------------------------------- Ref (selection)

/// Something the user selected in the UI.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KRef {
    pub feature: String,
    pub body: String,
    pub kind: String,
    pub name: String,
}
starlark_simple_value!(KRef);

impl fmt::Display for KRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ref({} {} {} {})", self.kind, self.feature, self.body, self.name)
    }
}

#[starlark_value(type = "Ref")]
impl<'v> StarlarkValue<'v> for KRef {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "feature" => Some(heap.alloc(self.feature.as_str())),
            "body" => Some(heap.alloc(self.body.as_str())),
            "kind" => Some(heap.alloc(self.kind.as_str())),
            "name" => Some(heap.alloc(self.name.as_str())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["feature", "body", "kind", "name"].iter().map(|s| s.to_string()).collect()
    }
}

// Silence "unused" for helpers only some builds use.
#[allow(dead_code)]
fn _unused(v: Value<'_>) -> anyhow::Result<[f64; 3]> {
    color(v)
}

/// What `import_report(body)` hands back: every number `import_mesh`
/// measured on the body's triangles, so a script can assert on
/// them. Read from the feature's recorded args, so it is the same on a memo
/// hit.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KImportReport {
    pub body: String,
    pub format: String,
    pub triangles_in: u32,
    pub vertices_in: u32,
    pub triangles: u32,
    pub vertices: u32,
    pub groups: u32,
    pub welded: u32,
    pub weld_pass: u32,
    pub collapsed: u32,
    pub degenerate: u32,
    pub shells: u32,
    pub open_edges: u32,
    pub non_manifold_edges: u32,
    pub flipped_edges: u32,
    pub crossings: u32,
    pub overlaps: u32,
    pub pairs_skipped: bool,
    pub folds: u32,
    pub closed: bool,
    pub oriented: bool,
    pub inverted: bool,
    pub solid: bool,
    pub genus: Option<i32>,
    pub volume: Option<f64>,
    pub area: f64,
    #[allocative(skip)]
    pub bbox: Option<(Vec3, Vec3)>,
    pub flipped_to_outward: bool,
    /// The file's own name for the body.
    pub object: String,
    /// The body carries a colour per vertex.
    pub vertex_colors: bool,
    pub summary: String,
}
starlark_simple_value!(KImportReport);

impl KImportReport {
    pub fn new(body: String, r: cadpole_import::mesh::Report) -> KImportReport {
        let n = |v: usize| v.min(u32::MAX as usize) as u32;
        KImportReport {
            body,
            summary: r.summary(),
            format: r.format,
            triangles_in: n(r.triangles_in),
            vertices_in: n(r.vertices_in),
            triangles: n(r.triangles),
            vertices: n(r.vertices),
            groups: n(r.groups),
            welded: n(r.welded),
            weld_pass: r.weld_pass as u32,
            collapsed: n(r.collapsed),
            degenerate: n(r.degenerate),
            shells: n(r.shells),
            open_edges: n(r.open_edges),
            non_manifold_edges: n(r.non_manifold_edges),
            flipped_edges: n(r.flipped_edges),
            crossings: n(r.crossings),
            overlaps: n(r.overlaps),
            pairs_skipped: r.pairs_skipped,
            folds: n(r.folds),
            closed: r.closed,
            oriented: r.oriented,
            inverted: r.inverted,
            solid: r.solid,
            genus: r.genus.map(|g| g.clamp(i32::MIN as i64, i32::MAX as i64) as i32),
            volume: r.volume,
            area: r.area,
            bbox: r.bbox.map(|(lo, hi)| (Vec3::new(lo[0], lo[1], lo[2]), Vec3::new(hi[0], hi[1], hi[2]))),
            flipped_to_outward: r.flipped_to_outward,
            object: r.object,
            vertex_colors: r.vertex_colors,
        }
    }
}

impl fmt::Display for KImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "import_report({}: {})", self.body, self.summary)
    }
}

const IMPORT_REPORT_ATTRS: &[&str] = &[
    "body",
    "format",
    "triangles_in",
    "vertices_in",
    "triangles",
    "vertices",
    "groups",
    "welded",
    "weld_pass",
    "collapsed",
    "degenerate",
    "shells",
    "open_edges",
    "non_manifold_edges",
    "flipped_edges",
    "crossings",
    "overlaps",
    "pairs_skipped",
    "folds",
    "closed",
    "oriented",
    "inverted",
    "solid",
    "genus",
    "volume",
    "area",
    "bbox",
    "flipped_to_outward",
    "chord_error",
    "object",
    "vertex_colors",
    "summary",
];

#[starlark_value(type = "ImportReport")]
impl<'v> StarlarkValue<'v> for KImportReport {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "body" => Some(heap.alloc(self.body.as_str())),
            "format" => Some(heap.alloc(self.format.as_str())),
            "triangles_in" => Some(heap.alloc(self.triangles_in as i32)),
            "vertices_in" => Some(heap.alloc(self.vertices_in as i32)),
            "triangles" => Some(heap.alloc(self.triangles as i32)),
            "vertices" => Some(heap.alloc(self.vertices as i32)),
            "groups" => Some(heap.alloc(self.groups as i32)),
            "welded" => Some(heap.alloc(self.welded as i32)),
            "weld_pass" => Some(heap.alloc(self.weld_pass as i32)),
            "collapsed" => Some(heap.alloc(self.collapsed as i32)),
            "degenerate" => Some(heap.alloc(self.degenerate as i32)),
            "shells" => Some(heap.alloc(self.shells as i32)),
            "open_edges" => Some(heap.alloc(self.open_edges as i32)),
            "non_manifold_edges" => Some(heap.alloc(self.non_manifold_edges as i32)),
            "flipped_edges" => Some(heap.alloc(self.flipped_edges as i32)),
            "crossings" => Some(heap.alloc(self.crossings as i32)),
            "overlaps" => Some(heap.alloc(self.overlaps as i32)),
            "pairs_skipped" => Some(heap.alloc(self.pairs_skipped)),
            "folds" => Some(heap.alloc(self.folds as i32)),
            "closed" => Some(heap.alloc(self.closed)),
            "oriented" => Some(heap.alloc(self.oriented)),
            "inverted" => Some(heap.alloc(self.inverted)),
            "solid" => Some(heap.alloc(self.solid)),
            "genus" => Some(match self.genus {
                Some(g) => heap.alloc(g),
                None => Value::new_none(),
            }),
            "volume" => Some(match self.volume {
                Some(v) => heap.alloc(v),
                None => Value::new_none(),
            }),
            "area" => Some(heap.alloc(self.area)),
            "bbox" => Some(match self.bbox {
                Some((lo, hi)) => heap.alloc((alloc_vec3(heap, lo), alloc_vec3(heap, hi))),
                None => Value::new_none(),
            }),
            "flipped_to_outward" => Some(heap.alloc(self.flipped_to_outward)),
            // The triangles are the geometry: nothing was approximated
            // here, so there is no chord error to report.
            "chord_error" => Some(Value::new_none()),
            "object" => Some(heap.alloc(self.object.as_str())),
            "vertex_colors" => Some(heap.alloc(self.vertex_colors)),
            "summary" => Some(heap.alloc(self.summary.as_str())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        IMPORT_REPORT_ATTRS.iter().map(|s| s.to_string()).collect()
    }
    fn has_attr(&self, attribute: &str, _heap: Heap<'v>) -> bool {
        IMPORT_REPORT_ATTRS.contains(&attribute)
    }
}
