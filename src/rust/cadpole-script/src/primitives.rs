//! The Starlark surface of the primitives `mirror`, `scale`,
//! `sphere`, `torus` and `hull` (docs/starlark-api.md). A module of
//! their own so that `globals.rs` stays small; the doc comment on each
//! function is its entry in `cadpole doc` (`apidoc`), so each is written for
//! someone who has never seen Cadpole.

use cadpole_core::geom::{Plane, Vec3};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::float::UnpackFloat;
use starlark::values::tuple::UnpackTuple;
use starlark::values::{UnpackValue, Value};

use crate::check::{VecArg, num, vec3};
use crate::globals::{bodies_value, collect_bodies};
use crate::store::{call_span, store};
use crate::values::{KBody, KPlane};

/// The centre a primitive is placed at: `center=` (the usual word for a
/// point) or `at=` (what `box`, `cylinder` and `cone` call it), never both.
fn centre_of(center: Option<VecArg>, at: Option<VecArg>, what: &str) -> anyhow::Result<Vec3> {
    match (center, at) {
        (Some(_), Some(_)) => anyhow::bail!("{what}: give center= or at=, not both (they mean the same point)"),
        (Some(c), None) => vec3(&c, &format!("{what}: center")),
        (None, Some(a)) => vec3(&a, &format!("{what}: at")),
        (None, None) => Ok(Vec3::ZERO),
    }
}

