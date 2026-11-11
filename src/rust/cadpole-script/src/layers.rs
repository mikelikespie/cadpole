//! Display layers: `layer(name, doc=, visible=, opacity=, color=)`
//! declares one, `layer=` on every body-producing builtin puts a body on
//! one, `paint(body, layer=)` and `Layer.add(*bodies)` tag bodies after the
//! fact. The model — the declarations, the per-body tag and how a picture
//! folds overrides over them — is `cadpole_core::layers`'; this is its
//! Starlark surface (docs/starlark-api.md).
//!
//! A layer is metadata: nothing here builds, moves or consumes a body, and
//! the tag rides along wherever the body's material does. What reads it is
//! the web viewer's Layers panel, `cadpole render --layer`, and a keyframe's
//! `view(layers=)`.

use allocative::Allocative;
use cadpole_core::BodyId;
use cadpole_core::layers::LayerDecl;
use starlark::environment::{GlobalsBuilder, Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::values::float::UnpackFloat;
use starlark::values::tuple::UnpackTuple;
use starlark::values::{Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueLike};
use starlark_derive::starlark_value;

use crate::assembly::KInstance;
use crate::check::opt_num;
use crate::store::{call_span, store};
use crate::values::KBody;

/// What `layer(...)` returns: the declaration, as declared (a layer's
/// settings never change after it is declared, so a copy is exact).
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KLayer {
    pub name: String,
    pub doc: String,
    pub visible: bool,
    pub opacity: f64,
    pub color: Option<[f64; 3]>,
}
starlark_simple_value!(KLayer);

impl std::fmt::Display for KLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "layer({:?})", self.name)
    }
}

#[starlark_value(type = "Layer")]
impl<'v> StarlarkValue<'v> for KLayer {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("layer_methods", layer_methods);
        Some(RES.methods())
    }

    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "name" => Some(heap.alloc(self.name.as_str())),
            "doc" => Some(heap.alloc(self.doc.as_str())),
            "visible" => Some(Value::new_bool(self.visible)),
            "opacity" => Some(heap.alloc(self.opacity)),
            "color" => Some(match self.color {
                Some(c) => heap.alloc((c[0], c[1], c[2])),
                None => Value::new_none(),
            }),
            _ => None,
        }
    }

    fn dir_attr(&self) -> Vec<String> {
        ["name", "doc", "visible", "opacity", "color"].iter().map(|s| s.to_string()).collect()
    }

    fn equals(&self, other: Value<'v>) -> starlark::Result<bool> {
        Ok(other.downcast_ref::<KLayer>().map(|o| o.name == self.name).unwrap_or(false))
    }
}

/// The layer a `layer=` keyword names: a `Layer` value or a declared
/// layer's name. `None` (the keyword left out, or `layer=None`) is no
/// layer, and the body keeps whatever it inherits from its source.
pub(crate) fn layer_name<'v>(v: Value<'v>, what: &str) -> anyhow::Result<Option<String>> {
    if v.is_none() {
        return Ok(None);
    }
    if let Some(l) = v.downcast_ref::<KLayer>() {
        return Ok(Some(l.name.clone()));
    }
    if let Some(s) = v.unpack_str() {
        if s.is_empty() {
            anyhow::bail!("{what}: layer= is an empty string; name a declared layer or leave it out");
        }
        return Ok(Some(s.to_owned()));
    }
    anyhow::bail!("{what}: layer= takes a Layer (from layer(...)) or a layer's name, got {}", v.get_type())
}

/// Put `ids` on the layer `layer=` names, when it names one.
pub(crate) fn tag_ids<'v>(
    ctx: &mut cadpole_core::Context,
    layer: Option<Value<'v>>,
    ids: &[BodyId],
    what: &str,
) -> anyhow::Result<()> {
    let Some(v) = layer else { return Ok(()) };
    let Some(name) = layer_name(v, what)? else { return Ok(()) };
    for id in ids {
        ctx.set_layer(*id, &name).map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
    }
    Ok(())
}

/// [`tag_ids`] for a builtin that already returned its body value.
pub(crate) fn tag_body<'v>(eval: &Evaluator<'v, '_, '_>, layer: Option<Value<'v>>, body: &KBody, what: &str) -> anyhow::Result<()> {
    if layer.is_none() {
        return Ok(());
    }
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    tag_ids(&mut ctx, layer, &[body.body_id()], what)
}

