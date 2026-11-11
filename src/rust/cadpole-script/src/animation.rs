//! Animations: `view(az=, el=, fov=)`, `keyframe(t, **params,
//! camera=, ease=)` and `animation(name, keyframes, *, fps=, ease=,
//! loop=)`. The model — the hold rule, the easing, the sampling — is
//! `cadpole_core::animation`'s; this is its Starlark surface
//! (docs/starlark-api.md).
//!
//! A declaration is pure: it builds no body, and an ordinary evaluation
//! records it and renders nothing, as `test()` does. `cadpole animate`
//! evaluates the document once per frame with the interpolated values as
//! overrides, so a keyframe can only name what an override can reach: the
//! document's `param()`s, declared *before* the animation, with values of
//! the declared kind and inside `min`/`max`. Anything else is refused at
//! the `animation(...)` call, with its span.

use allocative::Allocative;
use cadpole_core::ParamValue;
use cadpole_core::animation::{AnimationDecl, CameraKey, Ease, Keyframe};
use cadpole_core::layers::LayerOverride;
use starlark::collections::SmallMap;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::values::float::UnpackFloat;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::none::NoneType;
use starlark::values::{Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, UnpackValue, Value, ValueLike};
use starlark_derive::starlark_value;

use crate::check::opt_num;
use crate::modeltest::param_value_of;
use crate::store::{call_span, store};

// ---------------------------------------------------------------- View

/// What `view(...)` returns: the camera fields a keyframe sets, and the
/// display layers it switches or ghosts.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KView {
    #[allocative(skip)]
    pub key: CameraKey,
    #[allocative(skip)]
    pub layers: Vec<LayerOverride>,
}
starlark_simple_value!(KView);

impl std::fmt::Display for KView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if let Some(az) = self.key.az {
            parts.push(format!("az={}", cadpole_core::context::fmt_num(az)));
        }
        if let Some(el) = self.key.el {
            parts.push(format!("el={}", cadpole_core::context::fmt_num(el)));
        }
        if let Some(fov) = self.key.fov {
            parts.push(format!("fov={}", cadpole_core::context::fmt_num(fov)));
        }
        if let Some(e) = self.key.eye {
            parts.push(format!("eye={}", cadpole_core::context::fmt_vec(e)));
        }
        if let Some(t) = self.key.target {
            parts.push(format!("target={}", cadpole_core::context::fmt_vec(t)));
        }
        if let Some(z) = self.key.zoom {
            parts.push(format!("zoom={}", cadpole_core::context::fmt_num(z)));
        }
        if let Some((x, y)) = self.key.pan {
            parts.push(format!("pan=({}, {})", cadpole_core::context::fmt_num(x), cadpole_core::context::fmt_num(y)));
        }
        if !self.layers.is_empty() {
            let items: Vec<String> = self
                .layers
                .iter()
                .map(|l| {
                    let v = match (l.visible, l.opacity) {
                        (_, Some(o)) => cadpole_core::context::fmt_num(o),
                        (Some(true), None) => "True".to_owned(),
                        _ => "False".to_owned(),
                    };
                    format!("{:?}: {v}", if l.name.is_empty() { "other" } else { l.name.as_str() })
                })
                .collect();
            parts.push(format!("layers={{{}}}", items.join(", ")));
        }
        write!(f, "view({})", parts.join(", "))
    }
}

#[starlark_value(type = "View")]
impl<'v> StarlarkValue<'v> for KView {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        let opt_vec = |v: Option<cadpole_core::Vec3>| match v {
            Some(p) => crate::convert::alloc_vec3(heap, p),
            None => Value::new_none(),
        };
        let v = match attribute {
            "az" => self.key.az,
            "el" => self.key.el,
            "fov" => self.key.fov,
            "zoom" => self.key.zoom,
            "pan" => {
                return Some(match self.key.pan {
                    Some(p) => heap.alloc(p),
                    None => Value::new_none(),
                });
            }
            "eye" => return Some(opt_vec(self.key.eye)),
            "target" => return Some(opt_vec(self.key.target)),
            "layers" => return Some(heap.alloc(self.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>())),
            _ => return None,
        };
        Some(match v {
            Some(n) => heap.alloc(n),
            None => Value::new_none(),
        })
    }
    fn dir_attr(&self) -> Vec<String> {
        ["az", "el", "fov", "eye", "target", "zoom", "pan", "layers"].iter().map(|s| s.to_string()).collect()
    }
}

