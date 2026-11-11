//! The `imprint`, `emboss` and `wrap` globals (docs/starlark-api.md),
//! over `cadpole_core::emboss`: a sketch laid on one face of a body and
//! the face split along its loops — never a boolean — with the pieces
//! raised, cut, painted or scribed.
//!
//! `emboss` and `wrap` are *callable vocabularies*: `emboss.raise`
//! is the string `"raise"` that `emboss(kind=)` takes, and `emboss(...)` is
//! the function, so the namespace is the function and a call reads
//! `emboss(plate, "plate/top", label, 1, kind=emboss.raised)`. The two native
//! functions live in [`NATIVES`], a `Globals` of their own that no script
//! sees; `vocab::all()` fetches them from there by name and the vocabulary's
//! `invoke` forwards the call to them. `Evaluator::eval_function` pushes a
//! frame with no location, which would hide the call site from
//! `call_span`, so the vocabulary hands it down through [`forwarding`].
//! `imprint` is an ordinary global.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::LazyLock;

use allocative::Allocative;
use cadpole_core::context::Span;
use cadpole_core::emboss::{Kind as EmbossKind, Method, Wrapped};
use cadpole_core::ids::{BodyId, FeatureId};
use cadpole_core::naming::Kind;
use starlark::environment::{Globals, GlobalsBuilder, Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::values::float::UnpackFloat;
use starlark::values::{
    FrozenValue, Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueLike,
};
use starlark::{starlark_module, starlark_simple_value};
use starlark_derive::starlark_value;

use crate::check::finite;
use crate::convert::{alloc_str_list, vec3, VecArg};
use crate::globals::sketch_arg;
use crate::selection::KSelection;
use crate::store::{call_span, store};
use crate::values::{pick, KBody, KCurve, KFace};
use crate::vocab::not_one_of;

// ------------------------------------------------------------ the natives

static NATIVES: LazyLock<Globals> =
    LazyLock::new(|| GlobalsBuilder::new().with(natives).with(crate::units::natives).build());

/// The native function behind a callable vocabulary, by name: what
/// `vocab::all()` hangs `emboss(...)`, `wrap(...)` and `units(...)` on.
pub(crate) fn native(name: &str) -> FrozenValue {
    crate::terrain::native(name)
        .or_else(|| NATIVES.iter().find(|(n, _)| *n == name).map(|(_, v)| v))
        .unwrap_or_else(|| panic!("no native function {name} behind the vocabulary"))
}

thread_local! {
    /// The call site of an `emboss(...)` or `wrap(...)` call while its
    /// vocabulary forwards it.
    static FORWARDED: RefCell<Option<Span>> = const { RefCell::new(None) };
}

/// Run `f` with `span` as the call site the natives record on their
/// features and diagnostics.
pub(crate) fn forwarding<R>(span: Option<Span>, f: impl FnOnce() -> R) -> R {
    let previous = FORWARDED.with(|s| s.replace(span));
    let out = f();
    FORWARDED.with(|s| *s.borrow_mut() = previous);
    out
}

/// The span a feature is recorded with: the call site when the evaluator
/// knows it, else the one the vocabulary forwarded.
fn span_of(eval: &Evaluator<'_, '_, '_>) -> Option<Span> {
    call_span(eval).or_else(|| FORWARDED.with(|s| s.borrow().clone()))
}

// ---------------------------------------------------------------- Wrapped

/// A sketch laid on a face: what `wrap(...)` returns and what `imprint` and
/// `emboss` take in place of a sketch.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KWrapped {
    /// The `wrap` feature's id.
    pub feature: String,
    pub body: u32,
    #[allocative(skip)]
    pub wrapped: Wrapped,
}
starlark_simple_value!(KWrapped);

impl fmt::Display for KWrapped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "wrapped({}, {} loops on {} by {})",
            self.feature,
            self.wrapped.sketch.loops.len(),
            self.wrapped.face,
            self.wrapped.method.name()
        )
    }
}

impl KWrapped {
    fn loop_names(&self) -> Vec<String> {
        (0..self.wrapped.sketch.loops.len()).map(|i| self.wrapped.loop_name(i)).collect()
    }
}

