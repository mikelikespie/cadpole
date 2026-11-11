//! The Starlark surface of `pattern_along` and `pattern_on_surface`
//! (docs/starlark-api.md). A module of their own, as `sweep` is, so
//! `globals.rs` stays small; the doc comment on each function is its
//! entry in `cadpole doc` (`apidoc`), so each is written for someone who has
//! never seen Cadpole.

use std::collections::BTreeMap;

use cadpole_core::context::fmt_vec;
use cadpole_core::curve::Curve;
use cadpole_core::ids::BodyId;
use cadpole_core::kernel::KernelChoice;
use cadpole_core::pattern::{AlongSpec, Grid, SurfaceSpec};
use cadpole_core::sweep::Frame;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::float::UnpackFloat;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::{UnpackValue, Value, ValueLike};

use crate::check::{count, num, opt_num, vec3, VecArg};
use crate::limits::Limits;
use crate::selection::KSelection;
use crate::store::{call_span, store};
use crate::values::{KBody, KCurve, KFace, KPath};

/// The curve of `pattern_along`: a `Curve`.
fn curve_arg<'v>(v: Value<'v>) -> anyhow::Result<(String, Curve)> {
    if let Some(c) = v.downcast_ref::<KCurve>() {
        return Ok((c.feature.clone(), c.curve.clone()));
    }
    if v.downcast_ref::<KPath>().is_some() {
        anyhow::bail!(
            "pattern_along: the curve is a sketch Path, which has no plane; lift it into space with path.on(plane), \
             which gives the Curve a pattern follows"
        );
    }
    anyhow::bail!(
        "pattern_along: curve must be a Curve (curve(...), helix(...), path.on(plane) or one of sketch.curves()), got {}",
        v.get_type()
    )
}

/// The face of `pattern_on_surface`: a `Face`, or a selection of faces on
/// one surface (a cylinder's side is two halves).
fn face_arg<'v>(v: Value<'v>) -> anyhow::Result<(u32, Vec<String>)> {
    if let Some(f) = v.downcast_ref::<KFace>() {
        return Ok((f.body, vec![f.name.clone()]));
    }
    if let Some(s) = v.downcast_ref::<KSelection>() {
        if s.what != "face" {
            anyhow::bail!("pattern_on_surface: {} is a selection of edges, and a pattern needs a face", s.query);
        }
        if s.faces.is_empty() {
            anyhow::bail!("pattern_on_surface: {} selects no face", s.query);
        }
        return Ok((s.body, s.faces.iter().map(|f| f.name.clone()).collect()));
    }
    anyhow::bail!("pattern_on_surface: face must be a Face (body.face(name)) or a selection of faces on one surface (body.faces(kind=kind.cylinder)), got {}", v.get_type())
}

/// `spacing=` on a surface: one number for both directions, or `(du, dv)`.
fn spacing_arg<'v>(v: Value<'v>) -> anyhow::Result<(f64, f64)> {
    if let Some(d) = UnpackFloat::unpack_value(v).map_err(|e| e.into_anyhow())? {
        let d = num(&d, "pattern_on_surface: spacing")?;
        return Ok((d, d));
    }
    if let Some(pair) = UnpackListOrTuple::<UnpackFloat>::unpack_value(v).map_err(|e| e.into_anyhow())? {
        if let [du, dv] = pair.items.as_slice() {
            return Ok((num(du, "pattern_on_surface: spacing along a row")?, num(dv, "pattern_on_surface: spacing between rows")?));
        }
        anyhow::bail!("pattern_on_surface: spacing must be one number or (du, dv), got {} numbers", pair.items.len());
    }
    anyhow::bail!("pattern_on_surface: spacing must be a number of mm or (du, dv), got {}", v.get_type())
}

fn kernel_arg(kernel: Option<&str>) -> anyhow::Result<KernelChoice> {
    match kernel {
        Some(k) => Ok(KernelChoice::parse(k)?),
        None => Ok(KernelChoice::Auto),
    }
}

/// One fused body with `merge`, else the list of copies (even of one).
fn result_value<'v>(ids: Vec<BodyId>, merge: bool, ctx: &cadpole_core::Context, eval: &Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
    let heap = eval.heap();
    if merge {
        return Ok(heap.alloc(KBody::from_ctx(ctx, ids[0])?));
    }
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(KBody::from_ctx(ctx, id)?);
    }
    Ok(heap.alloc(out))
}