#[starlark_module]
pub(crate) fn primitive_globals(builder: &mut GlobalsBuilder) {
    /// Mirror `body` across `plane` (default `YZ`, the plane x = 0). The
    /// mirror image is a solid the right way out, and every face keeps the
    /// name it had: `blk/top` is still `blk/top` in the image, so a script
    /// that selected it before the mirror selects it after.
    ///
    /// `keep=True` (the default) keeps the original, and `merge=True` (the
    /// default) fuses the two into one body where they touch or overlap —
    /// the "model half, mirror it" flow. A half that ends on the plane meets
    /// its image face to face, and Cadpole stitches the two along that face
    /// rather than running a boolean, so there is no internal face and the
    /// result stays exact; a half that crosses the plane overlaps its image
    /// and takes the ordinary `union` route. If the body and its image never
    /// touch, the call returns a list of the two bodies and says so. With
    /// `merge=False` it is always that list; with `keep=False` the image
    /// replaces the body, as `translate` moves one.
    ///
    /// Feature: `mirror`, args `plane`, `keep`, `merge`.
    ///
    /// ```python
    /// half = box(20, 10, 6, name="half")             # x from 0 to 20
    /// whole = mirror(half, plane=YZ, name="whole")   # x from -20 to 20, one body
    /// left = mirror(arm, plane=XZ, keep=False)       # the image alone
    /// ```
    fn mirror<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = named)] plane: Option<&KPlane>,
        #[starlark(require = named, default = true)] keep: bool,
        #[starlark(require = named, default = true)] merge: bool,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let plane = plane.map(|p| p.0).unwrap_or(Plane::YZ);
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = ctx.mirror(body.body_id(), &plane, keep, merge, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &ids, "mirror")?;
        bodies_value(heap, &ctx, ids)
    }

    /// Scale `body` by `factor` about `center` (default the origin): one
    /// number for a uniform scale, or `(sx, sy, sz)` for one factor per
    /// axis. Factors must be positive — a negative factor is a mirror, and
    /// `mirror()` is the call for that. Face names are kept.
    ///
    /// A uniform scale keeps every surface what it was: a cylinder's wall is
    /// still `kind.cylinder`, with the scaled radius. A non-uniform scale
    /// squashes a cylinder into an elliptic one, which is no analytic surface
    /// Cadpole names, so such a face reports `kind == "nurbs"` (or `"revolved"`)
    /// afterwards — recognised from the scaled surface, never assumed from
    /// the old one — and `faces(kind="cylinder")` no longer finds it. Planes
    /// stay planes under any scale.
    ///
    /// Feature: `scale`, args `factor` (or `factors`) and `center`.
    ///
    /// ```python
    /// big = scale(part, 2, name="big")
    /// wide = scale(part, (1.5, 1, 1), center=(0, 0, 0), name="wide")
    /// ```
    fn scale<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] factor: Value<'v>,
        #[starlark(require = named)] center: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let factors = if let Some(f) = UnpackFloat::unpack_value(factor).map_err(|e| e.into_anyhow())? {
            let f = num(&f, "scale: factor")?;
            Vec3::new(f, f, f)
        } else if let Some(v) = VecArg::unpack_value(factor).map_err(|e| e.into_anyhow())? {
            if v.items.len() != 3 {
                anyhow::bail!("scale: factor must be one number or three (sx, sy, sz), got {} numbers", v.items.len());
            }
            vec3(&v, "scale: factor")?
        } else {
            anyhow::bail!("scale: factor must be a number or an (sx, sy, sz) tuple, got {}", factor.get_type());
        };
        let center = center.as_ref().map(|c| vec3(c, "scale: center")).transpose()?.unwrap_or(Vec3::ZERO);
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = ctx.scale(body.body_id(), center, factors, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "scale")?;
        KBody::from_ctx(&ctx, id)
    }

    /// An exact sphere of radius `r` centred at `center` (default the
    /// origin; `at=` is accepted too, as for `box` and `cylinder`).
    ///
    /// Two exact spherical caps meeting at the equator — the poles are inside
    /// their faces, where a surface of revolution tessellates cleanly — so the
    /// surface is the group `surface` of two faces, `surface#0` and
    /// `surface#1`, each of which `Face.kind` reports as `sphere`, with the
    /// same centre and radius. Feature: `sphere`, args `r`, `center`.
    ///
    /// ```python
    /// ball = sphere(6, center=(0, 0, 6), name="ball")
    /// assert_eq(ball.faces(kind=kind.sphere).count(), 2)
    /// ```
    fn sphere<'v>(
        #[starlark(require = pos)] r: UnpackFloat,
        #[starlark(require = named)] center: Option<VecArg>,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let r = num(&r, "sphere: r")?;
        let center = centre_of(center, at, "sphere")?;
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = ctx.sphere(center, r, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "sphere")?;
        KBody::from_ctx(&ctx, id)
    }

    /// An exact torus: a tube of radius `minor` around a ring of radius
    /// `major`, centred at `center` (default the origin; `at=` is accepted
    /// too) about `axis` (default +Z). `minor` must be smaller than `major`:
    /// a tube as fat as its ring crosses its own axis and is not a solid.
    ///
    /// Built by revolving the tube's circle, drawn as an outer and an inner
    /// arc split at the top and bottom of the tube, so the faces are the
    /// groups `outer` (the half of the tube facing away from the axis) and
    /// `inner` (the half facing it), three pieces each (`outer#0..2`,
    /// `inner#0..2`), every one of which `Face.kind` reports as `torus`.
    /// Feature: `torus`, args `major`, `minor`, `center`, `axis`.
    ///
    /// ```python
    /// ring = torus(12, 3, center=(0, 0, 3), name="ring")
    /// outside = ring.faces(kind=kind.torus).named("ring/outer")
    /// ```
    fn torus<'v>(
        #[starlark(require = pos)] major: UnpackFloat,
        #[starlark(require = pos)] minor: UnpackFloat,
        #[starlark(require = named)] center: Option<VecArg>,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] axis: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let major = num(&major, "torus: major")?;
        let minor = num(&minor, "torus: minor")?;
        let center = centre_of(center, at, "torus")?;
        let axis = axis.as_ref().map(|a| vec3(a, "torus: axis")).transpose()?.unwrap_or(Vec3::Z);
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = ctx.torus(center, axis, major, minor, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "torus")?;
        KBody::from_ctx(&ctx, id)
    }

    /// The convex hull of one or more bodies (Bodies or lists of Bodies), as
    /// one **mesh** body. Inputs are consumed, as `union`'s are; the result
    /// carries the first body's material.
    ///
    /// The hull is taken over the operands' tessellations at the document
    /// tolerance — quickhull over every triangle vertex — so the hull of a
    /// sphere and a box is right, not just the hull of their corners, and
    /// accurate to the same chord error as every other mesh-path result.
    /// Because it is a mesh body it keeps face names, selection, STL and 3MF
    /// and has no exact surfaces or STEP; the call says so in an info
    /// diagnostic. Coplanar triangles of the hull are one face, so the hull
    /// of boxes has one face per flat side; every face is named `hull`, and
    /// several are `hull#0`, `hull#1`, … under that group alias.
    ///
    /// Feature: `hull`, args `bodies`, `kind=mesh`.
    ///
    /// ```python
    /// pad = hull(sphere(4, center=(0, 0, 0)), sphere(4, center=(30, 0, 0)), name="pad")   # a capsule
    /// foot = hull(box(20, 20, 2), cylinder(3, 15, at=(10, 10, 0)), name="foot")
    /// ```
    fn hull<'v>(
        #[starlark(args)] bodies: UnpackTuple<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let items = collect_bodies(&bodies.items, eval.heap(), "hull")?;
        if items.is_empty() {
            anyhow::bail!("hull needs at least one body");
        }
        let ids: Vec<_> = items.iter().map(|b| b.body_id()).collect();
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = ctx.hull(&ids, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[id], "hull")?;
        KBody::from_ctx(&ctx, id)
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    fn infos(ctx: &cadpole_core::Context) -> Vec<String> {
        ctx.diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Info)
            .map(|d| d.message.clone())
            .collect()
    }

    /// The mirror image is a solid the right way out, of the same volume,
    /// on the other side of the plane, and every face keeps its generator
    /// name — so `blk/right` is now the face at x = -20 pointing -x.
    #[test]
    fn a_mirrored_box_keeps_its_names_volume_and_orientation() {
        let ctx = run(r#"
b = box(20, 10, 6, name="blk")
m = mirror(b, plane=YZ, keep=False, name="m")
assert_eq(type(m), "Body")
assert_eq(m.kind, "brep")
assert_solid(m)
assert_volume(m, 1200)
assert_bbox(m, (-20, 0, 0), (0, 10, 6))
assert_eq(m.faces(kind="plane").count(), 6)
assert_close(m.face("blk/top").normal, (0, 0, 1))
assert_close(m.face("blk/top").centroid, (-10, 5, 6))
assert_close(m.face("blk/right").normal, (-1, 0, 0))
assert_close(m.face("blk/right").centroid, (-20, 5, 3))
assert_close(m.face("blk/front").normal, (0, -1, 0))
# The same selection before and after: the top is still `top`.
assert_eq(b.faces(normal=(0, 0, 1)).names(), ["blk/top"])
assert_eq(m.faces(normal=(0, 0, 1)).names(), ["blk/top"])
# Twice is the identity.
back = mirror(m, plane=YZ, keep=False, name="back")
assert_bbox(back, (0, 0, 0), (20, 10, 6))
assert_close(back.face("blk/right").centroid, (20, 5, 3))
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// A reflection is a similarity: the wall of a mirrored rod is still a
    /// cylinder of the same radius, so `faces(kind="cylinder")` still finds
    /// it, and a tilted mirror plane is handled like an axis plane.
    #[test]
    fn a_mirrored_cylinder_is_still_a_cylinder() {
        let ctx = run(r#"
c = cylinder(5, 12, name="c")
m = mirror(c, plane=XZ.offset(-3), keep=False, name="m")
assert_solid(m)
assert_volume(m, c.volume(), rel=1e-6)
assert_eq(m.faces(kind=kind.cylinder).count(), 2)
assert_eq(m.faces(kind=kind.plane).count(), 2)
assert_eq(sorted(m.faces(kind=kind.cylinder).names()), ["c/side#0", "c/side#1"])
assert_close(m.face("c/top").centroid, (0, 6, 12))
t = mirror(cylinder(5, 12, name="c2"), plane=plane(origin=(3, 1, 2), normal=(1, 1, 0.5)), keep=False, name="t")
assert_solid(t)
assert_eq(t.faces(kind=kind.cylinder).count(), 2)
assert_volume(t, m.volume(), rel=1e-6)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// The "model half, mirror it" flow. A half that ends on the plane
    /// meets its image on that face, back to back, and the two are stitched
    /// there rather than unioned: one exact solid, twice the volume, no face
    /// in the plane, and the names of both halves. The face that lay in the
    /// plane (`half/left`) is gone; every other face is there twice under
    /// its group name, because a stitch merges no faces across the seam.
    #[test]
    fn a_half_touching_the_plane_is_stitched_to_its_image() {
        let ctx = run(r#"
half = box(20, 10, 6, name="half")
whole = mirror(half, plane=YZ, name="whole")
assert_eq(type(whole), "Body")
assert_eq(whole.kind, "brep")
assert_solid(whole)
assert_volume(whole, 2400)
assert_bbox(whole, (-20, 0, 0), (20, 10, 6))
assert_eq(whole.faces(kind="plane").count(), 10)
for f in whole.faces():
    assert_true(f.name.startswith("half/"), msg=f.name)
    assert_true("half/left" not in f.aliases and not f.name.startswith("half/left"), msg=f.name)
    # nothing lies in the mirror plane
    assert_true(abs(f.centroid[0]) > 1e-6 or abs(f.normal[0]) < 0.5, msg=f.name)
assert_eq(whole.faces(normal=(1, 0, 0)).count(), 1)
assert_eq(whole.faces(normal=(-1, 0, 0)).count(), 1)
# the pieces of `half/right` are ordered geometrically, lowest x first
assert_eq(whole.faces(normal=(-1, 0, 0)).names(), ["half/right#0"])
assert_eq(whole.faces(normal=(1, 0, 0)).names(), ["half/right#1"])
assert_eq(whole.faces(name="half/right").count(), 2)
assert_eq(whole.faces(normal=(0, 0, 1)).count(), 2)
assert_eq(sorted(whole.faces(normal=(0, 0, 1)).names()), ["half/top#0", "half/top#1"])
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // The stitch is the exact kernel's, so nothing routed to the mesh kernel.
        assert!(!infos(&ctx).iter().any(|m| m.contains("mesh kernel")), "{:?}", infos(&ctx));
        assert!(ctx.features.iter().any(|f| f.kind == "mirror" && f.args.get("keep").map(String::as_str) == Some("true")));
    }

    /// A half that crosses the plane overlaps its image and takes the
    /// boolean route; a body clear of the plane has nothing to fuse with
    /// and comes back beside its image, with a note saying so; and
    /// `merge=False` is always the pair.
    #[test]
    fn a_crossing_half_unions_and_a_clear_one_stays_a_pair() {
        let ctx = run(r#"
b = box(22, 10, 6, at=(-2, 0, 0), name="b")
w = mirror(b, plane=YZ, name="w")
assert_eq(type(w), "Body")
assert_solid(w)
assert_volume(w, 2400, rel=1e-6)
assert_bbox(w, (-20, 0, 0), (20, 10, 6))

apart = box(10, 10, 10, at=(5, 0, 0), name="apart")
pair = mirror(apart, plane=YZ, name="pair")
assert_eq(type(pair), "list")
assert_eq(len(pair), 2)
assert_eq(pair[0].name, "apart")
assert_eq(pair[1].name, "pair")
assert_bbox(pair[1], (-15, 0, 0), (-5, 10, 10))
assert_volume(pair[1], 1000)

two = mirror(box(10, 10, 10, name="c"), plane=YZ, merge=False, name="two")
assert_eq(len(two), 2)
assert_eq(two[0].name, "c")
assert_bbox(two[1], (-10, 0, 0), (0, 10, 10))
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let notes = infos(&ctx);
        assert_eq!(notes.iter().filter(|m| m.contains("never touch")).count(), 1, "{notes:?}");
    }

    /// A mesh body mirrors too, through the mesh path: same volume, the
    /// right way out, names kept.
    #[test]
    fn a_mesh_body_mirrors_with_its_names() {
        let ctx = run(r#"
h = hull(box(20, 10, 6, name="b"), name="h")
assert_eq(h.kind, "mesh")
m = mirror(h, plane=YZ, keep=False, name="m")
assert_eq(m.kind, "mesh")
assert_solid(m)
assert_volume(m, 1200, rel=1e-6)
assert_bbox(m, (-20, 0, 0), (0, 10, 6))
assert_eq(sorted(m.faces().names()), sorted(h.faces().names()))
assert_true(m.faces().names()[0].startswith("h/hull"))
whole = mirror(hull(box(20, 10, 6, name="b2"), name="h2"), plane=YZ, name="whole")
assert_eq(whole.kind, "mesh")
assert_solid(whole)
assert_volume(whole, 2400, rel=1e-6)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// An exact sphere: two caps recognised as the sphere, named
    /// `surface#0` / `surface#1` under the group `surface`, the textbook
    /// volume to within the chord error, and the size checks.
    #[test]
    fn a_sphere_is_two_recognised_caps_named_surface() {
        let ctx = run(r#"
s = sphere(10, center=(1, 2, 3), name="ball")
assert_eq(s.kind, "brep")
assert_solid(s)
assert_volume(s, 4.0 / 3.0 * math.pi * 1000, rel=0.02)
assert_bbox(s, (-9, -8, -7), (11, 12, 13))
assert_eq(s.faces().count(), 2)
assert_eq(s.faces(kind=kind.sphere).count(), 2)
assert_eq(sorted(s.faces().names()), ["ball/surface#0", "ball/surface#1"])
for f in s.faces():
    assert_eq(f.kind, "sphere")
    assert_true("ball/surface" in f.aliases, msg=str(f.aliases))
assert_eq(s.faces(name="ball/surface").count(), 2)
at = sphere(2, at=(5, 5, 5), name="at")
assert_bbox(at, (3, 3, 3), (7, 7, 7))
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        for (src, want) in [
            ("sphere(0)", "positive"),
            ("sphere(5, center=(0, 0, 0), at=(1, 1, 1))", "not both"),
            ("sphere(float('nan'))", "NaN"),
        ] {
            let ctx = run(src);
            assert!(errors(&ctx).iter().any(|e| e.contains(want)), "{src}: {:?}", errors(&ctx));
        }
    }

    /// An exact torus: six patches recognised as the torus, `outer` and
    /// `inner` in three pieces each, the textbook volume, a tilted axis,
    /// and the spindle refusal.
    #[test]
    fn a_torus_is_six_recognised_patches_named_outer_and_inner() {
        let ctx = run(r#"
t = torus(10, 3, name="ring")
assert_eq(t.kind, "brep")
assert_solid(t)
assert_volume(t, 2 * math.pi * math.pi * 10 * 9, rel=0.03)
assert_bbox(t, (-13, -13, -3), (13, 13, 3))
assert_eq(t.faces().count(), 6)
assert_eq(t.faces(kind=kind.torus).count(), 6)
assert_eq(sorted(t.faces().names()), ["ring/inner#0", "ring/inner#1", "ring/inner#2", "ring/outer#0", "ring/outer#1", "ring/outer#2"])
assert_eq(t.faces(name="ring/outer").count(), 3)
assert_eq(t.faces(name="ring/seg[0.1]").count(), 3)
for f in t.faces(name="ring/outer"):
    assert_true(f.normal[0] * f.centroid[0] + f.normal[1] * f.centroid[1] > 0, msg=f.name)
tilted = torus(8, 2, center=(5, 0, 0), axis=(1, 1, 1), name="tilted")
assert_solid(tilted)
assert_eq(tilted.faces(kind=kind.torus).count(), 6)
assert_volume(tilted, 2 * math.pi * math.pi * 8 * 4, rel=0.03)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let ctx = run("torus(3, 3)");
        assert!(errors(&ctx)[0].contains("spindle"), "{:?}", errors(&ctx));
        let ctx = run("torus(3, 5)");
        assert!(errors(&ctx)[0].contains("smaller"), "{:?}", errors(&ctx));
    }

    /// A uniform scale keeps a cylinder a cylinder; a non-uniform one makes
    /// its wall free-form, which `Face.kind` reports as the truck type again
    /// while the planes stay planes. Names survive both.
    #[test]
    fn a_non_uniform_scale_makes_a_cylinder_wall_freeform_and_a_uniform_one_does_not() {
        let ctx = run(r#"
c = cylinder(5, 12, name="c")
u = scale(c, 2, name="u")
assert_eq(u.kind, "brep")
assert_solid(u)
assert_volume(u, 8 * math.pi * 25 * 12, rel=0.02)
assert_bbox(u, (-10, -10, 0), (10, 10, 24))
assert_eq(u.faces(kind=kind.cylinder).count(), 2)
assert_close(u.face("c/top").centroid, (0, 0, 24))

n = scale(cylinder(5, 12, name="c2"), (2, 1, 1), name="n")
assert_solid(n)
assert_volume(n, 2 * math.pi * 25 * 12, rel=0.02)
assert_bbox(n, (-10, -5, 0), (10, 5, 12))
assert_eq(n.faces(kind=kind.cylinder, allow_empty=True).count(), 0)
assert_eq(n.faces(kind=kind.plane).count(), 2)
assert_eq(n.faces(kind=kind.nurbs).count(), 2)
for f in n.faces(name="c2/side"):
    assert_eq(f.kind, "nurbs")
assert_close(n.face("c2/top").normal, (0, 0, 1))
assert_close(n.face("c2/top").centroid, (0, 0, 12))

about = scale(box(10, 10, 10, name="b"), (1, 1, 3), center=(5, 5, 10), name="about")
assert_bbox(about, (0, 0, -20), (10, 10, 10))
assert_eq(about.faces(kind=kind.plane).count(), 6)
# and the scaled body still names its faces through a boolean
cut = subtract(about, cylinder(2, 40, at=(5, 5, -25), name="drill"), name="cut")
assert_eq(cut.faces(name="b/top").count(), 1)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        for (src, want) in [
            ("scale(box(1, 1, 1), -1)", "mirror"),
            ("scale(box(1, 1, 1), (1, 2))", "three"),
            ("scale(box(1, 1, 1), \"big\")", "number"),
        ] {
            let ctx = run(src);
            assert!(errors(&ctx).iter().any(|e| e.contains(want)), "{src}: {:?}", errors(&ctx));
        }
    }

    /// The hull of two spheres is a capsule, a mesh body that says so; the
    /// hull of a box is the box with six faces; every face is `hull`.
    #[test]
    fn a_hull_is_a_mesh_body_named_hull() {
        let ctx = run(r#"
h = hull(sphere(5, name="a"), sphere(5, center=(20, 0, 0), name="b"), name="h")
assert_eq(h.kind, "mesh")
assert_solid(h)
assert_volume(h, 4.0 / 3.0 * math.pi * 125 + math.pi * 25 * 20, rel=0.03)
assert_bbox(h, (-5, -5, -5), (25, 5, 5))
for f in h.faces():
    assert_true(f.name == "h/hull" or (f.name.startswith("h/hull#") and "h/hull" in f.aliases), msg=f.name)

hb = hull([box(20, 10, 6, name="b1"), box(5, 5, 5, at=(2, 2, 0), name="inside")], name="hb")
assert_eq(hb.faces().count(), 6)
assert_volume(hb, 1200, rel=1e-6)
assert_eq(hb.faces(normal=(0, 0, 1)).count(), 1)
assert_close(hb.faces(normal=(0, 0, 1)).one().area, 200)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let notes = infos(&ctx);
        assert_eq!(notes.iter().filter(|m| m.contains("hull is a mesh body")).count(), 2, "{notes:?}");
        assert!(ctx.features.iter().any(|f| f.kind == "hull" && f.args.get("kind").map(String::as_str) == Some("mesh")));
        let ctx = run("hull()");
        assert!(errors(&ctx)[0].contains("at least one"), "{:?}", errors(&ctx));
    }
}
