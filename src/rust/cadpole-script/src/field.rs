//! The `field` namespace, the `Field` value, the `mesh` global, and the
//! field path of `fillet` and `chamfer` — `kernel="field"` — which the
//! `fillet` and `chamfer` modules reach through [`blend`]
//! (docs/starlark-api.md).
//!
//! A `Field` is a **value**, not a body. It records nothing in the
//! feature tree and has no id; it becomes geometry through exactly one call,
//! `mesh(field, resolution=)`, which adds a mesh body like any other.
//!
//! On that path `fillet` and `chamfer` are the same machinery pointed at an existing body:
//! the body is imported as a field, a blend correction is applied near the
//! selected edges only, and the result is contoured back to triangles. That
//! makes them approximate, and they say so — every call emits an info
//! diagnostic naming the resolution it used and the error it measured, and a
//! warning when the requested size does not fit the geometry.

use std::collections::BTreeMap;
use std::fmt;

use allocative::Allocative;
use cadpole_core::context::{fmt_num, Body, Diagnostic};
use cadpole_core::memo::BodyKey;
use cadpole_field::edges::BlendKind;
use cadpole_field::fillet::EdgeSelection;
use cadpole_field::mdc::ContourOptions;
use cadpole_field::Field;
use starlark::environment::{GlobalsBuilder, Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::float::UnpackFloat;
use starlark::values::{
    Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueLike,
};
use starlark::starlark_simple_value;
use starlark_derive::starlark_value;

use crate::check::{finite, vec3, VecArg};
use crate::store::{call_span, store};
use crate::selection::KSelection;
use crate::values::{KBody, KEdge, KFace};

/// An SDF expression. Immutable; every method returns a new one.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KField {
    #[allocative(skip)]
    pub field: Field,
    /// What built it, for `str()` and diagnostics.
    pub label: String,
}
starlark_simple_value!(KField);

impl fmt::Display for KField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "field({})", self.label)
    }
}

impl KField {
    fn new(field: Field, label: impl Into<String>) -> KField {
        KField { field, label: label.into() }
    }
}

#[starlark_value(type = "Field")]
impl<'v> StarlarkValue<'v> for KField {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("field_methods", field_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            // `None` for an unbounded expression (a bare half-space, an
            // inverted solid): the value None, not a missing attribute.
            "bounds" => Some(match self.field.bounds() {
                Some(b) => {
                    let lo = b.lo();
                    let hi = b.hi();
                    heap.alloc(((lo[0], lo[1], lo[2]), (hi[0], hi[1], hi[2])))
                }
                None => Value::new_none(),
            }),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        vec!["bounds".to_owned()]
    }
}

fn v3(v: &VecArg, what: &str) -> anyhow::Result<[f64; 3]> {
    let v = vec3(v, what)?;
    Ok([v.x, v.y, v.z])
}

/// A direction that has to be one: finite and not the zero vector, which has
/// no direction to normalise to.
fn direction(v: &VecArg, what: &str) -> anyhow::Result<[f64; 3]> {
    let d = v3(v, what)?;
    if d[0] == 0.0 && d[1] == 0.0 && d[2] == 0.0 {
        anyhow::bail!("{what} must be non-zero, got (0, 0, 0)");
    }
    Ok(d)
}

/// A blend size: 0 is the sharp boolean, negative is a mistake rather than
/// another way of writing its absolute value.
fn blend_size(v: f64, what: &str) -> anyhow::Result<f64> {
    let v = finite(v, what)?;
    if v < 0.0 {
        anyhow::bail!("{what} must be >= 0 (0 is the sharp boolean), got {v}");
    }
    Ok(v)
}

fn positive(v: f64, what: &str) -> anyhow::Result<f64> {
    if !(v > 0.0) || !v.is_finite() {
        anyhow::bail!("{what} must be a positive number of millimetres, got {v}");
    }
    Ok(v)
}

