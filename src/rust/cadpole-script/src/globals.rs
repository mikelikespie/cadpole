//! Global functions of the Cadpole Starlark API. Each function's doc comment is
//! the API documentation (see `api_doc`).

use std::collections::BTreeMap;

use cadpole_core::context::{fmt_num, fmt_vec};
use cadpole_core::geom::{Plane, Transform, Vec3};
use cadpole_core::context::SplitTool;
use cadpole_core::ids::BodyId;
use cadpole_core::kernel::{BoolOp, KernelChoice};
use cadpole_core::params::{ParamDecl, ParamValue};
use cadpole_core::sketch::{Loop, P2, Path, Sketch};
use starlark::environment::GlobalsBuilder;
use starlark::starlark_module;
use starlark::eval::Evaluator;
use starlark::values::float::UnpackFloat;
use starlark::values::list::UnpackList;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::none::{NoneOr, NoneType};
use starlark::values::tuple::UnpackTuple;
use starlark::values::{Heap, UnpackValue, Value, ValueLike};

// Every point and number a builtin takes comes through `check`, which
// refuses NaN and infinity with a diagnostic naming the argument.
use crate::check::{self, VecArg, num, opt_num, p2, vec3};
use crate::clearance::KClearance;
use crate::constrained::{self, KConstrained};
use crate::convert::alloc_vec3;
use crate::limits::Limits;
use crate::modeltest;
use crate::drawing::{KDrawing, OutputFile, target_of, unit_format};
use crate::store::{Store, call_span, store};
use crate::selection::KSelection;
use crate::values::{KAsset, KBody, KCurve, KFace, KPath, KPlane, KPrintReport, KPrinter, KRef, KSketch};

/// The largest magnitude below which every whole f64 is exact (2^53): the
/// range an int `param()` can take an override in.
const MAX_EXACT_INT: f64 = 9_007_199_254_740_992.0;

/// The unit format an `export_*` call writes in: its own `units=`, else the
/// document's `units(...)`, else millimetres. `units=None` is the
/// same as leaving it out.
fn export_units<'v>(units: Option<&str>, what: &str, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<cadpole_drawing::units::UnitFormat> {
    match units {
        Some(u) => unit_format(u, what),
        None => Ok(crate::units::document_format(&store(eval)?.ctx.borrow()).unwrap_or_else(cadpole_drawing::units::UnitFormat::mm)),
    }
}

/// Accept an int or an integral float where a count is expected.
pub(crate) fn int_arg(v: UnpackFloat, what: &str) -> anyhow::Result<i32> {
    if v.0.fract() != 0.0 || v.0.abs() > i32::MAX as f64 {
        anyhow::bail!("{what} must be a whole number, got {}", v.0);
    }
    Ok(v.0 as i32)
}

/// `|x - y| <= max(tol, rel * |y|)`: the one closeness rule every numeric
/// assertion uses, so `tol=` and `rel=` mean the same thing in each.
fn close(x: f64, y: f64, tol: f64, rel: f64) -> bool {
    (x - y).abs() <= tol.max(rel * y.abs())
}

/// Flatten a number, or a list or tuple of numbers to any depth, into `out`,
/// so two `bbox()` results compare element by element.
fn numbers_of<'v>(v: Value<'v>, out: &mut Vec<f64>, what: &str) -> anyhow::Result<()> {
    if let Some(n) = UnpackFloat::unpack_value(v).map_err(|e| e.into_anyhow())? {
        out.push(check::finite(n.0, what)?);
        return Ok(());
    }
    if let Some(items) = <UnpackListOrTuple<Value<'v>>>::unpack_value(v).map_err(|e| e.into_anyhow())? {
        for item in items.items {
            numbers_of(item, out, what)?;
        }
        return Ok(());
    }
    anyhow::bail!("{what} must be a number or a list/tuple of numbers, got {}", v.get_type())
}

/// `bed=(x, y)` or `(x, y, z)`, positive millimetres; a missing z is 0,
/// "the printer's height".
fn bed_of<'v>(v: Value<'v>, what: &str) -> anyhow::Result<[f64; 3]> {
    let mut n = Vec::new();
    numbers_of(v, &mut n, what)?;
    if !(n.len() == 2 || n.len() == 3) || n.iter().any(|x| *x <= 0.0) {
        anyhow::bail!("{what} takes (x, y) or (x, y, z) in mm, every one positive; got {n:?}");
    }
    Ok([n[0], n[1], n.get(2).copied().unwrap_or(0.0)])
}

pub(crate) fn bodies_value<'v>(heap: Heap<'v>, ctx: &cadpole_core::Context, ids: Vec<cadpole_core::BodyId>) -> anyhow::Result<Value<'v>> {
    let mut out = Vec::new();
    for id in ids {
        out.push(KBody::from_ctx(ctx, id)?);
    }
    if out.len() == 1 {
        Ok(heap.alloc(out.pop().unwrap()))
    } else {
        Ok(heap.alloc(out))
    }
}

fn new_sketch(
    plane: Plane,
    on: &str,
    name: Option<&str>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KSketch> {
    let span = call_span(eval);
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let mut args = BTreeMap::new();
    args.insert("plane".into(), format!("{}", KPlane(plane)));
    // The face the plane came from, so `ExplainRef` and the feature tree can
    // say "this sketch sits on lip/end" rather than only where it happens to be.
    if !on.is_empty() {
        args.insert("on".into(), on.to_owned());
    }
    args.insert("loops".into(), "0".into());
    let fid = ctx.new_feature("sketch", name.map(str::to_owned), vec![], args, span)?;
    Ok(KSketch::new(fid.0, Sketch::new(plane)))
}

/// The plane a `sketch(on=...)` / `constrained_sketch(on=...)` call asked for,
/// and the name of the face it came from.
///
/// A planar face's sketch plane is defined in `cadpole_core::select::face_plane`:
/// the face's own normal, its centroid as the origin, and the canonical
/// in-surface frame (`naming::canonical_frame`) as the x direction, so a parameter change
/// moves the sketch but cannot spin it.
fn sketch_plane_on<'v>(v: Value<'v>) -> anyhow::Result<(Plane, String)> {
    if let Some(p) = v.downcast_ref::<KPlane>() {
        return Ok((p.0, String::new()));
    }
    let face = if let Some(f) = v.downcast_ref::<KFace>() {
        f.clone()
    } else if let Some(sel) = v.downcast_ref::<KSelection>() {
        if sel.what != "face" {
            anyhow::bail!("sketch(on=): a selection of edges cannot carry a sketch; select a face");
        }
        match sel.faces.len() {
            1 => sel.faces[0].clone(),
            0 => anyhow::bail!("sketch(on=): {} matched no faces", sel.query),
            n => anyhow::bail!(
                "sketch(on=): {} matched {n} faces ({}); narrow it, or pick one with .one() / .largest()",
                sel.query,
                crate::selection::listing(&sel.names())
            ),
        }
    } else {
        anyhow::bail!(
            "sketch(on=): expected a Face, a Plane, or a Selection holding one face, got {}",
            v.get_type()
        );
    };
    match cadpole_core::select::face_plane(&face.kind, face.normal, face.centroid) {
        Ok(p) => Ok((p, face.name.clone())),
        Err(cadpole_core::select::PlaneError::NotPlanar(kind)) => anyhow::bail!(
            "sketch(on={:?}): that face is a {kind} surface, not a plane; a sketch needs a planar \
             face. Sketch on a plane through it instead, or pick a planar face",
            face.name
        ),
        Err(cadpole_core::select::PlaneError::Degenerate) => {
            anyhow::bail!("sketch(on={:?}): that face has no usable normal", face.name)
        }
    }
}

/// Resolve the plane arguments every sketch constructor shares.
fn sketch_plane<'v>(
    plane: Option<&KPlane>,
    on: Option<Value<'v>>,
    origin: Option<VecArg>,
    x_dir: Option<VecArg>,
    what: &str,
) -> anyhow::Result<(Plane, String)> {
    let (base, on_name) = match (plane, on) {
        (Some(_), Some(_)) => {
            anyhow::bail!("{what}: pass a plane or on=, not both")
        }
        (None, Some(v)) => sketch_plane_on(v)?,
        (Some(p), None) => (p.0, String::new()),
        (None, None) => (Plane::XY, String::new()),
    };
    if origin.is_none() && x_dir.is_none() {
        return Ok((base, on_name));
    }
    let n = base.normal;
    let org = match origin {
        // Projected onto the plane: an origin off the plane is a typo, not a
        // request to move the plane.
        Some(v) => {
            let p = vec3(&v, "origin")?;
            p - n * (p - base.origin).dot(n)
        }
        None => base.origin,
    };
    let xd = match x_dir {
        Some(v) => vec3(&v, "x_dir")?,
        None => base.x_dir,
    };
    Ok((Plane::new(org, n, xd)?, on_name))
}

/// Resolve a sketch argument for an operation that consumes geometry.
///
/// Either kind of sketch is accepted: the literal one, whose loops are already
/// coordinates, or a constrained one, which is solved here (and only here —
/// building it was lazy) and then behaves exactly like the literal one.
pub(crate) fn sketch_arg<'v>(
    v: Value<'v>,
    what: &str,
    eval: &Evaluator<'v, '_, '_>,
) -> anyhow::Result<(cadpole_core::FeatureId, Sketch)> {
    if let Some((feature, sketch)) = constrained::as_solved_sketch(v, eval, what)? {
        return Ok((cadpole_core::FeatureId(feature), sketch));
    }
    // An imported drawing is its sketch with the defaults: every
    // layer the import kept, loops and construction paths, on XY, in the
    // drawing's own coordinates.
    if let Some(d) = v.downcast_ref::<crate::import2d::KDrawing2D>() {
        let (sketch, _) = d.to_sketch(&[], None, Plane::XY, P2::new(0.0, 0.0), "none", cadpole_import::dxf::DEFAULT_TOLERANCE)?;
        return Ok((cadpole_core::FeatureId(d.feature.clone()), sketch));
    }
    match v.downcast_ref::<KSketch>() {
        Some(s) => Ok((cadpole_core::FeatureId(s.feature.clone()), s.sketch())),
        None => anyhow::bail!("{what}: expected a Sketch, a ConstrainedSketch or a Drawing2D, got {}", v.get_type()),
    }
}

/// Resolve one `loft()` section: a `Sketch`, a `ConstrainedSketch`, a closed
/// `Curve`, or a 3-number tuple/list, which is a *point* — the tip the loft
/// ends in.
fn loft_section<'v>(
    v: Value<'v>,
    i: usize,
    eval: &Evaluator<'v, '_, '_>,
) -> anyhow::Result<(Option<cadpole_core::FeatureId>, cadpole_core::loft::Section)> {
    if let Some(c) = v.downcast_ref::<KCurve>() {
        if !c.curve.closed {
            anyhow::bail!(
                "loft: section {i} is an open curve ({} segments, ending at ({}, {}, {})): close it \
                 with .close() first. A Cadpole body is a solid, so every section must be a closed loop; \
                 open sections would make a surface, which Cadpole does not have",
                c.curve.segments.len(),
                fmt_num(c.curve.end().x),
                fmt_num(c.curve.end().y),
                fmt_num(c.curve.end().z)
            );
        }
        return Ok((Some(cadpole_core::FeatureId(c.feature.clone())), cadpole_core::loft::Section::Curve(c.curve.clone())));
    }
    if v.get_type() == "list" || v.get_type() == "tuple" {
        let arg: VecArg = starlark::values::UnpackValue::unpack_value(v)
            .map_err(|e| e.into_anyhow())?
            .ok_or_else(|| anyhow::anyhow!("loft: section {i} must be a Sketch, a closed Curve or a point (x, y, z)"))?;
        let p = vec3(&arg, &format!("loft: section {i}"))?;
        return Ok((None, cadpole_core::loft::Section::Point(p)));
    }
    let (fid, sketch) = sketch_arg(v, &format!("loft: section {i}"), eval)?;
    Ok((Some(fid), cadpole_core::loft::Section::Profile(sketch)))
}

/// The `cap=` argument of `loft`: one spelling for both ends, or a 2-tuple
/// (or list) giving the start and the end.
fn loft_caps<'v>(v: Option<Value<'v>>) -> anyhow::Result<[cadpole_core::loft::Cap; 2]> {
    use cadpole_core::loft::Cap;
    let Some(v) = v else { return Ok([Cap::Plane, Cap::Plane]) };
    if let Some(s) = v.unpack_str() {
        let c = Cap::parse(s)?;
        return Ok([c, c]);
    }
    if let Some(items) = <UnpackListOrTuple<Value<'v>>>::unpack_value(v).map_err(|e| e.into_anyhow())? {
        let strs: Vec<&str> = items.items.iter().filter_map(|i| i.unpack_str()).collect();
        if strs.len() == 2 && items.items.len() == 2 {
            return Ok([Cap::parse(strs[0])?, Cap::parse(strs[1])?]);
        }
        anyhow::bail!(
            "loft: cap= takes one spelling for both ends (\"plane\" or \"ruled\") or a 2-tuple of them \
             for the start and the end, got {} item(s)",
            items.items.len()
        );
    }
    anyhow::bail!(
        "loft: cap= must be \"plane\", \"ruled\" or a 2-tuple of those for the start and the end, got {}",
        v.get_type()
    )
}

/// Flatten positional boolean operands: each may be a Body or a list/tuple
/// of Bodies (so `subtract(base, holes, name=...)` works; starlark-rust does
/// not allow named arguments after `*args`).
pub(crate) fn collect_bodies<'v>(args: &[Value<'v>], heap: Heap<'v>, what: &str) -> anyhow::Result<Vec<KBody>> {
    let mut out = Vec::new();
    for v in args {
        // A Body, or anything that stands for bodies: an Instance's posed
        // copies, every instance of an Assembly, a Part's own.
        if let Some(bs) = crate::assembly::bodies_of(*v) {
            out.extend(bs);
        } else if v.get_type() == "list" || v.get_type() == "tuple" {
            for item in v.iterate(heap).map_err(|e| e.into_anyhow())? {
                match crate::assembly::bodies_of(item) {
                    Some(bs) => out.extend(bs),
                    None => anyhow::bail!("{what}: expected Body values, got {} in a list", item.get_type()),
                }
            }
        } else {
            anyhow::bail!("{what}: expected a Body, Instance or Assembly, or a list of them, got {}", v.get_type());
        }
    }
    Ok(out)
}

/// Shared body of `pattern_linear` / `pattern_circular`: a list of bodies, or
/// one fused body when `merge` is set.
#[allow(clippy::too_many_arguments)]
fn pattern<'v>(
    body: &KBody,
    kind: &str,
    transforms: &[Transform],
    merge: bool,
    kernel: Option<&str>,
    args: BTreeMap<String, String>,
    name: Option<&str>,
    heap: Heap<'v>,
    eval: &Evaluator<'v, '_, '_>,
) -> anyhow::Result<Value<'v>> {
    let choice = match kernel {
        Some(k) => KernelChoice::parse(k)?,
        None => KernelChoice::Auto,
    };
    let span = call_span(eval);
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let ids =
        ctx.pattern(kind, body.body_id(), transforms, merge, choice, args, name.map(str::to_owned), span)?;
    if merge {
        return Ok(heap.alloc(KBody::from_ctx(&ctx, ids[0])?));
    }
    // Without merge the shape of the result is a list, even of one: scripts
    // iterate it.
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(KBody::from_ctx(&ctx, id)?);
    }
    Ok(heap.alloc(out))
}

fn boolean<'v>(
    op: BoolOp,
    bodies: &[KBody],
    name: Option<&str>,
    kernel: Option<&str>,
    eval: &Evaluator<'v, '_, '_>,
) -> anyhow::Result<KBody> {
    if bodies.len() < 2 {
        anyhow::bail!("{} needs at least two bodies", op.kind_name());
    }
    let choice = match kernel {
        Some(k) => KernelChoice::parse(k)?,
        None => KernelChoice::Auto,
    };
    let span = call_span(eval);
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let others: Vec<_> = bodies[1..].iter().map(|b| b.body_id()).collect();
    let id = ctx.boolean(op, bodies[0].body_id(), &others, choice, name.map(str::to_owned), span)?;
    KBody::from_ctx(&ctx, id)
}

/// The printer profile a print call should use: the named machine, if any,
/// with every explicitly-passed argument layered on top.
///
/// The layering is what makes `printer=` a *starting point* rather than a
/// mode: naming a machine and then passing `nozzle=0.6` means that machine
/// with a 0.6 mm nozzle, and `min_wall` follows the nozzle unless it is also
/// spelled out.
#[allow(clippy::too_many_arguments)]
/// The chord-error-vs-gap finding for one body (`cadpole_print::chord`),
/// when the document has another body close enough for the rule to trip:
/// what `print_check` adds to its report and `assert_printable` fails on,
/// so a model test sees an export that would eat a gap.
fn chord_issue_for(
    ctx: &mut cadpole_core::Context,
    id: BodyId,
    fraction: f64,
) -> anyhow::Result<Option<cadpole_print::Issue>> {
    let issues = cadpole_print::chord::chord_issues(ctx, fraction)?;
    Ok(issues.into_iter().find(|(b, _)| *b == id).map(|(_, i)| i))
}

/// `max_bridge=`: the longest flat span held up at both ends that
/// counts as a bridge, in mm. Left out, Cadpole's default; 0 turns bridging off;
/// a negative number is a mistake.
fn bridge_limit(max_bridge: Option<UnpackFloat>, what: &str) -> anyhow::Result<f64> {
    match opt_num(&max_bridge, &format!("{what}: max_bridge"))? {
        None => Ok(cadpole_print::overhang::DEFAULT_MAX_BRIDGE),
        Some(m) if m < 0.0 => anyhow::bail!("{what}: max_bridge is a length in mm, 0 or more, got {m}"),
        Some(m) => Ok(m),
    }
}

/// The support rules a call may set (`cadpole_print::SupportRules`), each
/// `None` when it was not passed.
#[derive(Default)]
struct SupportArgs {
    max_overhang_area: Option<f64>,
    max_cantilever: Option<f64>,
    max_horizontal_hole: Option<f64>,
    min_island: Option<f64>,
    max_slenderness: Option<f64>,
    slender_width: Option<f64>,
    max_height_to_base: Option<f64>,
}