#[starlark_value(type = "Wrapped")]
impl<'v> StarlarkValue<'v> for KWrapped {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("wrapped_methods", wrapped_methods);
        Some(RES.methods())
    }

    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "id" => Some(heap.alloc(self.feature.as_str())),
            "face" => Some(heap.alloc(self.wrapped.face.as_str())),
            "method" => Some(heap.alloc(self.wrapped.method.name())),
            "kind" => Some(heap.alloc(self.wrapped.class.kind_name())),
            "deviation" => Some(heap.alloc(self.wrapped.deviation)),
            "loops" => Some(heap.alloc(self.wrapped.sketch.loops.len() as i32)),
            "names" => Some(alloc_str_list(heap, &self.loop_names())),
            _ => None,
        }
    }

    fn has_attr(&self, attribute: &str, _heap: Heap<'v>) -> bool {
        matches!(attribute, "id" | "face" | "method" | "kind" | "deviation" | "loops" | "names")
    }

    fn dir_attr(&self) -> Vec<String> {
        ["id", "face", "method", "kind", "deviation", "loops", "names"].iter().map(|s| s.to_string()).collect()
    }
}

#[starlark_module]
fn wrapped_methods(builder: &mut MethodsBuilder) {
    /// One closed `Curve` per loop, as it lies on the face: lines, arcs
    /// and Béziers where the kernel has an exact edge (a plane; a
    /// generator, a parallel or a plane section of a cylinder, a cone or a
    /// sphere), a helix in closed form for a line developed round a
    /// cylinder, and cubic Béziers fitted within 1e-7 mm for the rest.
    /// Usable as a `loft` section or a `sweep_along` path.
    fn curves<'v>(this: &KWrapped, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        let curves: Vec<KCurve> = this
            .wrapped
            .curves()
            .into_iter()
            .map(|curve| KCurve { feature: this.feature.clone(), curve })
            .collect();
        Ok(heap.alloc(curves))
    }
}

// ------------------------------------------------------------- arguments

fn body_of(id: u32, eval: &Evaluator<'_, '_, '_>) -> anyhow::Result<KBody> {
    let st = store(eval)?;
    let ctx = st.ctx.borrow();
    KBody::from_ctx(&ctx, BodyId(id))
}

/// The face a call names — a `Face`, a `Selection` of one face, or a name
/// looked up on `body` — and the body it is on.
fn face_arg<'v>(
    face: Value<'v>,
    body: Option<&KBody>,
    what: &str,
    eval: &Evaluator<'v, '_, '_>,
) -> anyhow::Result<(KBody, String)> {
    let same_body = |owner: u32, desc: &str| -> anyhow::Result<()> {
        match body {
            Some(b) if b.id != owner => {
                anyhow::bail!("{what}: {desc} is a face of body#{owner}, not of {b}; pass the body the face belongs to")
            }
            _ => Ok(()),
        }
    };
    if let Some(f) = face.downcast_ref::<KFace>() {
        same_body(f.body, &f.name)?;
        return Ok((body_of(f.body, eval)?, f.name.clone()));
    }
    if let Some(sel) = face.downcast_ref::<KSelection>() {
        if sel.what != "face" {
            anyhow::bail!("{what}: {} selects edges; a face is needed", sel.query);
        }
        if sel.faces.len() != 1 {
            let names = sel.names();
            let shown = if names.len() > 6 { format!("{}, ...", names[..6].join(", ")) } else { names.join(", ") };
            anyhow::bail!(
                "{what}: {} holds {} faces ({shown}); an imprint splits one face, so pass one of them (`.one()`, \
                 `.nth(k)` or its name)",
                sel.query,
                sel.faces.len()
            );
        }
        same_body(sel.body, &sel.faces[0].name)?;
        return Ok((body_of(sel.body, eval)?, sel.faces[0].name.clone()));
    }
    if let Some(s) = face.unpack_str() {
        let Some(b) = body else {
            anyhow::bail!(
                "{what}: a face named by a string ({s:?}) needs its body: pass body=, or a Face or a Selection \
                 value instead"
            )
        };
        let faces = b.faces(eval)?;
        let f = pick(&faces, Kind::Face, &b.feature, s, "face").map_err(|e| anyhow::anyhow!("{what}: {b}: {e}"))?;
        return Ok((b.clone(), f.name));
    }
    anyhow::bail!(
        "{what}: face must be a Face, a Selection of one face or a face name, got {}",
        face.get_type()
    )
}

