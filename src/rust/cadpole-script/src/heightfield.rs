//! `heightfield(...)` and `lithophane(...)`: a wall whose thickness follows
//! a picture, a grid of numbers or a function, on a plane, a cylinder or a
//! revolved profile (docs/starlark-api.md §4.7.2, `cadpole_core::heightfield`).
//!
//! The values are settled here, before the kernel op: a picture is decoded
//! and resampled onto the grid's nodes, a list of rows is read as a
//! picture, and a function is called once per node — which has to happen
//! in the evaluator, so it happens outside `Context::produce`. The op is
//! then keyed by the values themselves (a SHA-256 of them) and the spec, so
//! a re-run that changes nothing rebuilds nothing.

use std::collections::BTreeMap;

use cadpole_core::context::{fmt_num, Body, NameSource};
use cadpole_core::heightfield::{Base, Grid, Layout, Side, Spec};
use cadpole_core::memo::{sha256, KeyBuilder, Produced};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::float::UnpackFloat;
use starlark::values::{UnpackValue, Value, ValueLike};

use crate::check::{num, opt_num};
use crate::globals::{asset_bytes, bodies_value};
use crate::import3d::source_name;
use crate::store::{call_span, store};
use crate::values::KAsset;

/// The most nodes a function source is called at: each call is a Starlark
/// call, so past this the evaluation's own step budget would end it with a
/// less helpful message.
const MAX_FUNCTION_NODES: usize = 4_000_000;

/// Where the values come from, before they are on the grid.
enum Source<'v> {
    /// A picture or a list of rows, stretched over the field.
    Grid { grid: Grid, label: String, key: String },
    /// `f(u, v)` in mm, called at every node.
    Function(Value<'v>),
}

fn number(v: Value<'_>, what: &str) -> anyhow::Result<f64> {
    match UnpackFloat::unpack_value(v).map_err(|e| anyhow::anyhow!("{what}: {e}"))? {
        Some(f) if f.0.is_finite() => Ok(f.0),
        Some(f) => anyhow::bail!("{what}: {} is not a finite number", f.0),
        None => anyhow::bail!("{what} must be a number, got {}", v.get_type()),
    }
}