// ---------------------------------------------------------------- Keyframe

/// What `keyframe(...)` returns, and what `animation(...)` takes a list of.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KKeyframe {
    #[allocative(skip)]
    pub key: Keyframe,
}
starlark_simple_value!(KKeyframe);

impl std::fmt::Display for KKeyframe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "keyframe(t={}", cadpole_core::context::fmt_num(self.key.t))?;
        for (n, v) in &self.key.params {
            write!(f, ", {n}={}", crate::modeltest::value_label(v))?;
        }
        if self.key.camera.is_some() || !self.key.layers.is_empty() {
            write!(f, ", camera={}", KView { key: self.key.camera.unwrap_or_default(), layers: self.key.layers.clone() })?;
        }
        if let Some(e) = self.key.ease {
            write!(f, ", ease={:?}", e.name())?;
        }
        write!(f, ")")
    }
}

#[starlark_value(type = "Keyframe")]
impl<'v> StarlarkValue<'v> for KKeyframe {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "t" => Some(heap.alloc(self.key.t)),
            "params" => Some(heap.alloc(self.key.params.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>())),
            "camera" => Some(match self.key.camera {
                Some(c) => heap.alloc(KView { key: c, layers: self.key.layers.clone() }),
                None if !self.key.layers.is_empty() => heap.alloc(KView { key: CameraKey::default(), layers: self.key.layers.clone() }),
                None => Value::new_none(),
            }),
            "ease" => Some(match self.key.ease {
                Some(e) => heap.alloc(e.name()),
                None => Value::new_none(),
            }),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["t", "params", "camera", "ease"].iter().map(|s| s.to_string()).collect()
    }
}

fn ease_of(v: Option<&str>, what: &str) -> anyhow::Result<Option<Ease>> {
    let Some(s) = v else { return Ok(None) };
    Ease::parse(s).map(Some).ok_or_else(|| {
        anyhow::anyhow!(crate::vocab::not_one_of(
            format!("{what}: unknown ease {s:?}"),
            &Ease::ALL.iter().map(|e| e.name()).collect::<Vec<_>>()
        ))
    })
}

/// `view(layers={...})`: a dict from layer name to `True`/`False` (shown
/// or hidden) or a number in 0..1 (shown, at that opacity).
fn layer_overrides_of<'v>(v: Option<Value<'v>>, heap: Heap<'v>) -> anyhow::Result<Vec<LayerOverride>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.is_none() {
        return Ok(Vec::new());
    }
    let Some(d) = starlark::values::dict::DictRef::from_value(v) else {
        anyhow::bail!("view: layers= takes a dict of layer name to True, False or an opacity, got {}", v.get_type());
    };
    let mut out = Vec::new();
    for (k, val) in d.iter() {
        let name = k
            .unpack_str()
            .ok_or_else(|| anyhow::anyhow!("view: layers= keys are layer names, got {}", k.get_type()))?
            .to_owned();
        if name.is_empty() {
            anyhow::bail!("view: layers= has an empty layer name");
        }
        let mut o = LayerOverride { name, visible: None, opacity: None };
        if let Some(b) = val.unpack_bool() {
            o.visible = Some(b);
        } else if let Ok(Some(n)) = UnpackFloat::unpack_value(val) {
            let n = n.0;
            if !(0.0..=1.0).contains(&n) {
                anyhow::bail!("view: layers[{:?}]: an opacity is between 0 and 1, got {}", o.name, cadpole_core::context::fmt_num(n));
            }
            o.visible = Some(true);
            o.opacity = Some(n);
        } else {
            anyhow::bail!("view: layers[{:?}] must be True, False or an opacity in 0..1, got {}", o.name, val.get_type());
        }
        out.push(o);
    }
    let _ = heap;
    Ok(out)
}

// ---------------------------------------------------------------- globals

