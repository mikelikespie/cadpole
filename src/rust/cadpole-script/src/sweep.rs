//! The Starlark surface of `sweep_along` and `helix`
//! (docs/starlark-api.md). A module of their own, as the
//! primitives are, so `globals.rs` stays small; the doc comment on each
//! function is its entry in `cadpole doc` (`apidoc`), so each is written for
//! someone who has never seen Cadpole.

use std::collections::BTreeMap;

use cadpole_core::context::{fmt_num, fmt_vec};
use cadpole_core::curve::Curve;
use cadpole_core::geom::Vec3;
use cadpole_core::sketch::Sketch;
use cadpole_core::sweep::Frame;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::float::UnpackFloat;
use starlark::values::{Value, ValueLike};

use crate::check::{VecArg, num, vec3};
use crate::globals::{bodies_value, sketch_arg};
use crate::store::{call_span, store};
use crate::values::{KCurve, KPath};
use crate::vocab::not_one_of;

/// The profile of a sweep: a `Sketch` or `ConstrainedSketch`, taken as
/// drawn, or a closed planar `Curve`, which becomes a one-loop sketch on its
/// own plane.
fn profile_arg<'v>(v: Value<'v>, eval: &Evaluator<'v, '_, '_>) -> anyhow::Result<(cadpole_core::FeatureId, Sketch)> {
    if let Some(c) = v.downcast_ref::<KCurve>() {
        let (plane, l) = c.curve.to_loop("sweep_along")?;
        return Ok((cadpole_core::FeatureId(c.feature.clone()), Sketch::new(plane).with_loop(l)));
    }
    if v.downcast_ref::<KPath>().is_some() {
        anyhow::bail!(
            "sweep_along: the profile is a sketch Path, which has no plane; add it to a sketch (sketch(plane).add(path)) \
             or lift it with path.on(plane).close()"
        );
    }
    sketch_arg(v, "sweep_along: profile", eval)
}

/// The path of a sweep: a `Curve`.
fn path_arg<'v>(v: Value<'v>) -> anyhow::Result<(cadpole_core::FeatureId, Curve)> {
    if let Some(c) = v.downcast_ref::<KCurve>() {
        return Ok((cadpole_core::FeatureId(c.feature.clone()), c.curve.clone()));
    }
    if v.downcast_ref::<KPath>().is_some() {
        anyhow::bail!(
            "sweep_along: the path is a sketch Path, which has no plane; lift it into space with path.on(plane), \
             which gives the Curve a sweep follows"
        );
    }
    anyhow::bail!("sweep_along: path must be a Curve (curve(...), helix(...) or path.on(plane)), got {}", v.get_type())
}