/// `field.*` — the primitives and sources a field is built from.
#[starlark_module]
pub(crate) fn field_namespace(builder: &mut GlobalsBuilder) {
    /// A sphere of radius `r`, centred on the origin.
    fn sphere(#[starlark(require = pos)] r: UnpackFloat) -> anyhow::Result<KField> {
        Ok(KField::new(Field::sphere(positive(r.0, "field.sphere radius")?), format!("sphere r={}", fmt_num(r.0))))
    }

    /// A box `w` by `d` by `h` centred on the origin, with every edge rounded
    /// by `round` (0 for sharp). `round` is clamped to half the smallest side.
    fn r#box(
        #[starlark(require = pos)] w: UnpackFloat,
        #[starlark(require = pos)] d: UnpackFloat,
        #[starlark(require = pos)] h: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.0))] round: UnpackFloat,
    ) -> anyhow::Result<KField> {
        let half = [
            positive(w.0, "field.box w")? / 2.0,
            positive(d.0, "field.box d")? / 2.0,
            positive(h.0, "field.box h")? / 2.0,
        ];
        Ok(KField::new(
            Field::boxed(half, finite(round.0, "field.box round")?),
            format!("box {}x{}x{} round={}", fmt_num(w.0), fmt_num(d.0), fmt_num(h.0), fmt_num(round.0)),
        ))
    }

    /// A cylinder of radius `r` and height `h` on the Z axis, centred on the
    /// origin, with its rims rounded by `round`.
    fn cylinder(
        #[starlark(require = pos)] r: UnpackFloat,
        #[starlark(require = pos)] h: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.0))] round: UnpackFloat,
    ) -> anyhow::Result<KField> {
        Ok(KField::new(
            Field::cylinder(
                positive(r.0, "field.cylinder r")?,
                positive(h.0, "field.cylinder h")?,
                finite(round.0, "field.cylinder round")?,
            ),
            format!("cylinder r={} h={}", fmt_num(r.0), fmt_num(h.0)),
        ))
    }

    /// A capped cone on the Z axis: radius `r1` at the bottom, `r2` at the
    /// top, height `h`, centred on the origin. Either radius may be 0.
    fn cone(
        #[starlark(require = pos)] r1: UnpackFloat,
        #[starlark(require = pos)] r2: UnpackFloat,
        #[starlark(require = pos)] h: UnpackFloat,
    ) -> anyhow::Result<KField> {
        finite(r1.0, "field.cone r1")?;
        finite(r2.0, "field.cone r2")?;
        if r1.0 < 0.0 || r2.0 < 0.0 || r1.0.max(r2.0) <= 0.0 {
            anyhow::bail!("field.cone: radii must be >= 0 and not both 0, got {} and {}", r1.0, r2.0);
        }
        Ok(KField::new(
            Field::cone(r1.0, r2.0, positive(h.0, "field.cone h")?),
            format!("cone {}->{} h={}", fmt_num(r1.0), fmt_num(r2.0), fmt_num(h.0)),
        ))
    }

    /// A torus in the XY plane: `major` is the ring radius, `minor` the tube.
    fn torus(
        #[starlark(require = pos)] major: UnpackFloat,
        #[starlark(require = pos)] minor: UnpackFloat,
    ) -> anyhow::Result<KField> {
        Ok(KField::new(
            Field::torus(positive(major.0, "field.torus major")?, positive(minor.0, "field.torus minor")?),
            format!("torus R={} r={}", fmt_num(major.0), fmt_num(minor.0)),
        ))
    }

    /// A capsule: a cylinder of radius `r` from `a` to `b` with hemispherical
    /// ends.
    fn capsule(
        #[starlark(require = pos)] a: VecArg,
        #[starlark(require = pos)] b: VecArg,
        #[starlark(require = pos)] r: UnpackFloat,
    ) -> anyhow::Result<KField> {
        Ok(KField::new(
            Field::capsule(v3(&a, "a")?, v3(&b, "b")?, positive(r.0, "field.capsule r")?),
            "capsule".to_owned(),
        ))
    }

    /// The half-space on the far side of the plane through `origin` with
    /// outward normal `normal`. Unbounded on its own: intersect it with
    /// something finite before meshing.
    fn half_space(
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] normal: Option<VecArg>,
    ) -> anyhow::Result<KField> {
        let o = origin.as_ref().map(|v| v3(v, "origin")).transpose()?.unwrap_or([0.0; 3]);
        let n = normal.as_ref().map(|v| direction(v, "field.half_space normal")).transpose()?.unwrap_or([0.0, 0.0, 1.0]);
        Ok(KField::new(Field::plane(o, n), "half_space".to_owned()))
    }

    /// Deterministic value noise, for `displace`. `scale` is the feature size
    /// in mm, `amplitude` the peak displacement, `octaves` how many halvings
    /// of scale and amplitude to sum. The same `seed` always gives the same
    /// field, on every machine.
    fn noise(
        #[starlark(require = named, default = UnpackFloat(10.0))] scale: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(1.0))] amplitude: UnpackFloat,
        #[starlark(require = named, default = 0)] seed: i32,
        #[starlark(require = named, default = 1)] octaves: i32,
    ) -> anyhow::Result<KField> {
        if !(1..=8).contains(&octaves) {
            anyhow::bail!("field.noise: octaves must be between 1 and 8, got {octaves}");
        }
        Ok(KField::new(
            Field::noise(
                positive(scale.0, "field.noise scale")?,
                finite(amplitude.0, "field.noise amplitude")?,
                seed as u32,
                octaves as u32,
            ),
            format!("noise scale={} seed={seed}", fmt_num(scale.0)),
        ))
    }

    /// Import an existing body as a field, by signed distance to its
    /// triangles (BVH plus angle-weighted pseudo-normals). A B-rep body is
    /// tessellated at the document tolerance first, so the field is
    /// ε-approximate in exactly that sense. The body must be a solid (closed,
    /// wound outward, not passing through itself); anything else is an error
    /// that names what is wrong. `import_mesh(..., repair="weld")` fixes
    /// welds and inside-out shells.
    fn from_body<'v>(
        #[starlark(require = pos)] body: &KBody,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KField> {
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let (mesh, _) = ctx.body_mesh(body.body_id())?;
        let f = cadpole_field::field_from_mesh(mesh).map_err(|e| anyhow::anyhow!("field.from_body: {}: {e}", body.name))?;
        Ok(KField::new(f, format!("body {}", body.name)))
    }
}