#[starlark_module]
pub(crate) fn pattern_globals(builder: &mut GlobalsBuilder) {
    /// Copies of `body` along `curve` — a rivet line along a rib, posts round
    /// a ring, beads on a helix. The body is placed as drawn at the
    /// origin: its origin lands on each station and, with `align=True` (the
    /// default), its **+z turns along the curve's tangent** and its +x along
    /// the frame's normal (`up=`, or a stable direction off the start), so a
    /// `cylinder(d=2, height=10)` becomes a stud pointing along the path;
    /// `align=False` only moves it. Stations are at **equal arc length**:
    /// `count=` copies from the curve's start to its end (a closed curve is
    /// divided like a full turn of `pattern_circular`, the last copy a step
    /// short of the seam), or `spacing=` mm apart from the start, or both
    /// (that many copies at that spacing; refused when the curve is too
    /// short). `start=` and `end=` window the copies to that stretch of arc
    /// length, in mm from the curve's start.
    ///
    /// `frame=` is `sweep_along`'s: `sweep_frame.rmf` (rotation-minimising,
    /// the default — no turning about the path), `sweep_frame.frenet` (the
    /// curvature normal: on a helix, the radial) or `sweep_frame.fixed`
    /// (`up=` stays up). A pattern reads the frame only at its stations, so
    /// the curve may be closed and may turn corners; at a corner the copy
    /// faces the way the curve goes on.
    ///
    /// Without `merge` the call returns the list of copies as **instances**
    /// of the source (`Body`s that share its tessellation, under
    /// one `pattern_along` feature, named `<name>/copy[k]`); the source is
    /// consumed, its copies being where it now is. `merge=True` fuses the
    /// copies into one Body exactly as `pattern_linear(merge=True)` does
    /// (one compose on the mesh kernel when they are apart; `kernel=`
    /// as for `union`). Copies whose bounding boxes overlap are a warning
    /// naming the pair.
    ///
    /// ```python
    /// rail = curve((0, 0, 0)).line((0, 0, 40)).arc((20, 0, 60), via=(5.86, 0, 54.14)).line((60, 0, 60))
    /// rivets = pattern_along(cylinder(d=2, height=3).translate((0, 0, -1.5)), rail, spacing=8, name="rivets")
    /// beads = pattern_along(sphere(2), helix(10, 6, 3), count=24, align=False, name="beads")
    /// ```
    #[allow(clippy::too_many_arguments)]
    fn pattern_along<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] curve: Value<'v>,
        #[starlark(require = named)] count: Option<UnpackFloat>,
        #[starlark(require = named)] spacing: Option<UnpackFloat>,
        #[starlark(require = named, default = "rmf")] frame: &str,
        #[starlark(require = named)] up: Option<VecArg>,
        #[starlark(require = named)] start: Option<UnpackFloat>,
        #[starlark(require = named)] end: Option<UnpackFloat>,
        #[starlark(require = named, default = true)] align: bool,
        #[starlark(require = named, default = false)] merge: bool,
        #[starlark(require = named)] kernel: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let count = count.as_ref().map(|c| count_arg(c, "pattern_along: count")).transpose()?;
        let spacing = opt_num(&spacing, "pattern_along: spacing")?;
        let frame = Frame::parse(frame)?;
        let up = up.as_ref().map(|u| vec3(u, "pattern_along: up")).transpose()?;
        let start = opt_num(&start, "pattern_along: start")?;
        let end = opt_num(&end, "pattern_along: end")?;
        let kernel = kernel_arg(kernel)?;
        let (curve_feature, curve) = curve_arg(curve)?;
        let spec = AlongSpec {
            curve: &curve,
            count,
            spacing,
            frame,
            up,
            start,
            end,
            align,
            max_count: Limits::MAX_PATTERN_COUNT,
        };
        let mut args = BTreeMap::new();
        args.insert("path".into(), curve_feature);
        if let Some(u) = up {
            args.insert("up".into(), fmt_vec(u));
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = ctx.pattern_along(body.body_id(), &spec, merge, kernel, args, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "pattern_along")?;
        result_value(ids, merge, &ctx, eval)
    }

    /// Copies of `body` spread over `face`'s surface — effusion holes on a
    /// conical liner, knobs on a tyre's crown, studs on a plate. The
    /// body is placed as drawn at the origin: its origin lands on each
    /// surface point and, with `normal=True` (the default), its **+z turns
    /// along the face's outward normal** and its +x along the row, so a
    /// `cylinder(d=3, height=5)` stands on the face; a hole cutter is drawn
    /// straddling z = 0 (`cylinder(d=0.6, height=4).translate((0, 0, -2))`).
    /// `normal=False` only moves it.
    ///
    /// **Rows and columns are measured in millimetres on the surface.** A
    /// plane's rows run along its stable in-plane direction;
    /// a cylinder's, cone's and torus's rows are circles about the axis
    /// (closed: the copies go all the way round, the first at the +X side),
    /// stacked along the axis, the slant or the tube; a sphere's are
    /// latitude circles about Z; a free-form face's are its NURBS isolines,
    /// sampled by chord length. Three ways to count:
    /// * `rows=, cols=` — the same number on every row (on a cone the
    ///   columns line up on the rulings and the pitch grows with the
    ///   radius);
    /// * `count=` — that many in all, **shared out among the rows in
    ///   proportion to their length**, so the pitch is the same in mm on
    ///   every row of a cone as nearly as whole numbers allow (`rows=` says
    ///   how many rows; without it, rows are chosen for near-square cells);
    /// * `spacing=(du, dv)` (or one number) — as many as fit, `du` mm apart
    ///   along a row and `dv` between rows, centred; on a closed row the
    ///   pitch is rounded so the ring closes.
    ///
    /// `stagger=True` shifts every other row by half a pitch (a brick
    /// course: an open row then has one copy fewer). `margin=` keeps every
    /// copy at least that far from the face's boundary, and copies that
    /// would fall outside the face's trimmed region (a plate's hole, a cut
    /// slot) are left out and counted in the feature's `skipped` arg. A
    /// mesh face with no surface is refused: it has no normal.
    ///
    /// Without `merge` the call returns the list of copies as instances
    /// under one `pattern_on_surface` feature, named `<name>/row[i].col[j]`
    /// (`j` counted among the copies kept on row `i`); `merge=True` fuses
    /// them into one Body as `pattern_linear(merge=True)` does — 600
    /// disjoint holes are one compose, and `subtract(liner, holes)` one
    /// boolean. Overlapping copies are a warning naming the pair.
    ///
    /// ```python
    /// hole = cylinder(d=0.6, height=4).translate((0, 0, -2))
    /// holes = pattern_on_surface(hole, liner.faces(kind=kind.cone), rows=10, count=600, merge=True, name="holes")
    /// liner = subtract(liner, holes, name="liner")
    /// studs = pattern_on_surface(cylinder(d=3, height=4), plate.face("top"), rows=3, cols=5, margin=6, stagger=True)
    /// ```
    #[allow(clippy::too_many_arguments)]
    fn pattern_on_surface<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] face: Value<'v>,
        #[starlark(require = named)] rows: Option<UnpackFloat>,
        #[starlark(require = named)] cols: Option<UnpackFloat>,
        #[starlark(require = named)] spacing: Option<Value<'v>>,
        #[starlark(require = named)] count: Option<UnpackFloat>,
        #[starlark(require = named, default = true)] normal: bool,
        #[starlark(require = named, default = false)] stagger: bool,
        #[starlark(require = named, default = UnpackFloat(0.0))] margin: UnpackFloat,
        #[starlark(require = named, default = false)] merge: bool,
        #[starlark(require = named)] kernel: Option<&str>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let rows = rows.as_ref().map(|r| count_arg(r, "pattern_on_surface: rows")).transpose()?;
        let cols = cols.as_ref().map(|c| count_arg(c, "pattern_on_surface: cols")).transpose()?;
        let total = count.as_ref().map(|c| count_arg(c, "pattern_on_surface: count")).transpose()?;
        let spacing = spacing.map(spacing_arg).transpose()?;
        let margin = num(&margin, "pattern_on_surface: margin")?;
        let kernel = kernel_arg(kernel)?;
        let forms = "give rows= and cols=, or count= (with rows= or not), or spacing=";
        let grid = match (rows, cols, total, spacing) {
            (_, _, _, Some((du, dv))) => {
                if rows.is_some() || cols.is_some() || total.is_some() {
                    anyhow::bail!("pattern_on_surface: spacing= fixes the pitch, so rows=, cols= and count= do not go with it; {forms}");
                }
                Grid::Spacing { du, dv }
            }
            (_, Some(_), Some(_), None) => anyhow::bail!("pattern_on_surface: cols= is a number per row and count= a total, so give one or the other; {forms}"),
            (rows, None, Some(count), None) => Grid::Count { rows, count },
            (Some(rows), Some(cols), None, None) => Grid::RowsCols { rows, cols },
            (Some(rows), None, None, None) => Grid::RowsCols { rows, cols: 1 },
            (None, Some(cols), None, None) => Grid::RowsCols { rows: 1, cols },
            (None, None, None, None) => anyhow::bail!("pattern_on_surface: how many copies? {forms}"),
        };
        let (face_body, face_names) = face_arg(face)?;
        let spec = SurfaceSpec { grid, normal, stagger, margin, max_count: Limits::MAX_PATTERN_COUNT };
        let mut args = BTreeMap::new();
        match grid {
            Grid::RowsCols { cols, .. } => {
                args.insert("cols".into(), cols.to_string());
            }
            Grid::Count { count, .. } => {
                args.insert("count".into(), count.to_string());
            }
            Grid::Spacing { du, dv } => {
                args.insert("spacing".into(), format!("({}, {})", cadpole_core::context::fmt_num(du), cadpole_core::context::fmt_num(dv)));
            }
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = ctx.pattern_on_surface(
            body.body_id(),
            BodyId(face_body),
            &face_names,
            &spec,
            merge,
            kernel,
            args,
            name.map(str::to_owned),
            span,
        )?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "pattern_on_surface")?;
        result_value(ids, merge, &ctx, eval)
    }
}