fn source<'v>(v: Value<'v>, eval: &Evaluator<'v, '_, '_>, what: &str) -> anyhow::Result<Source<'v>> {
    if v.downcast_ref::<KAsset>().is_some() || v.unpack_str().is_some() {
        let name = source_name(v, what)?;
        let st = store(eval)?;
        let bytes = asset_bytes(st, &name, what)?;
        let grid = Grid::from_image(&bytes).map_err(|e| anyhow::anyhow!("{what}: asset {name:?}: {e}"))?;
        let key = format!("asset:{}", hex(&sha256(&bytes)));
        return Ok(Source::Grid { grid, label: format!("asset({name:?})"), key });
    }
    let ty = v.get_type();
    if ty == "list" || ty == "tuple" {
        let rows: Vec<Value<'v>> = v.iterate(eval.heap()).map_err(|e| e.into_anyhow())?.collect();
        let mut values = Vec::new();
        let mut width = None;
        for (j, row) in rows.iter().enumerate() {
            let cells: Vec<Value<'v>> = row
                .iterate(eval.heap())
                .map_err(|_| anyhow::anyhow!("{what}: row {j} is a {}, not a list of numbers", row.get_type()))?
                .collect();
            match width {
                None => width = Some(cells.len()),
                Some(w) if w != cells.len() => {
                    anyhow::bail!("{what}: row {j} has {} values but row 0 has {w}; every row must be as long", cells.len())
                }
                _ => {}
            }
            for (i, c) in cells.iter().enumerate() {
                values.push(number(*c, &format!("{what}: row {j}, column {i}"))? as f32);
            }
        }
        let width = width.unwrap_or(0);
        let grid = Grid::new(width, rows.len(), values).map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
        let bytes: Vec<u8> = grid.values.iter().flat_map(|f| f.to_le_bytes()).collect();
        let key = format!("rows:{}x{}:{}", grid.width, grid.height, hex(&sha256(&bytes)));
        return Ok(Source::Grid { label: format!("{} x {} values", grid.width, grid.height), grid, key });
    }
    if ty == "function" || ty == "native_function" || ty == "partial" {
        return Ok(Source::Function(v));
    }
    anyhow::bail!(
        "{what}: the source must be a picture (asset(\"x.png\") or its name), a list of rows of numbers \
         in [0, 1], or a function f(u, v) of millimetres; got {ty}"
    )
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The base a call describes: exactly one of `size=` (a plane), `radius=`
/// (a cylinder, with `height=`) or `profile=` (a revolved profile).
#[allow(clippy::too_many_arguments)]
fn base<'v>(
    size: Option<Value<'v>>,
    radius: Option<UnpackFloat>,
    height: Option<UnpackFloat>,
    profile: Option<Value<'v>>,
    angle: f64,
    aspect: Option<f64>,
    eval: &Evaluator<'v, '_, '_>,
    what: &str,
) -> anyhow::Result<Base> {
    let given = [size.is_some(), radius.is_some(), profile.is_some()].iter().filter(|b| **b).count();
    if given != 1 {
        anyhow::bail!(
            "{what}: give exactly one base: size=(width, height) for a flat panel, radius= and height= for \
             a cylinder, or profile=[(r, z), ...] for a revolved shade"
        );
    }
    if let Some(s) = size {
        let parts: Vec<Value<'v>> = s.iterate(eval.heap()).map_err(|e| e.into_anyhow())?.collect();
        if parts.len() != 2 {
            anyhow::bail!("{what}: size is (width, height) in mm");
        }
        let width = number(parts[0], &format!("{what} size width"))?;
        let height = if parts[1].is_none() {
            match aspect {
                Some(a) => width * a,
                None => anyhow::bail!(
                    "{what}: size=(width, None) takes the height from the picture's proportions, and a \
                     function has none; give the height"
                ),
            }
        } else {
            number(parts[1], &format!("{what} size height"))?
        };
        if angle != 360.0 {
            anyhow::bail!("{what}: angle= is for a cylinder or a profile, not a flat panel");
        }
        return Ok(Base::Plane { width, height });
    }
    if let Some(r) = radius {
        let r = num(&r, &format!("{what} radius"))?;
        let Some(h) = height else { anyhow::bail!("{what}: a cylinder needs height= as well as radius=") };
        return Ok(Base::Cylinder { radius: r, height: num(&h, &format!("{what} height"))?, angle });
    }
    if height.is_some() {
        anyhow::bail!("{what}: height= goes with radius=; a profile's height is its own");
    }
    let p = profile.expect("one base is given");
    let pts: Vec<Value<'v>> = p.iterate(eval.heap()).map_err(|e| e.into_anyhow())?.collect();
    let mut out = Vec::with_capacity(pts.len());
    for (k, pt) in pts.iter().enumerate() {
        let xy: Vec<Value<'v>> = pt.iterate(eval.heap()).map_err(|e| e.into_anyhow())?.collect();
        if xy.len() != 2 {
            anyhow::bail!("{what}: profile point {k} must be (r, z)");
        }
        out.push((number(xy[0], &format!("{what} profile point {k}"))?, number(xy[1], &format!("{what} profile point {k}"))?));
    }
    Ok(Base::Revolve { profile: out, angle })
}

