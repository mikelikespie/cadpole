//! The 2D toolkit's Starlark surface: `Sketch.offset`, the 2D
//! booleans, `fillet2d`/`chamfer2d`, `mirror2d` and `pattern2d_*`, each as
//! a global and as a `Sketch` method — the same feature either way, as the
//! transforms are for bodies. Thin on purpose: every call is one
//! call into `cadpole_core::sketch2d`, one feature recorded with the source
//! sketch as its input, and the notes the core hands back (a loop that
//! collapsed, an empty intersection) attached to that feature as info
//! diagnostics. Nothing here knows how an offset is computed.

use std::collections::BTreeMap;

use cadpole_core::FeatureId;
use cadpole_core::context::{Diagnostic, fmt_num};
use cadpole_core::sketch::{P2, Sketch};
use cadpole_core::sketch2d::{self, BoolOp, CornerSelect, Join, Outcome, Xf2};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::float::UnpackFloat;
use starlark::values::list::UnpackList;
use starlark::values::{UnpackValue, Value};

use crate::check::{self, VecArg, num, p2};
use crate::globals::sketch_arg;
use crate::limits::Limits;
use crate::store::{call_span, store};
use crate::values::KSketch;

/// The source of a toolkit operation: a `Sketch` or a `ConstrainedSketch`
/// (solved here, as `extrude` solves it), as its feature id and geometry.
pub(crate) fn source<'v>(v: Value<'v>, what: &str, eval: &Evaluator<'v, '_, '_>) -> anyhow::Result<(String, Sketch)> {
    let (fid, sketch) = sketch_arg(v, what, eval)?;
    Ok((fid.0, sketch))
}