#[starlark_module]
fn field_methods(builder: &mut MethodsBuilder) {
    /// Exact union: the field of both solids together.
    fn union(this: &KField, #[starlark(require = pos)] other: &KField) -> anyhow::Result<KField> {
        Ok(KField::new(this.field.union(&other.field), format!("{} u {}", this.label, other.label)))
    }
    /// Exact intersection.
    fn intersect(this: &KField, #[starlark(require = pos)] other: &KField) -> anyhow::Result<KField> {
        Ok(KField::new(this.field.intersect(&other.field), format!("{} n {}", this.label, other.label)))
    }
    /// Exact difference: `this` minus `other`.
    fn subtract(this: &KField, #[starlark(require = pos)] other: &KField) -> anyhow::Result<KField> {
        Ok(KField::new(this.field.subtract(&other.field), format!("{} - {}", this.label, other.label)))
    }

    /// Union with a blend of radius `k` millimetres along the seam. On a pair
    /// of planar faces this is the exact rolling-ball fillet, not an
    /// approximation of one -- which is why building a shape from fields
    /// gives fillets for free.
    fn smooth_union(
        this: &KField,
        #[starlark(require = pos)] other: &KField,
        #[starlark(require = pos)] k: UnpackFloat,
    ) -> anyhow::Result<KField> {
        let k = blend_size(k.0, "smooth_union k")?;
        Ok(KField::new(this.field.smooth_union(&other.field, k), format!("{} ~u {}", this.label, other.label)))
    }
    /// Intersection with a blend of radius `k`.
    fn smooth_intersect(
        this: &KField,
        #[starlark(require = pos)] other: &KField,
        #[starlark(require = pos)] k: UnpackFloat,
    ) -> anyhow::Result<KField> {
        let k = blend_size(k.0, "smooth_intersect k")?;
        Ok(KField::new(this.field.smooth_intersect(&other.field, k), format!("{} ~n {}", this.label, other.label)))
    }
    /// Difference with a blend of radius `k` where the cut meets the surface.
    fn smooth_subtract(
        this: &KField,
        #[starlark(require = pos)] other: &KField,
        #[starlark(require = pos)] k: UnpackFloat,
    ) -> anyhow::Result<KField> {
        let k = blend_size(k.0, "smooth_subtract k")?;
        Ok(KField::new(this.field.smooth_subtract(&other.field, k), format!("{} ~- {}", this.label, other.label)))
    }

    /// Union with a 45-degree chamfer of setback `d` along the seam.
    fn chamfer_union(
        this: &KField,
        #[starlark(require = pos)] other: &KField,
        #[starlark(require = pos)] d: UnpackFloat,
    ) -> anyhow::Result<KField> {
        let d = blend_size(d.0, "chamfer_union d")?;
        Ok(KField::new(this.field.chamfer_union(&other.field, d), format!("{} +u {}", this.label, other.label)))
    }
    /// Intersection with a 45-degree chamfer of setback `d`.
    fn chamfer_intersect(
        this: &KField,
        #[starlark(require = pos)] other: &KField,
        #[starlark(require = pos)] d: UnpackFloat,
    ) -> anyhow::Result<KField> {
        let d = blend_size(d.0, "chamfer_intersect d")?;
        Ok(KField::new(this.field.chamfer_intersect(&other.field, d), format!("{} +n {}", this.label, other.label)))
    }
    /// Difference with a 45-degree chamfer of setback `d`.
    fn chamfer_subtract(
        this: &KField,
        #[starlark(require = pos)] other: &KField,
        #[starlark(require = pos)] d: UnpackFloat,
    ) -> anyhow::Result<KField> {
        let d = blend_size(d.0, "chamfer_subtract d")?;
        Ok(KField::new(this.field.chamfer_subtract(&other.field, d), format!("{} +- {}", this.label, other.label)))
    }

    /// Grow the solid by `d` millimetres in every direction (negative shrinks).
    fn offset(this: &KField, #[starlark(require = pos)] d: UnpackFloat) -> anyhow::Result<KField> {
        let d = finite(d.0, "offset d")?;
        Ok(KField::new(this.field.offset(d), format!("{} offset {}", this.label, fmt_num(d))))
    }

    /// Hollow the solid into a wall of thickness `t`, centred on its surface.
    fn shell(this: &KField, #[starlark(require = pos)] t: UnpackFloat) -> anyhow::Result<KField> {
        Ok(KField::new(this.field.shell(positive(t.0, "shell thickness")?), format!("{} shell {}", this.label, fmt_num(t.0))))
    }

    /// Swap inside for outside.
    fn invert(this: &KField) -> anyhow::Result<KField> {
        Ok(KField::new(this.field.negate(), format!("!{}", this.label)))
    }

    fn translate(this: &KField, #[starlark(require = pos)] offset: VecArg) -> anyhow::Result<KField> {
        Ok(KField::new(this.field.translate(v3(&offset, "offset")?), this.label.clone()))
    }

    /// Rotate by `angle` degrees about `axis` through the origin.
    fn rotate(
        this: &KField,
        #[starlark(require = pos)] angle: UnpackFloat,
        #[starlark(require = named)] axis: Option<VecArg>,
    ) -> anyhow::Result<KField> {
        let axis = axis.as_ref().map(|a| direction(a, "rotate axis")).transpose()?.unwrap_or([0.0, 0.0, 1.0]);
        let angle = finite(angle.0, "rotate angle")?;
        Ok(KField::new(this.field.rotate(axis, angle.to_radians()), this.label.clone()))
    }

    /// Scale uniformly about the origin. Non-uniform scaling is not offered:
    /// it does not preserve distance, so the result would not be a distance
    /// field and the mesher's bounds would stop being sound.
    fn scale(this: &KField, #[starlark(require = pos)] s: UnpackFloat) -> anyhow::Result<KField> {
        if s.0 == 0.0 || !s.0.is_finite() {
            anyhow::bail!("scale factor must be non-zero and finite, got {}", s.0);
        }
        Ok(KField::new(this.field.scale(s.0), this.label.clone()))
    }

    /// Mirror across the plane through the origin with normal `normal`.
    fn mirror(this: &KField, #[starlark(require = named)] normal: Option<VecArg>) -> anyhow::Result<KField> {
        let n = normal.as_ref().map(|v| direction(v, "mirror normal")).transpose()?.unwrap_or([1.0, 0.0, 0.0]);
        Ok(KField::new(this.field.mirror(n), this.label.clone()))
    }

    /// Twist about the Z axis by `degrees_per_mm` of height.
    fn twist(this: &KField, #[starlark(require = pos)] degrees_per_mm: UnpackFloat) -> anyhow::Result<KField> {
        let rate = finite(degrees_per_mm.0, "twist degrees_per_mm")?;
        Ok(KField::new(this.field.twist(rate.to_radians()), format!("{} twisted", this.label)))
    }

    /// Scale x and y linearly with z: `bottom` at `z0`, `top` at `z1`, held
    /// constant outside that range.
    fn taper(
        this: &KField,
        #[starlark(require = pos)] z0: UnpackFloat,
        #[starlark(require = pos)] z1: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(1.0))] bottom: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(1.0))] top: UnpackFloat,
    ) -> anyhow::Result<KField> {
        let z0 = finite(z0.0, "taper z0")?;
        let z1 = finite(z1.0, "taper z1")?;
        if z0 == z1 {
            anyhow::bail!("taper: z0 and z1 must differ (both are {z0}): there is no height to go from bottom to top over");
        }
        let bottom = positive(bottom.0, "taper bottom")?;
        let top = positive(top.0, "taper top")?;
        Ok(KField::new(this.field.taper(z0, z1, bottom, top), format!("{} tapered", this.label)))
    }

    /// Push the surface along its own normal by `strength` times `source`,
    /// which is usually `field.noise(...)`. The result is still meshable but
    /// is no longer an exact distance field, so its bounds are grown by the
    /// displacement.
    fn displace(
        this: &KField,
        #[starlark(require = pos)] source: &KField,
        #[starlark(require = named, default = UnpackFloat(1.0))] strength: UnpackFloat,
    ) -> anyhow::Result<KField> {
        Ok(KField::new(
            this.field.displace(&source.field, finite(strength.0, "displace strength")?),
            format!("{} displaced", this.label),
        ))
    }

    /// Mesh this field. Identical to the `mesh(field, ...)` global.
    fn mesh<'v>(
        this: &KField,
        #[starlark(require = named)] resolution: UnpackFloat,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named, default = true)] simplify: bool,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        mesh_field_value(this, resolution.0, name, simplify, layer, eval)
    }
}