impl SupportArgs {
    #[allow(clippy::too_many_arguments)]
    fn new(
        what: &str,
        max_overhang_area: Option<UnpackFloat>,
        max_cantilever: Option<UnpackFloat>,
        max_horizontal_hole: Option<UnpackFloat>,
        min_island: Option<UnpackFloat>,
        max_slenderness: Option<UnpackFloat>,
        slender_width: Option<UnpackFloat>,
        max_height_to_base: Option<UnpackFloat>,
    ) -> anyhow::Result<SupportArgs> {
        let n = |v: Option<UnpackFloat>, k: &str| opt_num(&v, &format!("{what}: {k}"));
        Ok(SupportArgs {
            max_overhang_area: n(max_overhang_area, "max_overhang_area")?,
            max_cantilever: n(max_cantilever, "max_cantilever")?,
            max_horizontal_hole: n(max_horizontal_hole, "max_horizontal_hole")?,
            min_island: n(min_island, "min_island")?,
            max_slenderness: n(max_slenderness, "max_slenderness")?,
            slender_width: n(slender_width, "slender_width")?,
            max_height_to_base: n(max_height_to_base, "max_height_to_base")?,
        })
    }

    /// Layer what was passed over `p`'s rules, and check the result.
    fn apply(&self, what: &str, p: &mut cadpole_print::Profile) -> anyhow::Result<()> {
        let r = &mut p.support;
        for (v, slot) in [
            (self.max_overhang_area, &mut r.max_overhang_area),
            (self.max_cantilever, &mut r.max_cantilever),
            (self.max_horizontal_hole, &mut r.max_horizontal_hole),
            (self.min_island, &mut r.min_island),
            (self.max_slenderness, &mut r.max_slenderness),
            (self.slender_width, &mut r.slender_width),
            (self.max_height_to_base, &mut r.max_height_to_base),
        ] {
            if let Some(v) = v {
                *slot = v;
            }
        }
        p.validate().map_err(|e| anyhow::anyhow!("{what}: {e}"))
    }
}

fn resolve_profile(
    what: &str,
    printer: Option<Value<'_>>,
    min_wall: Option<UnpackFloat>,
    nozzle: Option<UnpackFloat>,
    perimeters: Option<UnpackFloat>,
    layer_height: Option<UnpackFloat>,
    material: Option<&str>,
    max_overhang: Option<UnpackFloat>,
    bed: Option<&VecArg>,
    bed_shape: Option<&str>,
    build_direction: Option<&VecArg>,
    min_gap: Option<UnpackFloat>,
) -> anyhow::Result<cadpole_print::Profile> {
    let mut p = match printer {
        None => cadpole_print::Profile::default(),
        Some(v) => {
            if let Some(k) = v.downcast_ref::<KPrinter>() {
                k.profile()
            } else if let Some(s) = v.unpack_str() {
                cadpole_print::Profile::named(s).map_err(|e| anyhow::anyhow!("{what}: {e}"))?
            } else {
                anyhow::bail!("{what}: printer= takes a name or a Printer, got {}", v.get_type());
            }
        }
    };
    let nozzle = opt_num(&nozzle, &format!("{what}: nozzle"))?;
    let perimeters = opt_num(&perimeters, &format!("{what}: perimeters"))?;
    let min_wall = opt_num(&min_wall, &format!("{what}: min_wall"))?;
    let layer_height = opt_num(&layer_height, &format!("{what}: layer_height"))?;
    let max_overhang = opt_num(&max_overhang, &format!("{what}: max_overhang"))?;
    let min_gap = opt_num(&min_gap, &format!("{what}: min_gap"))?;
    // Every value is taken as given and the whole profile checked below: a
    // nozzle of 0 or a build direction of (0, 0, 0) is a mistake to report,
    // not a value to ignore in favour of the default.
    if let Some(n) = min_gap {
        p.min_gap = n;
    }
    if let Some(n) = nozzle {
        p.nozzle = n;
        p.min_wall = n * p.perimeters;
    }
    if let Some(n) = perimeters {
        p.perimeters = n;
        p.min_wall = p.nozzle * n;
    }
    if let Some(n) = min_wall {
        p.min_wall = n;
    }
    if let Some(n) = layer_height {
        p.layer_height = n;
        // A tip within one layer is not an island, whatever the layer.
        p.support.min_island = n;
    }
    if let Some(m) = material {
        p.material = cadpole_print::Material::parse(m).ok_or_else(|| {
            anyhow::anyhow!("{what}: unknown material {m:?}; Cadpole knows {}", cadpole_print::Material::list())
        })?;
    }
    if let Some(n) = max_overhang {
        p.max_overhang_degrees = n;
    }
    if let Some(b) = bed {
        let v = vec3(b, "bed")?;
        if v.x <= 0.0 || v.y <= 0.0 || v.z <= 0.0 {
            anyhow::bail!("{what}: bed must be three positive numbers, got ({}, {}, {})", v.x, v.y, v.z);
        }
        p.bed = [v.x, v.y, v.z];
    }
    if let Some(s) = bed_shape {
        p.bed_shape = cadpole_print::BedShape::parse(s)
            .ok_or_else(|| anyhow::anyhow!("{what}: bed_shape must be \"rectangular\" or \"circular\", got {s:?}"))?;
    }
    if let Some(d) = build_direction {
        p.build_direction = vec3(d, "build_direction")?;
    }
    p.validate().map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
    Ok(p)
}

/// Resolve `name` in the document's asset table and return its bytes.
///
/// The two failure modes are deliberately distinct: a name that is not in the
/// table at all is a typo, and the diagnostic lists what *is* there; a name
/// that is in the table but whose bytes never arrived is a transport problem
/// (a share link without its assets, a wasm engine that has not been fed
/// `set_asset`), and the diagnostic says so.
pub(crate) fn asset_bytes(st: &Store, name: &str, what: &str) -> anyhow::Result<std::sync::Arc<[u8]>> {
    if let Some((_, bytes)) = st.assets.get(name) {
        return Ok(bytes);
    }
    match st.assets.record(name) {
        Some(a) => anyhow::bail!(
            "{what}: asset {name:?} (sha256 {}) is not loaded — its bytes did not travel with the \
             document",
            a.sha256.chars().take(12).collect::<String>()
        ),
        None => {
            let have = st.assets.names();
            anyhow::bail!(
                "{what}: no asset named {name:?} (available: {})",
                if have.is_empty() { "none".to_owned() } else { have.join(", ") }
            )
        }
    }
}

/// Alignment of imported geometry against `at`, shared by every importer.
fn placement(im: &cadpole_import_2d::Import, align: &str, at: P2) -> anyhow::Result<(f64, f64)> {
    match align {
        "none" => Ok((at.x, at.y)),
        "min" | "center" | "centre" => {
            let Some((lo, hi)) = im.bbox() else { return Ok((at.x, at.y)) };
            if align == "min" {
                Ok((at.x - lo.x, at.y - lo.y))
            } else {
                Ok((at.x - (lo.x + hi.x) / 2.0, at.y - (lo.y + hi.y) / 2.0))
            }
        }
        other => anyhow::bail!("align {other:?} must be \"min\", \"center\" or \"none\""),
    }
}

fn note_severity(level: cadpole_import_2d::Level) -> cadpole_core::context::Severity {
    match level {
        cadpole_import_2d::Level::Info => cadpole_core::context::Severity::Info,
        cadpole_import_2d::Level::Warning => cadpole_core::context::Severity::Warning,
    }
}