#[starlark_module]
pub(crate) fn animation_globals(builder: &mut GlobalsBuilder) {
    /// The camera at a keyframe: `az` (degrees from the front view toward
    /// the right), `el` (degrees above the horizon, -90..90) and `fov`
    /// (vertical field of view in degrees, 1..170). Every field is
    /// optional; a keyframe's `camera=view(az=50)` turns the camera and
    /// leaves its elevation and lens where the previous keyframe put them.
    /// The framing itself — where the camera looks and how far back it
    /// stands — is fitted once, to the whole animation, and only the
    /// direction and the lens move (docs/starlark-api.md).
    /// `eye=(x, y, z)` and `target=(x, y, z)` are a free camera in
    /// model space instead — a neighbour's window looking at the house —
    /// which stands exactly there, easing point to point between keyframes;
    /// once a keyframe places the eye, `az`/`el` no longer apply. The same
    /// value is `render_defaults(camera=view(eye=, target=))`.
    /// `zoom` (0.1..10) and `pan=(x, y)` move the fitted camera without
    /// turning it: `zoom=1.15` stands it 15 % closer than the animation's
    /// widest frame, and `pan=(0.05, 0)` slides it right by a twentieth of
    /// the frame's width, so the subject drifts left. The fit allows for
    /// both, so the subject stays in the picture throughout.
    /// `layers={"envelope": False, "terrain": 0.4}` switches display layers
    /// off (`False`), on (`True`) or ghosts them at an opacity,
    /// from this keyframe on: a visibility switches at the keyframe, an
    /// opacity eases to the next keyframe that names the layer.
    fn view<'v>(
        #[starlark(require = named)] az: Option<UnpackFloat>,
        #[starlark(require = named)] el: Option<UnpackFloat>,
        #[starlark(require = named)] fov: Option<UnpackFloat>,
        #[starlark(require = named)] eye: Option<crate::check::VecArg>,
        #[starlark(require = named)] target: Option<crate::check::VecArg>,
        #[starlark(require = named)] zoom: Option<UnpackFloat>,
        #[starlark(require = named)] pan: Option<UnpackListOrTuple<UnpackFloat>>,
        #[starlark(require = named)] layers: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KView> {
        let pan = match &pan {
            None => None,
            Some(p) if p.items.len() == 2 => {
                Some((crate::check::finite(p.items[0].0, "view: pan.x")?, crate::check::finite(p.items[1].0, "view: pan.y")?))
            }
            Some(p) => anyhow::bail!("view: pan takes (x, y) as fractions of the frame, got {} numbers", p.items.len()),
        };
        let key = CameraKey {
            az: opt_num(&az, "view: az")?,
            el: opt_num(&el, "view: el")?,
            fov: opt_num(&fov, "view: fov")?,
            eye: eye.as_ref().map(|v| crate::check::vec3(v, "view: eye")).transpose()?,
            target: target.as_ref().map(|v| crate::check::vec3(v, "view: target")).transpose()?,
            zoom: opt_num(&zoom, "view: zoom")?,
            pan,
        };
        if let Some(z) = key.zoom {
            let (lo, hi) = cadpole_core::animation::ZOOM_RANGE;
            if !(lo..=hi).contains(&z) {
                anyhow::bail!("view: zoom must be between {lo} and {hi}, got {}", cadpole_core::context::fmt_num(z));
            }
        }
        if let Some((x, y)) = key.pan {
            let m = cadpole_core::animation::PAN_MAX;
            if x.abs() > m || y.abs() > m {
                anyhow::bail!("view: pan must stay within -{m}..{m} of the frame on each axis");
            }
        }
        if key.eye.is_some() && (key.zoom.is_some() || key.pan.is_some()) {
            anyhow::bail!("view: zoom and pan move a fitted camera; with eye= the camera stands where it is put");
        }
        if let (Some(e), Some(t)) = (key.eye, key.target) {
            if (t - e).length() <= 0.0 {
                anyhow::bail!("view: eye and target coincide");
            }
        }
        let layers = layer_overrides_of(layers, eval.heap())?;
        if let Some(el) = key.el {
            if !(-90.0..=90.0).contains(&el) {
                anyhow::bail!("view: el must be between -90 and 90 degrees, got {}", cadpole_core::context::fmt_num(el));
            }
        }
        if let Some(fov) = key.fov {
            if !(1.0..=170.0).contains(&fov) {
                anyhow::bail!("view: fov must be between 1 and 170 degrees, got {}", cadpole_core::context::fmt_num(fov));
            }
        }
        Ok(KView { key, layers })
    }

    /// One instant of an animation: `t` seconds from its start, then the
    /// parameters that take a value there as keywords — `keyframe(2.5,
    /// explode=0, crank_turn=720)` — with values of the kinds a `param`
    /// default takes. A parameter the keyframe does not name holds its
    /// previous value; before its first mention it is the document's own.
    /// `camera=view(...)` moves the camera; `ease=` (`ease.linear`,
    /// `.smooth`, `.ease_in`, `.ease_out`) sets the curve of the segment
    /// that *starts* here, over the animation's own.
    fn keyframe<'v>(
        #[starlark(require = pos)] t: UnpackFloat,
        #[starlark(require = named)] camera: Option<Value<'v>>,
        #[starlark(require = named)] ease: Option<&str>,
        #[starlark(kwargs)] params: SmallMap<String, Value<'v>>,
    ) -> anyhow::Result<KKeyframe> {
        let t = crate::check::num(&t, "keyframe: t")?;
        if t < 0.0 {
            anyhow::bail!("keyframe: t must be a non-negative number of seconds, got {}", cadpole_core::context::fmt_num(t));
        }
        let (camera, layers) = match camera {
            None => (None, Vec::new()),
            Some(v) if v.is_none() => (None, Vec::new()),
            Some(v) => {
                let view = v
                    .downcast_ref::<KView>()
                    .ok_or_else(|| anyhow::anyhow!("keyframe: camera= takes a view(...), got {}", v.get_type()))?;
                // A view that only names layers moves no camera: `camera`
                // stays `None` so the fitted framing holds.
                (if view.key.is_empty() { None } else { Some(view.key) }, view.layers.clone())
            }
        };
        let mut values: Vec<(String, ParamValue)> = Vec::with_capacity(params.len());
        for (name, v) in params.iter() {
            let Some(pv) = param_value_of(*v) else {
                anyhow::bail!(
                    "keyframe: {name}= must be a number, bool, string or (x, y, z), got {}",
                    v.get_type()
                );
            };
            values.push((name.clone(), pv));
        }
        Ok(KKeyframe { key: Keyframe { t, params: values, camera, ease: ease_of(ease, "keyframe")?, layers } })
    }

    /// Declare an animation: a name and a list of `keyframe(...)`s in
    /// increasing time order. Between keyframes every parameter eases from
    /// one value to the next (`ease=`: `ease.smooth`, the default, eases
    /// in and out; `ease.linear` is constant speed; `.ease_in` and
    /// `.ease_out` one side each), a bool or a string switching at the
    /// later keyframe. The
    /// camera is fixed for the whole animation — framed once on everything
    /// the keyframes show — unless a keyframe carries `camera=view(...)`,
    /// which turns it; `fps` is the frame rate `cadpole animate` renders at
    /// unless overridden, and `loop=True` says the animation repeats, so
    /// its last instant is not rendered twice.
    ///
    /// Nothing runs here: the declaration is data, like `test()`'s, and an
    /// ordinary evaluation records it. `cadpole animate main.star --name x`
    /// evaluates the document once per frame with the interpolated values
    /// as parameter overrides and renders each frame, so a keyframe may
    /// only name the document's `param()`s — declared before this call,
    /// with values of the declared kind inside their `min`/`max` — and
    /// anything else is refused here, with the keyframe named.
    fn animation<'v>(
        #[starlark(require = pos)] name: &str,
        #[starlark(require = pos)] keyframes: Value<'v>,
        #[starlark(require = named)] fps: Option<UnpackFloat>,
        #[starlark(require = named)] ease: Option<&str>,
        #[starlark(require = named, default = false)] r#loop: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let span = call_span(eval);
        let st = store(eval)?;
        let Some(list) = <UnpackListOrTuple<Value<'v>>>::unpack_value(keyframes).map_err(|e| e.into_anyhow())? else {
            anyhow::bail!("animation {name:?}: keyframes is a list of keyframe(...), got {}", keyframes.get_type());
        };
        let mut frames = Vec::with_capacity(list.items.len());
        for (i, v) in list.items.iter().enumerate() {
            let Some(k) = v.downcast_ref::<KKeyframe>() else {
                anyhow::bail!("animation {name:?}: keyframes[{i}] is a {}, not a keyframe(...)", v.get_type());
            };
            frames.push(k.key.clone());
        }
        let fps = opt_num(&fps, &format!("animation {name:?}: fps"))?.unwrap_or(30.0);
        let mut decl = AnimationDecl {
            name: name.to_owned(),
            keyframes: frames,
            fps,
            ease: ease_of(ease, &format!("animation {name:?}"))?.unwrap_or(Ease::Smooth),
            looped: r#loop,
            span,
        };
        let mut ctx = st.ctx.borrow_mut();
        if st.in_test.get() {
            anyhow::bail!("animation {name:?}: an animation cannot be declared inside a test");
        }
        if ctx.animations.iter().any(|a| a.name == name) {
            anyhow::bail!("animation {name:?} declared twice");
        }
        decl.validate(&ctx.params).map_err(|e| anyhow::anyhow!(e))?;
        // `"other"` is the implicit layer of untagged bodies unless the
        // script declares a layer of that name -- the CLI's `--layer
        // other=off` rule (cadpole_core::layers::LayerOverride::parse).
        for k in &mut decl.keyframes {
            k.layers = cadpole_core::layers::resolve_implicit(&k.layers, &ctx.layers);
        }
        decl.validate_layers(&ctx.layers).map_err(|e| anyhow::anyhow!(e))?;
        ctx.animations.push(decl);
        Ok(NoneType)
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};
    use cadpole_core::ParamValue;
    use cadpole_core::animation::{CameraKey, Ease};

    const TOY: &str = "lift = param(\"explode\", 0.0, unit=\"mm\", min=0, max=80)\n\
                       turn = param(\"turn\", 0.0, unit=\"deg\")\n\
                       open = param(\"open\", False)\n\
                       b = box(10, 10, 10 + lift / 8, name=\"b\")\n";

    /// `view(layers=)` records display-layer overrides on the
    /// keyframe, `other` is the implicit layer, a view of layers alone
    /// moves no camera, and an undeclared layer or an opacity out of range
    /// is refused at the `animation(...)` call.
    #[test]
    fn a_keyframe_view_names_layers_and_an_undeclared_one_is_refused() {
        let ctx = run(&format!(
            "{TOY}env = layer(\"envelope\")\n\
             animation(\"a\", [\n\
                 keyframe(0, camera=view(layers={{\"envelope\": False, \"other\": 0.4}})),\n\
                 keyframe(1, explode=10, camera=view(az=10, layers={{\"envelope\": True}})),\n\
             ])\n"
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let a = &ctx.animations[0];
        assert!(a.has_layers());
        assert_eq!(a.keyframes[0].camera, None, "a view of layers alone moves no camera");
        let named: Vec<(&str, Option<bool>, Option<f64>)> =
            a.keyframes[0].layers.iter().map(|l| (l.name.as_str(), l.visible, l.opacity)).collect();
        assert_eq!(named, vec![("envelope", Some(false), None), ("", Some(true), Some(0.4))]);
        assert_eq!(a.keyframes[1].camera.map(|c| c.az), Some(Some(10.0)));
        assert_eq!(a.keyframes[1].layers.len(), 1);
        for (src, needle) in [
            ("animation(\"a\", [keyframe(0, camera=view(layers={\"nope\": False}))])", "layer \"nope\" is not declared"),
            ("animation(\"a\", [keyframe(0, camera=view(layers={\"x\": 2}))])", "between 0 and 1"),
            ("animation(\"a\", [keyframe(0, camera=view(layers=[\"x\"]))])", "takes a dict"),
        ] {
            let ctx = run(&format!("{TOY}{src}\n"));
            assert!(errors(&ctx).iter().any(|e| e.contains(needle)), "{src}: {:?}", errors(&ctx));
        }
    }

    /// A declaration is recorded, with its keyframes, easing and camera,
    /// and nothing about the model changes: no feature, no body, no
    /// diagnostic.
    #[test]
    fn a_declaration_is_recorded_and_builds_nothing() {
        let ctx = run(&format!(
            "{TOY}\
             animation(\"assemble\", [\n\
                 keyframe(0, explode=80, camera=view(az=30, el=18, fov=24)),\n\
                 keyframe(2.5, explode=0, ease=ease.linear),\n\
                 keyframe(3, open=True),\n\
                 keyframe(9, turn=720, camera=view(az=50)),\n\
             ], fps=24, ease=\"smooth\", loop=True)\n\
             animation(\"still\", [keyframe(0), keyframe(1, explode=10)])\n"
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.animations.len(), 2);
        let a = &ctx.animations[0];
        assert_eq!(a.name, "assemble");
        assert_eq!((a.fps, a.ease, a.looped), (24.0, Ease::Smooth, true));
        assert_eq!(a.keyframes.len(), 4);
        assert_eq!(a.keyframes[0].t, 0.0);
        assert_eq!(a.keyframes[0].params, vec![("explode".to_owned(), ParamValue::Number(80.0))]);
        assert_eq!(a.keyframes[0].camera, Some(CameraKey { az: Some(30.0), el: Some(18.0), fov: Some(24.0), eye: None, target: None, zoom: None, pan: None }));
        assert_eq!(a.keyframes[1].ease, Some(Ease::Linear));
        assert_eq!(a.keyframes[2].params, vec![("open".to_owned(), ParamValue::Bool(true))]);
        assert_eq!(a.keyframes[3].camera, Some(CameraKey { az: Some(50.0), el: None, fov: None, eye: None, target: None, zoom: None, pan: None }));
        assert_eq!(a.span.as_ref().map(|s| s.start_line), Some(5));
        assert_eq!(a.duration(), 9.0);
        assert_eq!(a.param_names(), ["explode", "open", "turn"]);
        assert!(ctx.features.iter().all(|f| f.kind != "animation"), "a declaration is not a feature");
        assert_eq!(ctx.bodies.len(), 1);
        let b = &ctx.animations[1];
        assert_eq!((b.fps, b.ease, b.looped), (30.0, Ease::Smooth, false));
    }

    /// Every refusal, at the line of the call.
    #[test]
    fn refusals_name_the_keyframe_and_point_at_the_call() {
        let refuse = |tail: &str, needle: &str| {
            let ctx = run(&format!("{TOY}{tail}"));
            let errs = errors(&ctx);
            assert_eq!(errs.len(), 1, "{tail}: {errs:?}");
            assert!(errs[0].contains(needle), "{tail}: {} does not mention {needle:?}", errs[0]);
            let span = ctx.diagnostics.iter().find(|d| d.severity == cadpole_core::context::Severity::Error).unwrap().span.clone();
            // The call that raised is the tail's last line, after TOY's four.
            let line = 4 + tail.lines().count() as u32;
            assert_eq!(span.map(|s| s.start_line), Some(line), "{tail}: the error should point at line {line}");
        };
        refuse("animation(\"a\", [keyframe(0, wall=3)])\n", "\"wall\" is not a param of this document (declared so far: explode, turn, open)");
        refuse("animation(\"a\", [keyframe(1, explode=0), keyframe(1, explode=10)])\n", "keyframe 1 (t=1): keyframes must be in increasing time order");
        refuse("animation(\"a\", [keyframe(0, explode=81)])\n", "explode=81 is above its max 80");
        refuse("animation(\"a\", [keyframe(0), keyframe(2, explode=-5)])\n", "keyframe 1 (t=2): explode=-5 is below its min 0");
        refuse("animation(\"a\", [keyframe(0, explode=True)])\n", "explode is a number but the keyframe gives it a bool");
        refuse("animation(\"a\", [])\n", "at least one keyframe");
        refuse("animation(\"a\", [keyframe(0)], fps=0)\n", "fps must be a positive number");
        refuse("animation(\"a\", [keyframe(0)], ease=\"bouncy\")\n", "unknown ease \"bouncy\"; valid: linear, smooth, ease_in, ease_out");
        refuse("animation(\"a\", [keyframe(0, ease=\"wobble\")])\n", "unknown ease \"wobble\"");
        refuse("animation(\"a\", [keyframe(-1)])\n", "t must be a non-negative number");
        refuse("animation(\"a\", [keyframe(0, camera=view(el=91))])\n", "el must be between -90 and 90");
        refuse("animation(\"a\", [keyframe(0, camera=3)])\n", "camera= takes a view(...)");
        refuse("animation(\"a\", [keyframe(0, camera=view(zoom=0))])\n", "zoom must be between 0.1 and 10, got 0");
        refuse("animation(\"a\", [keyframe(0, camera=view(pan=(0.6, 0)))])\n", "pan must stay within -0.45..0.45");
        refuse("animation(\"a\", [keyframe(0, camera=view(pan=(0.1,)))])\n", "pan takes (x, y)");
        refuse("animation(\"a\", [keyframe(0, camera=view(eye=(1, 2, 3), zoom=1.1))])\n", "with eye= the camera stands where it is put");
        refuse("animation(\"a\", [keyframe(0, explode=[1, 2])])\n", "explode= must be a number, bool, string or (x, y, z)");
        refuse("animation(\"a\", [1])\n", "keyframes[0] is a int, not a keyframe(...)");
        refuse("animation(\"a\", [keyframe(0)])\nanimation(\"a\", [keyframe(0)])\n", "declared twice");
        refuse("animation(\"\", [keyframe(0)])\n", "the name is empty");
    }

    /// A parameter declared after the animation is unknown to it: the rule
    /// is "declare parameters before animations", and the message says so.
    #[test]
    fn a_parameter_declared_later_is_refused_by_name() {
        let ctx = run("animation(\"a\", [keyframe(0, late=1)])\nlate = param(\"late\", 0)\n");
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("\"late\" is not a param of this document (declared so far: none); declare parameters before animations"), "{}", errs[0]);
    }

    /// The `ease` vocabulary is the string, and a keyframe reads back.
    #[test]
    fn the_vocabulary_and_the_values_read_back() {
        let ctx = run(&format!(
            "{TOY}\
             assert_eq(ease.smooth, \"smooth\")\n\
             assert_eq(ease.linear, \"linear\")\n\
             k = keyframe(1.5, explode=20, camera=view(az=10, fov=30), ease=ease.ease_out)\n\
             assert_eq(k.t, 1.5)\n\
             assert_eq(k.params, [\"explode\"])\n\
             assert_eq(k.camera.az, 10)\n\
             assert_eq(k.camera.el, None)\n\
             assert_eq(k.camera.fov, 30)\n\
             assert_eq(k.ease, \"ease_out\")\n\
             assert_eq(str(k), \"keyframe(t=1.5, explode=20, camera=view(az=10, fov=30), ease=\\\"ease_out\\\")\")\n\
             assert_eq(keyframe(0).camera, None)\n\
             v = view(az=40, zoom=1.15, pan=(0.05, -0.1))\n\
             assert_eq((v.zoom, v.pan), (1.15, (0.05, -0.1)))\n\
             assert_eq(str(v), \"view(az=40, zoom=1.15, pan=(0.05, -0.1))\")\n\
             assert_eq((view(az=1).zoom, view(az=1).pan), (None, None))\n\
             animation(\"a\", [keyframe(0), k], ease=ease.ease_in)\n"
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.animations[0].ease, Ease::In);
    }

    /// Inside a test function a declaration is refused, as `test()` is.
    #[test]
    fn a_declaration_inside_a_test_is_refused() {
        let ev = crate::evaluate_full(crate::EvalInput {
            scripts: vec![crate::ScriptFile {
                path: "main.star".into(),
                source: format!("{TOY}def t():\n    animation(\"a\", [keyframe(0)])\ntest(\"t\", t)\n"),
            }],
            tests: crate::TestMode::Unswept { only: None },
            ..Default::default()
        });
        assert!(errors(&ev.ctx).is_empty());
        assert_eq!(ev.outcomes.len(), 1);
        assert!(!ev.outcomes[0].ok);
        assert!(ev.outcomes[0].message.contains("cannot be declared inside a test"), "{}", ev.outcomes[0].message);
        assert!(ev.ctx.animations.is_empty());
    }
}