/// Shared by the `mesh()` global and `Field.mesh()`.
pub(crate) fn mesh_field_value<'v>(
    f: &KField,
    resolution: f64,
    name: Option<&str>,
    simplify: bool,
    layer: Option<Value<'v>>,
    eval: &Evaluator<'v, '_, '_>,
) -> anyhow::Result<KBody> {
    let resolution = positive(resolution, "mesh resolution")?;
    let span = call_span(eval);
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    // `simplify` maps onto the contouring error threshold: a quarter of a
    // cell, which on a flat face removes most of the triangles and leaves the
    // volume within a fraction of a percent (cadpole-field's adaptive test).
    let error = if simplify { resolution * 0.25 } else { 0.0 };
    let group = name.unwrap_or("surface").to_owned();
    let opts = ContourOptions { resolution, error, ..Default::default() };
    let (mesh, topo, stats) = cadpole_field::mesh_field(&f.field, &opts, &group)
        .map_err(|e| anyhow::anyhow!("mesh: {e}"))?;
    let mut args = BTreeMap::new();
    args.insert("resolution".into(), fmt_num(resolution));
    args.insert("field".into(), f.label.clone());
    args.insert("simplify".into(), simplify.to_string());
    args.insert("triangles".into(), stats.triangles.to_string());
    let fid = ctx.new_feature("mesh", name.map(str::to_owned), vec![], args, span.clone())?;
    let d = Diagnostic::info(format!(
        "mesh: {} triangles at resolution {} mm ({} surface cells of a {}^3 grid; {} culled by \
         interval arithmetic)",
        stats.triangles,
        fmt_num(resolution),
        stats.surface_cells,
        stats.grid[0],
        stats.nodes_culled
    ))
    .with_span(span)
    .with_feature(fid.clone());
    ctx.feature_mut(&fid)?.diagnostics.push(d.clone());
    ctx.diagnostics.push(d);
    // A field's contour is keyed by its content: hashing the
    // triangles is the cost, paid only when a memo is attached.
    let key = ctx.wants_keys().then(|| BodyKey::of_mesh(&mesh, &topo));
    let id = ctx.add_body(&fid, Body::Mesh((mesh, topo)), name.map(str::to_owned))?;
    if let Some(k) = key {
        ctx.set_body_key(id, k)?;
    }
    crate::layers::tag_ids(&mut ctx, layer, &[id], "mesh")?;
    KBody::from_ctx(&ctx, id)
}