fn method_names() -> Vec<&'static str> {
    Method::ALL.iter().map(|m| m.name()).collect()
}

fn kind_names() -> Vec<&'static str> {
    EmbossKind::ALL.iter().map(|k| k.name()).collect()
}

/// `method=` on `wrap()`, `wrap=` on `imprint()` and `emboss()`: project
/// unless told otherwise.
fn method_arg(s: Option<&str>, what: &str, keyword: &str) -> anyhow::Result<Method> {
    match s {
        None => Ok(Method::Project),
        Some(s) => Method::parse(s).map_err(|_| {
            anyhow::anyhow!(not_one_of(format!("{what}: {keyword}={s:?} is not a wrap method"), &method_names()))
        }),
    }
}

fn point_arg(v: Option<VecArg>, what: &str) -> anyhow::Result<Option<cadpole_core::Vec3>> {
    v.map(|v| vec3(&v, what)).transpose()
}

/// The artwork on the face: a `Wrapped` from `wrap()`, which must sit on
/// this body and face and takes no `wrap=`, `origin=` or `up=` of its own,
/// or a sketch laid on the face here.
#[allow(clippy::too_many_arguments)]
fn wrapped_arg<'v>(
    body: &KBody,
    face: Value<'v>,
    art: Value<'v>,
    wrap: Option<&str>,
    origin: Option<VecArg>,
    up: Option<VecArg>,
    what: &str,
    eval: &mut Evaluator<'v, '_, '_>,
) -> anyhow::Result<Wrapped> {
    let (_, face_name) = face_arg(face, Some(body), what, eval)?;
    if let Some(kw) = art.downcast_ref::<KWrapped>() {
        if kw.body != body.id {
            anyhow::bail!("{what}: {kw} was wrapped on body#{}, not on {body}", kw.body);
        }
        if kw.wrapped.face != face_name {
            anyhow::bail!("{what}: {kw} was wrapped on `{}`, not on `{face_name}`", kw.wrapped.face);
        }
        if wrap.is_some() || origin.is_some() || up.is_some() {
            anyhow::bail!("{what}: wrap=, origin= and up= belong to wrap(); {kw} is already placed");
        }
        return Ok(kw.wrapped.clone());
    }
    let (sketch_fid, sketch) = sketch_arg(art, what, eval)
        .map_err(|e| anyhow::anyhow!("{e} (a Wrapped from wrap() is accepted too)"))?;
    let method = method_arg(wrap, what, "wrap")?;
    let origin = point_arg(origin, &format!("{what}: origin"))?;
    let up = point_arg(up, &format!("{what}: up"))?;
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    let mut w = ctx.wrap_sketch(body.body_id(), &face_name, &sketch, method, origin, up)?;
    w.source = Some(sketch_fid);
    Ok(w)
}

// --------------------------------------------------------------- globals

#[starlark_module]
pub(crate) fn emboss_globals(builder: &mut GlobalsBuilder) {
    /// Split one face of a body along the loops of a sketch: the face keeps
    /// its surface and its name and gets the loops as new inner boundaries,
    /// the pieces inside them become faces of their own on the same surface
    /// (`<name>/imprint[<loop>]`, the group `<name>/imprint`; a loop inside
    /// a hole of the sketch is an island, `<name>/island[<loop>]`), and the
    /// body is rebuilt around them. No boolean runs and nothing is
    /// intersected — the loops *are* the new edges — so the split is exact
    /// wherever the loops lie: on a plane, a cylinder, a cone, a spline face.
    /// The result is a B-rep body; the primitive behind `emboss`.
    ///
    /// `face` is a `Face`, a `Selection` of one face, or a face name on
    /// `body`. `art` is a `Sketch` or `ConstrainedSketch`, laid on the face
    /// by `wrap=` (`wrap.project`, the default, or `wrap.unroll` on a
    /// cylinder or cone), placed at `origin=` with its `+y` along `up=` when
    /// given and otherwise taken where it is in space; or a `Wrapped` from
    /// `wrap()`, already placed.
    ///
    /// Refused by name: a mesh body; loops that leave the face, cross its
    /// boundary or its seam (clip the sketch with `intersect2d`, or move it
    /// with `origin=`); a face that closes through a pole of its surface (a
    /// hemisphere) or lies on a surface of revolution with a curved profile
    /// (a sphere, a torus), which the tessellator cannot triangulate once a
    /// loop is cut into it.
    fn imprint<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] face: Value<'v>,
        #[starlark(require = pos)] art: Value<'v>,
        #[starlark(require = named)] wrap: Option<&str>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] up: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let w = wrapped_arg(body, face, art, wrap, origin, up, "imprint", eval)?;
        let span = span_of(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let out = ctx.imprint(&w, name.map(str::to_owned), span)?;
        crate::layers::tag_ids(&mut ctx, layer, &[out.body], "imprint")?;
        KBody::from_ctx(&ctx, out.body)
    }
}