/// Record an import as a feature and hand back the sketch it produced.
///
/// Every loop and path keeps a fully qualified name
/// (`import_svg_1/badge/path[3]/loop[0]`), so a face extruded from an
/// imported loop traces back to an element of the file and survives
/// re-evaluation after the file changes elsewhere.
fn finish_import(
    kind: &str,
    mut im: cadpole_import_2d::Import,
    plane: Plane,
    at: P2,
    align: &str,
    mut args: BTreeMap<String, String>,
    name: Option<&str>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KSketch> {
    let (dx, dy) = placement(&im, align, at)?;
    if dx != 0.0 || dy != 0.0 {
        im.map_points(move |p| P2::new(p.x + dx, p.y + dy));
    }
    args.insert("loops".into(), im.loops.len().to_string());
    args.insert("paths".into(), im.paths.len().to_string());
    args.insert("plane".into(), format!("{}", KPlane(plane)));

    let span = call_span(eval);
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let fid = ctx.new_feature(kind, name.map(str::to_owned), vec![], args, span.clone())?;

    let mut sketch = Sketch::new(plane);
    for (n, l) in im.loops {
        sketch = sketch.with_named_loop(l, format!("{fid}/{n}"));
    }
    for (n, p) in im.paths {
        sketch = sketch.with_path(p, format!("{fid}/{n}"));
    }
    for note in im.notes {
        let d = cadpole_core::context::Diagnostic {
            severity: note_severity(note.level),
            message: note.message,
            span: span.clone(),
            feature: Some(fid.clone()),
        };
        if d.severity == cadpole_core::context::Severity::Warning {
            ctx.diagnostics.push(d.clone());
        }
        ctx.feature_mut(&fid)?.diagnostics.push(d);
    }
    Ok(KSketch::new(fid.0, sketch))
}

/// `["a", "b"]` from an optional list argument.
fn strings(v: Option<UnpackList<String>>) -> Vec<String> {
    v.map(|l| l.items).unwrap_or_default()
}

#[starlark_module]
pub(crate) fn cadpole_globals(builder: &mut GlobalsBuilder) {
    /// The XY plane: sketch x → world X, sketch y → world Y, normal +Z.
    const XY: KPlane = KPlane(Plane::XY);
    /// The XZ plane: sketch x → world X, sketch y → world Z, normal −Y.
    const XZ: KPlane = KPlane(Plane::XZ);
    /// The YZ plane: sketch x → world Y, sketch y → world Z, normal +X.
    const YZ: KPlane = KPlane(Plane::YZ);

    /// Declare a parameter and return its effective value. The document can
    /// override the default; the UI shows a control built from the metadata.
    /// `default` may be a number, bool, string or (x, y, z) tuple.
    fn param<'v>(
        #[starlark(require = pos)] name: &str,
        #[starlark(require = pos)] default: Value<'v>,
        #[starlark(require = named)] doc: Option<&str>,
        #[starlark(require = named)] unit: Option<&str>,
        #[starlark(require = named)] min: Option<UnpackFloat>,
        #[starlark(require = named)] max: Option<UnpackFloat>,
        #[starlark(require = named)] step: Option<UnpackFloat>,
        #[starlark(require = named)] choices: Option<UnpackList<String>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        // A library takes values; the document declares parameters.
        if let Some(at) = eval.call_stack_top_location()
            && crate::stdlib::is_library(at.filename())
        {
            anyhow::bail!(
                "param {name:?}: a library ({}) may not declare parameters; it takes values as arguments \
                 and the document that loads it declares the parameter",
                at.filename()
            );
        }
        let heap = eval.heap();
        let is_int = default.unpack_i32().is_some();
        let default_value = if let Some(b) = default.unpack_bool() {
            ParamValue::Bool(b)
        } else if let Some(s) = default.unpack_str() {
            ParamValue::Text(s.to_owned())
        } else if let Some(n) = starlark::values::UnpackValue::unpack_value(default).map_err(|e| e.into_anyhow())? {
            let n: UnpackFloat = n;
            ParamValue::Number(n.0)
        } else if let Some(v) = starlark::values::UnpackValue::unpack_value(default).map_err(|e| e.into_anyhow())? {
            let v: VecArg = v;
            // Exactly three: a 2-element default silently became (x, y, 0),
            // so `param("a", [1, 2])` produced a vector the author did not
            // write and the UI offered a z control for.
            if v.items.len() != 3 {
                anyhow::bail!(
                    "param {name:?}: a vector default needs 3 numbers (x, y, z), got {}",
                    v.items.len()
                );
            }
            ParamValue::Vector(vec3(&v, &format!("param {name:?}: default"))?)
        } else {
            anyhow::bail!("param {name:?}: default must be a number, bool, string or (x, y, z)");
        };
        let choices = choices.map(|c| c.items).unwrap_or_default();
        if !choices.is_empty() {
            match &default_value {
                ParamValue::Text(t) => {
                    if !choices.contains(t) {
                        anyhow::bail!("param {name:?}: default {t:?} is not one of choices {choices:?}");
                    }
                }
                // Anything else accepted `choices` and then ignored it, so the
                // UI was shown a set nothing was ever checked against.
                other => anyhow::bail!(
                    "param {name:?}: choices only apply to a string param, but the default is {}",
                    other.kind_name()
                ),
            }
        }
        let mut min = opt_num(&min, &format!("param {name:?}: min"))?;
        let mut max = opt_num(&max, &format!("param {name:?}: max"))?;
        let step = opt_num(&step, &format!("param {name:?}: step"))?;
        // A contradictory declaration clamped silently to `max` — 50 with
        // min=200, max=10 came out as 10 — which is nobody's intent.
        if let (Some(lo), Some(hi)) = (min, max) {
            if lo > hi {
                anyhow::bail!("param {name:?}: min {lo} is greater than max {hi}");
            }
        }
        // An int param keeps integer bounds, so it stays an int whatever the
        // override: `param("n", 4, min=2.5)` overridden with 1 must not clamp
        // to 2.5 and return a float, or the `range(n)` two lines later
        // fails with a type error pointing at neither. Fractional bounds on
        // an int are common and harmless (`min=0.5` on a radius of 4), so
        // they tighten to the nearest whole number inside themselves rather
        // than being refused.
        if is_int {
            min = min.map(f64::ceil);
            max = max.map(f64::floor);
            if let (Some(lo), Some(hi)) = (min, max) {
                if lo > hi {
                    anyhow::bail!(
                        "param {name:?}: no whole number lies between min and max, and the default \
                         is an int"
                    );
                }
            }
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let value = ctx.declare_param(ParamDecl {
            name: name.to_owned(),
            default: default_value,
            description: doc.unwrap_or("").to_owned(),
            unit: unit.unwrap_or("").to_owned(),
            min,
            max,
            step,
            choices,
            span,
        })?;
        Ok(match value {
            // Integer defaults stay integers so counts and indices work.
            ParamValue::Number(n) if is_int && n.fract() == 0.0 && n.abs() <= MAX_EXACT_INT => heap.alloc(n as i64),
            ParamValue::Number(n) => heap.alloc(n),
            ParamValue::Bool(b) => Value::new_bool(b),
            ParamValue::Text(s) => heap.alloc(s),
            ParamValue::Vector(v) => heap.alloc((v.x, v.y, v.z)),
        })
    }

    /// A plane from an origin, a normal and (optionally) the in-plane x direction.
    fn plane(
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] normal: Option<VecArg>,
        #[starlark(require = named)] x_dir: Option<VecArg>,
    ) -> anyhow::Result<KPlane> {
        let origin = origin.as_ref().map(|v| vec3(v, "plane: origin")).transpose()?.unwrap_or(Vec3::ZERO);
        let normal = normal.as_ref().map(|v| vec3(v, "plane: normal")).transpose()?.unwrap_or(Vec3::Z);
        let x_dir = match x_dir {
            Some(v) => vec3(&v, "plane: x_dir")?,
            None => {
                // Pick any direction not parallel to the normal.
                if normal.normalized().cross(Vec3::X).length() > 1e-6 { Vec3::X } else { Vec3::Y }
            }
        };
        Ok(KPlane(Plane::new(origin, normal, x_dir)?))
    }

    /// Start an open line/arc path at `start` (sketch coordinates); build it
    /// with `.line()`, `.h()`, `.v()`, `.arc(to, via=)`, `.tangent_arc()` and
    /// add it to a sketch with `sketch.add(path)`.
    fn path(#[starlark(require = pos)] start: VecArg) -> anyhow::Result<KPath> {
        Ok(KPath(Path::new(p2(&start, "start")?)))
    }

    /// Start a curve in space at `start`, a world point `(x, y, z)`; build it
    /// with `.line(to)`, `.line_by(delta)`, `.arc(to, via=)`,
    /// `.bezier(to, controls=)` and `.spline(points, tension=)` — `path()`'s
    /// vocabulary with 3D points — then `.close()` it to make a `loft`
    /// section that need not lie in any plane. A feature of kind `curve`.
    ///
    /// A `Curve` is the one curve type in the language: `Path.on(plane)`
    /// lifts a sketch path into one and `Sketch.curves()` gives a sketch's
    /// loops as them, so a loft can mix sketches and curves in one list.
    ///
    /// ```python
    /// # A blade section at the shroud, drawn from the middle of its trailing
    /// # edge: pressure flank, round nose, suction flank.
    /// tip = (curve(te)
    ///        .line(press[-1])
    ///        .spline(press[-2::-1])
    ///        .arc(suct[0], via=nose)
    ///        .spline(suct[1:])
    ///        .close())
    /// ```
    fn curve<'v>(
        #[starlark(require = pos)] start: VecArg,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KCurve> {
        let start = vec3(&start, "curve: start")?;
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut args = BTreeMap::new();
        args.insert("start".into(), fmt_vec(start));
        args.insert("segments".into(), "0".into());
        args.insert("closed".into(), "false".into());
        let fid = ctx.new_feature("curve", name.map(str::to_owned), vec![], args, span)?;
        Ok(KCurve { feature: fid.0, curve: cadpole_core::curve::Curve::new(start) })
    }

    /// Start a sketch on `plane` (default XY). Add loops with `.rect()`,
    /// `.circle()`, `.polygon()`, `.ngon()`; nested loops become holes.
    fn sketch<'v>(
        #[starlark(require = pos)] plane: Option<&KPlane>,
        #[starlark(require = named)] on: Option<Value<'v>>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] x_dir: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let (plane, on_name) = sketch_plane(plane, on, origin, x_dir, "sketch")?;
        new_sketch(plane, &on_name, name, eval)
    }

    /// Start a sketch built from dimensions instead of coordinates.
    ///
    /// Add named entities with `.point()`, `.line()`, `.arc()` and
    /// `.circle()`, relate them with `.constrain(kind, ...)`, and extrude or
    /// revolve the result: the solver works out the coordinates when the
    /// sketch is first used, and its answer is cached until the sketch
    /// changes. Under-constrained is allowed and reported; a redundant
    /// constraint is a warning naming it; contradictory constraints are an
    /// error naming both sides and the gap between them.
    ///
    /// ```python
    /// w = param("width", 60)
    /// s = constrained_sketch(XY, name="plate")
    /// a = s.point(0, 0, name="a", fix=True)
    /// b = s.point(w, 0, name="b")
    /// s.line(a, b, name="bottom")
    /// s.constrain("horizontal", "bottom")
    /// s.constrain("distance", a, b, w, name="width")
    /// ```
    fn constrained_sketch<'v>(
        #[starlark(require = pos)] plane: Option<&KPlane>,
        #[starlark(require = named)] on: Option<Value<'v>>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] x_dir: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KConstrained> {
        let (plane, _) = sketch_plane(plane, on, origin, x_dir, "constrained_sketch")?;
        constrained::new_constrained(plane, name, eval)
    }

    /// Extrude a sketch by `distance` along its plane normal (negative goes
    /// the other way). Returns a Body, or a list of Bodies if the sketch has
    /// several separate regions. `sketch` may be a literal `Sketch` or a
    /// `ConstrainedSketch`, which is solved at this point.
    fn extrude<'v>(
        #[starlark(require = pos)] sketch: Value<'v>,
        #[starlark(require = pos)] distance: UnpackFloat,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let distance = num(&distance, "extrude: distance")?;
        let span = call_span(eval);
        let (fid, sketch) = sketch_arg(sketch, "extrude", eval)?;
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = ctx.extrude("extrude", &sketch, Some(&fid), distance, BTreeMap::new(), name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "extrude")?;
        bodies_value(heap, &ctx, ids)
    }

    /// Revolve a sketch about the axis through `origin` along `axis` by
    /// `angle` degrees (default a full turn).
    fn revolve<'v>(
        #[starlark(require = pos)] sketch: Value<'v>,
        #[starlark(require = named)] axis: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named, default = UnpackFloat(360.0))] angle: UnpackFloat,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let axis = axis.as_ref().map(|a| vec3(a, "revolve: axis")).transpose()?.unwrap_or(Vec3::Z);
        let origin = origin.as_ref().map(|o| vec3(o, "revolve: origin")).transpose()?.unwrap_or(Vec3::ZERO);
        // A NaN angle or axis is the case that never returns: truck's sweep
        // spins forever and the deadline cannot interrupt it.
        let angle = num(&angle, "revolve: angle")?;
        if axis.length() == 0.0 {
            anyhow::bail!("revolve: axis must be non-zero");
        }
        let span = call_span(eval);
        let (fid, sketch) = sketch_arg(sketch, "revolve", eval)?;
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = ctx.revolve("revolve", &sketch, Some(&fid), origin, axis, angle.to_radians(), name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "revolve")?;
        bodies_value(heap, &ctx, ids)
    }

    /// A box of size `w` × `d` × `h` (x, y, z). By default its corner is at
    /// `at` (origin); with `center=True` it is centred there in x and y and
    /// sits on the plane z = at.z.
    fn r#box<'v>(
        #[starlark(require = pos)] w: UnpackFloat,
        #[starlark(require = pos)] d: UnpackFloat,
        #[starlark(require = pos)] h: UnpackFloat,
        #[starlark(require = named, default = false)] center: bool,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        num(&w, "box: w")?;
        num(&d, "box: d")?;
        num(&h, "box: h")?;
        if w.0 <= 0.0 || d.0 <= 0.0 || h.0 <= 0.0 {
            anyhow::bail!("box: all sizes must be positive");
        }
        let at = at.as_ref().map(|a| vec3(a, "box: at")).transpose()?.unwrap_or(Vec3::ZERO);
        let c = if center { P2::new(at.x, at.y) } else { P2::new(at.x + w.0 / 2.0, at.y + d.0 / 2.0) };
        let plane = Plane { origin: Vec3::new(0.0, 0.0, at.z), ..Plane::XY };
        let s = Sketch::new(plane).with_loop(Loop::rect(c, w.0, d.0));
        let mut args = BTreeMap::new();
        args.insert("size".into(), format!("({}, {}, {})", fmt_num(w.0), fmt_num(d.0), fmt_num(h.0)));
        args.insert("at".into(), fmt_vec(at));
        args.insert("center".into(), center.to_string());
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = ctx.extrude("box", &s, None, h.0, args, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "box")?;
        bodies_value(heap, &ctx, ids)
    }

    /// A solid through two or more sections: the Gridfinity foot, a
    /// stacking-lip socket, a wall-to-lip frustum, a lid cup — or a
    /// compressor blade between its hub outline and its shroud outline.
    ///
    /// `sections` are, in any mix: `Sketch`es (or `ConstrainedSketch`es),
    /// which must be parallel and listed in order along their normal; closed
    /// `Curve`s, which may lie anywhere in space, planar or not; and a
    /// point `(x, y, z)` as the first or last section, which ends the loft in
    /// a tip — a pyramid, or a cone. With any curve in the list, *list order
    /// is the order*: nothing is stacked along a normal, a sketch section is
    /// lifted onto its plane as drawn (so two sketches on planes that are not
    /// parallel loft as `a.curves()[0]` and `b.curves()[0]`), every section
    /// is one loop, and the band index `#b` in a face name is the script's
    /// list index and nothing else.
    ///
    /// **`cap=` — how the two ends are closed.** `"plane"` (the default) is a
    /// flat face, and needs a planar end section: a non-planar one is a
    /// diagnostic naming the section. `"ruled"` closes an end that lies in no
    /// plane by ruling the loop against itself: the loop is split at its seam
    /// (where the script started drawing it) and half-way round by arc length,
    /// and the two halves get the same ruled band the sides get, with the two
    /// corners closed by a flat face where the segments meeting there share a
    /// plane (two arcs of one circle, two lines) or by the neighbouring band
    /// where they are collinear. That is exact for a blade tip, which
    /// physically *is* the ribbon between its two flanks — draw the section
    /// from the middle of its trailing edge with a round nose and both corners
    /// qualify. The split point becomes a vertex of every section. A 2-tuple
    /// gives the start and the end separately: `cap=("plane", "ruled")`. Cadpole
    /// warns when the ribbon would have been 3x narrower with the loop started
    /// elsewhere, which is what a seam in the middle of a flank looks like.
    ///
    /// Which way is *out* is read from the solid's own volume once it is
    /// closed, so reversing the list, or drawing every section the other way
    /// round, gives the same solid; a section drawn the opposite way round
    /// from its neighbour is warned about (the bands would cross), and
    /// `.reverse()` is the fix.
    ///
    /// **How sections are matched.** Loop `l` of every section is paired with
    /// loop `l` of the next, and segment `s` with segment `s`, in the order the
    /// sketches were *written*. Never by a geometric search: a search can
    /// change its mind when a parameter moves and silently twist the solid,
    /// whereas construction order is a pure function of the script. So two
    /// `rounded_rect`s of different sizes and different corner radii pair line
    /// to line and corner arc to corner arc, and each corner band comes out as
    /// an exact cone — which is exactly what a 45-degree chamfer of a filleted
    /// vertical edge is. Cadpole warns if a different seam would have fitted much
    /// better, which is what a loop drawn from a different starting corner
    /// looks like.
    ///
    /// **When the segment counts differ: `match="arclen"`.** Some sections
    /// cannot be drawn with the same segment count — a rim shell that
    /// lofts a plain wall ring up to a ring that detours in a U around every
    /// magnet boss. `match="arclen"` splits the sections' loops at each other's
    /// vertices, measured as a fraction of the way round the loop, until the
    /// counts agree, and then pairs them in construction order as usual. The
    /// splitting is exact: a line stays a line, an arc stays a true arc of the
    /// same circle, so the corners are still cones and nothing is turned into
    /// chords. Sections that already agree are left alone.
    ///
    /// It is off by default, and deliberately: arc length is shared out over
    /// the whole loop, so a detour that exists on one section and not the other
    /// spends a proportional share of the other section's perimeter. On that
    /// rim shell it is what splays each tab's support ramp, which is right,
    /// but it also drifts the two rings' corners a few millimetres apart. That
    /// is the author's call, so the author makes it.
    ///
    /// **What this version needs.** Sections must be parallel, listed in order
    /// along the axis, with the same loops and the same holes. Without
    /// `match="arclen"` they need the same number of segments in each loop too;
    /// `rounded_rect(w, h, 0)` is a 4-segment rectangle, not an 8-segment
    /// rounded one, so give a "square" section a small radius if it has to pair
    /// with a rounded one. Anything else is a diagnostic naming the section and
    /// the reason.
    ///
    /// **`ruled=` — straight sides or a smooth skin.** `ruled=True` (the
    /// default) joins each pair of neighbouring sections with a straight ruled
    /// surface, so the side is a stack of bands with a tangent crease at every
    /// section plane. That is what a chamfer or a frustum wants, and it is what
    /// makes a band between two concentric arcs an *exact* cone.
    ///
    /// `ruled=False` runs one B-spline surface through **all** the sections at
    /// once — a cubic in the section direction where there are enough sections
    /// for one, so the surface is curvature-continuous across the sections in
    /// the middle instead of creased. It passes through every section exactly:
    /// the sections are what you drew, not control points to be approximated.
    /// The side of a column is then a single face rather than one per band, so
    /// its name loses the `#b` suffix.
    ///
    /// With two sections `ruled=False` builds the ruled loft, because there is
    /// nothing to be smooth between. And if what you are after is a surface of
    /// revolution, use `revolve` rather than a stack of circles: a revolved
    /// curve is exact, it needs no sections at all, and it tessellates far
    /// more cheaply than any loft of circles can.
    ///
    /// The result is an exact B-rep solid. Face names: `start` and `end` for
    /// the two end caps, `side[l.s]` for the face swept from segment `s` of
    /// loop `l`, and — for a **ruled** loft through more than two sections —
    /// `side[l.s]#b` for the band between section `b` and section `b + 1`, with
    /// `side[l.s]` staying on as the name of the whole column. Under
    /// `match="arclen"` `s` counts the *matched* segments, which is the list
    /// both sections were split into, not the segments of either sketch on its
    /// own.
    ///
    /// ```python
    /// # One Gridfinity foot: four rounded rectangles, no chamfer needed.
    /// def ring(inset, z):
    ///     return sketch(XY.offset(z)).rounded_rect(42 - 2 * inset, 42 - 2 * inset, 4 - inset)
    /// foot = loft([ring(3.2, -5.0), ring(2.4, -4.2), ring(2.4, -2.4), ring(0, 0)], name="foot")
    ///
    /// # A smooth hub: six circles, one surface, no visible bands.
    /// def ring(z, r):
    ///     return sketch(XY.offset(z)).circle(r)
    /// hub = loft([ring(0, 8), ring(7, 11), ring(14, 17), ring(22, 24), ring(30, 28), ring(36, 30)],
    ///            ruled=False, name="hub")
    /// ```
    fn loft<'v>(
        #[starlark(require = pos)] sections: UnpackList<Value<'v>>,
        #[starlark(require = named, default = true)] ruled: bool,
        #[starlark(require = named, default = "index")] r#match: &str,
        #[starlark(require = named)] cap: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let matching = cadpole_core::loft::SectionMatch::parse(r#match)?;
        let caps = loft_caps(cap)?;
        if sections.items.len() < 2 {
            anyhow::bail!("loft: needs at least two sections, got {}", sections.items.len());
        }
        let mut secs = Vec::with_capacity(sections.items.len());
        let mut features = Vec::new();
        for (i, v) in sections.items.iter().enumerate() {
            let (fid, sec) = loft_section(*v, i, eval)?;
            if let Some(f) = fid {
                if !features.contains(&f) {
                    features.push(f);
                }
            }
            secs.push(sec);
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids =
            ctx.loft("loft", &secs, &features, ruled, matching, caps, BTreeMap::new(), name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "loft")?;
        bodies_value(heap, &ctx, ids)
    }

    /// A cone from radius `r0` at `at` to radius `r1` a height `h` above it,
    /// axis +Z. `r1 = 0` makes a point.
    ///
    /// Built as a two-section loft of concentric circles, so the wall is an
    /// exact conical surface rather than a facetted approximation — which is
    /// what makes it behave in a boolean. Gridfinity uses it for the 0.2 mm
    /// 45-degree lead-in chamfer at the mouth of every magnet bore.
    ///
    /// Face names: `bottom`, `top` (absent when `r1 = 0`) and the two-face
    /// group `side`, as for `cylinder`.
    fn cone<'v>(
        #[starlark(require = pos)] r0: UnpackFloat,
        #[starlark(require = pos)] r1: UnpackFloat,
        #[starlark(require = pos)] h: UnpackFloat,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let at = at.as_ref().map(|a| vec3(a, "cone: at")).transpose()?.unwrap_or(Vec3::ZERO);
        let (r0, r1, h) = (num(&r0, "cone: r0")?, num(&r1, "cone: r1")?, num(&h, "cone: h")?);
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = ctx.cone(at, r0, r1, h, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "cone")?;
        KBody::from_ctx(&ctx, id)
    }

    /// A cylinder of radius `r` (or diameter `d`) and height `h` along +Z,
    /// with its base centre at `at`.
    fn cylinder<'v>(
        #[starlark(require = pos)] r: Option<UnpackFloat>,
        #[starlark(require = pos)] h: Option<UnpackFloat>,
        #[starlark(require = named)] d: Option<UnpackFloat>,
        #[starlark(require = named)] height: Option<UnpackFloat>,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let r = match (r, d) {
            (Some(r), None) => r,
            (None, Some(d)) => UnpackFloat(d.0 / 2.0),
            (Some(_), Some(_)) => anyhow::bail!("cylinder: give either r or d, not both"),
            (None, None) => anyhow::bail!("cylinder: needs r (or d=)"),
        };
        let h = match (h, height) {
            (Some(h), _) | (None, Some(h)) => h,
            (None, None) => anyhow::bail!("cylinder: needs h (or height=)"),
        };
        // `inf <= 0.0` is false, so the sign check alone let an infinite
        // height through to a sweep that never finished.
        num(&r, "cylinder: r")?;
        num(&h, "cylinder: h")?;
        if r.0 <= 0.0 || h.0 <= 0.0 {
            anyhow::bail!("cylinder: r and h must be positive");
        }
        let at = at.as_ref().map(|a| vec3(a, "cylinder: at")).transpose()?.unwrap_or(Vec3::ZERO);
        let plane = Plane { origin: Vec3::new(0.0, 0.0, at.z), ..Plane::XY };
        let s = Sketch::new(plane).with_loop(Loop::circle(P2::new(at.x, at.y), r.0));
        let mut args = BTreeMap::new();
        args.insert("r".into(), fmt_num(r.0));
        args.insert("h".into(), fmt_num(h.0));
        args.insert("at".into(), fmt_vec(at));
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = ctx.extrude("cylinder", &s, None, h.0, args, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "cylinder")?;
        bodies_value(heap, &ctx, ids)
    }

    /// Union of two or more bodies (Bodies or lists of Bodies). Inputs are consumed.
    ///
    /// `kernel=` picks the geometry kernel: `"auto"` (the default) runs the
    /// exact B-rep kernel and falls back to the mesh kernel when it fails or
    /// when the operands are too big for it to be quick, `"brep"` insists on
    /// the exact kernel (a failure is an error), `"mesh"` always uses the mesh
    /// kernel and returns a mesh body.
    ///
    /// `weld=` (a length) joins the bodies the way a printer fuses the parts
    /// of one object: every axis-aligned box among them is joined
    /// exactly on a grid -- faces closer than the weld meet, a box thinner
    /// than it closes to nothing (a warning says which), and boxes that touch
    /// only along an edge or at a corner are bridged `weld` wide -- and every
    /// other body is unioned onto that exactly. One body is enough. The
    /// result is a mesh body and always a closed, oriented solid. For a model
    /// built out of boxes, a story at a time: `union(parts, weld=0.01)`.
    fn union<'v>(
        #[starlark(args)] bodies: UnpackTuple<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] kernel: Option<&str>,
        #[starlark(require = named)] weld: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let items = collect_bodies(&bodies.items, eval.heap(), "union")?;
        let b = match opt_num(&weld, "union: weld")? {
            None => boolean(BoolOp::Union, &items, name, kernel, eval)?,
            Some(w) => {
                if items.is_empty() {
                    anyhow::bail!("union needs at least one body");
                }
                let choice = match kernel {
                    Some(k) => KernelChoice::parse(k)?,
                    None => KernelChoice::Auto,
                };
                let span = call_span(eval);
                let st = store(eval)?;
                let mut ctx = st.ctx.borrow_mut();
                let others: Vec<_> = items[1..].iter().map(|b| b.body_id()).collect();
                let id = ctx.union_welded(items[0].body_id(), &others, w, choice, name.map(str::to_owned), span)?;
                KBody::from_ctx(&ctx, id)?
            }
        };
        crate::layers::tag_body(eval, layer, &b, "union")?;
        Ok(b)
    }

    /// Subtract the second and later bodies (Bodies or lists of Bodies) from the first. Inputs are consumed.
    ///
    /// `kernel=` is `"auto"`, `"brep"` or `"mesh"`, as for `union`.
    fn subtract<'v>(
        #[starlark(args)] bodies: UnpackTuple<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] kernel: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let items = collect_bodies(&bodies.items, eval.heap(), "subtract")?;
        let b = boolean(BoolOp::Subtract, &items, name, kernel, eval)?;
        crate::layers::tag_body(eval, layer, &b, "subtract")?;
        Ok(b)
    }

    /// Intersection of two or more bodies (Bodies or lists of Bodies). Inputs are consumed.
    ///
    /// `kernel=` is `"auto"`, `"brep"` or `"mesh"`, as for `union`.
    fn intersect<'v>(
        #[starlark(args)] bodies: UnpackTuple<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] kernel: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let items = collect_bodies(&bodies.items, eval.heap(), "intersect")?;
        let b = boolean(BoolOp::Intersect, &items, name, kernel, eval)?;
        crate::layers::tag_body(eval, layer, &b, "intersect")?;
        Ok(b)
    }

    /// Join bodies that touch on coincident faces, by merging those faces. Inputs are consumed.
    ///
    /// This is `union` for parts that meet flush rather than overlapping: the
    /// Gridfinity feet on a bin's floor, a lip on a wall top, a tab flush with
    /// a wall. The touching faces are merged and the two shells become one, so
    /// nothing is intersected and no tolerance enters the answer — it is exact
    /// where a kernel boolean is not even defined.
    ///
    /// `union` does this by itself when the bodies turn out to touch, so you
    /// never *have* to call `glue`. What calling it buys is the refusal: it is
    /// an error if the bodies overlap, or touch only along an edge, or their
    /// faces coincide only in part — cases where `union` would quietly run a
    /// kernel boolean instead. Say `glue` when two parts are meant to meet
    /// flush and you want to hear about it if they stop.
    ///
    /// Always the exact B-rep kernel; there is no `kernel=`.
    fn glue<'v>(
        #[starlark(args)] bodies: UnpackTuple<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let items = collect_bodies(&bodies.items, eval.heap(), "glue")?;
        if items.len() < 2 {
            anyhow::bail!("glue needs at least two bodies");
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let others: Vec<_> = items[1..].iter().map(|b| b.body_id()).collect();
        let id = ctx.glue(items[0].body_id(), &others, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "glue")?;
        KBody::from_ctx(&ctx, id)
    }

    /// Move a body by `offset`. Same as `body.translate(offset)`.
    fn translate<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] offset: VecArg,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let v = vec3(&offset, "translate: offset")?;
        let mut args = BTreeMap::new();
        args.insert("offset".into(), fmt_vec(v));
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = ctx.transform(body.body_id(), &Transform::translation(v), "translate", args, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "translate")?;
        KBody::from_ctx(&ctx, id)
    }

    /// Rotate a body by `angle` degrees about `axis` (default Z) through `origin`.
    fn rotate<'v>(
        #[starlark(require = pos)] body: &KBody,
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
        args.insert("axis".into(), fmt_vec(axis));
        args.insert("origin".into(), fmt_vec(origin));
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = ctx.transform(
            body.body_id(),
            &Transform::rotation(origin, axis, angle.to_radians()),
            "rotate",
            args,
            name.map(str::to_owned),
            span,
        )?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "rotate")?;
        KBody::from_ctx(&ctx, id)
    }

    /// A copy of `body`, optionally moved by `offset`. The original stays.
    fn copy<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = named)] offset: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let v = offset.as_ref().map(|o| vec3(o, "copy: offset")).transpose()?.unwrap_or(Vec3::ZERO);
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = ctx.duplicate(body.body_id(), &Transform::translation(v), name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "copy")?;
        KBody::from_ctx(&ctx, id)
    }

    /// `count` copies of `body` spaced `spacing` apart (the original is the
    /// first). Returns the list of bodies, or — with `merge=True` — one Body
    /// holding every copy, fused where they touch.
    ///
    /// `merge=True` is what you want when the copies are part of one part
    /// rather than a set of parts: it is one boolean instead of `count - 1`
    /// script-level unions and a pile of intermediate bodies. It **routes to
    /// the mesh kernel** by default, and says so in an info diagnostic: copies
    /// of a pattern characteristically *touch* — Gridfinity's feet meet along
    /// their top edges and nowhere else — and a union whose operands merely
    /// touch, or share a face, is exactly what the exact B-rep kernel refuses.
    /// The result is then a mesh body: it keeps face names, selection, STL and
    /// 3MF, and loses exact surfaces and STEP. Pass `kernel="brep"` to insist
    /// on the exact kernel (a failure is then an error), or `kernel="mesh"` to
    /// ask for the mesh kernel without the diagnostic.
    fn pattern_linear<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] count: UnpackFloat,
        #[starlark(require = pos)] spacing: VecArg,
        #[starlark(require = named, default = false)] merge: bool,
        #[starlark(require = named)] kernel: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        // Capped before the transforms are sized by it (see `check`).
        let count = check::count(&count, "pattern_linear: count", 1, Limits::MAX_PATTERN_COUNT)?;
        let step = vec3(&spacing, "pattern_linear: spacing")?;
        let transforms: Vec<Transform> =
            (0..count).map(|i| Transform::translation(step * i as f64)).collect();
        let mut args = BTreeMap::new();
        args.insert("spacing".into(), fmt_vec(step));
        let v = pattern(body, "pattern_linear", &transforms, merge, kernel, args, name, heap, eval)?;
        crate::layers::tag_value(eval, layer, v, "pattern_linear")?;
        Ok(v)
    }

    /// `count` copies of `body` evenly rotated about `axis` through `origin`
    /// over `angle` degrees (default a full turn). Returns the list of bodies,
    /// or — with `merge=True` — one Body holding every copy.
    ///
    /// `merge=` and `kernel=` behave exactly as for `pattern_linear`.
    fn pattern_circular<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] count: UnpackFloat,
        #[starlark(require = named)] axis: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named, default = UnpackFloat(360.0))] angle: UnpackFloat,
        #[starlark(require = named, default = false)] merge: bool,
        #[starlark(require = named)] kernel: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let count = check::count(&count, "pattern_circular: count", 1, Limits::MAX_PATTERN_COUNT)?;
        let axis = axis.as_ref().map(|a| vec3(a, "pattern_circular: axis")).transpose()?.unwrap_or(Vec3::Z);
        let origin = origin.as_ref().map(|o| vec3(o, "pattern_circular: origin")).transpose()?.unwrap_or(Vec3::ZERO);
        let angle = num(&angle, "pattern_circular: angle")?;
        if axis.length() == 0.0 {
            anyhow::bail!("pattern_circular: axis must be non-zero");
        }
        let full = (angle - 360.0).abs() < 1e-9;
        let denom = if full { count } else { (count - 1).max(1) };
        let transforms: Vec<Transform> = (0..count)
            .map(|i| {
                Transform::rotation(origin, axis, angle.to_radians() * i as f64 / denom as f64)
            })
            .collect();
        let mut args = BTreeMap::new();
        args.insert("axis".into(), fmt_vec(axis));
        args.insert("origin".into(), fmt_vec(origin));
        args.insert("angle_deg".into(), fmt_num(angle));
        let v = pattern(body, "pattern_circular", &transforms, merge, kernel, args, name, heap, eval)?;
        crate::layers::tag_value(eval, layer, v, "pattern_circular")?;
        Ok(v)
    }


    /// Check whether a body — or a set of bodies printed together — can
    /// actually be printed, and say *where* the problems are. Never raises:
    /// printing checks are advice, so a finding is a warning diagnostic and
    /// a value the script can assert on.
    ///
    /// Five checks on each body's triangles. The surface is closed,
    /// consistently oriented and does not run through itself (holes are
    /// located and measured). No wall is thinner than `min_wall`, found by
    /// casting rays into the solid against the opposite surface. Nothing
    /// overhangs past `max_overhang` degrees from vertical, with the total
    /// unsupported area. And the part fits `bed`, with the excess per axis if
    /// it does not.
    ///
    /// **A list of bodies is one print**, and three more checks run between
    /// them: `fuse` — two bodies closer than `min_gap` (0.3 mm), touching or
    /// overlapping come off the printer as one part (an error, with the
    /// gap, the position and the two faces); `layer_gap` — a horizontal gap
    /// between two bodies that is not a whole number of `layer_height`s
    /// gets rounded by the slicer, up or down (a warning naming the two
    /// nearest multiples); `bridge` — a body that does not touch the bed and
    /// has nothing under its lowest face within 2 mm prints in the air (a
    /// warning). A finding between two bodies lands on the first one's
    /// report; the list comes back as a list of reports in the same order.
    /// What is not judged: whether a bridged gap's span prints, and a body
    /// whose underside is curved.
    ///
    /// `printer=` names a machine — `print_check(b, printer="bambu_a1")` or a
    /// value from `printer(...)` — and fills in its bed, nozzle, layer height
    /// and material. Anything spelled out at the call site wins over it, so
    /// `printer="prusa_mk4", nozzle=0.6` is an MK4 bed with a 0.6 mm nozzle.
    /// Without a printer the defaults are a 0.4 mm nozzle, two perimeters, a
    /// 0.2 mm layer, a 0.3 mm fuse gap, PLA and a 256 mm cube.
    ///
    /// `min_wall` defaults to `nozzle * perimeters` — two 0.4 mm extrusions,
    /// 0.8 mm. Pass `orient=True` to also be told which way up to print it.
    ///
    /// **Bridges are not overhangs**: a flat downward face held up at
    /// both ends -- a window or door head, a lintel -- is bridged by the
    /// slicer without support when its span between the supports is no more
    /// than `max_bridge` mm (default 10, Bambu Studio's documented setting;
    /// 0 counts every such face as an overhang). Bridged faces are left out
    /// of `unsupported_area` and reported as `bridge_area`, `longest_bridge`
    /// and `bridges`; a span longer than `max_bridge` stays an overhang, and
    /// the warning says it is a span that is too long.
    ///
    /// **Support rules** beyond the angle, each a keyword, a printer
    /// field and a `cadpole check` flag: `max_overhang_area` (default 4 mm²)
    /// is the largest *connected* overhang patch that prints anyway, and the
    /// warning names the largest; `max_cantilever` (3 mm) is how far a
    /// near-level ledge held up on one side may stick out (`cantilever`
    /// past it); `min_island` (one layer) is how deep a region that starts
    /// in mid-air -- a downward tip, a hanging finger, a ball's underside --
    /// may hang before it is an `island`; `max_horizontal_hole` (7 mm) is
    /// the widest round horizontal hole whose top a slicer bridges
    /// (`horizontal_hole` past it, with the teardrop fix);
    /// `max_slenderness` (8) and `slender_width` (3 mm) make a `slender`
    /// warning of a thin feature too tall for its width; and
    /// `max_height_to_base` (6) makes a `bed_contact` warning of a first
    /// layer small for the part's height. `report.support` lists the
    /// findings that need support and `report.support_free` is true when
    /// there are none; `assert_support_free(body)` insists on it.
    ///
    /// **When the part does not fit the bed** the diagnostic says where to cut
    /// it, and `report.split_advice` is the `split(...)` call that does it.
    ///
    /// The returned `PrintReport` has `ok`, `solid`, `watertight`, `min_wall`,
    /// `thin_area`, `unsupported_area`, `max_overhang`, `bed_contact_area`,
    /// `bridge_area`, `longest_bridge`, `bridges`, `support_area`,
    /// `largest_overhang`, `ledge_area`, `longest_cantilever`, `islands`,
    /// `deepest_island`, `horizontal_holes`, `wide_holes`, `slender`,
    /// `slenderness`, `first_layer_area`, `height_to_base`, `support`,
    /// `support_free`, `fits_bed`, `bed_excess`, `size`, `holes`, `self_intersections`,
    /// `internal_faces`, `components`, `genus`, `errors`, `warnings`,
    /// `summary`, `profile`, `mass`, `layers`, `split_advice`, `split_axis`,
    /// `split_pieces` and `split_at`. `solid` is the verdict of the checks
    /// on the surface itself — closed, consistently oriented, no crossings,
    /// no doubled or internal faces — and `assert_solid(body)` is the short
    /// way to insist on it.
    fn print_check<'v>(
        #[starlark(require = pos)] body: Value<'v>,
        #[starlark(require = named)] printer: Option<Value<'v>>,
        #[starlark(require = named)] min_wall: Option<UnpackFloat>,
        #[starlark(require = named)] nozzle: Option<UnpackFloat>,
        #[starlark(require = named)] perimeters: Option<UnpackFloat>,
        #[starlark(require = named)] layer_height: Option<UnpackFloat>,
        #[starlark(require = named)] material: Option<&str>,
        #[starlark(require = named)] max_overhang: Option<UnpackFloat>,
        #[starlark(require = named)] bed: Option<VecArg>,
        #[starlark(require = named)] bed_shape: Option<&str>,
        #[starlark(require = named)] build_direction: Option<VecArg>,
        #[starlark(require = named)] min_gap: Option<UnpackFloat>,
        #[starlark(require = named, default = false)] orient: bool,
        #[starlark(require = named)] max_bridge: Option<UnpackFloat>,
        #[starlark(require = named)] max_overhang_area: Option<UnpackFloat>,
        #[starlark(require = named)] max_cantilever: Option<UnpackFloat>,
        #[starlark(require = named)] max_horizontal_hole: Option<UnpackFloat>,
        #[starlark(require = named)] min_island: Option<UnpackFloat>,
        #[starlark(require = named)] max_slenderness: Option<UnpackFloat>,
        #[starlark(require = named)] slender_width: Option<UnpackFloat>,
        #[starlark(require = named)] max_height_to_base: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let max_bridge = bridge_limit(max_bridge, "print_check")?;
        let rules = SupportArgs::new(
            "print_check",
            max_overhang_area,
            max_cantilever,
            max_horizontal_hole,
            min_island,
            max_slenderness,
            slender_width,
            max_height_to_base,
        )?;
        let mut profile = resolve_profile(
            "print_check",
            printer,
            min_wall,
            nozzle,
            perimeters,
            layer_height,
            material,
            max_overhang,
            bed.as_ref(),
            bed_shape,
            build_direction.as_ref(),
            min_gap,
        )?;
        rules.apply("print_check", &mut profile)?;
        let heap = eval.heap();
        // One report for one body -- a Body, or an Instance of one;
        // a list of reports for a list, an Assembly, or a many-body Instance.
        let bodies = collect_bodies(&[body], heap, "print_check")?;
        let single = bodies.len() == 1 && body.get_type() != "list" && body.get_type() != "tuple"
            && body.downcast_ref::<crate::assembly::KAssembly>().is_none();
        if bodies.is_empty() {
            anyhow::bail!("print_check: the list of bodies is empty");
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        // Most messages already say where; only add a position when the
        // text does not carry one.
        let where_ = |issue: &cadpole_print::Issue| match issue.at {
            Some(p) if !issue.message.contains(" at (") => format!(" at ({:.2}, {:.2}, {:.2})", p.x, p.y, p.z),
            _ => String::new(),
        };
        let mut reports = Vec::with_capacity(bodies.len());
        for b in &bodies {
            let name = ctx.body(b.body_id())?.name.clone();
            let (mesh, topo) = ctx.body_mesh(b.body_id())?.clone();
            let mut report = cadpole_print::check_named(&mesh, Some(&topo), &profile, max_bridge);
            if let Some(issue) = chord_issue_for(&mut ctx, b.body_id(), profile.chord_gap_fraction)? {
                report.push_issue(issue);
            }
            let orientation = orient.then(|| cadpole_print::orient::orient(&mesh, &profile));
            // Every finding is a warning: a print check must never turn a
            // working script into a failed evaluation.
            // What was bridged, and overhang the allowances cover, is
            // information, not a finding.
            for issue in &report.issues {
                let text = format!("print_check({name}): [{}] {}{}", issue.check.as_str(), issue.message, where_(issue));
                let d = if issue.severity == cadpole_print::Severity::Info {
                    cadpole_core::context::Diagnostic::info(text)
                } else {
                    cadpole_core::context::Diagnostic::warning(text)
                }
                .with_span(span.clone());
                ctx.diagnostics.push(d);
            }
            if let Some(o) = &orientation {
                ctx.diagnostics
                    .push(cadpole_core::context::Diagnostic::info(format!("orient_for_print({name}): {}", o.reasoning))
                        .with_span(span.clone()));
            }
            reports.push(KPrintReport::from_report(&name, &report, orientation.as_ref()));
        }
        // Between the bodies: the print-in-place rules. A finding
        // lands on the first body of its pair, and on the diagnostics like
        // every other.
        let ids: Vec<cadpole_core::BodyId> = bodies.iter().map(|b| b.body_id()).collect();
        for f in crate::clearance::assembly_findings(&mut ctx, &ids, &profile)? {
            let issue = &f.issue;
            let d = cadpole_core::context::Diagnostic::warning(format!(
                "print_check({}): [{}] {}{}",
                reports[f.body].body,
                issue.check.as_str(),
                issue.message,
                where_(issue)
            ))
            .with_span(span.clone());
            ctx.diagnostics.push(d);
            let r = &mut reports[f.body];
            if issue.check.is_support() {
                r.support.push(format!("[{}] {}", issue.check.as_str(), issue.message));
            }
            if issue.severity == cadpole_print::Severity::Error {
                r.errors.push(issue.message.clone());
                r.ok = false;
            } else {
                r.warnings.push(issue.message.clone());
            }
        }
        Ok(if single { heap.alloc(reports.pop().expect("one body, one report")) } else { heap.alloc(reports) })
    }

    /// Return `body` turned the way it should be printed, sitting on the bed.
    ///
    /// Candidate orientations are the six axis directions plus the normals of
    /// the model's largest flat faces; each is scored on the area that would
    /// need support (the dominant term), how much of it sits on the bed, and
    /// how tall it ends up, with a heavy penalty for not fitting the build
    /// volume. A tie leaves the model exactly as the script built it. Simple
    /// and explainable rather than exhaustive: it will not discover a clever
    /// 23-degree tilt.
    ///
    /// The reasoning is recorded as an `info` diagnostic and on the feature,
    /// and `print_check(body, orient=True).orientation_reasoning` returns the
    /// same sentence without moving anything.
    fn orient_for_print<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = named)] printer: Option<Value<'v>>,
        #[starlark(require = named)] max_overhang: Option<UnpackFloat>,
        #[starlark(require = named)] bed: Option<VecArg>,
        #[starlark(require = named)] bed_shape: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let profile = resolve_profile(
            "orient_for_print",
            printer,
            None,
            None,
            None,
            None,
            None,
            max_overhang,
            bed.as_ref(),
            bed_shape,
            None,
            None,
        )?;
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let source = ctx.body(body.body_id())?.name.clone();
        let mesh = ctx.body_mesh(body.body_id())?.0.clone();
        let o = cadpole_print::orient::orient(&mesh, &profile);
        let mut args = BTreeMap::new();
        args.insert("chose".into(), o.chosen.label.clone());
        args.insert("down".into(), fmt_vec(o.chosen.down));
        args.insert("unsupported_mm2".into(), fmt_num(o.chosen.unsupported_area));
        args.insert("bed_contact_mm2".into(), fmt_num(o.chosen.bed_contact_area));
        args.insert("height_mm".into(), fmt_num(o.chosen.height));
        args.insert("reasoning".into(), o.reasoning.clone());
        ctx.diagnostics.push(
            cadpole_core::context::Diagnostic::info(format!("orient_for_print({source}): {}", o.reasoning))
                .with_span(span.clone()),
        );
        let id = ctx.transform(body.body_id(), &o.transform, "orient_for_print", args, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "orient_for_print")?;
        KBody::from_ctx(&ctx, id)
    }

    /// Cut `body` in two and return the pieces, so a model too big for the
    /// bed can be printed in parts.
    ///
    /// Cut with a plane — `split(body, plane(origin=(0, 0, 120), normal=(0, 0, 1)))`
    /// — or with another body's surface — `split(body, by=cutter)`. The input
    /// body is consumed; a `by=` cutter is not, because the same cutter
    /// usually cuts several bodies.
    ///
    /// **Order.** With a plane the pieces come back along its normal: the
    /// piece on the negative side first, so an upward-facing plane gives
    /// bottom then top. With `by=` the piece *inside* the cutter comes first.
    /// Names follow the same rule as a multi-region extrude: `name`, `name#1`.
    ///
    /// **Names.** Every face that survives keeps the name it had; a face the
    /// cut divided becomes `name#k`; the new face the cut exposed is
    /// `<feature>/cut` on both pieces (docs/design.md).
    ///
    /// `kernel=` is `"auto"`, `"brep"` or `"mesh"`, as for `union`. A part
    /// that has to export STEP wants `"brep"`.
    ///
    /// `print_check` names this call for you: when a body does not fit the
    /// build volume, `report.split_advice` is the `split(...)` to write.
    ///
    /// `keep=`: `"both"` (the default) returns both pieces as a list;
    /// `"below"` or `"above"` (a plane, along its normal) and `"inside"` or
    /// `"outside"` (`by=`) return that one piece as a Body, the other made
    /// and consumed, so nothing exports it. Registration pegs across the
    /// cut are `pegs(*split(...))` from `@cadpole/print.star`.
    fn split<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] plane: Option<&KPlane>,
        #[starlark(require = named)] by: Option<&KBody>,
        #[starlark(require = named)] kernel: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] keep: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let tool = match (plane, by) {
            (Some(p), None) => SplitTool::Plane(p.0),
            (None, Some(b)) => SplitTool::Body(b.body_id()),
            (Some(_), Some(_)) => anyhow::bail!("split: pass a plane or by=, not both"),
            (None, None) => anyhow::bail!("split: pass a plane to cut with, or by=<body>"),
        };
        // Which piece `keep=` names: index 0 is below the plane (or inside
        // the cutter), index 1 above it (outside).
        let keep_word = keep.unwrap_or("both");
        let keep_index = match (keep_word, &tool) {
            ("both", _) => None,
            ("below", SplitTool::Plane(_)) => Some(0usize),
            ("above", SplitTool::Plane(_)) => Some(1),
            ("inside", SplitTool::Body(_)) => Some(0),
            ("outside", SplitTool::Body(_)) => Some(1),
            (other, SplitTool::Plane(_)) => {
                anyhow::bail!("split: keep= is \"both\", \"below\" or \"above\" for a plane, got {other:?}")
            }
            (other, SplitTool::Body(_)) => {
                anyhow::bail!("split: keep= is \"both\", \"inside\" or \"outside\" with by=, got {other:?}")
            }
        };
        let choice = match kernel {
            Some(k) => KernelChoice::parse(k)?,
            None => KernelChoice::Auto,
        };
        let heap = eval.heap();
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = ctx.split(body.body_id(), &tool, choice, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "split")?;
        match keep_index {
            None => {
                let out = ids.into_iter().map(|id| KBody::from_ctx(&ctx, id)).collect::<anyhow::Result<Vec<KBody>>>()?;
                Ok(heap.alloc(out))
            }
            Some(k) => {
                let fid = ctx.body(ids[k])?.feature.clone();
                ctx.set_feature_arg(&fid, "keep", keep_word);
                ctx.consume_body(ids[1 - k])?;
                Ok(heap.alloc(KBody::from_ctx(&ctx, ids[k])?))
            }
        }
    }

    /// A named printer profile: bed size and shape, nozzle, layer height, the
    /// support angle and the material.
    ///
    /// Pass it to `print_check(body, printer=...)` and
    /// `orient_for_print(body, printer=...)`, or read the numbers off it
    /// (`.bed`, `.nozzle`, `.layer_height`, `.material`, `.source`).
    /// `print_check(body, printer="bambu_a1")` takes the name directly.
    ///
    /// Names: `bambu_a1`, `bambu_a1_mini`, `bambu_p1s`, `bambu_p2s`,
    /// `bambu_x1c`, `bambu_x1e`, `bambu_h2d`, `bambu_h2c`, `bambu_h2s`,
    /// `prusa_mk4`, `ender3`, plus the obvious short forms (`a1`, `mk4`,
    /// `x1c`, `h2d`). Case, dashes, underscores and spaces are all the same.
    ///
    /// Build volume, nozzle and layer height are the vendor's published
    /// numbers — for a Bambu machine, what Bambu Studio's own profile lets you
    /// print where that is less (the X1C's 250 mm height) — and
    /// `.source` says which page they came from. The 45 degree
    /// support angle and the two perimeters are **not** vendor numbers: they
    /// are Cadpole's defaults, because every slicer's own thresholds differ and
    /// change between releases.
    fn printer(#[starlark(require = pos)] name: &str) -> anyhow::Result<KPrinter> {
        let p = cadpole_print::printer(name).ok_or_else(|| {
            anyhow::anyhow!("printer: unknown printer {name:?}; Cadpole knows {}", cadpole_print::printer_list())
        })?;
        Ok(KPrinter::from_profile(&p.profile(), p.display_name, p.source))
    }

    /// Paint faces of `body` a colour, for the viewport and for multi-colour
    /// export. Returns the same body.
    ///
    /// `faces` is a list of Faces (`body.faces(normal=(0,0,1))`) or of
    /// topological names (`"face:3"`); leave it out to colour the whole body,
    /// which is how a body's filament slot gets pinned. `color` is
    /// `"#rrggbb"` or `(r, g, b)` in 0..1.
    ///
    /// Filament slots are numbered in order of first use — walking the bodies
    /// and then the paint calls — so the same script always exports the same
    /// slot numbering. Pass `slot=` to pin one instead.
    ///
    /// `layer=` puts the body on a display layer, a `Layer` from
    /// `layer(...)` or its name; on its own, with no `color=`, it records
    /// nothing but the tag.
    ///
    /// In a 3MF export every painted triangle carries the colour three ways:
    /// the core specification's `p1` property index, PrusaSlicer's
    /// `mmu_segmentation` and (with `slicer="bambu"`) Bambu's `paint_color`,
    /// so the part opens already coloured rather than needing repainting in
    /// the slicer.
    ///
    /// `shadow=False` makes the body guide geometry: a zoning
    /// envelope, a setback plane, a lot line — something the camera sees
    /// and light does not, so it neither shadows nor tints nor blocks light
    /// in either renderer. It is the whole body's, whatever `faces=` says,
    /// and is the same flag as `Body.material(shadow=)`; `paint(body,
    /// shadow=False)` alone sets it and records no feature.
    fn paint<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = named)] faces: Option<Value<'v>>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named)] slot: Option<UnpackFloat>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] shadow: Option<bool>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let heap = eval.heap();
        // The shadow flag and the layer are body state, as `Body.material`
        // and `layer=` set them; the colour is a feature. Any one alone is a
        // complete call, and every argument is checked before the body is
        // touched.
        let rgb = match color {
            Some(c) if !c.is_none() => Some(crate::convert::color(c)?),
            _ if layer.as_ref().map_or(false, |l| !l.is_none()) || shadow.is_some() => {
                // `paint(body, layer=)` and `paint(body, shadow=)` alone are
                // tags: no colour, no feature, the body handed
                // back on its layer and with its flag.
                if faces.is_some() || slot.is_some() || name.is_some() {
                    anyhow::bail!("paint: faces=, slot= and name= need a color= to paint with; layer= and shadow= alone only tag the body");
                }
                None
            }
            _ => anyhow::bail!("paint: color= is required (\"#rrggbb\" or (r, g, b)), or layer= / shadow= alone"),
        };
        if layer.as_ref().map_or(false, |l| !l.is_none()) {
            crate::layers::tag_body(eval, layer, body, "paint")?;
        }
        if let Some(s) = shadow {
            let st = store(eval)?;
            let mut ctx = st.ctx.borrow_mut();
            crate::values::MaterialEdit::shadow_only(s).apply(&mut ctx.body_mut(body.body_id())?.material);
        }
        let Some(rgb) = rgb else { return Ok(body.clone()) };
        let slot = match slot {
            Some(s) => {
                let n = int_arg(s, "paint: slot")?;
                if n < 1 {
                    anyhow::bail!("paint: slot must be 1 or more (filament slots are 1-based), got {n}");
                }
                Some(n as u32)
            }
            None => None,
        };
        // Face names: Face values, plain strings, or a single one of either.
        let mut names: Vec<String> = Vec::new();
        if let Some(v) = faces {
            let mut one = |v: Value<'v>| -> anyhow::Result<()> {
                if let Some(f) = v.downcast_ref::<KFace>() {
                    names.push(f.name.clone());
                } else if let Some(sel) = v.downcast_ref::<KSelection>() {
                    if sel.what != "face" {
                        anyhow::bail!("paint: faces= needs a selection of faces, not edges");
                    }
                    names.extend(sel.names());
                } else if let Some(s) = v.unpack_str() {
                    names.push(s.to_owned());
                } else {
                    anyhow::bail!(
                        "paint: faces must be Faces, a Selection or face names, got {}",
                        v.get_type()
                    );
                }
                Ok(())
            };
            if v.get_type() == "list" || v.get_type() == "tuple" {
                for item in v.iterate(heap).map_err(|e| e.into_anyhow())? {
                    one(item)?;
                }
            } else {
                one(v)?;
            }
            if names.is_empty() {
                anyhow::bail!("paint: faces= was empty; leave it out to paint the whole body");
            }
        }
        // Every name is resolved the way `Body.face()` resolves one, so the
        // two builtins agree on what a face name is: the full generated name
        // (`b/top`), the local name inside this body's feature (`top`), an
        // alias, or the `face:3` compatibility form the docs use. They did
        // not — paint compared strings to generated names and rejected the
        // rest — and the feature records the resolved name, so export sees a
        // name that exists. A typo is still a diagnostic here rather than
        // silence at export time.
        if !names.is_empty() {
            let faces = body.faces(eval)?;
            names = names
                .iter()
                .map(|n| {
                    crate::values::pick(&faces, cadpole_core::naming::Kind::Face, &body.feature, n, "face")
                        .map(|f| f.name)
                        .map_err(|e| anyhow::anyhow!("paint: {body}: {e}"))
                })
                .collect::<anyhow::Result<Vec<String>>>()?;
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id_string = ctx.body(body.body_id())?.id_string();
        let feature = ctx.new_feature(
            cadpole_core::paint::KIND,
            name.map(str::to_owned),
            vec![cadpole_core::FeatureId(body.feature.clone())],
            cadpole_core::paint::args(&id_string, &names, rgb, slot),
            span,
        )?;
        let _ = feature;
        crate::layers::tag_ids(&mut ctx, layer, &[body.body_id()], "paint")?;
        Ok(body.clone())
    }

    /// Draw the model as a 2D engineering drawing: hidden-line views with real
    /// dimensions on them, ready for `export_svg` or `export_dxf`.
    ///
    /// `view="three-view"` (the default) lays out front, top and right in
    /// third-angle projection with an isometric in the corner, fits them to the
    /// sheet at a standard scale, and figures the overall sizes. A single view
    /// name — `"front"`, `"top"`, `"right"`, `"iso"`, … — draws just that one,
    /// and `"section:z=40"` cuts the model with a plane and hatches the cut
    /// face. `units="ft-in"` writes lengths as `12'-6"` instead of millimetres;
    /// left out, the document's own `units(...)` decides, and a
    /// document that declared none figures in millimetres.
    ///
    /// With no `bodies`, every visible body is drawn, so hidden-line removal
    /// works between parts as well as within one. `font="helvetica"` (or
    /// `"courier"`) sets every string in that standard face, which
    /// `export_pdf` writes as real, selectable text; the default is the
    /// bundled face, written as outlines.
    fn drawing<'v>(
        #[starlark(require = pos)] bodies: Option<Value<'v>>,
        #[starlark(require = named)] view: Option<&str>,
        #[starlark(require = named)] scale: Option<&str>,
        #[starlark(require = named)] sheet: Option<&str>,
        #[starlark(require = named, default = true)] dimensions: bool,
        #[starlark(require = named)] hidden: Option<&str>,
        #[starlark(require = named, default = NoneOr::None)] units: NoneOr<&str>,
        #[starlark(require = named)] title: Option<&str>,
        #[starlark(require = named)] font: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KDrawing> {
        let heap = eval.heap();
        // `units=None` is the same as leaving it out: the document decides.
        let units = units.into_option();
        let mut ids: Vec<String> = Vec::new();
        // An explicit `None` means the same as leaving it out: draw whatever
        // is visible.
        if let Some(v) = bodies.filter(|v| !v.is_none()) {
            if let Some(b) = v.downcast_ref::<KBody>() {
                ids.push(format!("{}#{}", b.feature, b.id));
            } else {
                for item in v.iterate(heap).map_err(|e| e.into_anyhow())? {
                    let b = item
                        .downcast_ref::<KBody>()
                        .ok_or_else(|| anyhow::anyhow!("drawing: expected bodies"))?;
                    ids.push(format!("{}#{}", b.feature, b.id));
                }
            }
        }
        let mut opts = cadpole_drawing::DrawingOptions::parse(
            view.unwrap_or(""),
            scale.unwrap_or(""),
            sheet.unwrap_or(""),
            if dimensions { "auto" } else { "none" },
            hidden.unwrap_or(""),
            units.unwrap_or(""),
        )
        .map_err(|e| anyhow::anyhow!("drawing: {e}"))?;
        opts.title = title.unwrap_or("").to_owned();

        let st = store(eval)?;
        let span = call_span(eval);
        // The document's `units(...)` is the default `units=`: a house
        // script's sheet reads in feet and inches without saying so twice.
        if units.is_none()
            && let Some(f) = crate::units::document_format(&st.ctx.borrow())
        {
            opts.unit_format = f;
        }
        let (mesh, topo) = {
            let mut ctx = st.ctx.borrow_mut();
            cadpole_drawing::model::combined_mesh(&mut ctx, &ids)
                .map_err(|e| anyhow::anyhow!("drawing: {e}"))?
        };
        let mut d = cadpole_drawing::model::build(&mesh, &topo, &opts)
            .map_err(|e| anyhow::anyhow!("drawing: {e}"))?;
        // `font="helvetica"` (or `"courier"`): every string in a standard
        // face, so `export_pdf` writes it as real text.
        match font.map(|f| f.trim().to_ascii_lowercase()) {
            None => {}
            Some(f) if f == "inter" || f == "builtin" => {}
            Some(f) => match cadpole_drawing::StdFont::parse(&f, false, false) {
                Some(face) => d.set_text_font(&cadpole_drawing::FontRef::Standard(face)),
                None => anyhow::bail!("drawing: font must be helvetica, courier or inter, got {f:?}"),
            },
        }
        for note in &d.notes {
            st.ctx
                .borrow_mut()
                .diagnostics
                .push(cadpole_core::context::Diagnostic::warning(note.clone()).with_span(span.clone()));
        }
        Ok(KDrawing { name: name.unwrap_or("drawing").to_owned(), drawing: d })
    }

    /// Record that a drawing or sketch should be written to `path` as SVG.
    ///
    /// Coordinates are millimetres and the root carries `width="Wmm"`, so the
    /// file re-imports at the right size in Cadpole and in anything else. Circular
    /// arcs are written as `A` commands, so a hole stays a hole rather than
    /// becoming a chain of chords, and each layer is one `<g>` with the loop
    /// ids on its paths.
    ///
    /// `fill=True` fills the sketch's regions instead of outlining its loops,
    /// which is what an artwork wants; the default draws line work, which is
    /// what a cut file wants. Like `export()`, this records intent — the host
    /// writes the file.
    fn export_svg<'v>(
        #[starlark(require = pos)] target: Value<'v>,
        #[starlark(require = pos)] path: &str,
        #[starlark(require = named, default = NoneOr::None)] units: NoneOr<&str>,
        #[starlark(require = named, default = UnpackFloat(0.25))] stroke: UnpackFloat,
        #[starlark(require = named, default = false)] fill: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let fmt = export_units(units.into_option(), "export_svg", eval)?;
        let stroke = num(&stroke, "export_svg: stroke")?.max(0.0);
        let d = target_of(target, heap, "export_svg")?.into_drawing(stroke, fill, fmt);
        let data = cadpole_drawing::svg::write(&d, &cadpole_drawing::svg::SvgOptions::default()).into_bytes();
        let st = store(eval)?;
        st.files.borrow_mut().push(OutputFile {
            path: path.to_owned(),
            format: "svg".into(),
            data,
        });
        Ok(NoneType)
    }

    /// Record that a drawing or sketch should be written to `path` as DXF.
    ///
    /// One layer per sketch feature (or per drawing layer, which for a plan
    /// means the standard `A-WALL` / `A-ANNO-DIMS` names), arcs written as
    /// polyline bulges so they stay exact, and `$INSUNITS` set to millimetres.
    /// Dimensions are exploded into lines, arrowheads and text, so every reader
    /// shows what Cadpole drew.
    fn export_dxf<'v>(
        #[starlark(require = pos)] target: Value<'v>,
        #[starlark(require = pos)] path: &str,
        #[starlark(require = named, default = NoneOr::None)] units: NoneOr<&str>,
        #[starlark(require = named, default = UnpackFloat(0.25))] stroke: UnpackFloat,
        #[starlark(require = named, default = false)] fill: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let fmt = export_units(units.into_option(), "export_dxf", eval)?;
        let stroke = num(&stroke, "export_dxf: stroke")?.max(0.0);
        let d = target_of(target, heap, "export_dxf")?.into_drawing(stroke, fill, fmt);
        let data = cadpole_drawing_out::write_dxf(&d, &cadpole_drawing_out::DxfOptions::default())
            .map_err(|e| anyhow::anyhow!("export_dxf: {e}"))?;
        let st = store(eval)?;
        st.files.borrow_mut().push(OutputFile {
            path: path.to_owned(),
            format: "dxf".into(),
            data,
        });
        Ok(NoneType)
    }

    /// Record that a drawing or sketch should be written to `path` as PDF: one
    /// vector page, text as filled outlines so it looks the same everywhere.
    fn export_pdf<'v>(
        #[starlark(require = pos)] target: Value<'v>,
        #[starlark(require = pos)] path: &str,
        #[starlark(require = named, default = NoneOr::None)] units: NoneOr<&str>,
        #[starlark(require = named, default = UnpackFloat(0.25))] stroke: UnpackFloat,
        #[starlark(require = named, default = false)] fill: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let heap = eval.heap();
        let fmt = export_units(units.into_option(), "export_pdf", eval)?;
        let stroke = num(&stroke, "export_pdf: stroke")?.max(0.0);
        let d = target_of(target, heap, "export_pdf")?.into_drawing(stroke, fill, fmt);
        let data = cadpole_drawing_out::write_pdf(&d, &cadpole_drawing_out::PdfOptions::default());
        let st = store(eval)?;
        st.files.borrow_mut().push(OutputFile {
            path: path.to_owned(),
            format: "pdf".into(),
            data,
        });
        Ok(NoneType)
    }

    /// Record that `bodies` should be exported to `path` (format from the
    /// extension: .stl, .obj, .3mf, .step). The host writes the file after
    /// evaluation.
    ///
    /// 3MF carries colour: each body's `material(color=…)` becomes a filament
    /// slot and every `paint(...)` call becomes per-triangle colour, so a
    /// multi-colour part opens in the slicer already coloured. Which slicer's
    /// conventions to add is `cadpole export --slicer`.
    ///
    /// STEP carries the exact surfaces — a hole stays a cylinder — which is
    /// what a CAD tool wants and what STL cannot give it. It needs every body
    /// on the B-rep path: a body the mesh kernel made cannot become a B-rep
    /// again, and the export says so rather than writing triangles. Use
    /// `kernel="brep"` on the booleans of a part that has to export STEP.
    ///
    /// `tolerance=` is the chordal tolerance in mm the mesh formats are
    /// tessellated to for this call, for every body that has no
    /// `body.tessellation(tolerance=)` of its own; without it the document's
    /// (`--tolerance`, 0.05 by default) applies. Body, then call, then
    /// document. STEP ignores it. The export reports each body's
    /// triangle count and the chord error it measured, and warns when that
    /// error is more than a third of the gap to another body.
    ///
    /// `flavor=flavor.bambu` writes a `.3mf` as a **Bambu Studio
    /// project**: several `export()` calls to the same path accumulate into
    /// one file with several plates. `plate=` is the plate this call's
    /// bodies land on, a number or a name (a name shows in Bambu's plate
    /// tab; named plates are numbered in order of first appearance).
    /// `name=` makes the call's bodies ONE object with that name, each body
    /// a part of it -- how a print-in-place set arrives so nothing can be
    /// moved apart; without it every body is its own object, named after
    /// the body. `extruder=` is the filament slot (1-based) the bodies print
    /// in, one number for all or a list with one per body; without it a
    /// body's material colour picks the slot, distinct colours in order of
    /// first use. `printer=` names the machine the plates are laid out for
    /// and the pieces fitted to (`"bambu_h2d"`, `"bambu_h2c"`,
    /// `"bambu_x1c"`, ...); the default is the 256 mm bed of the X1C, P1S
    /// and A1. `bed=(x, y)` or `(x, y, z)` in mm is a custom bed instead.
    /// `template=` is a Bambu Studio project you saved for your printer --
    /// `asset("h2d_4.3mf")` or its name -- whose printer, process and
    /// filament settings the project carries, the model's filament slots
    /// mapped onto its filaments; without it the project carries
    /// Cadpole's minimal settings and the slicer's defaults fill the rest.
    /// Plates are laid out as Bambu does, `ceil(sqrt(N))` to a row; each
    /// plate's pieces keep their arrangement and are centred where every
    /// nozzle reaches, every object set down on the bed; a plate whose
    /// pieces do not fit is refused, naming it.
    #[allow(clippy::too_many_arguments)]
    fn export<'v>(
        #[starlark(require = pos)] bodies: Value<'v>,
        #[starlark(require = pos)] path: &str,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        #[starlark(require = named)] flavor: Option<&str>,
        #[starlark(require = named)] plate: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] extruder: Option<Value<'v>>,
        #[starlark(require = named)] printer: Option<&str>,
        #[starlark(require = named)] bed: Option<Value<'v>>,
        #[starlark(require = named)] template: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        use cadpole_core::context::PlateRef;
        use cadpole_core::export::Flavor;
        // A sheet set or one of its sheets: the host writes the pages.
        // `template=None` / `bed=None` are "not given", so a script can pass
        // a parameter's choice through unconditionally.
        let bed = bed.filter(|v| !v.is_none());
        let template = template.filter(|v| !v.is_none());
        let given = [
            ("tolerance", tolerance.is_some()),
            ("flavor", flavor.is_some()),
            ("plate", plate.is_some()),
            ("name", name.is_some()),
            ("extruder", extruder.is_some()),
            ("printer", printer.is_some()),
            ("bed", bed.is_some()),
            ("template", template.is_some()),
        ];
        if let Some(done) = crate::sheets::record_export(bodies, path, &given, eval)? {
            return Ok(done);
        }
        let tolerance = opt_num(&tolerance, "export: tolerance")?;
        if let Some(t) = tolerance {
            if !(t > 0.0) {
                anyhow::bail!("export: tolerance must be positive, got {t}");
            }
        }
        let heap = eval.heap();
        // Bodies, or an assembly / instances: their posed bodies.
        let ids: Vec<BodyId> = collect_bodies(&[bodies], heap, "export")?.iter().map(|b| b.body_id()).collect();
        if ids.is_empty() {
            anyhow::bail!("export: nothing to export");
        }
        let format = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let format = if format == "stp" { "step".to_owned() } else { format };
        if !matches!(format.as_str(), "stl" | "obj" | "3mf" | "step") {
            anyhow::bail!("export: unsupported format {format:?} (use .stl, .obj, .3mf or .step)");
        }
        // The host writes the file name and ignores the directory part, so a
        // path that is nothing but an extension asks for a hidden dotfile.
        let file_name = path.rsplit('/').next().unwrap_or(path);
        if file_name.rsplit_once('.').map(|(stem, _)| stem.is_empty()).unwrap_or(false) {
            anyhow::bail!("export: {path:?} has no file name in front of the extension");
        }
        let flavor = Flavor::parse(flavor.unwrap_or("")).map_err(|e| anyhow::anyhow!("export: {e}"))?;
        if flavor != Flavor::Standard && format != "3mf" {
            anyhow::bail!("export: flavor={:?} is a 3MF dialect and {path:?} is not a .3mf", flavor.name());
        }
        let plate = match plate {
            None => None,
            Some(v) => {
                if flavor != Flavor::Bambu {
                    anyhow::bail!("export: plate= is a Bambu Studio project's plate; pass flavor=flavor.bambu");
                }
                if let Some(n) = v.unpack_i32() {
                    if n < 1 {
                        anyhow::bail!("export: plate must be 1 or more, got {n}");
                    }
                    Some(PlateRef::Number(n as u32))
                } else if let Some(s) = v.unpack_str() {
                    if s.trim().is_empty() {
                        anyhow::bail!("export: a plate name must not be empty");
                    }
                    Some(PlateRef::Name(s.to_owned()))
                } else {
                    anyhow::bail!("export: plate= takes a number or a name, got {}", v.get_type());
                }
            }
        };
        let extruders: Vec<u32> = match extruder {
            None => Vec::new(),
            Some(v) => {
                let list: Vec<i32> = if let Some(n) = v.unpack_i32() {
                    vec![n]
                } else if let Some(items) = <UnpackListOrTuple<Value<'v>>>::unpack_value(v).map_err(|e| e.into_anyhow())? {
                    items
                        .items
                        .iter()
                        .map(|i| {
                            i.unpack_i32()
                                .ok_or_else(|| anyhow::anyhow!("export: extruder= takes whole numbers, got {}", i.get_type()))
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?
                } else {
                    anyhow::bail!("export: extruder= takes a number or a list of numbers, got {}", v.get_type());
                };
                if list.len() != 1 && list.len() != ids.len() {
                    anyhow::bail!(
                        "export: extruder= names {} slots for {} bodies; give one for all or one per body",
                        list.len(),
                        ids.len()
                    );
                }
                for n in &list {
                    if *n < 1 {
                        anyhow::bail!("export: extruder slots are numbered from 1, got {n}");
                    }
                }
                if list.len() == 1 { vec![list[0] as u32; ids.len()] } else { list.into_iter().map(|n| n as u32).collect() }
            }
        };
        let printer = match printer {
            None => String::new(),
            Some(p) => {
                if flavor != Flavor::Bambu {
                    anyhow::bail!("export: printer= lays out a Bambu Studio project's plates; pass flavor=flavor.bambu");
                }
                match cadpole_print::printers::printer(p) {
                    Some(known) => known.id.to_owned(),
                    None => anyhow::bail!("export: unknown printer {p:?}; valid: {}", cadpole_print::printers::printer_list()),
                }
            }
        };
        let bed = match bed {
            None => None,
            Some(v) => {
                if flavor != Flavor::Bambu {
                    anyhow::bail!("export: bed= is the bed a Bambu Studio project's plates are laid out on; pass flavor=flavor.bambu");
                }
                Some(bed_of(v, "export: bed")?)
            }
        };
        let st = store(eval)?;
        let template = match template {
            None => None,
            Some(v) => {
                if flavor != Flavor::Bambu {
                    anyhow::bail!("export: template= is a Bambu Studio project's settings; pass flavor=flavor.bambu");
                }
                if bed.is_some() {
                    anyhow::bail!("export: a template= brings its printer's bed; drop bed=");
                }
                let asset = crate::import3d::source_name(v, "export: template")?;
                let (record, bytes) =
                    crate::import3d::asset_of(&st, &asset, "export: template", &[cadpole_assets::AssetKind::ThreeMf])?;
                let (application, settings) = cadpole_export_3mf::bambu_settings::template_parts(&bytes)
                    .map_err(|e| anyhow::anyhow!("export: template {asset:?} is not a Bambu Studio project: {e}"))?;
                let t = cadpole_export_3mf::bambu_settings::Template::from_parts(&application, &settings)
                    .map_err(|e| anyhow::anyhow!("export: template {asset:?}: {e}"))?;
                // printer= and template= name one machine, or the plates
                // would be laid out for one and printed on the other.
                if let Some(p) = cadpole_print::printers::printer(&printer).and_then(|p| p.bambu.map(|b| (p, b))) {
                    if !t.printer_model().is_empty() && t.printer_model() != p.1.model {
                        anyhow::bail!(
                            "export: template {asset:?} was saved for a {} and printer={:?} is a {}; drop printer= \
                             (the template says which printer) or pass a template saved for the {}",
                            t.printer_model(),
                            p.0.id,
                            p.1.model,
                            p.1.model
                        );
                    }
                }
                Some(cadpole_core::context::BambuTemplate { asset, sha256: record.sha256, application, settings })
            }
        };
        st.ctx.borrow_mut().exports.push(cadpole_core::context::ExportSpec {
            bodies: ids,
            path: path.to_owned(),
            format,
            tolerance,
            flavor,
            plate,
            name: name.map(str::to_owned),
            extruders,
            printer,
            bed,
            template,
        });
        Ok(NoneType)
    }

    /// Ask for a standalone, shareable image of every plate of the Bambu
    /// Studio project `path` -- the plate's parts in their filament colours
    /// standing on a light bed, from a three-quarter view, edges drawn --
    /// plus a contact sheet of every plate side by side with its name under
    /// it. `path` is the `.3mf` the `export(..., flavor=flavor.bambu)` calls
    /// name, and they come first: declare this after them. The host writes
    /// `<stem>_plate_<n>_<name>.png` per plate and `<stem>_plates.png`
    /// beside the project when it runs the script's exports (`cadpole export
    /// --run-exports`).
    ///
    /// `size=(width, height)` in pixels, 1600 x 1200 by default.
    /// `background=` is `"transparent"` (the default: alpha 0 around the
    /// plate), `"studio"` (a mid grey), `"white"`, `"black"` or `"#rrggbb"`.
    /// `backend=` is `"raster"` (the default, milliseconds) or `"path"`
    /// (path traced for a finished shot; seconds to minutes). The project's
    /// own thumbnails are unchanged by this: these are a second, larger
    /// render of the same plate scenes.
    fn plate_images<'v>(
        #[starlark(require = pos)] path: &str,
        #[starlark(require = named)] size: Option<Value<'v>>,
        #[starlark(require = named)] background: Option<&str>,
        #[starlark(require = named)] backend: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        use cadpole_core::context::PlateImageSpec;
        use cadpole_core::export::Flavor;
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        if st.in_test.get() {
            anyhow::bail!("plate_images: plate images cannot be declared inside a test");
        }
        let project = ctx.exports.iter().any(|e| e.path == path && e.flavor == Flavor::Bambu);
        if !project {
            anyhow::bail!(
                "plate_images: no export(..., {path:?}, flavor=flavor.bambu) has been declared yet; \
                 declare the plate images after the project's export() calls"
            );
        }
        if ctx.plate_images.iter().any(|p| p.path == path) {
            anyhow::bail!("plate_images: {path:?} declared twice");
        }
        let (width, height) = match size {
            None => PlateImageSpec::DEFAULT_SIZE,
            Some(v) => {
                let Some(items) = <UnpackListOrTuple<Value<'v>>>::unpack_value(v).map_err(|e| e.into_anyhow())? else {
                    anyhow::bail!("plate_images: size= takes (width, height) in pixels, got {}", v.get_type());
                };
                let dim = |i: usize| -> anyhow::Result<u32> {
                    let v = items.items[i];
                    let n = v
                        .unpack_i32()
                        .ok_or_else(|| anyhow::anyhow!("plate_images: size= takes whole pixels, got {}", v.get_type()))?;
                    if !(16..=8192).contains(&n) {
                        anyhow::bail!("plate_images: size= is 16 to 8192 pixels a side, got {n}");
                    }
                    Ok(n as u32)
                };
                if items.items.len() != 2 {
                    anyhow::bail!("plate_images: size= takes (width, height), got {} values", items.items.len());
                }
                (dim(0)?, dim(1)?)
            }
        };
        let background = match background.map(str::trim).unwrap_or(PlateImageSpec::DEFAULT_BACKGROUND) {
            "transparent" => "transparent".to_owned(),
            b => match cadpole_core::render::Backdrop::parse(b) {
                Some(_) => b.to_owned(),
                None => anyhow::bail!(
                    "plate_images: background={b:?} is not one of \"transparent\", \"studio\", \"white\", \
                     \"black\" or \"#rrggbb\""
                ),
            },
        };
        let backend = match backend.map(str::trim).unwrap_or(PlateImageSpec::DEFAULT_BACKEND) {
            "raster" => "raster".to_owned(),
            "path" => "path".to_owned(),
            b => anyhow::bail!("plate_images: backend={b:?} is not \"raster\" or \"path\""),
        };
        ctx.plate_images.push(PlateImageSpec { path: path.to_owned(), width, height, background, backend });
        Ok(NoneType)
    }

    /// What the user currently has selected in the UI, as a list of refs with
    /// `.kind`, `.feature`, `.body` and `.name`. Empty when nothing is selected.
    fn selection<'v>(eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Vec<KRef>> {
        let st = store(eval)?;
        let ctx = st.ctx.borrow();
        Ok(ctx
            .selection
            .iter()
            .map(|r| KRef {
                feature: r.feature_id.clone(),
                body: r.body_id.clone(),
                kind: r.kind.clone(),
                name: r.topo_name.clone(),
            })
            .collect())
    }

    /// Fail evaluation unless `a == b`. Handy in scripts and `cadpole test`.
    fn assert_eq<'v>(
        #[starlark(require = pos)] a: Value<'v>,
        #[starlark(require = pos)] b: Value<'v>,
        #[starlark(require = named)] msg: Option<&str>,
    ) -> anyhow::Result<NoneType> {
        if !a.equals(b).map_err(|e| e.into_anyhow())? {
            anyhow::bail!("assert_eq failed: {} != {}{}", a, b, msg.map(|m| format!(" ({m})")).unwrap_or_default());
        }
        Ok(NoneType)
    }

    /// Fail evaluation unless `cond` is truthy.
    fn assert_true<'v>(
        #[starlark(require = pos)] cond: Value<'v>,
        #[starlark(require = named)] msg: Option<&str>,
    ) -> anyhow::Result<NoneType> {
        if !cond.to_bool() {
            anyhow::bail!("assert_true failed{}", msg.map(|m| format!(": {m}")).unwrap_or_default());
        }
        Ok(NoneType)
    }

    /// Fail evaluation unless `body` is a solid: a closed surface, every
    /// edge on exactly two triangles wound against each other, normals
    /// outward, and no two triangles passing through or lying on top of each
    /// other. The message names every failing check, so a script that builds
    /// a part with booleans can insist the kernel returned a solid and not
    /// merely a mesh. Checked on the body's tessellation at the document
    /// tolerance, the same triangles `export` and `print_check` see.
    fn assert_solid<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = named)] msg: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let name = ctx.body(body.body_id())?.name.clone();
        let feature = ctx.body(body.body_id())?.feature.clone();
        let report = cadpole_core::solid::validate(&ctx.body_mesh(body.body_id())?.0);
        if !report.is_solid() {
            anyhow::bail!(
                "assert_solid failed: {name} is {}{}",
                report.summary(),
                msg.map(|m| format!(" ({m})")).unwrap_or_default()
            );
        }
        // A solid with a fold in it passes -- it is a solid -- but the
        // script asked to be told about this body, and a fold is the one
        // thing about it the next boolean will trip over. Said at the
        // assertion, where the author is looking.
        for w in report.warnings() {
            ctx.diagnostics.push(
                cadpole_core::context::Diagnostic::warning(format!("assert_solid: {name} is a solid, but carries {w}"))
                    .with_span(span.clone())
                    .with_feature(feature.clone()),
            );
        }
        Ok(NoneType)
    }

    /// Fail evaluation unless `a` and `b` agree to within a tolerance:
    /// `|a - b| <= max(tol, rel * |b|)`. Numbers, or lists and tuples of
    /// numbers compared element by element — nested ones too, so two
    /// `bbox()` results compare directly. `tol` is absolute (mm, mm³, ...),
    /// `rel` a fraction of `b`; the default is a tight absolute match, and
    /// `rel=0.01` is the right shape for anything measured on a
    /// tessellation, whose chord error scales with the size.
    fn assert_close<'v>(
        #[starlark(require = pos)] a: Value<'v>,
        #[starlark(require = pos)] b: Value<'v>,
        #[starlark(require = named, default = UnpackFloat(1e-6))] tol: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.0))] rel: UnpackFloat,
        #[starlark(require = named)] msg: Option<&str>,
    ) -> anyhow::Result<NoneType> {
        let tol = num(&tol, "assert_close: tol")?;
        let rel = num(&rel, "assert_close: rel")?;
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        numbers_of(a, &mut xs, "assert_close: a")?;
        numbers_of(b, &mut ys, "assert_close: b")?;
        let suffix = msg.map(|m| format!(" ({m})")).unwrap_or_default();
        if xs.len() != ys.len() {
            anyhow::bail!(
                "assert_close failed: {a} has {} numbers and {b} has {}{suffix}",
                xs.len(),
                ys.len()
            );
        }
        for (i, (x, y)) in xs.iter().zip(&ys).enumerate() {
            if !close(*x, *y, tol, rel) {
                let at = if xs.len() > 1 { format!(" at index {i}") } else { String::new() };
                anyhow::bail!(
                    "assert_close failed: {a} != {b}{at}: {} vs {} differ by {}, allowed {}{suffix}",
                    fmt_num(*x),
                    fmt_num(*y),
                    fmt_num((x - y).abs()),
                    fmt_num(tol.max(rel * y.abs()))
                );
            }
        }
        Ok(NoneType)
    }

    /// Fail evaluation unless `body`'s volume is `mm3`, to within
    /// `max(tol, rel * mm3)`. Measured on the tessellation like
    /// `body.volume()`, which lies inside a curved surface, so a curved
    /// body measures short, and the smaller its radius the shorter: at the
    /// default 0.05 mm document tolerance a sphere is about 0.3 % short at
    /// r=50, 0.6 % at r=20, 1.1 % at r=10, 1.7 % at r=5 and 6 % at r=1, a
    /// cylinder 0.6 % at r=3 and 2.5 % at r=1 or 2. The 1 % default covers a
    /// sphere from about r=20 up and a cylinder from r=3 up; for smaller
    /// radii pass a wider `rel=` (`rel=0.02` for a 5 to 10 mm ball). A
    /// polyhedron can take `rel=0, tol=1e-6`.
    fn assert_volume<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] mm3: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.0))] tol: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.01))] rel: UnpackFloat,
        #[starlark(require = named)] msg: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let want = num(&mm3, "assert_volume: mm3")?;
        let tol = num(&tol, "assert_volume: tol")?;
        let rel = num(&rel, "assert_volume: rel")?;
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let name = ctx.body(body.body_id())?.name.clone();
        let got = ctx.body_mesh(body.body_id())?.0.volume();
        if !close(got, want, tol, rel) {
            anyhow::bail!(
                "assert_volume failed: {name} is {} mm³, expected {} ± {}{}",
                fmt_num(got),
                fmt_num(want),
                fmt_num(tol.max(rel * want.abs())),
                msg.map(|m| format!(" ({m})")).unwrap_or_default()
            );
        }
        Ok(NoneType)
    }

    /// Fail evaluation unless `body`'s bounding box is `lo`..`hi`, every
    /// coordinate within `tol` mm. The box is measured on the tessellation,
    /// whose vertices lie on the true surface, so a curved body's extent can
    /// fall short of its radius by up to the document tolerance — the
    /// default here is a little wider than that.
    fn assert_bbox<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] lo: VecArg,
        #[starlark(require = pos)] hi: VecArg,
        #[starlark(require = named, default = UnpackFloat(0.1))] tol: UnpackFloat,
        #[starlark(require = named)] msg: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let lo = vec3(&lo, "assert_bbox: lo")?;
        let hi = vec3(&hi, "assert_bbox: hi")?;
        let tol = num(&tol, "assert_bbox: tol")?;
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let name = ctx.body(body.body_id())?.name.clone();
        let (got_lo, got_hi) = ctx
            .body_mesh(body.body_id())?
            .0
            .bbox()
            .ok_or_else(|| anyhow::anyhow!("assert_bbox failed: {name} is empty"))?;
        let off = |a: Vec3, b: Vec3| (a.x - b.x).abs().max((a.y - b.y).abs()).max((a.z - b.z).abs());
        if off(got_lo, lo) > tol || off(got_hi, hi) > tol {
            anyhow::bail!(
                "assert_bbox failed: {name} spans {}..{}, expected {}..{} within {} mm{}",
                fmt_vec(got_lo),
                fmt_vec(got_hi),
                fmt_vec(lo),
                fmt_vec(hi),
                fmt_num(tol),
                msg.map(|m| format!(" ({m})")).unwrap_or_default()
            );
        }
        Ok(NoneType)
    }

    /// Fail evaluation unless `print_check(body, ...)` has nothing to
    /// report: the body is a solid, no wall is thinner than `min_wall`,
    /// nothing overhangs past `max_overhang` and it fits the bed. The
    /// keyword arguments are `print_check`'s, so `assert_printable(b,
    /// printer="bambu_a1")` judges the part against that machine. Every
    /// finding fails it, the ones `print_check` grades as warnings included,
    /// because a test wants a verdict where `print_check` gives advice; the
    /// message lists them all, and none is recorded as a diagnostic.
    ///
    /// A list of bodies is one print, as for `print_check`: the checks
    /// between them run too, so `assert_printable([sun, ring, carrier])` is
    /// the planetary's fit test — no gap below `min_gap`, every horizontal
    /// gap a whole number of layers, nothing printing in the air.
    fn assert_printable<'v>(
        #[starlark(require = pos)] body: Value<'v>,
        #[starlark(require = named)] printer: Option<Value<'v>>,
        #[starlark(require = named)] min_wall: Option<UnpackFloat>,
        #[starlark(require = named)] nozzle: Option<UnpackFloat>,
        #[starlark(require = named)] perimeters: Option<UnpackFloat>,
        #[starlark(require = named)] layer_height: Option<UnpackFloat>,
        #[starlark(require = named)] material: Option<&str>,
        #[starlark(require = named)] max_overhang: Option<UnpackFloat>,
        #[starlark(require = named)] bed: Option<VecArg>,
        #[starlark(require = named)] bed_shape: Option<&str>,
        #[starlark(require = named)] min_gap: Option<UnpackFloat>,
        #[starlark(require = named)] msg: Option<&str>,
        #[starlark(require = named)] max_bridge: Option<UnpackFloat>,
        #[starlark(require = named)] max_overhang_area: Option<UnpackFloat>,
        #[starlark(require = named)] max_cantilever: Option<UnpackFloat>,
        #[starlark(require = named)] max_horizontal_hole: Option<UnpackFloat>,
        #[starlark(require = named)] min_island: Option<UnpackFloat>,
        #[starlark(require = named)] max_slenderness: Option<UnpackFloat>,
        #[starlark(require = named)] slender_width: Option<UnpackFloat>,
        #[starlark(require = named)] max_height_to_base: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let max_bridge = bridge_limit(max_bridge, "assert_printable")?;
        let rules = SupportArgs::new(
            "assert_printable",
            max_overhang_area,
            max_cantilever,
            max_horizontal_hole,
            min_island,
            max_slenderness,
            slender_width,
            max_height_to_base,
        )?;
        let mut profile = resolve_profile(
            "assert_printable",
            printer,
            min_wall,
            nozzle,
            perimeters,
            layer_height,
            material,
            max_overhang,
            bed.as_ref(),
            bed_shape,
            None,
            min_gap,
        )?;
        rules.apply("assert_printable", &mut profile)?;
        let bodies = collect_bodies(&[body], eval.heap(), "assert_printable")?;
        let single = bodies.len() == 1 && body.get_type() != "list" && body.get_type() != "tuple"
            && body.downcast_ref::<crate::assembly::KAssembly>().is_none();
        if bodies.is_empty() {
            anyhow::bail!("assert_printable: the list of bodies is empty");
        }
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut why: Vec<String> = Vec::new();
        let mut names = Vec::with_capacity(bodies.len());
        for b in &bodies {
            let name = ctx.body(b.body_id())?.name.clone();
            let (mesh, topo) = ctx.body_mesh(b.body_id())?.clone();
            let mut report = cadpole_print::check_named(&mesh, Some(&topo), &profile, max_bridge);
            if let Some(issue) = chord_issue_for(&mut ctx, b.body_id(), profile.chord_gap_fraction)? {
                report.push_issue(issue);
            }
            // A bridge is what a slicer does, not a finding; nor is overhang
            // the allowances cover.
            for i in report.issues.iter().filter(|i| i.severity != cadpole_print::Severity::Info) {
                let line = format!("[{}] {}", i.check.as_str(), i.message);
                why.push(if single { line } else { format!("{name}: {line}") });
            }
            names.push(name);
        }
        let ids: Vec<cadpole_core::BodyId> = bodies.iter().map(|b| b.body_id()).collect();
        for f in crate::clearance::assembly_findings(&mut ctx, &ids, &profile)? {
            why.push(format!("{}: [{}] {}", names[f.body], f.issue.check.as_str(), f.issue.message));
        }
        if !why.is_empty() {
            let who = if single { format!("{}: ", names[0]) } else { String::new() };
            anyhow::bail!(
                "assert_printable failed: {who}{}{}",
                why.join("; "),
                msg.map(|m| format!(" ({m})")).unwrap_or_default()
            );
        }
        Ok(NoneType)
    }

    /// Fail evaluation unless `body` prints **without support**: the
    /// support checks of `print_check` find nothing. Those are the overhang
    /// rule past its allowances (`max_overhang` degrees, flat spans up to
    /// `max_bridge` mm held up at both ends, patches up to
    /// `max_overhang_area` mm², ledges sticking out up to `max_cantilever`
    /// mm), `island` (a region whose lowest point is in mid-air, deeper than
    /// `min_island`), `cantilever`, `horizontal_hole` (a round horizontal
    /// hole wider than `max_horizontal_hole` mm with a round top) and, for a
    /// list of bodies printed together, `bridge` (a body in the air). Walls,
    /// bed fit, slender features and the first layer are `assert_printable`'s
    /// business, not this one's.
    ///
    /// The keywords override the printer's rules (`printer=` names one, as
    /// for `print_check`); `build_direction=` judges the part another way up.
    /// The failure lists every finding on a line of its own, each with its
    /// position and the face it is on.
    fn assert_support_free<'v>(
        #[starlark(require = pos)] body: Value<'v>,
        #[starlark(require = named)] printer: Option<Value<'v>>,
        #[starlark(require = named)] layer_height: Option<UnpackFloat>,
        #[starlark(require = named)] max_overhang: Option<UnpackFloat>,
        #[starlark(require = named)] max_bridge: Option<UnpackFloat>,
        #[starlark(require = named)] max_overhang_area: Option<UnpackFloat>,
        #[starlark(require = named)] max_cantilever: Option<UnpackFloat>,
        #[starlark(require = named)] max_horizontal_hole: Option<UnpackFloat>,
        #[starlark(require = named)] min_island: Option<UnpackFloat>,
        #[starlark(require = named)] build_direction: Option<VecArg>,
        #[starlark(require = named)] msg: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let what = "assert_support_free";
        let max_bridge = bridge_limit(max_bridge, what)?;
        let rules =
            SupportArgs::new(what, max_overhang_area, max_cantilever, max_horizontal_hole, min_island, None, None, None)?;
        let mut profile = resolve_profile(
            what,
            printer,
            None,
            None,
            None,
            layer_height,
            None,
            max_overhang,
            None,
            None,
            build_direction.as_ref(),
            None,
        )?;
        rules.apply(what, &mut profile)?;
        let bodies = collect_bodies(&[body], eval.heap(), what)?;
        if bodies.is_empty() {
            anyhow::bail!("{what}: the list of bodies is empty");
        }
        let single = bodies.len() == 1 && body.get_type() != "list" && body.get_type() != "tuple"
            && body.downcast_ref::<crate::assembly::KAssembly>().is_none();
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut why: Vec<String> = Vec::new();
        let mut names = Vec::with_capacity(bodies.len());
        for b in &bodies {
            let name = ctx.body(b.body_id())?.name.clone();
            let (mesh, topo) = ctx.body_mesh(b.body_id())?.clone();
            let report = cadpole_print::check_named(&mesh, Some(&topo), &profile, max_bridge);
            for i in report.support_findings() {
                why.push(format!("{name}: [{}] {}", i.check.as_str(), i.message));
            }
            names.push(name);
        }
        if !single {
            let ids: Vec<cadpole_core::BodyId> = bodies.iter().map(|b| b.body_id()).collect();
            for f in crate::clearance::assembly_findings(&mut ctx, &ids, &profile)? {
                if f.issue.check.is_support() {
                    why.push(format!("{}: [{}] {}", names[f.body], f.issue.check.as_str(), f.issue.message));
                }
            }
        }
        if !why.is_empty() {
            anyhow::bail!(
                "{what} failed{}: {} finding{} need support\n  - {}",
                msg.map(|m| format!(" ({m})")).unwrap_or_default(),
                why.len(),
                if why.len() == 1 { "" } else { "s" },
                why.join("\n  - ")
            );
        }
        Ok(NoneType)
    }

    /// Declare a test of the model. `fn` runs after the whole script has
    /// built the model, with every body and parameter still in scope, and
    /// fails by raising: `assert_*`, `fail()`, or any script error. Nothing
    /// runs in an ordinary evaluation, so a test costs the viewer nothing
    /// and may be as expensive as it likes; `cadpole test` runs them and the
    /// example suite runs the examples'.
    ///
    /// `sweep={"blades": [3, 5, 7, 12]}` re-evaluates the document at each
    /// listed value — the cartesian product, for several parameters — and
    /// runs the test at each point; the report names the point that failed,
    /// and a point at which the model itself does not build is a failure at
    /// that point. Values take the forms a `param` default does. A sweep is
    /// capped at 256 points.
    ///
    /// `monotone="increasing"` (or `"decreasing"`) is a claim across a
    /// sweep over one parameter: the test returns a number, and the claim
    /// fails unless it strictly rises (falls) from point to point — "more
    /// blades, more volume". The failure names both points.
    fn test<'v>(
        #[starlark(require = pos)] name: &str,
        #[starlark(require = pos)] func: Value<'v>,
        #[starlark(require = named)] sweep: Option<Value<'v>>,
        #[starlark(require = named)] monotone: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        modeltest::register(eval, name, func, sweep, monotone)?;
        Ok(NoneType)
    }

    /// An external file the document carries, by the name it was added under
    /// (`asset("logo.svg")`). Scripts never see file paths: the name is
    /// resolved in the document's asset table and the bytes are pinned by
    /// SHA-256, so the same script gives the same model in the CLI, in the
    /// browser and on a render host.
    ///
    /// `cadpole eval main.star` picks up `./assets/<name>` next to the script.
    /// Pass `sha256=` to assert the version the script was written against;
    /// a mismatch is a warning, never an error.
    fn asset<'v>(
        #[starlark(require = pos)] name: &str,
        #[starlark(require = named)] sha256: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KAsset> {
        let span = call_span(eval);
        let st = store(eval)?;
        let Some(a) = st.assets.record(name) else {
            let have = st.assets.names();
            anyhow::bail!(
                "asset {name:?} not found (available: {})",
                if have.is_empty() { "none".to_owned() } else { have.join(", ") }
            );
        };
        if let Some(want) = sha256 {
            if !want.eq_ignore_ascii_case(&a.sha256) {
                st.ctx.borrow_mut().diagnostics.push(
                    cadpole_core::context::Diagnostic::warning(format!(
                        "asset {name:?} has sha256 {} but the script expects {want}; it has \
                         changed since this script was written",
                        a.sha256
                    ))
                    .with_span(span),
                );
            }
        }
        Ok(KAsset {
            name: a.name,
            sha256: a.sha256,
            mime: a.mime,
            kind: a.kind.name().to_owned(),
            size: a.size,
        })
    }

    /// Import an SVG as a sketch. Filled shapes become loops (nested ones
    /// become holes); circles, rounded corners and `A` commands with equal
    /// radii stay real arcs, so a hole extrudes to a cylinder.
    ///
    /// `ids=["badge"]` imports only those elements (a group brings its
    /// subtree); `layer=` takes an Inkscape layer by label. Units come from
    /// the file's `width`/`height` when they carry one, else CSS pixels at
    /// `dpi` (Illustrator exports at 72). `flip_y` mirrors SVG's downward y
    /// into sketch y-up. `align` places the result's bounding box at `at`.
    fn import_svg<'v>(
        #[starlark(require = pos)] asset: &KAsset,
        #[starlark(require = named)] ids: Option<UnpackList<String>>,
        #[starlark(require = named)] layer: Option<&str>,
        #[starlark(require = named)] plane: Option<&KPlane>,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] align: Option<&str>,
        #[starlark(require = named, default = UnpackFloat(1.0))] scale: UnpackFloat,
        #[starlark(require = named)] units: Option<&str>,
        #[starlark(require = named, default = UnpackFloat(96.0))] dpi: UnpackFloat,
        #[starlark(require = named, default = true)] flip_y: bool,
        #[starlark(require = named)] fill_rule: Option<&str>,
        #[starlark(require = named, default = true)] merge: bool,
        #[starlark(require = named, default = UnpackFloat(0.01))] tolerance: UnpackFloat,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        num(&scale, "import_svg: scale")?;
        num(&dpi, "import_svg: dpi")?;
        num(&tolerance, "import_svg: tolerance")?;
        if scale.0 <= 0.0 {
            anyhow::bail!("import_svg: scale must be positive, got {}", scale.0);
        }
        if dpi.0 <= 0.0 {
            anyhow::bail!("import_svg: dpi must be positive, got {}", dpi.0);
        }
        let opts = cadpole_import_2d::svg::Options {
            ids: strings(ids),
            layer: layer.map(str::to_owned),
            units: units.unwrap_or("auto").to_owned(),
            dpi: dpi.0,
            scale: scale.0,
            flip_y,
            fill_rule: fill_rule.unwrap_or("auto").to_owned(),
            merge,
            tolerance: tolerance.0.max(1e-6),
        };
        if !matches!(opts.fill_rule.as_str(), "auto" | "nonzero" | "evenodd") {
            anyhow::bail!(
                "import_svg: fill_rule {:?} must be \"auto\", \"nonzero\" or \"evenodd\"",
                opts.fill_rule
            );
        }
        let bytes = {
            let st = store(eval)?;
            asset_bytes(st, &asset.name, "import_svg")?
        };
        let im = cadpole_import_2d::svg::import(&bytes, &opts)
            .map_err(|e| anyhow::anyhow!("import_svg({:?}): {e}", asset.name))?;
        let at = at.as_ref().map(|a| p2(a, "at")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        let mut args = BTreeMap::new();
        args.insert("asset".into(), asset.name.clone());
        args.insert("sha256".into(), asset.sha256.clone());
        if !opts.ids.is_empty() {
            args.insert("ids".into(), opts.ids.join(", "));
        }
        if let Some(l) = &opts.layer {
            args.insert("layer".into(), l.clone());
        }
        args.insert("units".into(), opts.units.clone());
        args.insert("scale".into(), fmt_num(opts.scale));
        args.insert("flip_y".into(), flip_y.to_string());
        finish_import(
            "import_svg",
            im,
            plane.map(|p| p.0).unwrap_or(Plane::XY),
            at,
            align.unwrap_or("min"),
            args,
            name,
            eval,
        )
    }

    // `import_dxf` lives in `crate::import2d`: it returns a
    // `Drawing2D`, which stands in for a Sketch where one is expected.

    /// Set `string` as a sketch: shaped glyph outlines as loops, ready to
    /// extrude into a raised label or subtract into an engraved one.
    ///
    /// `font` is `"builtin"` (Inter, bundled), an `asset("MyFont.ttf")`
    /// pinned by hash, or `"system:Helvetica"` (native only, and it warns —
    /// a model that depends on the local font set is not the same model
    /// elsewhere). `size` is the em size in millimetres; the `text` feature
    /// records the face's `cap_height` (how tall the capitals actually are,
    /// 0.7275 em in the bundled Inter) and the run's `width`. Kerning and
    /// ligatures are applied. `\n` starts a new line.
    fn text<'v>(
        #[starlark(require = pos)] string: &str,
        #[starlark(require = named)] font: Option<Value<'v>>,
        #[starlark(require = named, default = UnpackFloat(10.0))] size: UnpackFloat,
        #[starlark(require = named)] align: Option<&str>,
        #[starlark(require = named)] baseline: Option<&str>,
        #[starlark(require = named, default = UnpackFloat(0.0))] spacing: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(1.2))] line_height: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(400.0))] weight: UnpackFloat,
        #[starlark(require = named)] features: Option<UnpackList<String>>,
        #[starlark(require = named, default = true)] merge: bool,
        #[starlark(require = named)] plane: Option<&KPlane>,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        num(&size, "text: size")?;
        num(&spacing, "text: spacing")?;
        num(&line_height, "text: line_height")?;
        num(&weight, "text: weight")?;
        if size.0 <= 0.0 {
            anyhow::bail!("text: size must be positive, got {}", size.0);
        }
        // Shaping plus the union of every glyph outline is a single native
        // operation, so the deadline cannot interrupt it, and its cost grows
        // faster than the string: 2 000 characters measured 2.4 s of CPU,
        // 8 000 took 55 s, 32 000 ran for minutes. A 1 MB script can carry a
        // million characters, so the length is capped here rather than left
        // to the clock that cannot see inside the call.
        let chars = string.chars().count();
        if chars > Limits::MAX_TEXT_CHARS {
            anyhow::bail!(
                "text: {chars} characters is over the limit of {}. Shaping and unioning glyph \
                 outlines is one operation that no time limit can interrupt, and its cost grows \
                 faster than the text; set several shorter labels instead",
                Limits::MAX_TEXT_CHARS
            );
        }
        // Resolve the face before anything else, so a bad font is one clean
        // error rather than an empty sketch.
        let (font_bytes, mut notes, font_desc) = {
            let st = store(eval)?;
            match font {
                None => {
                    let (b, n) = cadpole_font::load(cadpole_font::Source::Builtin)
                        .map_err(|e| anyhow::anyhow!("text: {e}"))?;
                    (b, n, cadpole_font::BUILTIN_NAME.to_owned())
                }
                Some(v) => {
                    if let Some(a) = v.downcast_ref::<KAsset>() {
                        let bytes = asset_bytes(st, &a.name, "text")?;
                        let (b, n) = cadpole_font::load(cadpole_font::Source::Bytes(&bytes))
                            .map_err(|e| anyhow::anyhow!("text: {e}"))?;
                        (b, n, a.name.clone())
                    } else if let Some(s) = v.unpack_str() {
                        match s {
                            "builtin" | "sans" | "default" => {
                                let (b, n) = cadpole_font::load(cadpole_font::Source::Builtin)
                                    .map_err(|e| anyhow::anyhow!("text: {e}"))?;
                                (b, n, cadpole_font::BUILTIN_NAME.to_owned())
                            }
                            other => match other.strip_prefix("system:") {
                                Some(family) => {
                                    let (b, n) = cadpole_font::load(cadpole_font::Source::System(family))
                                        .map_err(|e| anyhow::anyhow!("text: {e}"))?;
                                    (b, n, other.to_owned())
                                }
                                None => anyhow::bail!(
                                    "text: font {other:?} must be \"builtin\", \
                                     \"system:<family>\" or an asset(...)"
                                ),
                            },
                        }
                    } else {
                        anyhow::bail!("text: font must be a string or an Asset, got {}", v.get_type());
                    }
                }
            }
        };

        let opts = cadpole_font::Options {
            size: size.0,
            align: align.unwrap_or("left").to_owned(),
            baseline: baseline.unwrap_or("alphabetic").to_owned(),
            spacing: spacing.0,
            line_height: line_height.0,
            weight: weight.0,
            axes: Default::default(),
            features: strings(features),
            merge,
        };
        let t = cadpole_font::text(&font_bytes, string, &opts)
            .map_err(|e| anyhow::anyhow!("text({string:?}): {e}"))?;
        notes.extend(t.notes);

        let at = at.as_ref().map(|a| p2(a, "at")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        let mut im = cadpole_import_2d::Import {
            loops: t.loops,
            paths: Vec::new(),
            notes: notes
                .into_iter()
                .map(|n| cadpole_import_2d::Note {
                    level: if n.warning { cadpole_import_2d::Level::Warning } else { cadpole_import_2d::Level::Info },
                    message: n.message,
                })
                .collect(),
        };
        // Text is placed by its baseline and alignment, not its bounding box.
        if at.x != 0.0 || at.y != 0.0 {
            im.map_points(move |p| P2::new(p.x + at.x, p.y + at.y));
        }
        let label = cadpole_core::context::TextLabel {
            text: string.to_owned(),
            font: font_desc.clone(),
            size: size.0,
            cap_height: t.cap_height,
            align: opts.align.clone(),
            baseline: opts.baseline.clone(),
            weight: weight.0,
            line_height: line_height.0,
            plain: spacing.0 == 0.0 && opts.features.is_empty(),
            plane: plane.map(|p| p.0).unwrap_or(Plane::XY),
            at,
        };
        let mut args = BTreeMap::new();
        args.insert("text".into(), string.to_owned());
        args.insert("font".into(), font_desc);
        args.insert("size".into(), fmt_num(size.0));
        args.insert("cap_height".into(), fmt_num((t.cap_height * 1000.0).round() / 1000.0));
        args.insert("width".into(), fmt_num((t.width * 1000.0).round() / 1000.0));
        args.insert("align".into(), opts.align.clone());
        if weight.0 != 400.0 {
            args.insert("weight".into(), fmt_num(weight.0));
        }
        let sketch = finish_import(
            "text",
            im,
            plane.map(|p| p.0).unwrap_or(Plane::XY),
            P2::new(0.0, 0.0),
            "none",
            args,
            name,
            eval,
        )?;
        store(eval)?.ctx.borrow_mut().text_labels.insert(sketch.feature.clone(), label);
        Ok(sketch)
    }

    /// Emit a warning diagnostic without stopping evaluation.
    fn warn<'v>(#[starlark(require = pos)] message: &str, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<NoneType> {
        let span = call_span(eval);
        let st = store(eval)?;
        st.ctx.borrow_mut().diagnostics.push(cadpole_core::context::Diagnostic::warning(message).with_span(span));
        Ok(NoneType)
    }

    /// The minimum distance between two bodies' surfaces, and where it is:
    /// the fit test a print-in-place part needs, which `intersect` cannot
    /// be (an empty result is an error). Returns a `Clearance` with
    /// `.distance` (mm; 0 when the bodies touch or overlap), `.overlap`
    /// (they share material — one crosses or sits inside the other) and
    /// `.touching` (zero distance without shared material), `.at` (the
    /// closest pair of points, `(a_point, b_point)`, also as `.a_point` and
    /// `.b_point`), `.face_a` / `.face_b` (the named faces those points lie
    /// on) and `.tolerance`.
    ///
    /// Measured on both bodies' tessellations at the document tolerance, the
    /// triangles `export` writes: a chord lies inside its true surface by up
    /// to that tolerance, so each surface may be off by `.tolerance` and
    /// the true gap lies within twice it of `.distance`; planar faces are
    /// exact. A body inside another's cavity is not an overlap. Two
    /// 100 000-triangle bodies take about 60 ms for a point of contact and
    /// about 0.6 s for a ring of contact all the way round.
    fn distance<'v>(
        #[starlark(require = pos)] a: Value<'v>,
        #[starlark(require = pos)] b: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KClearance> {
        // An Instance or an Assembly stands for its posed bodies:
        // the answer is the closest pair over every body of one against
        // every body of the other, measured on the assembled position
        // whatever an exploded view shows.
        let heap = eval.heap();
        let (av, bv) = (collect_bodies(&[a], heap, "distance")?, collect_bodies(&[b], heap, "distance")?);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut best: Option<KClearance> = None;
        for x in &av {
            for y in &bv {
                let c = crate::clearance::measure(&mut ctx, x.body_id(), y.body_id())?;
                let closer = match &best {
                    None => true,
                    Some(b) => c.overlap && !b.overlap || (c.overlap == b.overlap && c.distance < b.distance),
                };
                if closer {
                    best = Some(c);
                }
            }
        }
        best.ok_or_else(|| anyhow::anyhow!("distance: nothing to measure"))
    }

    /// Every region where the gap between `a` and `b` is below `below` mm,
    /// as a list of `(distance, a_point, b_point, face_a, face_b)` tuples,
    /// closest first. Space is divided into `spacing` mm cells (default 1)
    /// and each cell keeps its closest pair, so a pin in a bore reports the
    /// ring once per millimetre rather than once per chord; `limit` (default
    /// 256) caps the list, and when it does a warning says how many regions
    /// there were. Measured as `distance` is, with the same uncertainty.
    fn clearance_map<'v>(
        #[starlark(require = pos)] a: Value<'v>,
        #[starlark(require = pos)] b: Value<'v>,
        #[starlark(require = named)] below: UnpackFloat,
        #[starlark(require = named)] spacing: Option<UnpackFloat>,
        #[starlark(require = named)] limit: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let below = num(&below, "clearance_map: below")?;
        if below <= 0.0 {
            anyhow::bail!("clearance_map: below must be a positive distance in mm, got {below}");
        }
        let spacing = opt_num(&spacing, "clearance_map: spacing")?.unwrap_or(1.0);
        if spacing <= 0.0 {
            anyhow::bail!("clearance_map: spacing must be a positive distance in mm, got {spacing}");
        }
        let limit = match limit {
            Some(v) => check::count(&v, "clearance_map: limit", 1, 100_000)?,
            None => 256,
        };
        let span = call_span(eval);
        let heap = eval.heap();
        // A one-body Instance stands for its posed body; the map is
        // between two surfaces, so a many-body value is asked to name one.
        let one = |v: Value<'v>, which: &str| -> anyhow::Result<KBody> {
            let bs = collect_bodies(&[v], heap, "clearance_map")?;
            match bs.len() {
                1 => Ok(bs.into_iter().next().unwrap()),
                n => anyhow::bail!("clearance_map: {which} stands for {n} bodies; pass one body (inst.body(\"...\"))"),
            }
        };
        let (a, b) = (one(a, "the first argument")?, one(b, "the second argument")?);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let (entries, regions) = crate::clearance::map(&mut ctx, a.body_id(), b.body_id(), below, spacing, limit)?;
        if regions > entries.len() {
            ctx.diagnostics.push(
                cadpole_core::context::Diagnostic::warning(format!(
                    "clearance_map({}, {}): {} of {} regions below {} mm shown; pass limit= for more, or a larger spacing= to merge neighbours",
                    a.name,
                    b.name,
                    entries.len(),
                    regions,
                    fmt_num(below)
                ))
                .with_span(span),
            );
        }
        let items: Vec<Value<'v>> = entries
            .iter()
            .map(|e| {
                heap.alloc(starlark::values::tuple::AllocTuple([
                    heap.alloc(e.distance),
                    alloc_vec3(heap, e.a_point),
                    alloc_vec3(heap, e.b_point),
                    heap.alloc(e.face_a.as_str()),
                    heap.alloc(e.face_b.as_str()),
                ]))
            })
            .collect();
        Ok(heap.alloc(items))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use cadpole_core::context::{Context, Severity};
    use cadpole_core::params::ParamValue;

    use crate::{EvalInput, ScriptFile, evaluate};

    fn eval_with(src: &str, overrides: &[(&str, ParamValue)]) -> Context {
        let overrides: HashMap<String, ParamValue> =
            overrides.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
        evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            overrides,
            ..Default::default()
        })
    }

    fn messages(ctx: &Context, severity: Severity) -> Vec<String> {
        ctx.diagnostics.iter().filter(|d| d.severity == severity).map(|d| d.message.clone()).collect()
    }

    /// An int param stays an int under a whole-number override (docs/starlark-api.md, 4.1:
    /// "Integer defaults return `int`"), also past i32, where it used to come
    /// back as a float. A fractional override is still honoured as a float:
    /// a whole-number default is as often a dimension as a count.
    #[test]
    fn an_int_param_stays_an_int_whatever_the_override() {
        let src = "n = param(\"n\", 3)\nprint(type(n), n)\nfor i in range(n):\n    pass\n";
        // A whole-number override overrides, and stays an int even past i32.
        let ctx = eval_with(src, &[("n", ParamValue::Number(4.0))]);
        assert!(messages(&ctx, Severity::Warning).is_empty(), "{:?}", messages(&ctx, Severity::Warning));
        assert!(messages(&ctx, Severity::Info).iter().any(|m| m == "print: int 4"), "{:?}", ctx.diagnostics);
        let big = "n = param(\"n\", 3)\nprint(type(n), n)\n";
        let ctx = eval_with(big, &[("n", ParamValue::Number(4294967296.0))]);
        assert!(messages(&ctx, Severity::Error).is_empty(), "{:?}", messages(&ctx, Severity::Error));
        assert!(
            messages(&ctx, Severity::Info).iter().any(|m| m == "print: int 4294967296"),
            "{:?}",
            ctx.diagnostics
        );
    }

    /// A colour tuple is three finite numbers. NaN and infinity went
    /// straight into the material (`eval --json` showed `null`s in
    /// `base_color` and `emissive`) while every scalar material argument
    /// refused them: `material(roughness=nan)` was an error,
    /// `material(color=(nan, 0, 0))` was not.
    #[test]
    fn a_colour_with_nan_or_infinity_is_refused() {
        for call in [
            "b.material(color=(float(\"nan\"), 0.5, 0.5))",
            "b.material(emissive=(0.0, float(\"inf\"), 1.0))",
            "paint(b, color=(0.2, float(\"-inf\"), 0.2))",
            "layer(\"l\", color=(float(\"nan\"), 0, 0))",
        ] {
            let ctx = eval_with(&format!("b = box(1, 2, 3)\n{call}\n"), &[]);
            let errs = messages(&ctx, Severity::Error);
            assert!(errs.iter().any(|e| e.contains("not a finite number")), "{call}: {errs:?}");
        }
        // Emissive above 1 stays allowed: it is a light, not a reflectance.
        let ctx = eval_with("b = box(1, 2, 3)\nb.material(color=(0.2, 0.4, 0.6), emissive=(4, 0, 0))\n", &[]);
        assert!(messages(&ctx, Severity::Error).is_empty(), "{:?}", messages(&ctx, Severity::Error));
    }

    /// The first ```python block after the heading line that contains
    /// `heading`, in docs/starlark-api.md (a data dependency of this test).
    fn reference_block(heading: &str) -> String {
        let doc = std::fs::read_to_string("docs/starlark-api.md").expect("docs/starlark-api.md is in the test's runfiles");
        let at = doc
            .lines()
            .position(|l| l.starts_with('#') && l.contains(heading))
            .unwrap_or_else(|| panic!("no heading containing {heading:?}"));
        let mut lines = doc.lines().skip(at + 1).skip_while(|l| l.trim() != "```python").skip(1);
        let block: Vec<&str> = lines.by_ref().take_while(|l| l.trim() != "```").collect();
        assert!(!block.is_empty(), "no python block under {heading:?}");
        block.join("\n") + "\n"
    }

    /// Code blocks in the reference that say they run must run as written.
    /// Each of these failed: `fillet(face.edges, r)` (a Face's `.edges` are
    /// names, which do not remember their body), `hole/side#0` on a cylinder
    /// centred on the block's corner (only its second half-face survives
    /// the cut), `subtract(body, *holes, name=)` (starlark-rust rejects a
    /// keyword after `*args`) and the vase at `resolution=0.35` (over the
    /// mesher's cell cap).
    #[test]
    fn reference_examples_that_say_they_run_do_run() {
        for heading in [
            "`fillet(target, radius",
            "Topological name grammar",
            "8.2 Bracket with fillets",
            "8.4 Organic vase",
        ] {
            let src = reference_block(heading);
            let ctx = eval_with(&src, &[]);
            let errs = messages(&ctx, Severity::Error);
            assert!(errs.is_empty(), "the block under {heading:?} fails:\n{src}\n{errs:?}");
        }
    }

    fn doc_of(name: &str) -> String {
        crate::api_doc().functions.into_iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no {name}")).doc
    }

    /// The tessellation is inscribed, so a small curved body measures short
    /// by more than the 1 % default: `assert_volume(sphere(5), 4/3 pi 125)`
    /// fails at -1.7 %. The docstring claimed the default "covers the chord
    /// error of a curved body"; it now says when it does and what to pass
    /// when it does not, and this pins both halves of what it says.
    #[test]
    fn assert_volume_says_when_the_default_covers_a_curved_body() {
        let doc = doc_of("assert_volume");
        assert!(!doc.contains("covers the chord error of a"), "the unqualified claim is gone: {doc}");
        assert!(doc.contains("short") && doc.contains("rel="), "{doc}");
        let ok = |src: &str| {
            let ctx = eval_with(src, &[]);
            messages(&ctx, Severity::Error)
        };
        // Large curved bodies: the default holds.
        assert_eq!(ok("assert_volume(sphere(20), 4 / 3 * math.pi * 20 * 20 * 20)\n"), Vec::<String>::new());
        assert_eq!(ok("assert_volume(cylinder(5, 10), math.pi * 25 * 10)\n"), Vec::<String>::new());
        // A 10 mm ball is just past it (1.1 % short), a 5 mm one well past;
        // the documented rel= covers both.
        assert!(!ok("assert_volume(sphere(10), 4 / 3 * math.pi * 1000)\n").is_empty());
        assert_eq!(ok("assert_volume(sphere(10), 4 / 3 * math.pi * 1000, rel=0.02)\n"), Vec::<String>::new());
        assert!(!ok("assert_volume(sphere(5), 4 / 3 * math.pi * 125)\n").is_empty());
        assert_eq!(ok("assert_volume(sphere(5), 4 / 3 * math.pi * 125, rel=0.02)\n"), Vec::<String>::new());
        assert_eq!(ok("assert_volume(sphere(1), 4 / 3 * math.pi, rel=0.07)\n"), Vec::<String>::new());
    }

    /// A print profile is taken as given and checked: a nozzle of 0, a zero
    /// layer height, a negative min_wall, an overhang past 90 degrees or a
    /// build direction of (0, 0, 0) used to be dropped in favour of the
    /// default, so the check ran clean on the wrong printer.
    #[test]
    fn print_check_refuses_a_profile_that_cannot_be() {
        for args in [
            "nozzle=0",
            "layer_height=0",
            "min_wall=-1",
            "max_overhang=-10",
            "max_overhang=120",
            "build_direction=(0, 0, 0)",
        ] {
            let ctx = eval_with(&format!("b = box(10, 10, 10)\nprint_check(b, {args})\n"), &[]);
            let errs = messages(&ctx, Severity::Error);
            assert!(errs.iter().any(|e| e.contains("print_check")), "{args}: {errs:?}");
        }
        let ctx = eval_with("b = box(10, 10, 10)\nprint_check(b, nozzle=0.6, min_wall=0)\n", &[]);
        assert!(messages(&ctx, Severity::Error).is_empty(), "{:?}", messages(&ctx, Severity::Error));
    }

    /// `text()` said "`.cap_height` of the result tells you how tall the
    /// capitals actually are", but a Sketch has no such attribute; the
    /// number is recorded on the `text` feature (as docs/starlark-api.md
    /// says), and the docstring now says that.
    #[test]
    fn text_documents_cap_height_where_it_is() {
        let doc = doc_of("text");
        assert!(!doc.contains("`.cap_height` of the"), "{doc}");
        assert!(doc.contains("cap_height") && doc.contains("feature"), "{doc}");
        let ctx = eval_with("s = text(\"A\", size=10)\n", &[]);
        assert!(messages(&ctx, Severity::Error).is_empty());
        let f = ctx.features.iter().find(|f| f.kind == "text").expect("a text feature");
        assert!(f.args.contains_key("cap_height"), "{:?}", f.args);
        let ctx = eval_with("print(text(\"A\", size=10).cap_height)\n", &[]);
        assert!(!messages(&ctx, Severity::Error).is_empty(), "a Sketch still has no .cap_height");
    }
}