/// Which body and which of its edges a `fillet`/`chamfer` call is about.
///
/// A `Selection`, a `Face`, an `Edge` and a name all say which edges; a `Body`
/// says which body. When no body was named outright, the entities say which
/// one they came from (they all carry a body id), so
/// `fillet(part.edges(on_plane=XY), 2)` needs no second argument. Selections
/// from two different bodies are an error, not a guess.
pub(crate) fn resolve_selection<'v>(
    target: Value<'v>,
    edges: Option<Value<'v>>,
    heap: Heap<'v>,
    what: &str,
    eval: &Evaluator<'v, '_, '_>,
) -> anyhow::Result<(KBody, EdgeSelection)> {
    // Collect any explicit edge selection first.
    let mut names: Vec<String> = Vec::new();
    let mut body: Option<KBody> = None;
    let mut from: Vec<u32> = Vec::new();
    let add = |v: Value<'v>,
               names: &mut Vec<String>,
               body: &mut Option<KBody>,
               from: &mut Vec<u32>|
     -> anyhow::Result<()> {
        if let Some(e) = v.downcast_ref::<KEdge>() {
            names.push(e.name.clone());
            from.push(e.body);
            return Ok(());
        }
        if let Some(f) = v.downcast_ref::<KFace>() {
            names.extend(f.edges.iter().cloned());
            from.push(f.body);
            return Ok(());
        }
        if let Some(sel) = v.downcast_ref::<KSelection>() {
            if sel.what == "face" {
                names.extend(sel.faces.iter().flat_map(|f| f.edges.iter().cloned()));
            } else {
                names.extend(sel.names());
            }
            from.push(sel.body);
            return Ok(());
        }
        if let Some(s) = v.unpack_str() {
            names.push(s.to_owned());
            return Ok(());
        }
        if let Some(b) = v.downcast_ref::<KBody>() {
            *body = Some(b.clone());
            return Ok(());
        }
        anyhow::bail!(
            "{what}: expected a Body, Selection, Edge, Face or edge name, got {}",
            v.get_type()
        )
    };
    let visit = |v: Value<'v>,
                 names: &mut Vec<String>,
                 body: &mut Option<KBody>,
                 from: &mut Vec<u32>|
     -> anyhow::Result<()> {
        if v.get_type() == "list" || v.get_type() == "tuple" {
            for item in v.iterate(heap).map_err(|e| e.into_anyhow())? {
                add(item, names, body, from)?;
            }
            Ok(())
        } else {
            add(v, names, body, from)
        }
    };
    visit(target, &mut names, &mut body, &mut from)?;
    if let Some(e) = edges {
        visit(e, &mut names, &mut body, &mut from)?;
    }

    // Which body: the explicit one, or the one the edges came from.
    let body = match body {
        Some(b) => b,
        None => {
            from.sort_unstable();
            from.dedup();
            match from.as_slice() {
                [one] => {
                    let st = store(eval)?;
                    let ctx = st.ctx.borrow();
                    KBody::from_ctx(&ctx, cadpole_core::BodyId(*one))?
                }
                [] => anyhow::bail!(
                    "{what}: could not tell which body these edges belong to; pass the body as well, \
                     e.g. {what}(part, edges=part.edges(...), ...)"
                ),
                many => anyhow::bail!(
                    "{what}: the selected edges come from {} different bodies; \
                     fillet one body at a time",
                    many.len()
                ),
            }
        }
    };
    if names.is_empty() && edges.is_some() {
        anyhow::bail!(
            "{what}: edges= selected nothing. An empty selection is allowed, but rounding \
             nothing is not — check the selector, or guard it with len(...) > 0"
        );
    }
    let selection = if names.is_empty() { EdgeSelection::All } else { EdgeSelection::Named(names) };
    Ok((body, selection))
}