/// The bodies a value stands for: a Body, an Instance (its posed bodies),
/// or a list or tuple of those.
pub(crate) fn body_ids_of<'v>(v: Value<'v>, heap: Heap<'v>, what: &str) -> anyhow::Result<Vec<BodyId>> {
    if let Some(b) = v.downcast_ref::<KBody>() {
        return Ok(vec![b.body_id()]);
    }
    if let Some(i) = v.downcast_ref::<KInstance>() {
        return Ok(i.bodies.iter().map(|b| b.body_id()).collect());
    }
    if v.get_type() == "list" || v.get_type() == "tuple" {
        let mut out = Vec::new();
        for item in v.iterate(heap).map_err(|e| e.into_anyhow())? {
            out.extend(body_ids_of(item, heap, what)?);
        }
        return Ok(out);
    }
    anyhow::bail!("{what}: expected a Body, an Instance or a list of them, got {}", v.get_type())
}

/// [`tag_ids`] for a builtin that returned a body or a list of bodies.
pub(crate) fn tag_value<'v>(eval: &Evaluator<'v, '_, '_>, layer: Option<Value<'v>>, value: Value<'v>, what: &str) -> anyhow::Result<()> {
    if layer.is_none() {
        return Ok(());
    }
    let ids = body_ids_of(value, eval.heap(), what)?;
    let st = store(eval)?;
    let mut ctx = st.ctx.borrow_mut();
    tag_ids(&mut ctx, layer, &ids, what)
}

#[starlark_module]
fn layer_methods(builder: &mut MethodsBuilder) {
    /// Put bodies on this layer and hand them back: `envelope.add(a, b)`
    /// returns `[a, b]`, `envelope.add(a)` returns `a`. Takes Body values,
    /// Instances (every posed body) and lists of them. A body is on one
    /// layer at a time, so this replaces whatever layer the body was on.
    fn add<'v>(
        this: &KLayer,
        #[starlark(args)] bodies: UnpackTuple<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let bodies = bodies.items;
        if bodies.is_empty() {
            anyhow::bail!("Layer.add: give it at least one body");
        }
        let what = format!("layer {:?}.add", this.name);
        let mut ids = Vec::new();
        for v in &bodies {
            ids.extend(body_ids_of(*v, heap, &what)?);
        }
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        for id in &ids {
            ctx.set_layer(*id, &this.name).map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
        }
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(KBody::from_ctx(&ctx, id)?);
        }
        Ok(if out.len() == 1 && bodies.len() == 1 && bodies[0].downcast_ref::<KBody>().is_some() {
            heap.alloc(out.pop().unwrap())
        } else {
            heap.alloc(out)
        })
    }
}

/// The layer names `bodies(layer=)` takes: a `Layer`, a name, or a list or
/// tuple of those, each resolved against the declarations (`other` is the
/// implicit layer) so an unknown name is an error, not an empty list.
fn wanted_layers<'v>(v: Value<'v>, heap: Heap<'v>, declared: &[LayerDecl]) -> anyhow::Result<Vec<String>> {
    let one = |v: Value<'v>| -> anyhow::Result<String> {
        let name = if let Some(l) = v.downcast_ref::<KLayer>() {
            l.name.clone()
        } else if let Some(s) = v.unpack_str() {
            s.to_owned()
        } else {
            anyhow::bail!("bodies: layer= takes a Layer, a layer's name or a list of them, got {}", v.get_type());
        };
        cadpole_core::layers::resolve_layer(&name, declared).map_err(|e| anyhow::anyhow!("bodies: {e}"))
    };
    if v.get_type() == "list" || v.get_type() == "tuple" {
        let mut out = Vec::new();
        for item in v.iterate(heap).map_err(|e| e.into_anyhow())? {
            out.push(one(item)?);
        }
        return Ok(out);
    }
    Ok(vec![one(v)?])
}