/// A count capped at the pattern limit before anything is sized by it.
fn count_arg(v: &UnpackFloat, what: &str) -> anyhow::Result<usize> {
    count(v, what, 1, Limits::MAX_PATTERN_COUNT)
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};
    use cadpole_core::context::Severity;
    use cadpole_core::fmath;
    use cadpole_core::ids::BodyId;
    use cadpole_core::Context;
    use cadpole_core::Vec3;

    fn body_named(ctx: &Context, name: &str) -> BodyId {
        ctx.bodies
            .iter()
            .find(|b| b.name == name)
            .unwrap_or_else(|| panic!("no body {name:?}; have {:?}", ctx.bodies.iter().map(|b| &b.name).collect::<Vec<_>>()))
            .id
    }

    fn warnings(ctx: &Context) -> Vec<String> {
        ctx.diagnostics.iter().filter(|d| d.severity == Severity::Warning).map(|d| d.message.clone()).collect()
    }

    /// Where a rigid copy's origin is: the body's origin carried through
    /// its chain of copies.
    fn origin_of(ctx: &Context, id: BodyId) -> Vec3 {
        let mut p = Vec3::ZERO;
        let mut chain = Vec::new();
        let mut cur = id;
        while let Some((src, t)) = ctx.body(cur).unwrap().source() {
            chain.push(t);
            cur = src;
        }
        for t in chain.iter().rev() {
            p = t.point(p);
        }
        p
    }

    /// The pose of a pattern copy: the transform it was placed by.
    fn pose(ctx: &Context, id: BodyId) -> cadpole_core::Transform {
        ctx.body(id).unwrap().source().unwrap().1
    }

    /// Along a line on +Z the copies are `pattern_linear`'s to the bit; they
    /// are instances (one tessellation for the lot), named under one
    /// feature, and the source is consumed.
    #[test]
    fn along_a_line_is_pattern_linear_to_the_bit() {
        let mut ctx = run(
            "a = box(4, 6, 2, name=\"a\")\n\
             b = box(4, 6, 2, name=\"b\")\n\
             lin = pattern_linear(a, 4, (0, 0, 10), name=\"lin\")\n\
             row = pattern_along(b, curve((0, 0, 0)).line((0, 0, 30)), count=4, name=\"row\")\n\
             assert_eq(len(row), 4)\n\
             assert_eq(row[1].name, \"row/copy[1]\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let lin: Vec<BodyId> = {
            let mut v = vec![body_named(&ctx, "a")];
            v.extend((1..4).map(|k| body_named(&ctx, &format!("lin_{k}"))));
            v
        };
        let row: Vec<BodyId> = (0..4).map(|k| body_named(&ctx, &format!("row/copy[{k}]"))).collect();
        // The two sources' tessellations, then nothing more for eight bodies.
        ctx.body_mesh(body_named(&ctx, "a")).unwrap();
        ctx.body_mesh(body_named(&ctx, "b")).unwrap();
        let before = ctx.stats.tessellations;
        for (l, r) in lin.iter().zip(&row) {
            let (lm, lt) = {
                let (m, t) = ctx.body_mesh(*l).unwrap();
                (m.clone(), t.clone())
            };
            let (rm, rt) = ctx.body_mesh(*r).unwrap();
            assert_eq!(lm.positions, rm.positions, "positions to the bit");
            assert_eq!(lm.indices, rm.indices);
            assert_eq!(lt.faces.len(), rt.faces.len());
        }
        assert_eq!(ctx.stats.tessellations - before, 0, "the copies share their source's mesh");
        assert!(!ctx.body(body_named(&ctx, "b")).unwrap().visible, "the source is consumed");
        let f = ctx.features.iter().find(|f| f.id.0 == "row").unwrap();
        assert_eq!(f.kind, "pattern_along");
        assert_eq!(f.outputs.len(), 4);
        assert_eq!(f.args["count"], "4");
        assert_eq!(f.args["length"], "30");
        assert_eq!(f.args["frame"], "rmf");
        assert!(row.iter().all(|id| ctx.body(*id).unwrap().is_copy()));
    }

    /// Round a circle the copies sit where `pattern_circular` puts them,
    /// each with its z along the tangent.
    #[test]
    fn along_a_circle_lands_where_pattern_circular_does() {
        let ctx = run(
            "s = sphere(2, name=\"s\")\n\
             t = sphere(2, name=\"t\")\n\
             ring = pattern_circular(translate(s, (20, 0, 0)), 8, name=\"ring\")\n\
             circle = sketch(XY).circle(20).curves()[0]\n\
             beads = pattern_along(t, circle, count=8, name=\"beads\")\n\
             assert_eq(len(beads), 8)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let ring: Vec<BodyId> = {
            let mut v = vec![body_named(&ctx, "translate_1")];
            v.extend((1..8).map(|k| body_named(&ctx, &format!("ring_{k}"))));
            v
        };
        let beads: Vec<BodyId> = (0..8).map(|k| body_named(&ctx, &format!("beads/copy[{k}]"))).collect();
        for (k, (r, b)) in ring.iter().zip(&beads).enumerate() {
            let (cr, cb) = (origin_of(&ctx, *r), origin_of(&ctx, *b));
            assert!((cr - cb).length() < 1e-9, "copy {k}: {cr:?} vs {cb:?}");
            let z = pose(&ctx, *b).direction(Vec3::Z);
            let angle = std::f64::consts::TAU * k as f64 / 8.0;
            let (s, c) = fmath::sin_cos(angle);
            let tangent = Vec3::new(-s, c, 0.0);
            assert!((z - tangent).length() < 1e-9, "copy {k}: z {z:?} along the tangent {tangent:?}");
        }
    }

    /// On a helix every copy's z is the helix tangent (to 1e-9), its origin
    /// on the helix, and the pattern records no tessellation of its own.
    #[test]
    fn along_a_helix_aligns_z_with_the_tangent() {
        let mut ctx = run(
            "h = helix(10, 6, 3, name=\"h\")\n\
             bead = cylinder(1, 2, name=\"bead\")\n\
             beads = pattern_along(bead, h, count=25, name=\"beads\")\n\
             assert_eq(len(beads), 25)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let seg = cadpole_core::Curve::helix(Vec3::ZERO, Vec3::Z, 10.0, 6.0, 3.0, true, 0.0, 0.0).unwrap();
        let helix = seg.segments[0].helix().unwrap();
        for k in 0..25 {
            let id = body_named(&ctx, &format!("beads/copy[{k}]"));
            let t = pose(&ctx, id);
            let f = k as f64 / 24.0;
            let p = helix.point(f);
            assert!((t.point(Vec3::ZERO) - p).length() < 1e-9, "copy {k} sits on the helix");
            let z = t.direction(Vec3::Z);
            assert!((z.dot(helix.tangent(f)) - 1.0).abs() < 1e-9, "copy {k}: z·tangent = {}", z.dot(helix.tangent(f)));
        }
        ctx.body_mesh(body_named(&ctx, "bead")).unwrap();
        let before = ctx.stats.tessellations;
        for k in 0..25 {
            let id = body_named(&ctx, &format!("beads/copy[{k}]"));
            ctx.body_mesh(id).unwrap();
        }
        assert_eq!(ctx.stats.tessellations - before, 0, "25 copies, one tessellation (the source's)");
    }

    /// The merged form is `pattern_linear(merge=True)`'s: one body, one
    /// compose when the copies are apart.
    #[test]
    fn merge_composes_the_copies_into_one_body() {
        let ctx = run(
            "post = box(4, 4, 5, name=\"post\")\n\
             fence = pattern_along(post, curve((0, 0, 0)).line((40, 0, 0)), count=5, align=False, merge=True, name=\"fence\")\n\
             assert_solid(fence)\n\
             assert_close(fence.volume(), 400)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "fence").unwrap();
        assert_eq!(f.kind, "pattern_along");
        assert_eq!(f.args["merge"], "true");
        assert_eq!(f.args["kind"], "mesh");
        let notes: Vec<&str> = ctx.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(notes.iter().any(|n| n.contains("composed in one step")), "{notes:?}");
    }

    /// On a plane the layout is a grid; margin keeps it off the edge; a
    /// staggered row is a brick course; the names say the cell.
    #[test]
    fn on_a_plane_is_a_grid() {
        let ctx = run(
            "plate = box(100, 60, 4, center=True, name=\"plate\")\n\
             stud = cylinder(2, 3, name=\"stud\")\n\
             studs = pattern_on_surface(stud, plate.face(\"top\"), rows=3, cols=5, margin=10, name=\"studs\")\n\
             assert_eq(len(studs), 15)\n\
             assert_eq(studs[0].name, \"studs/row[0].col[0]\")\n\
             assert_eq(studs[14].name, \"studs/row[2].col[4]\")\n\
             brick = pattern_on_surface(cylinder(2, 3), plate.face(\"bottom\"), rows=3, cols=5, margin=10, stagger=True, name=\"brick\")\n\
             assert_eq(len(brick), 14)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // The top spans x ∈ [-50, 50], y ∈ [-30, 30] at z = 4; with a 10 mm
        // margin the grid spans [-40, 40] × [-20, 20].
        for i in 0..3 {
            for j in 0..5 {
                let id = body_named(&ctx, &format!("studs/row[{i}].col[{j}]"));
                let t = pose(&ctx, id);
                let o = t.point(Vec3::ZERO);
                assert!((o.z - 4.0).abs() < 1e-9, "on the top face: {o:?}");
                assert!((t.direction(Vec3::Z) - Vec3::Z).length() < 1e-12, "z along the outward normal");
                assert!((o.x - (-40.0 + 20.0 * j as f64)).abs() < 1e-9, "x of ({i},{j}) = {}", o.x);
                assert!((o.y - (-20.0 + 20.0 * i as f64)).abs() < 1e-9, "y of ({i},{j}) = {}", o.y);
            }
        }
        // The brick course's middle row sits between the others' columns
        // and points down (the bottom face's outward normal).
        let id = body_named(&ctx, "brick/row[1].col[0]");
        let t = pose(&ctx, id);
        let o = t.point(Vec3::ZERO);
        assert!((o.x.abs() - 30.0).abs() < 1e-9 || (o.x.abs() - 10.0).abs() < 1e-9, "{o:?}");
        assert!((t.direction(Vec3::Z) + Vec3::Z).length() < 1e-12);
        let f = ctx.features.iter().find(|f| f.id.0 == "studs").unwrap();
        assert_eq!(f.kind, "pattern_on_surface");
        assert_eq!(f.args["surface"], "plane");
        assert_eq!(f.args["rows"], "3");
        assert_eq!(f.args["per_row"], "5 × 3");
        assert_eq!(f.args["row_pitch"], "20");
        assert_eq!(f.args["across_pitch"], "20");
        assert_eq!(f.args["margin"], "10");
    }

    /// On a cylinder the rows are circles about the axis: every copy's z
    /// is the radial normal, its origin at the radius, and a row goes all
    /// the way round (the side's two halves are one surface).
    #[test]
    fn on_a_cylinder_rows_are_circles_and_z_is_radial() {
        let ctx = run(
            "can = cylinder(20, 50, name=\"can\")\n\
             knob = sphere(1.5, name=\"knob\")\n\
             knobs = pattern_on_surface(knob, can.faces(kind=kind.cylinder), rows=4, cols=12, margin=5, name=\"knobs\")\n\
             assert_eq(len(knobs), 48)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        for i in 0..4 {
            for j in 0..12 {
                let id = body_named(&ctx, &format!("knobs/row[{i}].col[{j}]"));
                let t = pose(&ctx, id);
                let o = t.point(Vec3::ZERO);
                let radial = Vec3::new(o.x, o.y, 0.0);
                assert!((radial.length() - 20.0).abs() < 1e-9, "({i},{j}) at the radius: {o:?}");
                assert!((o.z - (5.0 + 40.0 * i as f64 / 3.0)).abs() < 1e-9, "row {i} at its height: {o:?}");
                let angle = std::f64::consts::TAU * j as f64 / 12.0;
                let (s, c) = fmath::sin_cos(angle);
                assert!(
                    (o.x - 20.0 * c).abs() < 1e-9 && (o.y - 20.0 * s).abs() < 1e-9,
                    "({i},{j}) at its angle: {o:?}"
                );
                assert!((t.direction(Vec3::Z) - radial.normalized()).length() < 1e-9, "z radial");
                let x = t.direction(Vec3::X);
                assert!((x - Vec3::new(-s, c, 0.0)).length() < 1e-9, "x along the row");
            }
        }
        let f = ctx.features.iter().find(|f| f.id.0 == "knobs").unwrap();
        assert_eq!(f.args["surface"], "cylinder");
        assert_eq!(f.args["closed_rows"], "true");
        assert_eq!(f.args["per_row"], "12 × 4");
    }

    /// On a cone the rows are circles of different radii; `count=` shares
    /// the copies out by row length so the pitch is equal in mm, `cols=`
    /// puts the same number on every row, and `spacing=` puts as many as
    /// fit at that pitch on each.
    #[test]
    fn on_a_cone_the_pitch_is_equal_in_mm_and_count_is_shared_by_row_length() {
        let ctx = run(
            "c = cone(10, 40, 60, name=\"c\")\n\
             pin = cylinder(0.5, 2, name=\"pin\")\n\
             shared = pattern_on_surface(pin, c.faces(kind=kind.cone), rows=5, count=200, name=\"shared\")\n\
             assert_eq(len(shared), 200)\n\
             cols = pattern_on_surface(cylinder(0.5, 2), c.faces(kind=kind.cone), rows=5, cols=20, name=\"cols\")\n\
             assert_eq(len(cols), 100)\n\
             spaced = pattern_on_surface(cylinder(0.5, 2), c.faces(kind=kind.cone), spacing=(6, 12), name=\"spaced\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let feature = |id: &str| ctx.features.iter().find(|f| f.id.0 == id).unwrap();
        // The radius grows from 10 to 40 over 60 mm, so the rows' lengths
        // are in the ratio of their radii: the counts follow them (16, 28,
        // 40, 52, 64) and the pitch is the same on every row.
        let shared = feature("shared");
        assert_eq!(shared.args["surface"], "cone");
        assert_eq!(shared.args["per_row"], "16, 28, 40, 52, 64");
        let per_row = [16usize, 28, 40, 52, 64];
        let radii = |i: usize| 10.0 + 30.0 * i as f64 / 4.0;
        let pitches: Vec<f64> = (0..5).map(|i| std::f64::consts::TAU * radii(i) / per_row[i] as f64).collect();
        for p in &pitches {
            assert!((p - pitches[0]).abs() < 1e-9, "equal pitch in mm: {pitches:?}");
        }
        let reported: f64 = shared.args["row_pitch"].parse().unwrap();
        assert!((reported - pitches[0]).abs() < 1e-6, "{reported} vs {}", pitches[0]);
        for i in 0..5 {
            for j in 0..per_row[i] {
                let id = body_named(&ctx, &format!("shared/row[{i}].col[{j}]"));
                let t = pose(&ctx, id);
                let o = t.point(Vec3::ZERO);
                let r = Vec3::new(o.x, o.y, 0.0).length();
                // The cone's apex and half-angle are recognised from the
                // surface, to a micron.
                assert!((r - radii(i)).abs() < 1e-6, "row {i} is a circle of radius {}: {o:?}", radii(i));
                // The outward normal of a cone widening upward leans down.
                let z = t.direction(Vec3::Z);
                let want = (Vec3::new(o.x, o.y, 0.0).normalized() * 2.0 - Vec3::Z).normalized();
                assert!((z - want).length() < 1e-6, "normal at ({i},{j}): {z:?} vs {want:?}");
            }
        }
        let cols = feature("cols");
        assert_eq!(cols.args["per_row"], "20 × 5");
        assert!(cols.args["row_pitch"].contains(".."), "the pitch differs by row: {}", cols.args["row_pitch"]);
        // Rows 12 mm apart along the 67.08 mm slant, centred: six of them,
        // each with as many pins as fit at 6 mm round its circle.
        let spaced = feature("spaced");
        assert_eq!(spaced.args["rows"], "6");
        let (v0, v1) = (fmath::sqrt(500.0), fmath::sqrt(8000.0));
        let lead = ((v1 - v0) - 60.0) * 0.5;
        let want: Vec<String> = (0..6)
            .map(|i| {
                let r = (v0 + lead + 12.0 * i as f64) / fmath::sqrt(5.0);
                ((std::f64::consts::TAU * r / 6.0).round() as usize).to_string()
            })
            .collect();
        assert_eq!(spaced.args["per_row"], want.join(", "));
    }

    /// On a sphere the rows are latitude circles and z is radial.
    #[test]
    fn on_a_sphere_z_is_radial() {
        let ctx = run(
            "ball = sphere(15, name=\"ball\")\n\
             dimple = sphere(1, name=\"dimple\")\n\
             dimples = pattern_on_surface(dimple, ball.faces(kind=kind.sphere), rows=4, cols=10, margin=3, name=\"dimples\")\n\
             assert_eq(len(dimples), 40)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "dimples").unwrap();
        assert_eq!(f.args["surface"], "sphere");
        let mut zs = std::collections::BTreeSet::new();
        for id in &f.outputs {
            let t = pose(&ctx, *id);
            let o = t.point(Vec3::ZERO);
            assert!((o.length() - 15.0).abs() < 1e-9, "on the sphere: {o:?}");
            assert!((t.direction(Vec3::Z) - o.normalized()).length() < 1e-9, "z radial");
            zs.insert((o.z * 1e6).round() as i64);
        }
        assert_eq!(zs.len(), 4, "four latitude rows: {zs:?}");
    }

    /// On a free-form face — a smooth loft's side, a NURBS surface with no
    /// class — the chart is the surface's own domain sampled by chord
    /// length: every copy sits on the surface (within the chord tolerance
    /// of the face's triangles) with z along the outward normal there and x
    /// in the surface, and the margin is judged against the face's curved
    /// boundary, so a cell the isoline puts `margin` from an end may still
    /// be nearer the edge where the edge bows in — those are skipped, and
    /// counted.
    #[test]
    fn on_a_nurbs_face_copies_sit_on_the_surface_with_z_along_its_normal() {
        use cadpole_core::naming::point_triangle_distance;
        let mut ctx = run(
            "sections = [sketch(XY).rect(40, 30), sketch(XY.offset(20)).rect(30, 36), sketch(XY.offset(40)).rect(44, 28)]\n\
             blob = loft(sections, ruled=False, name=\"blob\")\n\
             side = blob.faces(kind=kind.nurbs, nearest=(25, 0, 20))\n\
             assert_eq(side.count(), 1)\n\
             studs = pattern_on_surface(cylinder(1, 2), side, rows=4, cols=6, margin=3, name=\"studs\")\n\
             assert_true(len(studs) >= 20)\n\
             assert_eq(studs[0].name, \"studs/row[0].col[0]\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (outputs, skipped) = {
            let f = ctx.features.iter().find(|f| f.id.0 == "studs").unwrap();
            assert_eq!(f.kind, "pattern_on_surface");
            assert_eq!(f.args["surface"], "nurbs");
            assert_eq!(f.args["face"], "blob/side[0.1]");
            assert_eq!(f.args["per_row"], "6 × 4");
            assert!(f.args["row_pitch"].contains(".."), "the sections differ in width, so the rows do: {}", f.args["row_pitch"]);
            let skipped: usize = f.args.get("skipped").map_or(0, |s| s.parse().unwrap());
            (f.outputs.clone(), skipped)
        };
        assert_eq!(outputs.len() + skipped, 24, "every cell laid out is kept or counted as skipped");
        assert!((1..=4).contains(&skipped), "{skipped} skipped: the boundary bows in past the isoline's margin at a corner or two");
        let poses: Vec<cadpole_core::Transform> = outputs.iter().map(|id| pose(&ctx, *id)).collect();
        let blob = body_named(&ctx, "blob");
        let tris: Vec<[Vec3; 3]> = {
            let (mesh, topo) = ctx.body_mesh(blob).unwrap();
            let face = topo.faces.iter().find(|f| f.name == "blob/side[0.1]").unwrap();
            (face.tri_start..face.tri_start + face.tri_count).filter_map(|t| mesh.triangle(t as usize)).collect()
        };
        for (k, t) in poses.iter().enumerate() {
            let o = t.point(Vec3::ZERO);
            let z = t.direction(Vec3::Z);
            let x = t.direction(Vec3::X);
            let (dist, n) = tris
                .iter()
                .map(|tri| (point_triangle_distance(o, tri[0], tri[1], tri[2]), (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalized()))
                .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
                .unwrap();
            assert!(dist < 0.1, "copy {k} at {o:?} sits {dist} mm off the face (chord tolerance 0.05)");
            assert!(z.dot(n) > 0.95, "copy {k}: z {z:?} against the nearest triangle's outward normal {n:?}");
            // The sections go 40, 30, 44 wide, so the side necks in and
            // out by up to about 30°: the normal leans, and still faces +x.
            assert!(z.x > 0.7, "the +x side's normal faces +x: {z:?}");
            assert!(x.dot(z).abs() < 1e-9 && (x.length() - 1.0).abs() < 1e-9, "x lies in the surface");
            assert!(o.x > 10.0, "on the +x side: {o:?}");
        }
    }

    /// Copies that fall outside the face's trimmed region are left out,
    /// and the margin keeps the rest off the boundary of the hole too.
    #[test]
    fn the_trimmed_region_and_the_margin_exclude_copies() {
        let ctx = run(
            "plate = subtract(box(100, 100, 4, center=True), cylinder(20, 10), name=\"plate\")\n\
             top = plate.faces(normal=(0, 0, 1))\n\
             studs = pattern_on_surface(cylinder(2, 3), top, rows=5, cols=5, margin=2, name=\"studs\")\n\
             assert_eq(len(studs), 24)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "studs").unwrap();
        assert_eq!(f.args["skipped"], "1");
        for id in &f.outputs {
            let o = pose(&ctx, *id).point(Vec3::ZERO);
            assert!(Vec3::new(o.x, o.y, 0.0).length() >= 22.0 - 1e-6, "at least the margin from the hole: {o:?}");
        }
        let infos: Vec<&str> =
            ctx.diagnostics.iter().filter(|d| d.severity == Severity::Info).map(|d| d.message.as_str()).collect();
        assert!(infos.iter().any(|m| m.contains("1 of 25 copies fell outside")), "{infos:?}");
    }

    /// The refusals name the cause; overlapping copies are a warning naming
    /// the pair.
    #[test]
    fn refusals_and_the_overlap_warning() {
        let cases: [(&str, &str); 7] = [
            (
                "b = box(2, 2, 2)\np = pattern_along(b, curve((0, 0, 0)).line((0, 0, 30)), count=5, spacing=10)\n",
                "5 copies 10 mm apart span 40 mm, and the curve is 30 mm long",
            ),
            ("b = box(2, 2, 2)\np = pattern_along(b, curve((0, 0, 0)).line((0, 0, 30)))\n", "give count="),
            ("b = box(2, 2, 2)\np = pattern_along(b, path((0, 0)).line((0, 10)), count=3)\n", "path.on(plane)"),
            (
                "b = box(2, 2, 2)\nh = hull(box(10, 10, 10), translate(sphere(5), (20, 0, 0)))\n\
                 p = pattern_on_surface(b, h.faces()[0], rows=2, cols=2)\n",
                "mesh face with no surface",
            ),
            ("b = box(2, 2, 2)\nplate = box(10, 10, 2)\np = pattern_on_surface(b, plate.face(\"top\"))\n", "how many copies?"),
            (
                "b = box(2, 2, 2)\nplate = box(10, 10, 2)\np = pattern_on_surface(b, plate.face(\"top\"), rows=2, cols=2, spacing=3)\n",
                "spacing= fixes the pitch",
            ),
            (
                "b = box(2, 2, 2)\nplate = box(10, 10, 2)\np = pattern_on_surface(b, plate.faces(kind=kind.plane), rows=2, cols=2)\n",
                "lie on different surfaces",
            ),
        ];
        for (src, want) in cases {
            let ctx = run(src);
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{src}\n{e:?}");
            assert!(e[0].contains(want), "{src}\n{}", e[0]);
        }
        let ctx = run(
            "b = box(10, 10, 2)\n\
             p = pattern_along(b, curve((0, 0, 0)).line((30, 0, 0)), count=7, align=False, name=\"p\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let w = warnings(&ctx);
        assert!(w.iter().any(|m| m.contains("copies copy[0] and copy[1] overlap") && m.contains("5 more pairs")), "{w:?}");
    }

    /// `sweep_frame.frenet` on a helix gives the curvature normal as x, and
    /// a fixed frame keeps `up` as x.
    #[test]
    fn the_frame_vocabulary_reaches_pattern_along() {
        let ctx = run(
            "h = helix(10, 6, 2)\n\
             a = pattern_along(cylinder(1, 2), h, count=9, frame=sweep_frame.frenet, name=\"a\")\n\
             b = pattern_along(cylinder(1, 2), curve((0, 0, 0)).line((30, 0, 0)), count=3, frame=sweep_frame.fixed, up=(0, 0, 1), name=\"b\")\n\
             c = pattern_along(cylinder(1, 2), h, count=3, frame=\"bishop\")\n",
        );
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("valid: rmf, frenet, fixed"), "{}", e[0]);
        for k in 0..9 {
            let id = body_named(&ctx, &format!("a/copy[{k}]"));
            let t = pose(&ctx, id);
            let o = t.point(Vec3::ZERO);
            let inward = -Vec3::new(o.x, o.y, 0.0).normalized();
            assert!((t.direction(Vec3::X) - inward).length() < 1e-9, "Frenet's normal is the curvature normal, toward the axis");
        }
        let t = pose(&ctx, body_named(&ctx, "b/copy[1]"));
        assert!((t.direction(Vec3::X) - Vec3::Z).length() < 1e-12);
        assert!((t.direction(Vec3::Z) - Vec3::X).length() < 1e-12);
    }
}