/// Shared implementation of `fillet` and `chamfer(kernel="field")`.
pub(crate) fn blend<'v>(
    kind_name: &'static str,
    target: Value<'v>,
    size: f64,
    edges: Option<Value<'v>>,
    resolution: Option<f64>,
    name: Option<&str>,
    eval: &Evaluator<'v, '_, '_>,
) -> anyhow::Result<KBody> {
    let heap = eval.heap();
    let (body, selection) = resolve_selection(target, edges, heap, kind_name, eval)?;
    let size = positive(size, &format!("{kind_name} {}", if kind_name == "fillet" { "radius" } else { "distance" }))?;
    let resolution = resolution.map(|r| positive(r, &format!("{kind_name} resolution"))).transpose()?;
    let kind = if kind_name == "fillet" {
        BlendKind::Fillet { radius: size }
    } else {
        BlendKind::Chamfer { distance: size }
    };
    let span = call_span(eval);
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();

    let bid = body.body_id();
    let (mesh, topo) = ctx.body_mesh(bid)?.clone();
    // The default resolution follows the blend size, not the part: an arc is
    // only as round as the cells that sample it.
    let diagonal = mesh
        .bbox()
        .map(|(lo, hi)| (hi - lo).length())
        .unwrap_or(10.0);
    let resolution = resolution.unwrap_or_else(|| cadpole_field::default_resolution(size, diagonal));
    let (out_mesh, out_topo, report) =
        cadpole_field::blend_body(&mesh, &topo, &selection, kind, Some(resolution), 0.0)
            .map_err(|e| anyhow::anyhow!("{kind_name}: {e}"))?;

    let mut args = BTreeMap::new();
    args.insert("body".into(), body.name.clone());
    args.insert(if kind_name == "fillet" { "radius".into() } else { "distance".into() }, fmt_num(size));
    args.insert("resolution".into(), fmt_num(resolution));
    args.insert("edges".into(), report.edges_selected.to_string());
    args.insert("kind".into(), "mesh".into());
    let inputs = vec![ctx.body(bid)?.feature.clone()];
    let fid = ctx.new_feature(kind_name, name.map(str::to_owned), inputs, args, span.clone())?;

    let note = |d: Diagnostic, ctx: &mut cadpole_core::Context| {
        let d = d.with_span(span.clone()).with_feature(fid.clone());
        if let Ok(f) = ctx.feature_mut(&fid) {
            f.diagnostics.push(d.clone());
        }
        ctx.diagnostics.push(d);
    };
    note(Diagnostic::info(report.summary()), &mut ctx);
    if !report.edges_skipped.is_empty() {
        note(
            Diagnostic::warning(format!(
                "{kind_name}: skipped {} of the selected edges: {}",
                report.edges_skipped.len(),
                report.edges_skipped.join(", ")
            )),
            &mut ctx,
        );
    }
    for (edge, fits) in &report.too_large {
        note(
            Diagnostic::warning(format!(
                "{kind_name}: {} mm does not fit at edge {edge}: the material there is only {} mm \
                 thick, so the blend is clipped. The largest size that fits is about {}.",
                fmt_num(size),
                fmt_num(*fits),
                fmt_num(*fits)
            )),
            &mut ctx,
        );
    }
    if !report.watertight || !report.vertex_manifold {
        anyhow::bail!(
            "{kind_name} produced a mesh that is not a closed 2-manifold (watertight: {}, \
             vertex-manifold: {}); try a finer resolution",
            report.watertight,
            report.vertex_manifold
        );
    }

    let material = ctx.body(bid)?.material.clone();
    ctx.consume_body(bid)?;
    let key = ctx.wants_keys().then(|| BodyKey::of_mesh(&out_mesh, &out_topo));
    let id = ctx.add_body(&fid, Body::Mesh((out_mesh, out_topo)), name.map(str::to_owned))?;
    ctx.body_mut(id)?.material = material;
    if let Some(k) = key {
        ctx.set_body_key(id, k)?;
    }
    KBody::from_ctx(&ctx, id)
}