#[starlark_module]
pub(crate) fn layer_globals(builder: &mut GlobalsBuilder) {
    /// The document's bodies so far, in the order they were made, as a list
    /// of `Body` values: what a print script that `load()`s a model
    /// filters before it cuts, joins and exports.
    ///
    /// `layer=` keeps the bodies on a layer -- a `Layer`, a declared name,
    /// or a list of them; `"other"` is the bodies on no layer, as `cadpole
    /// render --layer` spells it. An unknown name is an error listing the
    /// declared layers. `visible=True` keeps the bodies a viewer shows and a
    /// whole-document export writes (not consumed by a boolean or a split,
    /// not on a hidden layer), `visible=False` the rest; left out, both.
    /// `name=` keeps the bodies whose name matches a glob (`*`, `?`):
    /// `bodies(name="*_glass*")`. Filters combine: every one given must
    /// hold.
    ///
    /// A snapshot: bodies made after the call are not in the list, so call
    /// it right after `load()` for the loaded model alone (the bodies a
    /// print script derives later carry their source's layer). Nothing is
    /// built or consumed.
    fn bodies<'v>(
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] visible: Option<bool>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let st = store(eval)?;
        let ctx = st.ctx.borrow();
        let wanted = match layer {
            Some(v) if !v.is_none() => Some(wanted_layers(v, heap, &ctx.layers)?),
            _ => None,
        };
        let mut out: Vec<KBody> = Vec::new();
        for b in &ctx.bodies {
            if let Some(w) = &wanted {
                if !w.contains(&b.layer) {
                    continue;
                }
            }
            if let Some(want) = visible {
                let shown = b.visible && cadpole_core::layers::shown(&b.layer, &ctx.layers);
                if shown != want {
                    continue;
                }
            }
            if let Some(pattern) = name {
                if !cadpole_core::select::glob_match(pattern, &b.name) {
                    continue;
                }
            }
            out.push(KBody::from_ctx(&ctx, b.id)?);
        }
        Ok(heap.alloc(out))
    }

    /// Declare a display layer: a named group the web viewer's Layers panel
    /// switches on and off and ghosts without re-evaluating the script, and
    /// `cadpole render --layer name=off|on|0.4` does the same for an image.
    /// `visible` and `opacity` (0..1) are the layer's own defaults; `doc`
    /// is the panel's tooltip; `color` (`"#rrggbb"` or `(r, g, b)`) tints
    /// its swatch. Declaration order is the panel's order. Bodies join a
    /// layer through `layer=` on the builtin that makes them (`box(...,
    /// layer=envelope)`, a `Layer` or its name), through `paint(body,
    /// layer=)`, or through `Layer.add(*bodies)`; a body that names no layer
    /// is on the implicit layer, listed as "Other". A layer follows a body
    /// wherever its material does: through transforms, copies, patterns,
    /// instances, and a boolean's result takes its first operand's.
    /// Declaring a name twice with the same settings is the same layer;
    /// with different settings it is an error. Nothing here builds
    /// anything: a layer is metadata.
    fn layer<'v>(
        #[starlark(require = pos)] name: &str,
        #[starlark(require = named)] doc: Option<&str>,
        #[starlark(require = named, default = true)] visible: bool,
        #[starlark(require = named)] opacity: Option<UnpackFloat>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KLayer> {
        if name.trim().is_empty() {
            anyhow::bail!("layer: the name is empty");
        }
        let opacity = opt_num(&opacity, &format!("layer {name:?}: opacity"))?.unwrap_or(1.0);
        if !(0.0..=1.0).contains(&opacity) {
            anyhow::bail!("layer {name:?}: opacity is between 0 and 1, got {}", cadpole_core::context::fmt_num(opacity));
        }
        let color = match color {
            None => None,
            Some(v) if v.is_none() => None,
            Some(v) => Some(crate::convert::color(v).map_err(|e| anyhow::anyhow!("layer {name:?}: color: {e}"))?),
        };
        let decl = LayerDecl {
            name: name.to_owned(),
            doc: doc.unwrap_or("").to_owned(),
            visible,
            opacity,
            color,
            span: call_span(eval),
        };
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let i = ctx.declare_layer(decl)?;
        let l = &ctx.layers[i];
        Ok(KLayer { name: l.name.clone(), doc: l.doc.clone(), visible: l.visible, opacity: l.opacity, color: l.color })
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    fn layer_of(ctx: &cadpole_core::Context, name: &str) -> String {
        ctx.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body {name}")).layer.clone()
    }

    /// A declaration is recorded in order with its settings, builds
    /// nothing, and the same name declared again with the same settings is
    /// the same layer.
    #[test]
    fn a_layer_is_declared_once_in_order_and_builds_nothing() {
        let mut ctx = run(concat!(
            "env = layer(\"envelope\", doc=\"the zoning envelope\", opacity=0.3, color=\"#ff8800\")\n",
            "site = layer(\"site\", visible=False)\n",
            "again = layer(\"envelope\", doc=\"the zoning envelope\", opacity=0.3, color=\"#ff8800\")\n",
            "assert_eq = env.name == again.name and env.opacity == 0.3 and env.doc == \"the zoning envelope\"\n",
            "if not assert_eq:\n    fail(\"attrs\")\n",
            "if env.color[0] != 1.0 or site.visible:\n    fail(\"color/visible\")\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(ctx.features.is_empty() && ctx.bodies.is_empty());
        let names: Vec<&str> = ctx.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["envelope", "site"]);
        assert_eq!((ctx.layers[0].visible, ctx.layers[0].opacity), (true, 0.3));
        // `#ff8800` decoded to linear, as every colour in Cadpole is.
        let c = ctx.layers[0].color.expect("a color");
        assert!((c[0] - 1.0).abs() < 1e-9 && c[1] > 0.2 && c[1] < 0.3 && c[2] == 0.0, "{c:?}");
        assert_eq!((ctx.layers[1].visible, ctx.layers[1].opacity), (false, 1.0));
        let m = ctx.to_model(true, 0.0);
        assert_eq!(m.layers.len(), 2);
        assert_eq!((m.layers[1].name.as_str(), m.layers[1].order, m.layers[1].visible), ("site", 1, false));
    }

    /// Every way a body joins a layer: the keyword (a Layer or a name),
    /// `paint(layer=)`, `Layer.add` — and a body on no layer reads empty.
    #[test]
    fn bodies_join_a_layer_by_keyword_paint_and_add() {
        let mut ctx = run(concat!(
            "env = layer(\"envelope\")\n",
            "fix = layer(\"fixture\")\n",
            "a = box(10, 10, 10, name=\"a\", layer=env)\n",
            "b = box(10, 10, 10, at=(20, 0, 0), name=\"b\", layer=\"fixture\")\n",
            "c = box(10, 10, 10, at=(40, 0, 0), name=\"c\")\n",
            "d = box(10, 10, 10, at=(60, 0, 0), name=\"d\")\n",
            "paint(d, layer=env)\n",
            "e = cylinder(3, 5, at=(80, 0, 0), name=\"e\")\n",
            "e2 = fix.add(e)\n",
            "if e2.id != e.id:\n    fail(\"add returns the body\")\n",
            "f = sphere(4, at=(100, 0, 0), name=\"f\")\n",
            "g = torus(6, 1, at=(120, 0, 0), name=\"g\")\n",
            "both = env.add(f, g)\n",
            "if len(both) != 2:\n    fail(\"add returns the list\")\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(layer_of(&ctx, "a"), "envelope");
        assert_eq!(layer_of(&ctx, "b"), "fixture");
        assert_eq!(layer_of(&ctx, "c"), "");
        assert_eq!(layer_of(&ctx, "d"), "envelope");
        assert_eq!(layer_of(&ctx, "e"), "fixture");
        assert_eq!(layer_of(&ctx, "f"), "envelope");
        assert_eq!(layer_of(&ctx, "g"), "envelope");
        // `paint(layer=)` alone records no feature: it is a tag, not paint.
        assert!(!ctx.features.iter().any(|f| f.kind == "paint"), "{:?}", ctx.features.iter().map(|f| &f.kind).collect::<Vec<_>>());
        let m = ctx.to_model(true, 0.0);
        let a = m.bodies.iter().find(|b| b.name == "a").unwrap();
        assert_eq!(a.layer, "envelope");
    }

    /// The tag follows the material's rule: a transform, a copy, a pattern
    /// and a mirror carry it; a boolean takes its first operand's; an
    /// instance's posed bodies take the part body's unless `instance(layer=)`
    /// says otherwise; a split's pieces keep it.
    #[test]
    fn a_layer_propagates_as_a_material_does() {
        let ctx = run(concat!(
            "env = layer(\"envelope\")\n",
            "other = layer(\"other_stuff\")\n",
            "a = box(10, 10, 10, name=\"a\", layer=env)\n",
            "moved = a.translate((30, 0, 0), name=\"moved\")\n",
            "dup = copy(moved, offset=(30, 0, 0), name=\"dup\")\n",
            "row = pattern_linear(dup, 2, (0, 30, 0), name=\"row\")\n",
            "m = mirror(row[0], plane=YZ, keep=True, merge=False, name=\"m\")\n",
            "tool = cylinder(2, 20, at=(60, 30, -5), name=\"tool\", layer=other)\n",
            "cut = subtract(row[1], tool, name=\"cut\")\n",
            "halves = split(cut, plane(origin=(0, 0, 5)), name=\"halves\")\n",
            "p = part(box(5, 5, 5, at=(0, 100, 0), name=\"pb\", layer=other), name=\"p\")\n",
            "i1 = instance(p, at=(0, 120, 0), name=\"i1\")\n",
            "i2 = instance(p, at=(0, 140, 0), name=\"i2\", layer=env)\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(layer_of(&ctx, "moved"), "envelope");
        assert_eq!(layer_of(&ctx, "dup"), "envelope");
        for b in ctx.bodies.iter().filter(|b| b.feature.0.starts_with("row")) {
            assert_eq!(b.layer, "envelope", "{}", b.name);
        }
        for b in ctx.bodies.iter().filter(|b| b.feature.0 == "m") {
            assert_eq!(b.layer, "envelope", "{}", b.name);
        }
        assert_eq!(layer_of(&ctx, "cut"), "envelope", "a boolean takes its first operand's layer");
        for b in ctx.bodies.iter().filter(|b| b.feature.0 == "halves") {
            assert_eq!(b.layer, "envelope", "{}", b.name);
        }
        assert_eq!(layer_of(&ctx, "i1/pb"), "other_stuff");
        assert_eq!(layer_of(&ctx, "i2/pb"), "envelope");
    }

    /// A print script that loaded a model lists its bodies by layer,
    /// by visibility and by name, and reads each body's layer, visibility
    /// and colour -- live, so a later `Layer.add` or boolean shows.
    #[test]
    fn bodies_lists_the_document_by_layer_visibility_and_name() {
        let ctx = run(concat!(
            "walls = layer(\"walls\")\n",
            "site = layer(\"site\", visible=False)\n",
            "a = box(10, 10, 10, name=\"wall_a\", layer=walls)\n",
            "b = box(10, 10, 10, at=(20, 0, 0), name=\"wall_b\", layer=\"walls\")\n",
            "c = box(10, 10, 10, at=(40, 0, 0), name=\"tree\", layer=site)\n",
            "d = box(10, 10, 10, at=(60, 0, 0), name=\"loose\")\n",
            "e = box(4, 4, 4, at=(62, 2, 8), name=\"knob\")\n",
            "u = union(d, e, name=\"joined\")\n",
            "def names(bs):\n    return [x.name for x in bs]\n",
            "checks = [\n",
            "  (names(bodies(layer=walls)), [\"wall_a\", \"wall_b\"]),\n",
            "  (names(bodies(layer=\"site\")), [\"tree\"]),\n",
            "  (names(bodies(layer=[\"site\", walls])), [\"wall_a\", \"wall_b\", \"tree\"]),\n",
            "  (names(bodies(layer=\"other\")), [\"loose\", \"knob\", \"joined\"]),\n",
            "  (names(bodies(visible=True)), [\"wall_a\", \"wall_b\", \"joined\"]),\n",
            "  (names(bodies(visible=False)), [\"tree\", \"loose\", \"knob\"]),\n",
            "  (names(bodies(name=\"wall_*\")), [\"wall_a\", \"wall_b\"]),\n",
            "  (names(bodies(layer=\"walls\", name=\"*_b\")), [\"wall_b\"]),\n",
            "  (len(bodies()), 6),\n",
            "  (a.layer(), \"walls\"), (d.layer(), None),\n",
            "  (a.visible(), True), (c.visible(), False), (d.visible(), False), (u.visible(), True),\n",
            "]\n",
            "for got, want in checks:\n    if got != want:\n        fail(\"got \" + str(got) + \", want \" + str(want))\n",
            "site.add(a)\n",
            "if a.layer() != \"site\" or a.visible():\n    fail(\"layer() and visible() read the live state\")\n",
            "a.material(color=\"#ff8800\")\n",
            "if a.color() != \"#FF8800\":\n    fail(a.color())\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // Listing builds and consumes nothing.
        assert_eq!(ctx.bodies.len(), 6);
    }

    /// The refusals, each with the offending name in the message: a body on
    /// a layer nothing declared, a redeclaration with different settings,
    /// an opacity out of range, a `layer=` of the wrong type.
    #[test]
    fn layer_errors_name_the_layer() {
        let cases: [(&str, &str); 8] = [
            ("b = box(1, 1, 1, layer=\"nope\")", "layer \"nope\" is not declared"),
            ("l = layer(\"a\")\nb = box(1, 1, 1, layer=\"nope\")", "the declared layers are a"),
            ("layer(\"a\", opacity=0.5)\nlayer(\"a\", opacity=0.6)", "already declared with different settings"),
            ("layer(\"a\", opacity=1.5)", "opacity is between 0 and 1"),
            ("b = box(1, 1, 1, layer=3)", "layer= takes a Layer"),
            ("l = layer(\"a\")\nl.add(3)", "expected a Body"),
            // A filter that names nothing is an error, not an empty list.
            ("l = layer(\"walls\")\nx = bodies(layer=\"wals\")", "no layer \"wals\"; the declared layers are walls"),
            ("x = bodies(layer=3)", "layer= takes a Layer, a layer's name or a list of them"),
        ];
        for (src, needle) in cases {
            let ctx = run(src);
            let errs = errors(&ctx);
            assert!(errs.iter().any(|e| e.contains(needle)), "{src}: {errs:?}");
        }
    }
}