/// The document's tolerance in mm: what a spline is sampled to on its way
/// through the clipper and re-fitted to on its way out.
fn tolerance(eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<f64> {
    Ok(store(eval)?.ctx.borrow().tolerance)
}

/// Record the operation as a feature of `kind` with `inputs`, attach the
/// core's notes to it, and hand back the sketch bound to the new feature.
fn record(
    kind: &str,
    name: Option<&str>,
    inputs: &[&str],
    mut args: BTreeMap<String, String>,
    outcome: Outcome,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KSketch> {
    let span = call_span(eval);
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    args.insert("loops".into(), outcome.sketch.loops.len().to_string());
    let inputs = inputs.iter().map(|s| FeatureId((*s).to_owned())).collect();
    let fid = ctx.new_feature(kind, name.map(str::to_owned), inputs, args, span.clone())?;
    for note in outcome.notes {
        let d = Diagnostic::info(note).with_span(span.clone()).with_feature(fid.clone());
        ctx.feature_mut(&fid)?.diagnostics.push(d.clone());
        ctx.diagnostics.push(d);
    }
    Ok(KSketch::new(fid.0, outcome.sketch))
}

pub(crate) fn offset_op(
    src: (String, Sketch),
    d: f64,
    join: Option<&str>,
    name: Option<&str>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KSketch> {
    let join = match join {
        Some(s) => Join::parse(s).map_err(|e| anyhow::anyhow!("offset: {e}"))?,
        None => Join::Round,
    };
    let outcome = sketch2d::offset(&src.1, &src.0, d, join, tolerance(eval)?)?;
    let mut args = BTreeMap::new();
    args.insert("distance".into(), fmt_num(d));
    args.insert("join".into(), join.name().to_owned());
    record("offset2d", name, &[&src.0], args, outcome, eval)
}

pub(crate) fn boolean_op(
    a: (String, Sketch),
    b: (String, Sketch),
    op: BoolOp,
    name: Option<&str>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KSketch> {
    let outcome = sketch2d::boolean(&a.1, &a.0, &b.1, &b.0, op, tolerance(eval)?)?;
    let mut args = BTreeMap::new();
    args.insert("other".into(), b.0.clone());
    record(&format!("{}2d", op.name()), name, &[&a.0, &b.0], args, outcome, eval)
}

/// The names of a sketch's corners, `<loop>/corner[k]`, in loop order.
pub(crate) fn corner_names(src: &(String, Sketch)) -> Vec<String> {
    sketch2d::corners(&src.1, &src.0).into_iter().map(|c| c.name).collect()
}

/// `corners=`: `None` is every corner; otherwise a list whose items are a
/// name (`"corner[2]"` matches that corner on every loop, a longer suffix
/// or the full name narrows it), an index (the corner on a one-loop
/// sketch), or a `(loop, corner)` pair.
fn resolve_corners<'v>(src: &(String, Sketch), what: &str, sel: Option<Value<'v>>) -> anyhow::Result<CornerSelect> {
    let Some(v) = sel else { return Ok(CornerSelect::All) };
    let all = sketch2d::corners(&src.1, &src.0);
    let listing = || all.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ");
    let items = UnpackList::<Value<'v>>::unpack_value(v)
        .map_err(|e| e.into_anyhow())?
        .ok_or_else(|| anyhow::anyhow!("{what}: corners= must be a list of corner names, indices or (loop, corner) pairs, got {}", v.get_type()))?;
    let mut picked = Vec::new();
    for item in items.items {
        if let Some(s) = item.unpack_str() {
            let hits: Vec<(usize, usize)> = all
                .iter()
                .filter(|c| c.name == s || c.name.ends_with(&format!("/{s}")))
                .map(|c| (c.loop_idx, c.vertex))
                .collect();
            if hits.is_empty() {
                anyhow::bail!("{what}: no corner named {s:?}; the corners are {}", listing());
            }
            picked.extend(hits);
        } else if let Some(k) = item.unpack_i32() {
            if src.1.loops.len() != 1 {
                anyhow::bail!(
                    "{what}: corner {k} needs a loop when the sketch has {} loops; pass (loop, corner) or a name from {}",
                    src.1.loops.len(),
                    listing()
                );
            }
            if k < 0 {
                anyhow::bail!("{what}: a corner index cannot be negative (got {k})");
            }
            picked.push((0, k as usize));
        } else if let Some(pair) = VecArg::unpack_value(item).map_err(|e| e.into_anyhow())? {
            match pair.items.as_slice() {
                [l, k] if l.0 >= 0.0 && k.0 >= 0.0 && l.0.fract() == 0.0 && k.0.fract() == 0.0 => picked.push((l.0 as usize, k.0 as usize)),
                _ => anyhow::bail!("{what}: a corner pair is (loop, corner), two non-negative integers"),
            }
        } else {
            anyhow::bail!("{what}: corners= items are names, indices or (loop, corner) pairs, got {}", item.get_type());
        }
    }
    Ok(CornerSelect::Some(picked))
}

pub(crate) fn corner_op<'v>(
    src: (String, Sketch),
    size: f64,
    corners: Option<Value<'v>>,
    chamfer: bool,
    name: Option<&str>,
    eval: &Evaluator<'v, '_, '_>,
) -> anyhow::Result<KSketch> {
    let what = if chamfer { "chamfer2d" } else { "fillet2d" };
    let sel = resolve_corners(&src, what, corners)?;
    let sketch = if chamfer {
        sketch2d::chamfer2d(&src.1, &src.0, size, &sel)?
    } else {
        sketch2d::fillet2d(&src.1, &src.0, size, &sel)?
    };
    let mut args = BTreeMap::new();
    args.insert(if chamfer { "distance" } else { "radius" }.into(), fmt_num(size));
    args.insert(
        "corners".into(),
        match &sel {
            CornerSelect::All => "all".to_owned(),
            CornerSelect::Some(list) => {
                let all = sketch2d::corners(&src.1, &src.0);
                list.iter()
                    .filter_map(|(l, k)| all.iter().find(|c| (c.loop_idx, c.vertex) == (*l, *k)).map(|c| c.name.clone()))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        },
    );
    record(what, name, &[&src.0], args, Outcome { sketch, notes: Vec::new() }, eval)
}

pub(crate) fn mirror_op(
    src: (String, Sketch),
    axis: Option<VecArg>,
    origin: Option<VecArg>,
    keep: bool,
    merge: bool,
    name: Option<&str>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KSketch> {
    let axis = axis.as_ref().map(|a| p2(a, "mirror2d: axis")).transpose()?.unwrap_or(P2::new(0.0, 1.0));
    let origin = origin.as_ref().map(|o| p2(o, "mirror2d: origin")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
    if axis.x == 0.0 && axis.y == 0.0 {
        anyhow::bail!("mirror2d: axis must be non-zero");
    }
    let outcome = sketch2d::mirror2d(&src.1, &src.0, origin, axis, keep, merge, tolerance(eval)?)?;
    let mut args = BTreeMap::new();
    args.insert("axis".into(), format!("({}, {})", fmt_num(axis.x), fmt_num(axis.y)));
    args.insert("origin".into(), format!("({}, {})", fmt_num(origin.x), fmt_num(origin.y)));
    args.insert("keep".into(), keep.to_string());
    args.insert("merge".into(), merge.to_string());
    record("mirror2d", name, &[&src.0], args, outcome, eval)
}

pub(crate) fn pattern_linear_op(
    src: (String, Sketch),
    count: usize,
    spacing: P2,
    merge: bool,
    name: Option<&str>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KSketch> {
    let xfs: Vec<Xf2> = (0..count).map(|k| Xf2::translation(P2::new(spacing.x * k as f64, spacing.y * k as f64))).collect();
    let outcome = sketch2d::pattern2d(&src.1, &src.0, &xfs, merge, tolerance(eval)?)?;
    let mut args = BTreeMap::new();
    args.insert("count".into(), count.to_string());
    args.insert("spacing".into(), format!("({}, {})", fmt_num(spacing.x), fmt_num(spacing.y)));
    args.insert("merge".into(), merge.to_string());
    record("pattern2d_linear", name, &[&src.0], args, outcome, eval)
}

pub(crate) fn pattern_circular_op(
    src: (String, Sketch),
    count: usize,
    center: P2,
    angle_deg: f64,
    merge: bool,
    name: Option<&str>,
    eval: &Evaluator<'_, '_, '_>,
) -> anyhow::Result<KSketch> {
    // A full turn spaces `count` copies around it; a partial sweep puts the
    // last copy at its end, as `pattern_circular` does.
    let full = (angle_deg - 360.0).abs() < 1e-9;
    let denom = if full { count } else { (count - 1).max(1) };
    let xfs: Vec<Xf2> = (0..count).map(|k| Xf2::rotation(center, angle_deg.to_radians() * k as f64 / denom as f64)).collect();
    let outcome = sketch2d::pattern2d(&src.1, &src.0, &xfs, merge, tolerance(eval)?)?;
    let mut args = BTreeMap::new();
    args.insert("count".into(), count.to_string());
    args.insert("center".into(), format!("({}, {})", fmt_num(center.x), fmt_num(center.y)));
    args.insert("angle_deg".into(), fmt_num(angle_deg));
    args.insert("merge".into(), merge.to_string());
    record("pattern2d_circular", name, &[&src.0], args, outcome, eval)
}

#[starlark_module]
pub(crate) fn sketch2d_globals(builder: &mut GlobalsBuilder) {
    /// Unite two sketches on one plane: one new sketch whose loops bound
    /// everything either covered. Overlapping loops are stitched at their
    /// crossings (exact for lines and arcs; a spline is sampled to the
    /// document tolerance and re-fitted), loops that touch nothing come
    /// through as drawn, and holes stay holes. Loop names follow
    /// provenance: `a/loop[0]|b/loop[0]` for a loop built from both.
    /// Same as `Sketch.union(other)`. Feature: `union2d`, input both sketches.
    fn union2d<'v>(
        #[starlark(require = pos)] a: Value<'v>,
        #[starlark(require = pos)] b: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        boolean_op(source(a, "union2d", eval)?, source(b, "union2d", eval)?, BoolOp::Union, name, eval)
    }

    /// Cut `b` out of `a` on their common plane. A `b` inside `a` becomes a
    /// hole; a `b` that covers all of `a` leaves an empty sketch with a note,
    /// never an error. Same as `Sketch.subtract(other)`. Feature: `subtract2d`.
    fn subtract2d<'v>(
        #[starlark(require = pos)] a: Value<'v>,
        #[starlark(require = pos)] b: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        boolean_op(source(a, "subtract2d", eval)?, source(b, "subtract2d", eval)?, BoolOp::Subtract, name, eval)
    }

    /// Keep what `a` and `b` both cover. Sketches that do not overlap give an
    /// empty sketch with a note, never an error. Same as
    /// `Sketch.intersect(other)`. Feature: `intersect2d`.
    fn intersect2d<'v>(
        #[starlark(require = pos)] a: Value<'v>,
        #[starlark(require = pos)] b: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        boolean_op(source(a, "intersect2d", eval)?, source(b, "intersect2d", eval)?, BoolOp::Intersect, name, eval)
    }

    /// Round the corners of a sketch with arcs of radius `r`, each tangent
    /// to the two segments it joins (exact, so an extrude of the result has
    /// true cylinders there). `corners=` picks which: a list of names
    /// (`"corner[2]"`, or the full `<loop>/corner[2]`; `Sketch.corners()`
    /// lists them), indices on a one-loop sketch, or `(loop, corner)` pairs;
    /// leave it out for every corner. Only corners between lines and arcs;
    /// a radius that does not fit is refused, naming the largest that would.
    /// Same as `Sketch.fillet2d(r, corners=)`. Feature: `fillet2d`.
    fn fillet2d<'v>(
        #[starlark(require = pos)] sketch: Value<'v>,
        #[starlark(require = pos)] r: UnpackFloat,
        #[starlark(require = named)] corners: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let r = num(&r, "fillet2d: r")?;
        corner_op(source(sketch, "fillet2d", eval)?, r, corners, false, name, eval)
    }

    /// Cut the corners of a sketch back by `d` along both segments and join
    /// the cuts with a line. `corners=` as for `fillet2d`. A distance that
    /// does not fit is refused, naming the largest that would; exactly half
    /// a segment from both ends is allowed and the segment vanishes.
    /// Same as `Sketch.chamfer2d(d, corners=)`. Feature: `chamfer2d`.
    fn chamfer2d<'v>(
        #[starlark(require = pos)] sketch: Value<'v>,
        #[starlark(require = pos)] d: UnpackFloat,
        #[starlark(require = named)] corners: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let d = num(&d, "chamfer2d: d")?;
        corner_op(source(sketch, "chamfer2d", eval)?, d, corners, true, name, eval)
    }

    /// Mirror every loop across the line through `origin` along `axis`, in
    /// sketch coordinates (the default is the sketch's y axis, so x becomes
    /// -x, as `mirror(plane=YZ)` does for a body). With `keep=True` the
    /// originals stay and with `merge=True` (both the default) the image is
    /// united with them, so a half drawn up to the axis becomes one loop;
    /// `merge=False` keeps both sets of loops, `keep=False` is the image
    /// alone. Image loops are `<loop>/mirror`. Feature: `mirror2d`.
    fn mirror2d<'v>(
        #[starlark(require = pos)] sketch: Value<'v>,
        #[starlark(require = named)] axis: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named, default = true)] keep: bool,
        #[starlark(require = named, default = true)] merge: bool,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        mirror_op(source(sketch, "mirror2d", eval)?, axis, origin, keep, merge, name, eval)
    }

    /// `count` copies of every loop, `spacing` (an `(dx, dy)` in sketch
    /// coordinates) apart: the original and `count - 1` copies named
    /// `<loop>/copy[k]`. With `merge=True` copies that overlap are united
    /// into one loop (three overlapping slots become one long slot); without
    /// it the copies must not overlap, or the sketch will not extrude.
    /// Feature: `pattern2d_linear`.
    fn pattern2d_linear<'v>(
        #[starlark(require = pos)] sketch: Value<'v>,
        #[starlark(require = pos)] count: UnpackFloat,
        #[starlark(require = pos)] spacing: VecArg,
        #[starlark(require = named, default = false)] merge: bool,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let count = check::count(&count, "pattern2d_linear: count", 1, Limits::MAX_PATTERN_COUNT)?;
        let spacing = p2(&spacing, "pattern2d_linear: spacing")?;
        pattern_linear_op(source(sketch, "pattern2d_linear", eval)?, count, spacing, merge, name, eval)
    }

    /// `count` copies of every loop evenly rotated about `center` over
    /// `angle` degrees (default a full turn), named `<loop>/copy[k]`;
    /// `merge=` as for `pattern2d_linear`. Feature: `pattern2d_circular`.
    fn pattern2d_circular<'v>(
        #[starlark(require = pos)] sketch: Value<'v>,
        #[starlark(require = pos)] count: UnpackFloat,
        #[starlark(require = named)] center: Option<VecArg>,
        #[starlark(require = named, default = UnpackFloat(360.0))] angle: UnpackFloat,
        #[starlark(require = named, default = false)] merge: bool,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSketch> {
        let count = check::count(&count, "pattern2d_circular: count", 1, Limits::MAX_PATTERN_COUNT)?;
        let center = center.as_ref().map(|c| p2(c, "pattern2d_circular: center")).transpose()?.unwrap_or(P2::new(0.0, 0.0));
        let angle = num(&angle, "pattern2d_circular: angle")?;
        pattern_circular_op(source(sketch, "pattern2d_circular", eval)?, count, center, angle, merge, name, eval)
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};
    use cadpole_core::context::{Context, Severity};

    fn infos(ctx: &Context) -> Vec<String> {
        ctx.diagnostics.iter().filter(|d| d.severity == Severity::Info).map(|d| d.message.clone()).collect()
    }

    /// An inward offset of a glyph trims its corners, and the trim used to
    /// leave sub-micron pieces (1.85e-6 mm on this "M") as segments of the
    /// result. emboss stands a wall on every segment, and a wall on a
    /// segment that short has two corners at one point, so the body could
    /// not be tessellated. No segment of an offset is that short (the bound,
    /// 1e-5 mm, fails on the old 1.85e-6 mm piece), and the emboss is a
    /// solid.
    #[test]
    fn an_offset_glyph_has_no_hairline_segments_and_embosses_to_a_solid() {
        let ctx = run(
            "g = text(\"M\", size=14.0, align=\"center\").offset(-0.2)\n\
             for l in g.points():\n\
             \x20   n = len(l)\n\
             \x20   for i in range(n):\n\
             \x20       a = l[i]\n\
             \x20       b = l[(i + 1) % n]\n\
             \x20       assert_true(math.sqrt((b[0] - a[0]) * (b[0] - a[0]) + (b[1] - a[1]) * (b[1] - a[1])) > 1e-5)\n\
             b = box(40, 40, 5, name=\"p\")\n\
             r = emboss(b, b.faces(normal=(0, 0, 1)), g, 0.8, origin=(20, 20, 5), name=\"m\")\n\
             assert_true(r.volume() > 8000)\n\
             assert_solid(r)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// An offset circle is a circle: its extrude has two cylinder faces and
    /// the volume of the new radius, in both directions. The feature tree
    /// records the offset with the sketch as its input.
    #[test]
    fn an_offset_circle_extrudes_to_a_cylinder() {
        let ctx = run(
            "s = sketch(XY, name=\"s\").circle(5)\n\
             o = s.offset(2, name=\"o\")\n\
             assert_eq(o.names, [\"s/loop[0]/offset\"])\n\
             b = extrude(o, 10, name=\"b\")\n\
             assert_eq(b.kind, \"brep\")\n\
             assert_eq(len(b.faces(kind=kind.cylinder).names()), 2)\n\
             v = b.volume()\n\
             assert_true(v > 0.98 * math.pi * 49 * 10 and v <= math.pi * 49 * 10 + 1e-6)\n\
             i = s.offset(-2, join=join.miter, name=\"i\")\n\
             w = extrude(i, 10, name=\"c\").volume()\n\
             assert_true(w > 0.98 * math.pi * 9 * 10 and w <= math.pi * 9 * 10 + 1e-6)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let o = ctx.features.iter().find(|f| f.id.0 == "o").unwrap();
        assert_eq!(o.kind, "offset2d");
        assert_eq!(o.inputs.iter().map(|i| i.0.as_str()).collect::<Vec<_>>(), ["s"]);
        assert_eq!(o.args.get("join").map(String::as_str), Some("round"));
        assert_eq!(o.args.get("distance").map(String::as_str), Some("2"));
    }

    /// The wing's use, as the acceptance test: a NACA section offset inward
    /// by the skin thickness pinches off at the trailing edge and stays one
    /// loop; the outer prism minus the inner one is an exact solid skin whose
    /// volume is the difference of the two prisms'.
    #[test]
    fn an_airfoil_skin_is_the_outer_prism_minus_the_inward_offset() {
        let ctx = run(
            r#"
n = 36
c = 100.0
t = 0.12
m = 0.02
p = 0.4

def yt(x):
    return 5 * t * (0.2969 * math.sqrt(x) - 0.1260 * x - 0.3516 * x * x + 0.2843 * x * x * x - 0.1015 * x * x * x * x)

def camber(x):
    if x < p:
        return (m / (p * p) * (2 * p * x - x * x), 2 * m / (p * p) * (p - x))
    return (m / ((1 - p) * (1 - p)) * ((1 - 2 * p) + 2 * p * x - x * x), 2 * m / ((1 - p) * (1 - p)) * (p - x))

def section():
    upper = []
    lower = []
    for i in range(n + 1):
        x = (1 - math.cos(180.0 * i / n)) / 2  # math.cos takes degrees
        th = math.atan(camber(x)[1])
        yc = camber(x)[0]
        upper.append(((x - yt(x) * math.sin(th)) * c, (yc + yt(x) * math.cos(th)) * c))
        lower.append(((x + yt(x) * math.sin(th)) * c, (yc - yt(x) * math.cos(th)) * c))
    return lower[1:] + reversed(upper[1:]) + [upper[0]]

foil = sketch(XY, name="foil").polygon(section())
inner = foil.offset(-0.8, join=join.miter, name="inner")
assert_eq(inner.loops, 1)
assert_eq(inner.names, ["foil/loop[0]/offset"])
outer = extrude(foil, 30, name="outer")
core = extrude(inner, 40, name="core")
core_volume = core.volume()
skin = subtract(outer, translate(core, (0, 0, -5)), name="skin")
assert_eq(skin.kind, "brep")
assert_true(skin.volume() > 0)
assert_close(outer.volume() - skin.volume(), core_volume * 30 / 40, tol=1e-3)
"#,
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(infos(&ctx).iter().all(|n| !n.contains("collapsed")), "{:?}", infos(&ctx));
    }

    /// Booleans through the script: names by provenance, a hole from a
    /// subtraction, and an empty intersection that is a note, not an error.
    #[test]
    fn booleans_name_by_provenance_and_never_error_on_empty() {
        let ctx = run(
            "a = sketch(XY, name=\"a\").rect(10, 10)\n\
             b = sketch(XY, name=\"b\").rect(10, 10, center=(5, 5))\n\
             u = union2d(a, b, name=\"u\")\n\
             assert_eq(u.loops, 1)\n\
             assert_eq(u.names, [\"a/loop[0]|b/loop[0]\"])\n\
             assert_close(extrude(u, 1, name=\"ub\").volume(), 175, tol=1e-6)\n\
             h = a.subtract(sketch(XY, name=\"c\").circle(2), name=\"h\")\n\
             assert_eq(h.loops, 2)\n\
             hv = extrude(h, 1, name=\"hb\").volume()\n\
             assert_true(hv > 100 - math.pi * 4 - 0.5 and hv < 100 - math.pi * 4 + 0.5)\n\
             e = a.intersect(sketch(XY, name=\"f\").rect(4, 4, center=(50, 0)), name=\"e\")\n\
             assert_eq(e.loops, 0)\n\
             i = intersect2d(a, b, name=\"i\")\n\
             assert_close(extrude(i, 1, name=\"ib\").volume(), 25, tol=1e-6)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(infos(&ctx).iter().any(|n| n.contains("do not overlap")), "{:?}", infos(&ctx));
        let e = ctx.features.iter().find(|f| f.id.0 == "e").unwrap();
        assert_eq!(e.kind, "intersect2d");
        assert_eq!(e.inputs.iter().map(|i| i.0.as_str()).collect::<Vec<_>>(), ["a", "f"]);
        assert!(e.diagnostics.iter().any(|d| d.message.contains("do not overlap")));
        let ctx = run("a = sketch(XY).rect(10, 10)\nb = sketch(XZ).rect(10, 10)\nunion2d(a, b)\n");
        assert!(errors(&ctx).iter().any(|e| e.contains("different planes")), "{:?}", errors(&ctx));
    }

    /// Corners are named and selectable three ways; a fillet that does not
    /// fit says the largest that would; the result extrudes with true
    /// cylinders at the rounded corners.
    #[test]
    fn fillet2d_and_chamfer2d_select_corners_and_refuse_with_the_max() {
        let ctx = run(
            "q = sketch(XY, name=\"q\").rect(20, 10)\n\
             assert_eq(q.corners(), [\"q/loop[0]/corner[0]\", \"q/loop[0]/corner[1]\", \"q/loop[0]/corner[2]\", \"q/loop[0]/corner[3]\"])\n\
             f = fillet2d(q, 2, name=\"f\")\n\
             assert_eq(f.corners(), [])\n\
             assert_eq(f.names, [\"q/loop[0]\"])\n\
             fb = extrude(f, 5, name=\"fb\")\n\
             assert_eq(len(fb.faces(kind=kind.cylinder).names()), 4)\n\
             one = q.fillet2d(3, corners=[\"corner[1]\"], name=\"one\")\n\
             assert_eq(len(one.corners()), 3)\n\
             two = q.chamfer2d(3, corners=[1, (0, 2)], name=\"two\")\n\
             assert_eq(len(two.corners()), 6)\n\
             c = chamfer2d(q, 4, name=\"c\")\n\
             assert_close(extrude(c, 1, name=\"cb\").volume(), 200 - 32, tol=1e-6)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "one").unwrap();
        assert_eq!(f.kind, "fillet2d");
        assert_eq!(f.args.get("corners").map(String::as_str), Some("q/loop[0]/corner[1]"));
        let ctx = run("q = sketch(XY, name=\"q\").rect(20, 10)\nfillet2d(q, 6)\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("does not fit") && e[0].contains("largest that fits both is 5"), "{}", e[0]);
        let ctx = run("q = sketch(XY, name=\"q\").rect(20, 10)\nfillet2d(q, 1, corners=[\"corner[9]\"])\n");
        assert!(errors(&ctx)[0].contains("no corner named"), "{:?}", errors(&ctx));
    }

    /// Mirror and pattern names, and a merged mirror image that becomes one loop.
    #[test]
    fn mirror2d_and_pattern2d_name_their_loops() {
        let ctx = run(
            "h = sketch(XY, name=\"h\").rect(10, 6, center=(5, 0))\n\
             m = mirror2d(h, name=\"m\")\n\
             assert_eq(m.loops, 1)\n\
             assert_eq(m.names, [\"h/loop[0]|h/loop[0]/mirror\"])\n\
             assert_close(extrude(m, 1, name=\"mb\").volume(), 120, tol=1e-6)\n\
             both = h.mirror2d(merge=False, name=\"both\")\n\
             assert_eq(both.names, [\"h/loop[0]\", \"h/loop[0]/mirror\"])\n\
             img = mirror2d(h, keep=False, axis=(1, 0), name=\"img\")\n\
             assert_eq(img.names, [\"h/loop[0]/mirror\"])\n\
             holes = sketch(XY, name=\"holes\").circle(1)\n\
             p = pattern2d_linear(holes, 3, (5, 0), name=\"p\")\n\
             assert_eq(p.names, [\"holes/loop[0]\", \"holes/loop[0]/copy[1]\", \"holes/loop[0]/copy[2]\"])\n\
             assert_eq(len(extrude(p, 2, name=\"pb\")), 3)\n\
             ring = pattern2d_circular(sketch(XY, name=\"r\").circle(1, center=(6, 0)), 4, name=\"ring\")\n\
             assert_eq(ring.loops, 4)\n\
             slots = sketch(XY, name=\"slot\").slot(6, 2).pattern2d_linear(3, (4, 0), merge=True, name=\"slots\")\n\
             assert_eq(slots.loops, 1)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let m = ctx.features.iter().find(|f| f.id.0 == "m").unwrap();
        assert_eq!((m.kind.as_str(), m.args.get("merge").map(String::as_str)), ("mirror2d", Some("true")));
        let p = ctx.features.iter().find(|f| f.id.0 == "p").unwrap();
        assert_eq!((p.kind.as_str(), p.args.get("count").map(String::as_str)), ("pattern2d_linear", Some("3")));
    }

    /// A collapsed loop is dropped with a note naming it; the offset of a
    /// ring whose hole grows into its wall is an empty sketch with a note.
    #[test]
    fn a_collapsed_loop_is_a_note_not_an_error() {
        let ctx = run(
            "r = sketch(XY, name=\"r\").circle(5).circle(3)\n\
             g = r.offset(-1.5, name=\"g\")\n\
             assert_eq(g.loops, 0)\n\
             s = sketch(XY, name=\"s\").rect(20, 20).circle(1)\n\
             k = s.offset(1.5, name=\"k\")\n\
             assert_eq(k.loops, 1)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let notes = infos(&ctx);
        assert!(notes.iter().any(|n| n.contains("s/loop[1] collapsed")), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("every loop collapsed")), "{notes:?}");
        let ctx = run("s = sketch(XY).rect(2, 2)\ns.offset(1, join=\"square\")\n");
        assert!(errors(&ctx)[0].contains("valid: round, miter, bevel"), "{:?}", errors(&ctx));
    }
}