/// The globals that live in the root namespace.
#[starlark_module]
pub(crate) fn field_globals(builder: &mut GlobalsBuilder) {
    /// Mesh a `Field` into a body (docs/starlark-api.md).
    ///
    /// This is the only way a field becomes geometry: a field is a value, so
    /// it has no id, is not in the feature tree and cannot be exported until
    /// it has been meshed. `resolution` is the cell size in millimetres at
    /// the finest octree level; `simplify` lets flat regions collapse to
    /// fewer triangles without opening the surface.
    ///
    /// The result is a mesh body (`body.kind == "mesh"`), watertight by
    /// construction, so it can be unioned with a B-rep body straight away.
    fn mesh<'v>(
        #[starlark(require = pos)] field: &KField,
        #[starlark(require = named)] resolution: UnpackFloat,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named, default = true)] simplify: bool,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        mesh_field_value(field, resolution.0, name, simplify, layer, eval)
    }

}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    #[test]
    fn an_unbounded_fields_bounds_are_none() {
        let ctx = run(concat!(
            "assert_eq(field.half_space().bounds, None)\n",
            "assert_eq(field.sphere(5).invert().bounds, None)\n",
            "assert_true(hasattr(field.half_space(), \"bounds\"))\n",
            "assert_eq(field.sphere(5).bounds, ((-5.0, -5.0, -5.0), (5.0, 5.0, 5.0)))\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// Degenerate arguments are refused by name, not reinterpreted: a zero
    /// axis is not Z, a negative blend is not its absolute value, and a
    /// taper over no height has no top.
    #[test]
    fn degenerate_field_arguments_are_errors() {
        let cases = [
            ("field.box(10, 6, 4).rotate(90, axis=(0, 0, 0))", "axis"),
            ("field.box(10, 6, 4).mirror(normal=(0, 0, 0))", "normal"),
            ("field.half_space(normal=(0, 0, 0))", "normal"),
            ("field.sphere(3).smooth_union(field.sphere(2), -2)", "smooth_union k"),
            ("field.sphere(3).smooth_intersect(field.sphere(2), -2)", "smooth_intersect k"),
            ("field.sphere(3).smooth_subtract(field.sphere(2), -2)", "smooth_subtract k"),
            ("field.sphere(3).chamfer_union(field.sphere(2), -1)", "chamfer_union d"),
            ("field.sphere(3).chamfer_intersect(field.sphere(2), -1)", "chamfer_intersect d"),
            ("field.sphere(3).chamfer_subtract(field.sphere(2), -1)", "chamfer_subtract d"),
            ("field.box(4, 4, 4).taper(3, 3, bottom=1, top=2)", "taper"),
        ];
        for (src, want) in cases {
            let ctx = run(&format!("f = {src}\n"));
            let e = errors(&ctx);
            assert!(e.iter().any(|m| m.contains(want)), "{src}: {e:?}");
        }
        // The good neighbours still work: a blend of 0 is the sharp boolean.
        let ctx = run(concat!(
            "f = field.box(10, 6, 4).rotate(90, axis=(0, 0, 2)).mirror(normal=(1, 0, 0))\n",
            "g = field.sphere(3).smooth_union(field.sphere(2), 0).chamfer_union(field.sphere(1), 0.5)\n",
            "h = field.box(4, 4, 4).taper(-2, 2, bottom=1, top=2)\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }
}