/// Everything both globals share.
#[allow(clippy::too_many_arguments)]
fn make_heightfield<'v>(
    kind: &str,
    src: Value<'v>,
    size: Option<Value<'v>>,
    radius: Option<UnpackFloat>,
    height: Option<UnpackFloat>,
    profile: Option<Value<'v>>,
    angle: UnpackFloat,
    thickness: Value<'v>,
    invert: bool,
    resolution: UnpackFloat,
    smoothing: UnpackFloat,
    border: UnpackFloat,
    border_thickness: Option<UnpackFloat>,
    samples: Option<UnpackFloat>,
    side: &str,
    name: Option<&str>,
    layer: Option<Value<'v>>,
    eval: &mut Evaluator<'v, '_, '_>,
) -> anyhow::Result<Value<'v>> {
    let what = kind;
    let span = call_span(eval);
    let source = source(src, eval, what)?;
    if samples.is_some() && !matches!(source, Source::Function(_)) {
        anyhow::bail!("{what}: samples= is the pitch a function is called at; a picture or a list has its own pixels");
    }
    let aspect = match &source {
        Source::Grid { grid, .. } => Some(grid.height as f64 / grid.width as f64),
        Source::Function(_) => None,
    };
    let angle = num(&angle, &format!("{what} angle"))?;
    let base = base(size, radius, height, profile, angle, aspect, eval, what)?;
    let th: Vec<Value<'v>> = thickness
        .iterate(eval.heap())
        .map_err(|_| anyhow::anyhow!("{what}: thickness is (thinnest, thickest) in mm"))?
        .collect();
    if th.len() != 2 {
        anyhow::bail!("{what}: thickness is (thinnest, thickest) in mm");
    }
    let thickness = (number(th[0], &format!("{what} thickness"))?, number(th[1], &format!("{what} thickness"))?);
    let side = Side::parse(side).ok_or_else(|| anyhow::anyhow!("{what}: side must be \"outside\" or \"inside\", got {side:?}"))?;
    let spec = Spec {
        base,
        thickness,
        invert,
        resolution: num(&resolution, &format!("{what} resolution"))?,
        smoothing: num(&smoothing, &format!("{what} smoothing"))?,
        border: num(&border, &format!("{what} border"))?,
        border_thickness: opt_num(&border_thickness, &format!("{what} border_thickness"))?,
        side,
    };
    let layout: Layout = spec.layout().map_err(|e| anyhow::anyhow!("{e}"))?;

    // The values at the nodes, settled before the context is borrowed: a
    // function source runs Starlark.
    let (values, label, source_key) = match source {
        Source::Grid { grid, label, key } => (spec.sample_grid(&layout, &grid), label, key),
        Source::Function(f) => {
            // At the nodes themselves, or on a picture of `samples` mm
            // pixels (sampled at their centres) that is then stretched over
            // the grid like any picture.
            let pitch = opt_num(&samples, &format!("{what} samples"))?;
            let (cols, rows) = match pitch {
                None => (layout.nu, layout.nv),
                Some(p) if p > 0.0 => (
                    ((layout.u_len / p).round() as usize).max(2),
                    ((layout.v_len / p).round() as usize).max(2),
                ),
                Some(p) => anyhow::bail!("{what}: samples must be a positive pitch in mm, got {p}"),
            };
            let n = cols * rows;
            if n > MAX_FUNCTION_NODES {
                anyhow::bail!(
                    "{what}: a function is called once per sample, and {cols} x {rows} = {n} samples is \
                     more than {MAX_FUNCTION_NODES}; sample it coarser with samples= or pass a list of rows"
                );
            }
            let at = |i: usize, j: usize| -> (f64, f64) {
                match pitch {
                    None => (layout.u(i), layout.v(j)),
                    // Row 0 is the top of a picture.
                    Some(_) => (
                        (i as f64 + 0.5) * layout.u_len / cols as f64,
                        layout.v_len - (j as f64 + 0.5) * layout.v_len / rows as f64,
                    ),
                }
            };
            let mut vals = Vec::with_capacity(n);
            let cap = store(eval)?.limits.max_heap_bytes;
            for j in 0..rows {
                // Nothing is collected until this call returns, so stop with
                // the reason before the heap cap stops the script without one.
                // The arena grows in chunks that double, so the next one can
                // take a heap at three quarters of its cap straight past it.
                let used = eval.heap().peak_allocated_bytes() + eval.frozen_heap().allocated_bytes();
                let full = (cap > 0 && used > cap / 4 * 3).then_some(cap);
                if let Some(cap) = full {
                    anyhow::bail!(
                        "{what}: the source function's temporaries are freed only when the call returns, \
                         and {j} of {rows} rows of {cols} calls have filled three quarters of the {} MB \
                         Starlark heap; sample it coarser with samples= (a 0.5 mm picture is enough for \
                         most pictures) or build a list of rows in a top-level loop",
                        cap / (1024 * 1024)
                    );
                }
                let v = eval.heap().alloc(at(0, j).1);
                for i in 0..cols {
                    let (uu, vv) = at(i, j);
                    let u = eval.heap().alloc(uu);
                    let r = eval.eval_function(f, &[u, v], &[]).map_err(|e| e.into_anyhow())?;
                    let x = number(r, &format!("{what}: the source function at u={}, v={}", fmt_num(uu), fmt_num(vv)))?;
                    vals.push(x.clamp(0.0, 1.0) as f32);
                }
            }
            let vals = match pitch {
                None => vals,
                Some(_) => spec.sample_grid(&layout, &Grid::new(cols, rows, vals).map_err(|e| anyhow::anyhow!("{what}: {e}"))?),
            };
            let bytes: Vec<u8> = vals.iter().flat_map(|f| f.to_le_bytes()).collect();
            let key = format!("fn:{cols}x{rows}:{}", hex(&sha256(&bytes)));
            let label = match pitch {
                None => "a function".to_owned(),
                Some(p) => format!("a function, sampled every {} mm", fmt_num(p)),
            };
            (vals, label, key)
        }
    };
    let t = spec.thickness_at_nodes(&layout, &values).map_err(|e| anyhow::anyhow!("{e}"))?;

    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let mut args = BTreeMap::new();
    args.insert("source".into(), label);
    args.insert(
        "base".into(),
        match &spec.base {
            Base::Plane { width, height } => format!("plane {} x {}", fmt_num(*width), fmt_num(*height)),
            Base::Cylinder { radius, height, angle } => {
                format!("cylinder r {} h {} angle {}", fmt_num(*radius), fmt_num(*height), fmt_num(*angle))
            }
            Base::Revolve { profile, angle } => format!("profile of {} points, angle {}", profile.len(), fmt_num(*angle)),
        },
    );
    args.insert("thickness".into(), format!("({}, {})", fmt_num(thickness.0), fmt_num(thickness.1)));
    args.insert("invert".into(), invert.to_string());
    args.insert("resolution".into(), fmt_num(spec.resolution));
    args.insert("grid".into(), format!("{} x {}", layout.nu, layout.nv));
    if spec.smoothing > 0.0 {
        args.insert("smoothing".into(), fmt_num(spec.smoothing));
    }
    if spec.border > 0.0 {
        args.insert("border".into(), fmt_num(spec.border));
    }
    args.insert("side".into(), side.name().to_owned());
    let fid = ctx.new_feature(kind, name.map(str::to_owned), vec![], args, span.clone())?;
    let mut key = KeyBuilder::new("heightfield");
    key.put(&fid).str(&source_key).f64(thickness.0).f64(thickness.1).bool(invert).f64(spec.resolution);
    key.f64(spec.smoothing).f64(spec.border).f64(spec.border_thickness.unwrap_or(-1.0)).str(side.name());
    match &spec.base {
        Base::Plane { width, height } => {
            key.str("plane").f64(*width).f64(*height);
        }
        Base::Cylinder { radius, height, angle } => {
            key.str("cylinder").f64(*radius).f64(*height).f64(*angle);
        }
        Base::Revolve { profile, angle } => {
            key.str("revolve").f64(*angle).u64(profile.len() as u64);
            for p in profile {
                key.f64(p.0).f64(p.1);
            }
        }
    }
    let fid_run = fid.clone();
    let ids = ctx.produce(kind, &fid, span, key.finish(), name.map(str::to_owned), move |_ctx| {
        let mb = spec.mesh_body(&layout, &t, &fid_run.0)?;
        Ok(Produced::one(Body::Mesh(mb), NameSource::Intrinsic))
    })?;
    crate::layers::tag_ids(&mut ctx, layer, &ids, kind)?;
    bodies_value(eval.heap(), &ctx, ids)
}