#[starlark_module]
fn natives(builder: &mut GlobalsBuilder) {
    /// Emboss a sketch on one face of a body: the face is split along the
    /// sketch's loops (`imprint`) and the pieces inside them are raised
    /// (`kind=emboss.raise`, the default) or sunk (`emboss.cut`) by `height`
    /// along the face's outward normal, painted (`emboss.paint`: no
    /// geometry, `color=` required) or scribed (`emboss.scribe`: the
    /// outlines and open paths of the sketch stroked `width=` wide, default
    /// twice the height, and cut `height` deep with round ends).
    ///
    /// A raise and a cut are one construction and never a boolean: the
    /// piece's loop is shared, edge for edge, between the parent face (as a
    /// hole), the walls and the top, so the result is an exact B-rep body
    /// whose faces are `<name>/end[<loop>]` (the tops or floors; the group
    /// `<name>/end`) and `<name>/side[<loop>.<segment>]` (the walls; the
    /// group `<name>/side`), with the parent keeping its name. `<loop>` is
    /// the loop's own name when it has one (`label/glyph[3]/loop[0]` for
    /// text) and its index otherwise. On a plane the walls are the extrude's
    /// planes and cylinders; on a cylinder or a cone the top is the offset
    /// cylinder (`r ± height`) or cone (the same half-angle), and the walls
    /// are ruled between the loop and its offset — exact surfaces. `draft=`
    /// (degrees) narrows the relief away from the face, mitred corner by
    /// corner, for lines and arcs. `color=` paints the artwork faces (the
    /// tops, or the pieces for `paint`).
    ///
    /// `face` is a `Face`, a `Selection` of one face, or a face name on
    /// `body`. `art` is a `Sketch` or `ConstrainedSketch`, laid on the face
    /// by `wrap=` (`wrap.project` along the face's normal, the default;
    /// `wrap.unroll` on the developed surface of a cylinder or a cone, so the
    /// sketch's millimetres are arc lengths, the way a label wraps a can),
    /// at `origin=` with its `+y` along `up=` when given; or a `Wrapped`
    /// from `wrap()`, already placed. Every edge the wrap makes is exact
    /// where the kernel has one and otherwise fitted within 1e-7 mm; the
    /// largest deviation is on the feature.
    ///
    /// Refused by name: a mesh body; a raise or a cut on a sphere, a torus
    /// or a free-form face (no offset surface the exact kernel represents —
    /// `paint` and `imprint` still work there); loops off the face, across
    /// its boundary or its seam; a draft on a spline or one the loop cannot
    /// take; a cut deeper than a cylinder's radius; a sketch wrapping most
    /// of the way round.
    #[allow(clippy::too_many_arguments)]
    fn emboss<'v>(
        #[starlark(require = pos)] body: &KBody,
        #[starlark(require = pos)] face: Value<'v>,
        #[starlark(require = pos)] art: Value<'v>,
        height: Option<UnpackFloat>,
        #[starlark(require = named)] kind: Option<&str>,
        #[starlark(require = named)] draft: Option<UnpackFloat>,
        #[starlark(require = named)] wrap: Option<&str>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] up: Option<VecArg>,
        #[starlark(require = named)] width: Option<UnpackFloat>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KBody> {
        let kind = match kind {
            None => EmbossKind::Raise,
            Some(s) => EmbossKind::parse(s).map_err(|_| {
                anyhow::anyhow!(not_one_of(format!("emboss: kind={s:?} is not a kind of emboss"), &kind_names()))
            })?,
        };
        let rgb = color.map(crate::convert::color).transpose()?;
        let height = match height {
            Some(h) => finite(h.0, "emboss: height")?,
            None => 0.0,
        };
        let draft = match draft {
            Some(d) => finite(d.0, "emboss: draft")?,
            None => 0.0,
        };
        let width = match width {
            Some(w) => Some(finite(w.0, "emboss: width")?),
            None => None,
        };
        if kind == EmbossKind::Paint {
            if rgb.is_none() {
                anyhow::bail!("emboss: kind=paint needs color= (\"#rrggbb\" or (r, g, b)); it colours the imprinted pieces");
            }
            if height != 0.0 || draft != 0.0 {
                anyhow::bail!("emboss: kind=paint changes no geometry, so it takes no height or draft");
            }
        } else if !(height > 0.0) {
            anyhow::bail!(
                "emboss: kind={} needs a positive height in millimetres, got {}",
                kind.name(),
                cadpole_core::context::fmt_num(height)
            );
        }
        if width.is_some() && kind != EmbossKind::Scribe {
            anyhow::bail!("emboss: width= is the stroke of kind=scribe; a {} has no width of its own", kind.name());
        }
        let w = wrapped_arg(body, face, art, wrap, origin, up, "emboss", eval)?;
        let span = span_of(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let out = ctx.emboss(&w, kind, height, draft, width, name.map(str::to_owned), span.clone())?;
        if let Some(rgb) = rgb {
            let id_string = ctx.body(out.body)?.id_string();
            ctx.new_feature(
                cadpole_core::paint::KIND,
                None,
                vec![out.feature.clone()],
                cadpole_core::paint::args(&id_string, &out.faces, rgb, None),
                span,
            )?;
        }
        crate::layers::tag_ids(&mut ctx, layer, &[out.body], "emboss")?;
        KBody::from_ctx(&ctx, out.body)
    }

    /// Lay a sketch on one face of a body and return it as a `Wrapped`,
    /// ready for `imprint` or `emboss` (or its `.curves()` for a `loft` or
    /// a `sweep_along`). `method=wrap.project` (the default) moves every
    /// point along the face's normal at the origin until it meets the face:
    /// exact on a plane and in closed form on a cylinder, a cone or a
    /// sphere, by Newton on a spline face. `method=wrap.unroll` lays the
    /// sketch on the *developed* surface of a cylinder or a cone — an
    /// isometry, so the sketch's millimetres are distances along the
    /// surface, the way a label wraps a can.
    ///
    /// `face` is a `Face` or a `Selection` of one face (which know their
    /// body), or a face name with `body=`. `origin=` is a point on (or
    /// near) the face where the sketch's own origin lands, with the
    /// sketch's `+y` along `up=` projected onto the face; without it,
    /// `project` takes the sketch where it is in space (its plane must then
    /// be parallel to the face at the origin) and `unroll` centres it on the
    /// face with `up` along the axis. The deviation of the fitted edges,
    /// the exact-edge count and any loop that was united are recorded on
    /// the `wrap` feature.
    fn wrap<'v>(
        #[starlark(require = pos)] sketch: Value<'v>,
        #[starlark(require = pos)] face: Value<'v>,
        #[starlark(require = named)] body: Option<&KBody>,
        #[starlark(require = named)] method: Option<&str>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] up: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KWrapped> {
        let (body, face_name) = face_arg(face, body, "wrap", eval)?;
        let (sketch_fid, sketch) = sketch_arg(sketch, "wrap", eval)?;
        let method = method_arg(method, "wrap", "method")?;
        let origin = point_arg(origin, "wrap: origin")?;
        let up = point_arg(up, "wrap: up")?;
        let span = span_of(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut w = ctx.wrap_sketch(body.body_id(), &face_name, &sketch, method, origin, up)?;
        let mut args = BTreeMap::new();
        args.insert("body".to_owned(), body.name.clone());
        args.insert("face".to_owned(), w.face.clone());
        args.insert("method".to_owned(), method.name().to_owned());
        args.insert("loops".to_owned(), w.sketch.loops.len().to_string());
        if w.deviation > 0.0 {
            args.insert("deviation".to_owned(), format!("{:.2e}", w.deviation));
        }
        let fid = ctx.new_feature(
            "wrap",
            name.map(str::to_owned),
            vec![FeatureId(body.feature.clone()), sketch_fid],
            args,
            span.clone(),
        )?;
        for note in w.take_notes() {
            ctx.info(note, &fid, span.clone());
        }
        w.source = Some(fid.clone());
        Ok(KWrapped { feature: fid.0, body: body.id, wrapped: w })
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    fn volume(ctx: &mut cadpole_core::Context, name: &str) -> f64 {
        let id = ctx.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body {name}")).id;
        ctx.body_mesh(id).unwrap().0.volume()
    }

    fn face_names(ctx: &mut cadpole_core::Context, name: &str) -> Vec<String> {
        let id = ctx.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body {name}")).id;
        let mut names: Vec<String> = ctx.body_mesh(id).unwrap().1.faces.iter().map(|f| f.name.clone()).collect();
        names.sort();
        names
    }

    /// A word raised on a plate through the vocabulary form and the string
    /// form: the same exact body, named tops per glyph, no mesh anywhere.
    #[test]
    fn a_word_raised_on_a_plate_is_exact_and_named_per_glyph() {
        let mut ctx = run(
            "plate = box(60, 30, 5, name=\"plate\")\n\
             label = text(\"KERN\", size=8, plane=XY.offset(5), at=(12, 10), name=\"label\")\n\
             out = emboss(plate, \"plate/top\", label, 1, kind=emboss.raised, name=\"raised\")\n\
             assert_eq(out.kind, \"brep\")\n\
             assert_true(out.faces(name=\"raised/end\").count() >= 4)\n\
             assert_true(out.faces(name=\"raised/side\").count() >= 16)\n\
             assert_eq(out.faces(name=\"plate/top\").count(), 1)\n\
             plate2 = box(60, 30, 5, name=\"plate2\")\n\
             label2 = text(\"KERN\", size=8, plane=XY.offset(5), at=(12, 10), name=\"label2\")\n\
             out2 = emboss(plate2, plate2.face(\"plate2/top\"), label2, 1, kind=\"raise\", name=\"raised2\")\n\
             assert_close(out.volume(), out2.volume(), tol=1e-9)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let v = volume(&mut ctx, "raised");
        assert!(v > 9000.0 && v < 9000.0 + 8.0 * 8.0 * 4.0, "{v}");
        let names = face_names(&mut ctx, "raised");
        assert!(names.contains(&"raised/end[label/glyph[0]/loop[0]]".to_owned()), "{names:?}");
        assert!(names.contains(&"raised/island[label/glyph[2]/loop[1]]".to_owned()), "the R's hole is an island: {names:?}");
        assert!(names.iter().any(|n| n.starts_with("raised/side[")), "{names:?}");
        assert!(names.contains(&"plate/top".to_owned()), "{names:?}");
        // The feature tree: one emboss whose inputs are the plate and the label.
        let f = ctx.features.iter().find(|f| f.id.0 == "raised").expect("the emboss feature");
        assert_eq!(f.kind, "emboss");
        assert_eq!(f.inputs.iter().map(|i| i.0.as_str()).collect::<Vec<_>>(), ["plate", "label"]);
        assert_eq!(f.args.get("kind").map(String::as_str), Some("raised"));
    }

    /// A word unrolled round a cylinder and cut in, painted: the wrap is a
    /// value with its curves and its measured deviation, the cut is exact,
    /// and the paint lands on the floors.
    #[test]
    fn a_word_unrolled_and_cut_into_a_cylinder() {
        let mut ctx = run(
            "cyl = cylinder(20, 40, name=\"cyl\")\n\
             word = text(\"MUG\", size=7, name=\"word\")\n\
             w = wrap(word, cyl.face(\"cyl/side[0.0]\"), method=wrap.unroll, name=\"placed\")\n\
             assert_eq(type(w), \"Wrapped\")\n\
             assert_eq(w.method, \"unroll\")\n\
             assert_eq(w.kind, \"cylinder\")\n\
             assert_eq(w.loops, 3)\n\
             assert_true(w.deviation <= 1e-7)\n\
             assert_eq(len(w.curves()), 3)\n\
             assert_true(w.curves()[0].closed)\n\
             out = emboss(cyl, \"cyl/side[0.0]\", w, 1, kind=emboss.cut, color=\"#ff0000\", name=\"cut\")\n\
             assert_eq(out.kind, \"brep\")\n\
             assert_true(out.volume() < cyl.volume())\n\
             assert_true(out.faces(name=\"cut/end\").count() >= 3)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "cut").expect("the emboss feature");
        assert_eq!(f.inputs.iter().map(|i| i.0.as_str()).collect::<Vec<_>>(), ["cyl", "placed"]);
        let wrap = ctx.features.iter().find(|f| f.id.0 == "placed").expect("the wrap feature");
        assert_eq!(wrap.kind, "wrap");
        assert_eq!(wrap.inputs.iter().map(|i| i.0.as_str()).collect::<Vec<_>>(), ["cyl", "word"]);
        let paint = ctx.features.iter().find(|f| f.kind == "paint").expect("the paint feature");
        assert!(paint.args.values().any(|v| v.contains("cut/end[")), "{:?}", paint.args);
        // The tops' floors are cylinders of radius 19.
        let names = face_names(&mut ctx, "cut");
        assert!(names.iter().any(|n| n.starts_with("cut/end[")), "{names:?}");
    }

    /// `imprint` alone, `paint` alone, and `scribe` on a path: the three
    /// kinds that are not a raise, through Starlark.
    #[test]
    fn imprint_paint_and_scribe() {
        let mut ctx = run(
            "plate = box(40, 30, 5, name=\"plate\")\n\
             ring = sketch(XY.offset(5), name=\"ring\").circle(6, center=(20, 15))\n\
             split = imprint(plate, \"plate/top\", ring, name=\"split\")\n\
             assert_eq(split.faces(name=\"split/imprint\").count(), 1)\n\
             assert_close(split.volume(), plate.volume(), tol=1e-6)\n\
             painted = emboss(split, \"plate/top\", sketch(XY.offset(5), name=\"dot\").circle(3, center=(12, 8)), kind=emboss.paint, color=(0, 0, 1), name=\"painted\")\n\
             assert_close(painted.volume(), plate.volume(), tol=1e-6)\n\
             groove = emboss(painted, \"plate/top\", sketch(XY.offset(5), name=\"stroke\").add(path((5, 25)).line((35, 25))), 0.5, kind=emboss.scribe, width=1, name=\"groove\")\n\
             assert_true(groove.volume() < plate.volume() - 15.2)\n\
             assert_true(groove.volume() > plate.volume() - 15.6)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let v = volume(&mut ctx, "groove");
        let groove = 0.5 * (30.0 + std::f64::consts::PI * 0.25);
        assert!((v - (6000.0 - groove)).abs() < 0.01 * groove, "{v}");
    }

    /// Every refusal is a diagnostic on the line of the call, in the
    /// `; valid: ...` shape where a vocabulary is involved.
    #[test]
    fn refusals_are_diagnostics_on_the_call_line() {
        let cases: &[(&str, &str, u32)] = &[
            (
                "plate = box(40, 30, 5, name=\"plate\")\nlabel = text(\"A\", size=8, plane=XY.offset(5))\nemboss(plate, \"plate/top\", label, 1, kind=\"engrave\")\n",
                "emboss: kind=\"engrave\" is not a kind of emboss; valid: raised, cut, paint, scribe",
                3,
            ),
            (
                "plate = box(40, 30, 5, name=\"plate\")\nlabel = text(\"A\", size=8, plane=XY.offset(5))\nemboss(plate, \"plate/top\", label, 1, kind=emboss.engrave)\n",
                "emboss.engrave is not a member of emboss; valid: raised, cut, paint, scribe",
                3,
            ),
            (
                "plate = box(40, 30, 5, name=\"plate\")\nlabel = text(\"A\", size=8, plane=XY.offset(5))\nemboss(plate, \"plate/top\", label, kind=emboss.paint)\n",
                "emboss: kind=paint needs color=",
                3,
            ),
            (
                "plate = box(40, 30, 5, name=\"plate\")\nlabel = text(\"A\", size=8, plane=XY.offset(5))\nemboss(plate, \"plate/top\", label, -1)\n",
                "emboss: kind=raised needs a positive height",
                3,
            ),
            (
                "plate = box(40, 30, 5, name=\"plate\")\nlabel = text(\"A\", size=8, plane=XY.offset(5), at=(30, 0))\n\nout = emboss(plate, \"plate/top\", label, 1)\n",
                "is not inside `plate/top`",
                4,
            ),
            (
                "ball = sphere(10, name=\"ball\")\nsq = sketch(XY).rect(3, 3)\nw = wrap(sq, ball.face(\"ball/surface#0\"), method=wrap.unroll)\n",
                "which does not develop onto a plane",
                3,
            ),
            (
                "cyl = cylinder(20, 40, name=\"cyl\")\nsq = sketch(XY).rect(3, 3)\nw = wrap(sq, \"cyl/side[0.0]\", method=wrap.unroll)\n",
                "needs its body: pass body=",
                3,
            ),
            (
                "cyl = cylinder(20, 40, name=\"cyl\")\nsq = sketch(XY).rect(3, 3)\nw = wrap(sq, cyl.faces(kind=kind.cylinder), method=wrap.unroll)\n",
                "holds 2 faces",
                3,
            ),
            (
                "cyl = cylinder(20, 40, name=\"cyl\")\nsq = sketch(XY).rect(3, 3)\nw = wrap(sq, cyl.face(\"cyl/side[0.0]\"), method=\"conformal\")\n",
                "wrap: method=\"conformal\" is not a wrap method; valid: project, unroll",
                3,
            ),
            (
                "cyl = cylinder(20, 40, name=\"cyl\")\nother = box(10, 10, 10, name=\"other\")\nsq = sketch(XY).rect(3, 3)\nw = wrap(sq, cyl.face(\"cyl/side[0.0]\"), method=wrap.unroll)\nemboss(other, \"other/top\", w, 1)\n",
                "was wrapped on body#",
                5,
            ),
        ];
        for (src, expect, line) in cases {
            let ctx = run(src);
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{src}\n{e:?}");
            assert!(e[0].contains(expect), "{src}\nexpected {expect:?}, got {:?}", e[0]);
            let d = ctx.diagnostics.iter().find(|d| d.message == e[0]).unwrap();
            assert_eq!(d.span.as_ref().map(|s| s.start_line), Some(*line), "{src}\n{:?}", d.span);
        }
    }

    /// `wrap` records a feature with the call's line, even though the call
    /// reaches the native through the vocabulary's `invoke`.
    #[test]
    fn a_forwarded_call_keeps_its_call_site() {
        let ctx = run(
            "cyl = cylinder(20, 40, name=\"cyl\")\n\
             sq = sketch(XY).rect(3, 3)\n\
             w = wrap(sq, cyl.face(\"cyl/side[0.0]\"), method=wrap.unroll, name=\"w\")\n\
             out = emboss(cyl, \"cyl/side[0.0]\", w, 1, kind=emboss.raised, name=\"r\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let w = ctx.features.iter().find(|f| f.id.0 == "w").unwrap();
        assert_eq!(w.span.as_ref().map(|s| s.start_line), Some(3));
        let r = ctx.features.iter().find(|f| f.id.0 == "r").unwrap();
        assert_eq!(r.span.as_ref().map(|s| s.start_line), Some(4));
        // The wrap's note (every edge exact on a cylinder) sits on the wrap feature.
        assert!(
            ctx.diagnostics.iter().any(|d| d.message.contains("every edge exact") && d.feature.as_ref().map(|f| f.0.as_str()) == Some("w")),
            "{:?}",
            ctx.diagnostics.iter().map(|d| (&d.message, &d.feature)).collect::<Vec<_>>()
        );
    }
}