#[starlark_module]
pub(crate) fn sweep_globals(builder: &mut GlobalsBuilder) {
    /// Sweep `profile` along `path`: the solid a planar section traces as
    /// it moves along a curve in space — a pipe, a spring, a thread, a
    /// chamfer cutter along an edge.
    ///
    /// `profile` is a `Sketch` (or `ConstrainedSketch`, or a closed planar
    /// `Curve`), taken **as drawn**: wherever its plane is, that is where
    /// the sweep starts, so a sketch on XY swept up +Z is the extrude. Draw
    /// it at the path's start, usually on `path.start_plane` (the plane
    /// perpendicular to the path there). `path` is a `Curve`: `curve(...)`
    /// with lines, arcs, béziers and splines, `helix(...)`, or a sketch
    /// path lifted with `path.on(plane)`; it must be open and
    /// tangent-continuous (a corner is a diagnostic — round it with an arc,
    /// the way a bent pipe is drawn).
    ///
    /// **What is exact.** A straight segment is a translation of the
    /// profile and a circular arc a rotation about its own axis, both exact
    /// B-rep (a circle swept round an arc is a true torus that
    /// `faces(kind=kind.torus)` finds). A spline or helix segment — and any
    /// segment with `twist=` — has no exact surface on this kernel, so Cadpole
    /// runs one cubic B-spline surface through copies of the profile placed
    /// along it (the smooth loft's own skinning), chooses the stations by
    /// the same bounds the tessellator divides a patch by, then *measures*
    /// how far the surface sits from the true sweep and refines until it is
    /// within a target of a fifth of the document tolerance (0.01 mm at the
    /// default); the measured figure is an info diagnostic on the feature
    /// and its `approximation` arg. A straight segment with `scale=` stays
    /// exact (a ruled surface).
    ///
    /// **`frame=`** says how the profile turns as it goes:
    /// * `sweep_frame.rmf` (default) — rotation-minimising: the profile never
    ///   turns about the path, so a square swept along any planar path is
    ///   still the same square, the same way up, at the end;
    /// * `sweep_frame.frenet` — the curvature normal: what a thread wants, since
    ///   on a helix it is the screw motion about the axis (a profile drawn
    ///   in the axial plane stays in it). Undefined on a straight segment
    ///   (the frame is carried through) and refused at an inflection or a
    ///   joint where the curvature normal jumps;
    /// * `sweep_frame.fixed` — `up=` stays up (projected perpendicular to the
    ///   tangent); refused where the path runs along `up`.
    ///
    /// `up=` is `sweep_frame.fixed`'s direction and the seed of the others'
    /// normal (it changes nothing but `end_plane` under `rmf`). `twist=` is
    /// an extra turn of the profile about the path over its whole length, in
    /// degrees; `scale=` the profile's size at the end relative to the start
    /// (about the path point), both shared out by arc length. `cap=` is
    /// `cap.plane`: a sweep's ends are copies of its planar profile.
    ///
    /// Faces: `start` and `end`; `side[l.s]` for the face swept from segment
    /// `s` of loop `l` of the profile, and `side[l.s]#b` along path segment
    /// `b` when the path has several, with `side[l.s]` the alias for the
    /// whole run. Feature: `sweep`, args `frame`, `path`,
    /// `segments` (how each was built), `length`, `approximation`.
    ///
    /// ```python
    /// # A bent tube: an annulus along line–arc–line, every segment exact.
    /// route = curve((0, 0, 0)).line((0, 0, 40)).arc((20, 0, 60), via=(5.86, 0, 54.14)).line((60, 0, 60))
    /// tube = sweep_along(sketch(XY).circle(5).circle(4), route, name="tube")
    ///
    /// # A spring: a round wire along a helix.
    /// coil = helix(10, 5, 6, name="coil")
    /// spring = sweep_along(sketch(coil.start_plane).circle(1.5), coil, name="spring")
    /// ```
    fn sweep_along<'v>(
        #[starlark(require = pos)] profile: Value<'v>,
        #[starlark(require = pos)] path: Value<'v>,
        #[starlark(require = named, default = "rmf")] frame: &str,
        #[starlark(require = named)] up: Option<VecArg>,
        #[starlark(require = named, default = UnpackFloat(1.0))] scale: UnpackFloat,
        #[starlark(require = named, default = UnpackFloat(0.0))] twist: UnpackFloat,
        #[starlark(require = named, default = "plane")] cap: &str,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let frame = Frame::parse(frame)?;
        if cap != "plane" {
            let valid = cadpole_core::loft::Cap::ALL.iter().map(|c| c.name()).collect::<Vec<_>>();
            if valid.contains(&cap) {
                anyhow::bail!(
                    "sweep_along: cap={cap:?} is not available on a sweep: its two ends are copies of its planar profile, \
                     so they are always planar caps (cap=\"plane\")"
                );
            }
            anyhow::bail!("{}", not_one_of(format!("sweep_along: cap={cap:?} is not a cap style"), &["plane"]));
        }
        let up = up.as_ref().map(|u| vec3(u, "sweep_along: up")).transpose()?;
        let scale = num(&scale, "sweep_along: scale")?;
        if !(scale > 0.0) {
            anyhow::bail!("sweep_along: scale must be positive, got {scale}");
        }
        let twist = num(&twist, "sweep_along: twist")?;
        let (profile_feature, sketch) = profile_arg(profile, eval)?;
        let (path_feature, curve) = path_arg(path)?;
        if sketch.loops.is_empty() {
            anyhow::bail!("sweep_along: the profile sketch has no loops to sweep");
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = ctx.sweep_along(
            &sketch,
            Some(&profile_feature),
            &curve,
            Some(&path_feature),
            frame,
            up,
            twist.to_radians(),
            scale,
            BTreeMap::new(),
            name.map(str::to_owned),
            span,
        )?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "sweep_along")?;
        bodies_value(heap, &ctx, ids)
    }

    /// A helix as a `Curve`: `turns` turns of radius `radius` about
    /// `axis` (default +Z) through `start` (the centre of the first turn,
    /// default the origin), advancing `pitch` mm per turn along the axis.
    /// `hand.right` (the default) turns counter-clockwise about the axis
    /// while advancing along it — a standard right-hand thread — and
    /// `hand.left` the other way. `taper` is how much the radius grows
    /// (shrinks, when negative) from the first turn to the last, in mm.
    /// The curve starts at `start + radius` along the world axis least
    /// aligned with `axis` (+X for a vertical helix), turned by `phase`
    /// degrees about the axis, so several helices interleave by phase.
    ///
    /// The helix is stored exactly and swept from directly: `sweep_along`
    /// reads its closed form, never a spline through points. Only where a
    /// helix has to become an edge of a solid (a loft section) is it
    /// approximated, within 0.001 mm. It is an open `Curve`, so `.line()`,
    /// `.arc()` and the rest continue it, and `.start_plane` / `.end_plane`
    /// are the planes perpendicular to it at its ends. Feature: `helix`,
    /// args `radius`, `pitch`, `turns`, `hand`, `taper`, `start`, `axis`,
    /// `phase`, `length`.
    ///
    /// ```python
    /// coil = helix(10, 5, 6, name="coil")                    # right-hand, 30 mm tall
    /// left = helix(4, 1.25, 8, hand=hand.left, start=(0, 0, 2))
    /// cone = helix(10, 4, 5, taper=-6)                       # closes from r=10 to r=4
    /// ```
    fn helix<'v>(
        #[starlark(require = pos)] radius: UnpackFloat,
        #[starlark(require = pos)] pitch: UnpackFloat,
        #[starlark(require = pos)] turns: UnpackFloat,
        #[starlark(require = named, default = "right")] hand: &str,
        #[starlark(require = named, default = UnpackFloat(0.0))] taper: UnpackFloat,
        #[starlark(require = named)] start: Option<VecArg>,
        #[starlark(require = named)] axis: Option<VecArg>,
        #[starlark(require = named, default = UnpackFloat(0.0))] phase: UnpackFloat,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KCurve> {
        let radius = num(&radius, "helix: radius")?;
        let pitch = num(&pitch, "helix: pitch")?;
        let turns = num(&turns, "helix: turns")?;
        let taper = num(&taper, "helix: taper")?;
        let phase = num(&phase, "helix: phase")?;
        if !crate::stdlib::HANDS.contains(&hand) {
            anyhow::bail!("{}", not_one_of(format!("helix: hand={hand:?} is not a hand"), crate::stdlib::HANDS));
        }
        let start = start.as_ref().map(|s| vec3(s, "helix: start")).transpose()?.unwrap_or(Vec3::ZERO);
        let axis = axis.as_ref().map(|a| vec3(a, "helix: axis")).transpose()?.unwrap_or(Vec3::Z);
        let curve = Curve::helix(start, axis, radius, pitch, turns, hand == "right", taper, phase.to_radians())?;
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut args = BTreeMap::new();
        args.insert("radius".into(), fmt_num(radius));
        args.insert("pitch".into(), fmt_num(pitch));
        args.insert("turns".into(), fmt_num(turns));
        args.insert("hand".into(), hand.to_owned());
        if taper != 0.0 {
            args.insert("taper".into(), fmt_num(taper));
        }
        args.insert("start".into(), fmt_vec(start));
        args.insert("axis".into(), fmt_vec(axis));
        if phase != 0.0 {
            args.insert("phase".into(), fmt_num(phase));
        }
        args.insert("length".into(), fmt_num(curve.length()));
        args.insert("segments".into(), "1".into());
        args.insert("kinds".into(), "helix".into());
        args.insert("closed".into(), "false".into());
        let fid = ctx.new_feature("helix", name.map(str::to_owned), vec![], args, span)?;
        Ok(KCurve { feature: fid.0, curve })
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};
    use cadpole_core::context::Severity;

    fn infos(ctx: &cadpole_core::Context) -> Vec<String> {
        ctx.diagnostics.iter().filter(|d| d.severity == Severity::Info).map(|d| d.message.clone()).collect()
    }

    /// The straight sweep is the extrude, the arc a torus, the helix a
    /// measured skin with its note on the feature and its args saying how.
    #[test]
    fn sweep_along_builds_and_reports_by_segment_kind() {
        let ctx = run(
            "sk = sketch(XY).rounded_rect(10, 6, 1.5)\n\
             up = curve((0, 0, 0)).line((0, 0, 12))\n\
             s = sweep_along(sk, up, name=\"s\")\n\
             e = extrude(sk, 12, name=\"e\")\n\
             assert_close(s.volume(), e.volume())\n\
             assert_eq(s.faces().count(), e.faces().count())\n\
             assert_eq(s.faces(name=\"side[0.1]\").count(), 1)\n\
             ring = sketch(plane(origin=(20, 0, 0), normal=(0, 1, 0), x_dir=(1, 0, 0))).circle(2)\n\
             arc = curve((20, 0, 0)).arc((0, 20, 0), via=(14.142135623730951, 14.142135623730951, 0))\n\
             t = sweep_along(ring, arc, name=\"t\")\n\
             assert_eq(t.faces(kind=kind.torus).count(), 2)\n\
             assert_close(t.volume(), math.pi * 4 * math.pi / 2 * 20, rel=0.025)\n\
             coil = helix(10, 5, 2, name=\"coil\")\n\
             assert_close(coil.length, 2 * math.hypot(2 * math.pi * 10, 5))\n\
             assert_close(coil.start_plane.origin, (10, 0, 0))\n\
             assert_close(coil.end_plane.origin, (10, 0, 10), tol=1e-9)\n\
             w = sweep_along(sketch(coil.start_plane).circle(1.5), coil, name=\"w\")\n\
             assert_solid(w)\n\
             assert_close(w.volume(), math.pi * 2.25 * coil.length, rel=0.01)\n\
             f = sweep_along(sketch(coil.start_plane).circle(1.5), coil, frame=sweep_frame.frenet, name=\"f\")\n\
             assert_close(f.volume(), w.volume(), rel=0.005)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let feature = |id: &str| ctx.features.iter().find(|f| f.id.0 == id).unwrap();
        assert_eq!(feature("s").kind, "sweep");
        assert_eq!(feature("s").args["frame"], "rmf");
        assert_eq!(feature("s").args["segments"], "line: translation (exact)");
        assert!(!feature("s").args.contains_key("approximation"));
        assert_eq!(feature("t").args["segments"], "arc: rotation (exact)");
        assert_eq!(feature("coil").kind, "helix");
        assert_eq!(feature("coil").args["hand"], "right");
        assert!(feature("w").args["segments"].starts_with("helix: skinned through"), "{}", feature("w").args["segments"]);
        assert!(feature("w").args["approximation"].contains("target 0.0100 mm"), "{}", feature("w").args["approximation"]);
        let notes = infos(&ctx);
        assert!(
            notes.iter().any(|n| n.contains("segment 0 (helix) is a NURBS skin through") && n.contains("")),
            "{notes:?}"
        );
        assert!(feature("w").inputs.iter().any(|i| i.0 == "coil"), "the sweep depends on its path");
    }

    /// The reference examples run as written: `sweep_along`'s bent tube and
    /// `pattern_along`'s rivets, whose arc is given by a via point typed to
    /// two decimals (0.02° off tangent at both joints).
    #[test]
    fn the_reference_examples_run_verbatim() {
        let ctx = run(
            "route = curve((0, 0, 0)).line((0, 0, 40)).arc((20, 0, 60), via=(5.86, 0, 54.14)).line((60, 0, 60))\n\
             tube = sweep_along(sketch(XY).circle(5).circle(4), route, name=\"tube\")\n\
             assert_solid(tube)\n\
             assert_close(tube.volume(), math.pi * 9 * route.length, rel=0.005)\n\
             rail = curve((0, 0, 0)).line((0, 0, 40)).arc((20, 0, 60), via=(5.86, 0, 54.14)).line((60, 0, 60))\n\
             rivets = pattern_along(cylinder(d=2, height=3).translate((0, 0, -1.5)), rail, spacing=8, name=\"rivets\")\n\
             beads = pattern_along(sphere(2), helix(10, 6, 3), count=24, align=False, name=\"beads\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let feature = |id: &str| ctx.features.iter().find(|f| f.id.0 == id).unwrap();
        assert_eq!(feature("tube").args["segments"], "line: translation (exact); arc: rotation (exact); line: translation (exact)");
    }

    /// Every refusal is a diagnostic with a span on the call, and names
    /// the fix.
    #[test]
    fn sweep_refusals_are_diagnostics_with_spans() {
        let cases: [(&str, &str, u32); 7] = [
            (
                "sk = sketch(XY).circle(1)\nc = curve((0, 0, 0)).line((0, 0, 10)).line((10, 0, 10))\ns = sweep_along(sk, c)\n",
                "corner of 90.00°",
                3,
            ),
            ("sk = sketch(XY).circle(1)\nc = curve((0, 0, 0)).line((0, 0, 10)).line((5, 0, 5)).close()\ns = sweep_along(sk, c)\n", "closed", 3),
            (
                "sk = sketch(XY).circle(1)\nc = curve((0, 0, 0)).bezier((0, 0, 30), controls=[(10, 0, 10), (-10, 0, 20)])\ns = sweep_along(sk, c, frame=sweep_frame.frenet)\n",
                "inflection",
                3,
            ),
            ("sk = sketch(XY).circle(1)\nc = curve((0, 0, 0)).line((0, 0, 10))\ns = sweep_along(sk, c, cap=cap.ruled)\n", "always planar caps", 3),
            ("sk = sketch(XY).circle(1)\np = path((0, 0)).line((0, 10))\ns = sweep_along(sk, p)\n", "path.on(plane)", 3),
            ("sk = sketch(XY).circle(1)\nc = curve((0, 0, 0)).line((0, 0, 10))\ns = sweep_along(sk, c, frame=\"bishop\")\n", "valid: rmf, frenet, fixed", 3),
            ("h = helix(4, 1.25, 3, hand=\"up\")\n", "valid: right, left", 1),
        ];
        for (src, want, line) in cases {
            let ctx = run(src);
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{src}\n{e:?}");
            assert!(e[0].contains(want), "{src}\n{}", e[0]);
            let d = ctx.diagnostics.iter().find(|d| d.message == e[0]).unwrap();
            assert_eq!(d.span.as_ref().map(|s| s.start_line), Some(line), "{src}");
        }
    }
}