#[starlark_module]
pub(crate) fn heightfield_globals(builder: &mut GlobalsBuilder) {
    /// A wall whose thickness follows a picture, a grid of numbers or a
    /// function: a relief panel, a textured shade (docs/starlark-api.md).
    ///
    /// `source` is a picture (`asset("x.png")` or its name; PNG or binary
    /// PGM, read as grey), a list of rows of numbers in `[0, 1]` (row 0 at
    /// the top, as a picture is written), or a function `f(u, v)` of
    /// millimetres on the developed surface returning a number in `[0, 1]`,
    /// called once per grid node — or, with `samples=p`, once per pixel of
    /// a `p` mm picture that is then stretched like any picture (a
    /// function's temporaries are freed only when the call returns, so a
    /// few hundred thousand calls is the practical limit). A picture or a
    /// list is stretched over the whole field.
    ///
    /// The base is exactly one of: `size=(width, height)`, a flat panel in
    /// XY centred on the origin, back at z = 0, relief up (`(width, None)`
    /// takes the height from the picture); `radius=` with `height=`, a
    /// cylinder about +z from z = 0; `profile=[(r, z), ...]`, an open
    /// profile revolved about +z, the relief on its left-hand side (out of
    /// a profile listed from the foot up). `angle=` (degrees) takes part of
    /// a turn, centred on −y.
    ///
    /// A value `x` becomes the wall `lo + (hi - lo) * x` for
    /// `thickness=(lo, hi)`; `invert=True` swaps the ends. `resolution` is
    /// the grid pitch in mm, `smoothing` a Gaussian blur in mm before the
    /// mapping, `border` a frame of that width round the open edges at
    /// `border_thickness` (default the thicker end). `side="inside"` puts
    /// the relief in a cylinder's bore. The result is a closed mesh body
    /// with groups `relief`, `back`, `bottom`, `top`, and `start` and `end`
    /// unless it is a full turn.
    fn heightfield<'v>(
        #[starlark(require = pos)] source: Value<'v>,
        #[starlark(require = named)] size: Option<Value<'v>>,
        #[starlark(require = named)] radius: Option<UnpackFloat>,
        #[starlark(require = named)] height: Option<UnpackFloat>,
        #[starlark(require = named)] profile: Option<Value<'v>>,
        #[starlark(require = named, default = UnpackFloat(360.0))] angle: UnpackFloat,
        #[starlark(require = named)] thickness: Option<Value<'v>>,
        #[starlark(require = named, default = false)] invert: bool,
        #[starlark(require = named, default = UnpackFloat(0.5))] resolution: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.0))] smoothing: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.0))] border: UnpackFloat,
        #[starlark(require = named)] border_thickness: Option<UnpackFloat>,
        #[starlark(require = named)] samples: Option<UnpackFloat>,
        #[starlark(require = named, default = "outside")] side: &str,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let thickness = thickness.unwrap_or_else(|| eval.heap().alloc((0.6, 3.0)));
        make_heightfield(
            "heightfield", source, size, radius, height, profile, angle, thickness, invert, resolution, smoothing,
            border, border_thickness, samples, side, name, layer, eval,
        )
    }

    /// A lithophane: `heightfield` with the picture inverted, so a bright
    /// pixel is thin wall and glows when lit from behind
    /// (docs/starlark-api.md).
    ///
    /// The same sources and bases as `heightfield`: `size=(width, height)`
    /// for a flat panel (`(width, None)` keeps the picture's proportions),
    /// `radius=` and `height=` for a cylinder, `profile=` for a shade. The
    /// defaults are a lithophane's: `thickness=(0.6, 3.0)` — 0.6 mm is a
    /// white that still holds together, 3 mm a black under an LED — a
    /// 0.25 mm grid, and a 3 mm frame at the full thickness that keeps a
    /// panel flat and gives a shade a foot and a lip. For a cylinder the
    /// relief faces out and the bore is smooth.
    fn lithophane<'v>(
        #[starlark(require = pos)] source: Value<'v>,
        #[starlark(require = named)] size: Option<Value<'v>>,
        #[starlark(require = named)] radius: Option<UnpackFloat>,
        #[starlark(require = named)] height: Option<UnpackFloat>,
        #[starlark(require = named)] profile: Option<Value<'v>>,
        #[starlark(require = named, default = UnpackFloat(360.0))] angle: UnpackFloat,
        #[starlark(require = named)] thickness: Option<Value<'v>>,
        #[starlark(require = named, default = UnpackFloat(0.25))] resolution: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.0))] smoothing: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(3.0))] border: UnpackFloat,
        #[starlark(require = named)] border_thickness: Option<UnpackFloat>,
        #[starlark(require = named)] samples: Option<UnpackFloat>,
        #[starlark(require = named, default = "outside")] side: &str,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let thickness = thickness.unwrap_or_else(|| eval.heap().alloc((0.6, 3.0)));
        make_heightfield(
            "lithophane", source, size, radius, height, profile, angle, thickness, true, resolution, smoothing,
            border, border_thickness, samples, side, name, layer, eval,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cadpole_core::Context;

    use crate::tests::{errors, run};
    use crate::{evaluate, EvalInput, ScriptFile};

    fn run_with_png(src: &str, png: &[u8]) -> Context {
        let mut store = cadpole_assets::MemoryAssetStore::new();
        store.insert("pic.pgm", png.to_vec());
        evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            assets: Some(Arc::new(store)),
            ..Default::default()
        })
    }

    /// An 8 x 4 binary PGM: a ramp from black on the left to white.
    fn ramp_png() -> Vec<u8> {
        let mut out = b"P5\n8 4\n255\n".to_vec();
        for _ in 0..4 {
            out.extend((0..8).map(|i| (i * 255 / 7) as u8));
        }
        out
    }

    fn body_mesh(ctx: &mut Context, name: &str) -> cadpole_core::tessellate::Mesh {
        let id = ctx.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body {name}")).id;
        ctx.body_mesh(id).unwrap().0.clone()
    }

    /// Every source and every base makes a closed solid that booleans and
    /// exports take: a picture, a list of rows and a function, on a plane,
    /// a cylinder and a revolved profile.
    #[test]
    fn every_source_and_base_is_a_solid_that_booleans_take() {
        let mut ctx = run_with_png(
            concat!(
                "a = lithophane(asset(\"pic.pgm\"), size=(40, None), resolution=0.5, name=\"panel\")\n",
                "assert_solid(a)\n",
                "assert_bbox(a, (-20, -10, 0), (20, 10, 3))\n",
                "b = heightfield([[0, 1, 0], [1, 0, 1]], radius=15, height=20, resolution=0.5, name=\"cyl\")\n",
                "assert_solid(b)\n",
                "def f(u, v):\n",
                "    return 0.5 + 0.5 * math.sin(u * 30)\n",
                "c = heightfield(f, profile=[(20, 0), (15, 20)], angle=180, resolution=0.5, name=\"shade\")\n",
                "assert_solid(c)\n",
                "d = heightfield(f, radius=15, height=20, side=\"inside\", samples=1, name=\"bore\")\n",
                "assert_solid(d)\n",
                "e = subtract(a, cylinder(4, 10, at=(0, 0, -1)), name=\"holed\")\n",
                "assert_solid(e)\n",
                "assert_true(e.volume() < a.volume() - 0.6 * 3.1 * 16)\n",
                "u = union(b, cylinder(15.5, 2), name=\"footed\")\n",
                "assert_solid(u)\n",
                "export([e, u], \"out.stl\")\n",
            ),
            &ramp_png(),
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let m = body_mesh(&mut ctx, "holed");
        let stl = cadpole_core::export::stl(&[&m]);
        assert_eq!(stl.len(), 84 + 50 * m.triangle_count());
    }

    /// The picture's ramp is the wall's ramp, inverted for a lithophane: the
    /// black left edge (inside the frame) is thick, the white right edge thin.
    #[test]
    fn a_lithophane_is_thick_where_the_picture_is_dark() {
        let mut ctx = run_with_png(
            "lithophane(asset(\"pic.pgm\"), size=(40, 20), border=0, resolution=0.5, name=\"panel\")\n",
            &ramp_png(),
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let m = body_mesh(&mut ctx, "panel");
        let (mut left, mut right) = (0.0f32, 10.0f32);
        for p in m.positions.chunks_exact(3) {
            if p[0] < -19.9 {
                left = left.max(p[2]);
            }
            if p[0] > 19.9 {
                right = right.min(p[2].max(0.01));
            }
        }
        assert!((left - 3.0).abs() < 0.05, "left {left}");
        assert!(right < 0.7, "right {right}");
    }

    #[test]
    fn mistakes_are_diagnostics_naming_the_argument() {
        let cases = [
            ("heightfield([[0, 1]], size=(10, 10), radius=5)", "exactly one base"),
            ("heightfield([[0, 1]])", "exactly one base"),
            ("heightfield([[0, 1]], radius=5)", "height="),
            ("heightfield([[0, 1], [1]], size=(10, 10))", "row 1"),
            ("heightfield([[0, \"x\"]], size=(10, 10))", "column 1"),
            ("heightfield([[0, 1]], size=(10, 10), thickness=(0, 2))", "thickness"),
            ("heightfield([[0, 1]], size=(10, 10), samples=1)", "samples="),
            ("heightfield(lambda u, v: 0.5, size=(10, None))", "give the height"),
            ("heightfield(lambda u, v: \"no\", size=(10, 10))", "must be a number"),
            ("heightfield([[0, 1]], size=(10, 10), side=\"under\")", "side"),
            ("heightfield(3, size=(10, 10))", "source must be"),
            ("heightfield([[0, 1]], size=(10, 10), angle=90)", "angle="),
        ];
        for (src, want) in cases {
            let ctx = run(&format!("x = {src}\n"));
            let e = errors(&ctx);
            assert!(e.iter().any(|m| m.contains(want)), "{src}: {e:?}");
        }
    }
}
