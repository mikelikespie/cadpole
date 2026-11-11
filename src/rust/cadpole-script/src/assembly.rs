//! Display assemblies: `part`, `instance`, `assembly`, `place`,
//! `pose`, `explode`, `frame`, and the `Part`, `Instance`, `Assembly` and
//! `Frame` values. The model is `cadpole_core::assembly`'s; this module is its
//! Starlark surface (docs/parts.md).
//!
//! `Part`, `Instance` and `Assembly` are handles into the context's tables,
//! the way `ConstrainedSketch` is: placing an instance re-poses the record
//! and its bodies in place, and the value you hold keeps naming it. `Frame`
//! is a plain snapshot.

use std::collections::BTreeMap;
use std::fmt;

use allocative::Allocative;
use cadpole_core::assembly::{self, Along, AssemblyId, Explode, Frame, InstanceId, Member, PartId};
use cadpole_core::context::{fmt_num, fmt_vec};
use cadpole_core::geom::{Transform, Vec3};
use cadpole_core::ids::BodyId;
use cadpole_core::naming::Kind;
use cadpole_core::Context;
use starlark::environment::{GlobalsBuilder, Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::values::float::UnpackFloat;
use starlark::values::none::NoneType;
use starlark::values::{Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, Value, ValueLike};
use starlark_derive::starlark_value;

use crate::check::{VecArg, num, opt_num, vec3};
use crate::convert::alloc_vec3;
use crate::store::{call_span, store};
use crate::values::{KBody, KEdge, KFace, KPlane, MaterialEdit, pick};

// ---------------------------------------------------------------- Frame

/// A right-handed coordinate system: `origin`, `z`, `x` (and `y = z × x`).
/// What `place` maps onto what.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KFrame {
    #[allocative(skip)]
    pub frame: Frame,
}
starlark_simple_value!(KFrame);

impl fmt::Display for KFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame(origin={}, z={}, x={})", fmt_vec(self.frame.origin), fmt_vec(self.frame.z), fmt_vec(self.frame.x))
    }
}

#[starlark_value(type = "Frame")]
impl<'v> StarlarkValue<'v> for KFrame {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("frame_methods", frame_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "origin" => Some(alloc_vec3(heap, self.frame.origin)),
            "x" => Some(alloc_vec3(heap, self.frame.x)),
            "y" => Some(alloc_vec3(heap, self.frame.y())),
            "z" => Some(alloc_vec3(heap, self.frame.z)),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["origin", "x", "y", "z"].iter().map(|s| s.to_string()).collect()
    }
}

#[starlark_module]
fn frame_methods(builder: &mut MethodsBuilder) {
    /// The same frame moved `distance` mm along its own `z`.
    fn offset(this: &KFrame, #[starlark(require = pos)] distance: UnpackFloat) -> anyhow::Result<KFrame> {
        let d = num(&distance, "Frame.offset: distance")?;
        Ok(KFrame { frame: this.frame.offset(d) })
    }

    /// The same frame with `z` reversed: a face's frame turned to look at
    /// the face it will sit on. `place` does this itself unless
    /// `flush=True`.
    fn flip(this: &KFrame) -> anyhow::Result<KFrame> {
        Ok(KFrame { frame: this.frame.flipped() })
    }

    /// The same frame turned `angle` degrees about its own `z`.
    fn turn(this: &KFrame, #[starlark(require = pos)] angle: UnpackFloat) -> anyhow::Result<KFrame> {
        let a = num(&angle, "Frame.turn: angle")?;
        Ok(KFrame { frame: this.frame.turned(a) })
    }
}

// ---------------------------------------------------------------- Part

/// A reusable definition: bodies built once, placed by `instance`.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KPart {
    pub id: u32,
    pub feature: String,
    pub name: String,
    pub bodies: Vec<KBody>,
}
starlark_simple_value!(KPart);

impl fmt::Display for KPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "part({})", self.name)
    }
}

impl KPart {
    fn part_id(&self) -> PartId {
        PartId(self.id)
    }

    pub(crate) fn from_ctx(ctx: &Context, id: PartId) -> anyhow::Result<KPart> {
        let p = ctx.part_record(id)?;
        let bodies = p.bodies.iter().map(|b| KBody::from_ctx(ctx, *b)).collect::<anyhow::Result<Vec<_>>>()?;
        Ok(KPart { id: id.0, feature: p.feature.0.clone(), name: p.name.clone(), bodies })
    }
}

#[starlark_value(type = "Part")]
impl<'v> StarlarkValue<'v> for KPart {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "id" => Some(heap.alloc(self.feature.as_str())),
            "name" => Some(heap.alloc(self.name.as_str())),
            "bodies" => Some(heap.alloc(self.bodies.clone())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["id", "name", "bodies"].iter().map(|s| s.to_string()).collect()
    }
}

// ---------------------------------------------------------------- Instance

/// A placed copy of a part: a pose, and the part's bodies where it puts
/// them.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KInstance {
    pub id: u32,
    pub feature: String,
    pub name: String,
    pub part: String,
    /// The posed bodies, one per part body.
    pub bodies: Vec<KBody>,
    /// For each posed body, the part body's feature: the local prefix a
    /// face name resolves under (`side` inside `rod`), since the faces keep
    /// the part's names.
    pub sources: Vec<String>,
}
starlark_simple_value!(KInstance);

impl fmt::Display for KInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "instance({} of {})", self.name, self.part)
    }
}

impl KInstance {
    fn instance_id(&self) -> InstanceId {
        InstanceId(self.id)
    }

    pub(crate) fn from_ctx(ctx: &Context, id: InstanceId) -> anyhow::Result<KInstance> {
        let r = ctx.instance_record(id)?;
        let part = ctx.part_record(r.part)?;
        let bodies = r.bodies.iter().map(|b| KBody::from_ctx(ctx, *b)).collect::<anyhow::Result<Vec<_>>>()?;
        let sources = part.bodies.iter().map(|b| Ok(ctx.body(*b)?.feature.0.clone())).collect::<anyhow::Result<Vec<_>>>()?;
        Ok(KInstance { id: id.0, feature: r.feature.0.clone(), name: r.name.clone(), part: part.name.clone(), bodies, sources })
    }

    fn pose(&self, ctx: &Context) -> anyhow::Result<Transform> {
        Ok(ctx.instance_record(self.instance_id())?.pose)
    }

    fn owns(&self, body: u32) -> bool {
        self.bodies.iter().any(|b| b.id == body)
    }
}

#[starlark_value(type = "Instance")]
impl<'v> StarlarkValue<'v> for KInstance {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("instance_methods", instance_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "id" => Some(heap.alloc(self.feature.as_str())),
            "name" => Some(heap.alloc(self.name.as_str())),
            "part" => Some(heap.alloc(self.part.as_str())),
            "bodies" => Some(heap.alloc(self.bodies.clone())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["id", "name", "part", "bodies"].iter().map(|s| s.to_string()).collect()
    }
}

#[starlark_module]
fn instance_methods(builder: &mut MethodsBuilder) {
    /// The face of this instance called `name` — the part's name for it
    /// (`rod/side`, or `side` inside the part body's own feature), where
    /// the instance puts it. Several matches is an error naming them.
    fn face<'v>(
        this: &KInstance,
        #[starlark(require = pos)] name: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KFace> {
        let mut found = Vec::new();
        for (body, source) in this.bodies.iter().zip(&this.sources) {
            let faces = body.faces(eval)?;
            if let Ok(f) = pick(&faces, Kind::Face, source, name, "face") {
                found.push(f);
            }
        }
        one_of(found, this, name, "face")
    }

    /// The edge of this instance called `name`, where the instance puts it.
    fn edge<'v>(
        this: &KInstance,
        #[starlark(require = pos)] name: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KEdge> {
        let mut found = Vec::new();
        for (body, source) in this.bodies.iter().zip(&this.sources) {
            let edges = body.edges(eval)?;
            if let Ok(e) = pick(&edges, Kind::Edge, source, name, "edge") {
                found.push(e);
            }
        }
        one_of(found, this, name, "edge")
    }

    /// One posed body of this instance: the only one, or the one whose
    /// part body is called `name`.
    fn body<'v>(this: &KInstance, #[starlark(require = pos)] name: Option<&str>) -> anyhow::Result<KBody> {
        match name {
            None if this.bodies.len() == 1 => Ok(this.bodies[0].clone()),
            None => anyhow::bail!(
                "{this} has {} bodies; name the one you mean: {}",
                this.bodies.len(),
                this.bodies.iter().map(|b| b.name.rsplit('/').next().unwrap_or("").to_owned()).collect::<Vec<_>>().join(", ")
            ),
            Some(n) => this
                .bodies
                .iter()
                .find(|b| b.name.rsplit('/').next() == Some(n) || b.name == n)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("{this} has no body called {n:?}")),
        }
    }

    /// The instance's own frame: where the part's origin and axes sit now.
    fn frame<'v>(this: &KInstance, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<KFrame> {
        let st = store(eval)?;
        let ctx = st.ctx.borrow();
        Ok(KFrame { frame: Frame::from_transform(&this.pose(&ctx)?) })
    }

    /// Give every body of this instance a material, over the part's. The
    /// arguments are `Body.material`'s. Returns the same instance.
    #[allow(clippy::too_many_arguments)]
    fn material<'v>(
        this: &KInstance,
        #[starlark(require = named)] preset: Option<&str>,
        #[starlark(require = named)] color: Option<Value<'v>>,
        #[starlark(require = named)] roughness: Option<UnpackFloat>,
        #[starlark(require = named)] metallic: Option<UnpackFloat>,
        #[starlark(require = named)] texture: Option<&str>,
        #[starlark(require = named)] scale: Option<UnpackFloat>,
        #[starlark(require = named)] opacity: Option<UnpackFloat>,
        #[starlark(require = named)] ior: Option<UnpackFloat>,
        #[starlark(require = named)] emissive: Option<Value<'v>>,
        #[starlark(require = named)] transmission: Option<UnpackFloat>,
        #[starlark(require = named)] attenuation: Option<UnpackFloat>,
        #[starlark(require = named)] scatter: Option<UnpackFloat>,
        #[starlark(require = named)] scatter_g: Option<UnpackFloat>,
        #[starlark(require = named)] anisotropy: Option<UnpackFloat>,
        #[starlark(require = named)] edge_break: Option<UnpackFloat>,
        #[starlark(require = named)] depth: Option<UnpackFloat>,
        #[starlark(require = named)] label: Option<&str>,
        #[starlark(require = named)] shadow: Option<bool>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KInstance> {
        let edit = MaterialEdit::parse(
            preset, color, roughness, metallic, texture, scale, opacity, ior, emissive, transmission, attenuation, scatter,
            scatter_g, anisotropy, edge_break, depth, label, shadow,
        )?;
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        apply_instance_material(&mut ctx, this.instance_id(), &edit)?;
        Ok(this.clone())
    }
}

fn apply_instance_material(ctx: &mut Context, id: InstanceId, edit: &MaterialEdit) -> anyhow::Result<()> {
    let rec = ctx.instance_record(id)?.clone();
    let mut first = None;
    for b in &rec.bodies {
        let m = &mut ctx.body_mut(*b)?.material;
        edit.apply(m);
        if first.is_none() {
            first = Some(m.clone());
        }
    }
    if let Some(m) = first {
        ctx.set_instance_material(id, m)?;
    }
    Ok(())
}

fn one_of<T>(found: Vec<T>, this: &KInstance, name: &str, what: &str) -> anyhow::Result<T> {
    match found.len() {
        1 => Ok(found.into_iter().next().unwrap()),
        0 => anyhow::bail!(
            "{this}: no {what} named {name:?} on any of its {} bod{}",
            this.bodies.len(),
            if this.bodies.len() == 1 { "y" } else { "ies" }
        ),
        n => anyhow::bail!("{this}: {name:?} names a {what} on {n} of its bodies; use inst.body(\"...\").{what}(...)"),
    }
}

// ---------------------------------------------------------------- Assembly

/// A group of instances and assemblies: what `explode` spreads.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KAssembly {
    pub id: u32,
    pub feature: String,
    pub name: String,
    /// Every instance under it, depth first.
    pub instances: Vec<KInstance>,
}
starlark_simple_value!(KAssembly);

impl fmt::Display for KAssembly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "assembly({})", self.name)
    }
}

impl KAssembly {
    fn assembly_id(&self) -> AssemblyId {
        AssemblyId(self.id)
    }

    pub(crate) fn from_ctx(ctx: &Context, id: AssemblyId) -> anyhow::Result<KAssembly> {
        let r = ctx.assembly_record(id)?;
        let instances =
            ctx.assembly_instances(id)?.into_iter().map(|i| KInstance::from_ctx(ctx, i)).collect::<anyhow::Result<Vec<_>>>()?;
        Ok(KAssembly { id: id.0, feature: r.feature.0.clone(), name: r.name.clone(), instances })
    }

    pub(crate) fn bodies(&self) -> Vec<KBody> {
        self.instances.iter().flat_map(|i| i.bodies.iter().cloned()).collect()
    }
}

#[starlark_value(type = "Assembly")]
impl<'v> StarlarkValue<'v> for KAssembly {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        match attribute {
            "id" => Some(heap.alloc(self.feature.as_str())),
            "name" => Some(heap.alloc(self.name.as_str())),
            "instances" => Some(heap.alloc(self.instances.clone())),
            "bodies" => Some(heap.alloc(self.bodies())),
            _ => None,
        }
    }
    fn dir_attr(&self) -> Vec<String> {
        ["id", "name", "instances", "bodies"].iter().map(|s| s.to_string()).collect()
    }
}

// ---------------------------------------------------------------- helpers

/// The body ids a value stands for: a `Body`, or every posed body of an
/// `Instance`, `Assembly` or `Part`. `None` for anything else.
pub(crate) fn bodies_of<'v>(v: Value<'v>) -> Option<Vec<KBody>> {
    if let Some(b) = v.downcast_ref::<KBody>() {
        Some(vec![b.clone()])
    } else if let Some(i) = v.downcast_ref::<KInstance>() {
        Some(i.bodies.clone())
    } else if let Some(a) = v.downcast_ref::<KAssembly>() {
        Some(a.bodies())
    } else if let Some(p) = v.downcast_ref::<KPart>() {
        Some(p.bodies.clone())
    } else {
        None
    }
}

/// A frame from what the script handed over: a `Frame`, a `Face` (its
/// centroid and normal, or its axis for a face of revolution), an `Edge`
/// (a circle's centre and axis, else its midpoint and chord), an
/// `Instance` (its own frame) or a `Plane`.
fn frame_of<'v>(v: Value<'v>, ctx: &mut Context, what: &str) -> anyhow::Result<Frame> {
    if let Some(f) = v.downcast_ref::<KFrame>() {
        return Ok(f.frame);
    }
    if let Some(p) = v.downcast_ref::<KPlane>() {
        return Ok(Frame::new(p.0.origin, p.0.normal, Some(p.0.x_dir))?);
    }
    if let Some(i) = v.downcast_ref::<KInstance>() {
        return Ok(Frame::from_transform(&i.pose(ctx)?));
    }
    if let Some(f) = v.downcast_ref::<KFace>() {
        let (_, topo) = ctx.body_mesh(BodyId(f.body))?;
        let info = topo
            .faces
            .iter()
            .find(|x| x.name == f.name)
            .ok_or_else(|| anyhow::anyhow!("{what}: face {:?} is no longer on its body", f.name))?;
        return Ok(assembly::face_frame(info)?);
    }
    if let Some(e) = v.downcast_ref::<KEdge>() {
        let (_, topo) = ctx.body_mesh(BodyId(e.body))?;
        let info = topo
            .edges
            .iter()
            .find(|x| x.name == e.name)
            .ok_or_else(|| anyhow::anyhow!("{what}: edge {:?} is no longer on its body", e.name))?;
        return Ok(assembly::edge_frame(info)?);
    }
    anyhow::bail!("{what}: expected a Frame, Face, Edge, Instance or Plane, got {}", v.get_type())
}

/// What a frame argument was, for the feature record.
fn describe<'v>(v: Value<'v>) -> String {
    if let Some(f) = v.downcast_ref::<KFace>() {
        format!("face {}", f.name)
    } else if let Some(e) = v.downcast_ref::<KEdge>() {
        format!("edge {}", e.name)
    } else if let Some(i) = v.downcast_ref::<KInstance>() {
        format!("instance {}", i.name)
    } else if let Some(f) = v.downcast_ref::<KFrame>() {
        f.to_string()
    } else {
        v.to_string()
    }
}

/// The body a Face or Edge sits on, so `place` can check its first frame is
/// on the instance being placed.
fn body_of_entity<'v>(v: Value<'v>) -> Option<u32> {
    if let Some(f) = v.downcast_ref::<KFace>() {
        Some(f.body)
    } else {
        v.downcast_ref::<KEdge>().map(|e| e.body)
    }
}

fn along_of<'v>(v: Option<Value<'v>>, what: &str) -> anyhow::Result<Along> {
    let Some(v) = v else { return Ok(Along::Z) };
    if let Some(s) = v.unpack_str() {
        return Along::parse(s).ok_or_else(|| {
            anyhow::anyhow!(crate::vocab::not_one_of(
                format!("{what}: unknown direction {s:?}"),
                &Along::ALL.iter().map(|a| a.name()).collect::<Vec<_>>()
            ))
        });
    }
    let arg = VecArg::unpack_value(v)
        .map_err(|e| anyhow::anyhow!("{what}: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("{what}: along must be \"x\", \"y\", \"z\", \"radial\" or a direction (x, y, z), got {}", v.get_type()))?;
    let d = vec3(&arg, &format!("{what}: along"))?;
    if d.length() == 0.0 {
        anyhow::bail!("{what}: along must be a non-zero direction");
    }
    Ok(Along::Dir(d))
}

use starlark::values::UnpackValue;

// ---------------------------------------------------------------- globals

#[starlark_module]
pub(crate) fn assembly_globals(builder: &mut GlobalsBuilder) {
    /// A reusable part from one body or a list of them: the bodies are
    /// built once and stop rendering on their own; `instance(...)` places
    /// copies of them. Faces and edges keep their names. Records a feature
    /// of kind `part`.
    fn part<'v>(
        #[starlark(require = pos)] bodies: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KPart> {
        let heap = eval.heap();
        let bodies = crate::globals::collect_bodies(&[bodies], heap, "part")?;
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids: Vec<BodyId> = bodies.iter().map(|b| b.body_id()).collect();
        let id = ctx.part(&ids, name.map(str::to_owned), span)?;
        KPart::from_ctx(&ctx, id)
    }

    /// A placed copy of a part: the part's bodies where `at=` (a point) and
    /// `rotate=` (degrees about `axis=`, `(0, 0, 1)` by default, through the
    /// part's origin, applied before the move) put them. The copy shares
    /// the part's geometry and tessellation — ten instances of a wheel
    /// cost one wheel — and its faces keep the part's names, so
    /// `inst.face("hub/bore")` works on every instance. A `Body` here is
    /// wrapped in a part of its own. `material=` is a preset name that
    /// every body of this instance takes over the part's; `Instance.material`
    /// takes the full set of `Body.material`'s arguments. Records a feature
    /// of kind `instance`.
    fn instance<'v>(
        #[starlark(require = pos)] part: Value<'v>,
        #[starlark(require = named)] at: Option<VecArg>,
        #[starlark(require = named)] rotate: Option<UnpackFloat>,
        #[starlark(require = named)] axis: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] material: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KInstance> {
        let at = at.as_ref().map(|a| vec3(a, "instance: at")).transpose()?.unwrap_or(Vec3::ZERO);
        let rotate = opt_num(&rotate, "instance: rotate")?.unwrap_or(0.0);
        let axis = axis.as_ref().map(|a| vec3(a, "instance: axis")).transpose()?.unwrap_or(Vec3::Z);
        if axis.length() == 0.0 {
            anyhow::bail!("instance: axis must be non-zero");
        }
        let edit = material.map(MaterialEdit::preset_only).transpose()?;
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let part_id = if let Some(p) = part.downcast_ref::<KPart>() {
            p.part_id()
        } else if let Some(b) = part.downcast_ref::<KBody>() {
            ctx.part(&[b.body_id()], None, span.clone())?
        } else {
            anyhow::bail!("instance: expected a Part or a Body, got {}", part.get_type());
        };
        let mut pose = Transform::identity();
        if rotate != 0.0 {
            pose = pose.then(Transform::rotation(Vec3::ZERO, axis, rotate.to_radians()));
        }
        pose = pose.then(Transform::translation(at));
        let mut args = BTreeMap::new();
        args.insert("at".into(), fmt_vec(at));
        if rotate != 0.0 {
            args.insert("rotate_deg".into(), fmt_num(rotate));
            args.insert("axis".into(), fmt_vec(axis));
        }
        let material = edit.map(|e| {
            let mut m = cadpole_core::context::Material::default();
            e.apply(&mut m);
            m
        });
        if let Some(m) = &material {
            args.insert("material".into(), m.name.clone());
        }
        let id = ctx.instance(part_id, pose, name.map(str::to_owned), material, args, span)?;
        if layer.is_some() {
            let bodies = ctx.instance_record(id)?.bodies.clone();
            crate::layers::tag_ids(&mut ctx, layer, &bodies, "instance")?;
        }
        KInstance::from_ctx(&ctx, id)
    }

    /// Group instances (and assemblies) so a view can explode them as one.
    /// Nothing moves: an assembly is a record. An instance belongs to one
    /// assembly. Records a feature of kind `assembly`.
    fn assembly<'v>(
        #[starlark(require = pos)] members: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KAssembly> {
        let heap = eval.heap();
        let mut list = Vec::new();
        let items: Vec<Value<'v>> = if members.downcast_ref::<KInstance>().is_some() || members.downcast_ref::<KAssembly>().is_some() {
            vec![members]
        } else {
            members.iterate(heap).map_err(|e| e.into_anyhow())?.collect()
        };
        for v in items {
            if let Some(i) = v.downcast_ref::<KInstance>() {
                list.push(Member::Instance(i.instance_id()));
            } else if let Some(a) = v.downcast_ref::<KAssembly>() {
                list.push(Member::Assembly(a.assembly_id()));
            } else {
                anyhow::bail!("assembly: expected Instance or Assembly values, got {}", v.get_type());
            }
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let id = ctx.assembly(&list, name.map(str::to_owned), span)?;
        KAssembly::from_ctx(&ctx, id)
    }

    /// Move `instance` so that a frame on it lands on a frame on something
    /// else: `place(nut, nut.face("seat"), wheel.face("nose"))` seats the
    /// nut on the nose. Either frame may be a `Face` (its centroid and
    /// normal; the axis, for a cylinder or cone), an `Edge` (a circle's
    /// centre and axis), an `Instance`, a `Plane` or a `Frame`. The first
    /// must be on `instance`, read where the instance is now.
    ///
    /// The frames' origins coincide, their `x` axes align, and their `z`
    /// axes *oppose* — two faces brought together look at each other —
    /// unless `flush=True`, which aligns them. `offset=` then moves the
    /// instance along the target's `z`: `offset=2` leaves a 2 mm gap. A
    /// second `place` moves the instance again from where the first left
    /// it. Records a feature of kind `place` and returns the instance.
    fn place<'v>(
        #[starlark(require = pos)] instance: &KInstance,
        #[starlark(require = pos)] on: Value<'v>,
        #[starlark(require = pos)] to: Value<'v>,
        #[starlark(require = named, default = false)] flush: bool,
        #[starlark(require = named)] offset: Option<UnpackFloat>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KInstance> {
        let offset = opt_num(&offset, "place: offset")?.unwrap_or(0.0);
        if let Some(b) = body_of_entity(on) {
            if !instance.owns(b) {
                anyhow::bail!(
                    "place: the first frame ({}) is not on {instance}; give a face or edge of the instance being placed",
                    describe(on)
                );
            }
        }
        if let Some(b) = body_of_entity(to) {
            if instance.owns(b) {
                anyhow::bail!(
                    "place: the target frame ({}) is on {instance} itself; give a face or edge of another instance or body",
                    describe(to)
                );
            }
        }
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let from = frame_of(on, &mut ctx, "place")?;
        let target = frame_of(to, &mut ctx, "place")?;
        let t = Frame::placing(&from, &target, flush, offset);
        let pose = instance.pose(&ctx)?.then(t);
        let mut args = BTreeMap::new();
        args.insert("on".into(), describe(on));
        args.insert("to".into(), describe(to));
        if flush {
            args.insert("flush".into(), "true".into());
        }
        if offset != 0.0 {
            args.insert("offset".into(), fmt_num(offset));
        }
        ctx.set_pose(instance.instance_id(), pose, "place", args, name.map(str::to_owned), span)?;
        KInstance::from_ctx(&ctx, instance.instance_id())
    }

    /// Displace `instance` from where it is: `rotate=` degrees about
    /// `axis=` (`(0, 0, 1)` by default) through `origin=` (the instance's
    /// own origin by default, so `pose(sun, rotate=15)` spins a gear about
    /// its axis), then `translate=`. Both may be parameters, which is
    /// kinematics-lite: `cadpole sweep --param angle=0,15,30` and
    /// `test(..., sweep={"angle": [...]})` pose the mechanism per point.
    /// Records a feature of kind `pose` and returns the instance.
    fn pose<'v>(
        #[starlark(require = pos)] instance: &KInstance,
        #[starlark(require = named)] translate: Option<VecArg>,
        #[starlark(require = named)] rotate: Option<UnpackFloat>,
        #[starlark(require = named)] axis: Option<VecArg>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] name: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KInstance> {
        let translate = translate.as_ref().map(|a| vec3(a, "pose: translate")).transpose()?.unwrap_or(Vec3::ZERO);
        let rotate = opt_num(&rotate, "pose: rotate")?.unwrap_or(0.0);
        let axis = axis.as_ref().map(|a| vec3(a, "pose: axis")).transpose()?.unwrap_or(Vec3::Z);
        if axis.length() == 0.0 {
            anyhow::bail!("pose: axis must be non-zero");
        }
        let origin = origin.as_ref().map(|a| vec3(a, "pose: origin")).transpose()?;
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let current = instance.pose(&ctx)?;
        let origin = origin.unwrap_or_else(|| current.translation_part());
        let mut pose = current;
        if rotate != 0.0 {
            pose = pose.then(Transform::rotation(origin, axis, rotate.to_radians()));
        }
        if translate.length() != 0.0 {
            pose = pose.then(Transform::translation(translate));
        }
        let mut args = BTreeMap::new();
        if rotate != 0.0 {
            args.insert("rotate_deg".into(), fmt_num(rotate));
            args.insert("axis".into(), fmt_vec(axis));
            args.insert("origin".into(), fmt_vec(origin));
        }
        if translate.length() != 0.0 {
            args.insert("translate".into(), fmt_vec(translate));
        }
        ctx.set_pose(instance.instance_id(), pose, "pose", args, name.map(str::to_owned), span)?;
        KInstance::from_ctx(&ctx, instance.instance_id())
    }

    /// An exploded view: a picture, never geometry. `distance()`,
    /// `print_check` and every export see the assembled position.
    ///
    /// `explode(assembly, distance, along="z")` spreads the assembly's
    /// members along an axis (`along.x`, `.y`, `.z`, or a direction
    /// `(x, y, z)`): members are ranked by their centre along it, lowest
    /// first — a tie goes to the larger part, which stays, so the part inside
    /// it comes out — and rank *k* moves *k × distance*. Members with the
    /// same centre and size share a rank and move together. `along.radial`
    /// moves every member `distance` away from the assembly's centre
    /// instead. A nested assembly moves as one member of its parent and
    /// then spreads its own.
    ///
    /// `explode(instance, (dx, dy, dz))` gives one instance an offset of
    /// its own, added to whatever its assemblies decide;
    /// `explode(instance, distance, along="z")` is the same offset written
    /// as a distance along an axis or direction (not `along.radial`: an
    /// instance has no centre to be radial from). `along=` with an offset
    /// is an error. `distance` may be a
    /// parameter, so `cadpole render --param explode=24` draws the exploded
    /// picture from the same script. Records a feature of kind `explode`.
    fn explode<'v>(
        #[starlark(require = pos)] target: Value<'v>,
        #[starlark(require = pos)] distance: Value<'v>,
        #[starlark(require = named)] along: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        if let Some(a) = target.downcast_ref::<KAssembly>() {
            let d = UnpackFloat::unpack_value(distance)
                .map_err(|e| anyhow::anyhow!("explode: {e}"))?
                .ok_or_else(|| anyhow::anyhow!("explode: distance must be a number in mm, got {}", distance.get_type()))?;
            let d = num(&d, "explode: distance")?;
            if d < 0.0 {
                anyhow::bail!("explode: distance must not be negative, got {d}");
            }
            let along = along_of(along, "explode")?;
            ctx.explode_assembly(a.assembly_id(), Explode { distance: d, along }, span)?;
            return Ok(NoneType);
        }
        if let Some(i) = target.downcast_ref::<KInstance>() {
            let offset = if let Some(d) = UnpackFloat::unpack_value(distance).map_err(|e| anyhow::anyhow!("explode: {e}"))? {
                let d = num(&d, "explode: distance")?;
                let along = along_of(along, "explode")?;
                match along.direction() {
                    Some(dir) => dir * d,
                    None => anyhow::bail!("explode: an instance has no centre to be radial from; give along= an axis or pass an offset (dx, dy, dz)"),
                }
            } else {
                // `along=` steers a distance; an offset is already a
                // direction and a length, so a second direction has nothing
                // to mean. Dropping it silently let `along="bogus"` through.
                if along.is_some() {
                    anyhow::bail!(
                        "explode: along= only applies to a distance; an offset (dx, dy, dz) is already the whole \
                         move, so give one or the other"
                    );
                }
                let arg = VecArg::unpack_value(distance)
                    .map_err(|e| anyhow::anyhow!("explode: {e}"))?
                    .ok_or_else(|| anyhow::anyhow!("explode: expected a distance in mm or an offset (dx, dy, dz), got {}", distance.get_type()))?;
                vec3(&arg, "explode: offset")?
            };
            ctx.explode_instance(i.instance_id(), offset, span)?;
            return Ok(NoneType);
        }
        anyhow::bail!("explode: expected an Assembly or an Instance, got {}", target.get_type())
    }

    /// A frame: `frame(face)` reads one off a `Face` (its centroid and
    /// normal; the axis for a cylinder, cone or torus), an `Edge` (a
    /// circle's centre and axis, else the midpoint and chord), an
    /// `Instance` (its own frame) or a `Plane`; `frame(origin=, z=, x=)`
    /// gives one explicitly, `x` optional. A frame's `x` is the world axis
    /// least aligned with its `z`, projected into its plane, unless given —
    /// a rule with no memory, so the same face gives the same frame after
    /// an edit that keeps its normal. Attributes `origin`, `x`, `y`, `z`;
    /// methods `offset(d)`, `flip()`, `turn(degrees)`.
    fn frame<'v>(
        #[starlark(require = pos)] of: Option<Value<'v>>,
        #[starlark(require = named)] origin: Option<VecArg>,
        #[starlark(require = named)] z: Option<VecArg>,
        #[starlark(require = named)] x: Option<VecArg>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KFrame> {
        if let Some(v) = of {
            if origin.is_some() || z.is_some() || x.is_some() {
                anyhow::bail!("frame: give either a face/edge/instance or origin=/z=/x=, not both");
            }
            let st = store(eval)?;
            let mut ctx = st.ctx.borrow_mut();
            return Ok(KFrame { frame: frame_of(v, &mut ctx, "frame")? });
        }
        let origin = origin.as_ref().map(|a| vec3(a, "frame: origin")).transpose()?.unwrap_or(Vec3::ZERO);
        let z = z.as_ref().map(|a| vec3(a, "frame: z")).transpose()?.unwrap_or(Vec3::Z);
        let x = x.as_ref().map(|a| vec3(a, "frame: x")).transpose()?;
        Ok(KFrame { frame: Frame::new(origin, z, x)? })
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};
    use cadpole_core::Context;
    use cadpole_core::ids::BodyId;

    fn body_named(ctx: &Context, name: &str) -> BodyId {
        ctx.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body {name:?}")).id
    }

    /// Ten instances of one part are one evaluation of the part and one
    /// tessellation of it:
    /// the kernel triangulates the wheel once, every instance is a rigid
    /// bake of the solid plus a vertex pass over that one mesh, and the
    /// face names are the part's on every copy.
    #[test]
    fn ten_instances_of_a_wheel_are_one_tessellation() {
        let mut ctx = run(concat!(
            "wheel = cylinder(20, 8, name=\"wheel\")\n",
            "hub = part(wheel, name=\"hub\")\n",
            "wheels = [instance(hub, at=(i * 50, 0, 0), name=\"w%d\" % i) for i in range(10)]\n",
            "assert_eq(len(wheels), 10)\n",
            "assert_eq(wheels[3].name, \"w3\")\n",
            "assert_eq(wheels[3].part, \"hub\")\n",
            "assert_eq(wheels[3].bodies[0].name, \"w3/wheel\")\n",
            "assert_eq(wheels[3].face(\"wheel/side#0\").name, \"wheel/side#0\")\n",
            "assert_eq(wheels[3].face(\"top\").name, \"wheel/top\")\n",
            "assert_close(wheels[3].face(\"wheel/top\").centroid[0], 150.0, tol=1e-6)\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let visible: Vec<BodyId> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
        assert_eq!(visible.len(), 10, "ten instance bodies are visible");
        assert!(!ctx.body(body_named(&ctx, "wheel")).unwrap().visible, "the part's body is consumed");
        // The script already meshed one instance for its face lookups;
        // meshing every visible body now still triangulates the wheel once.
        for id in &visible {
            ctx.body_mesh(*id).unwrap();
        }
        assert_eq!(ctx.stats.tessellations, 1, "one tessellation for ten instances");
        assert_eq!(ctx.stats.bakes, 10, "one rigid bake per instance");
        // Every copy has the part's face names, moved.
        let names = |ctx: &mut Context, id: BodyId| {
            ctx.body_mesh(id).unwrap().1.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>()
        };
        let first = names(&mut ctx, visible[0]);
        for id in &visible[1..] {
            assert_eq!(names(&mut ctx, *id), first);
        }
        assert_eq!(ctx.parts.len(), 1);
        assert_eq!(ctx.instances.len(), 10);
        let kinds: Vec<&str> = ctx.features.iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == "instance").count(), 10);
        assert_eq!(kinds.iter().filter(|k| **k == "part").count(), 1);
    }

    /// The scene sends the part's mesh once and every instance as a pointer
    /// at it with a transform: `Model.bodies` holds the part body
    /// invisible with its mesh, and each instance body with no mesh, its
    /// `part_body`, its `transform`, and a bbox where the transform puts
    /// it. A document with no assemblies has none of the new fields.
    #[test]
    fn the_model_carries_instances_as_part_id_plus_transform() {
        let mut ctx = run(concat!(
            "hub = part(cylinder(20, 8, name=\"wheel\"), name=\"hub\")\n",
            "a = instance(hub, name=\"a\")\n",
            "b = instance(hub, at=(100, 0, 0), rotate=90, axis=(1, 0, 0), name=\"b\")\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let model = ctx.to_model(false, 0.0);
        let part_body = model.bodies.iter().find(|b| b.name == "wheel").expect("the part body is in the model");
        assert!(!part_body.visible);
        assert!(part_body.mesh.is_some(), "the part body carries the mesh");
        assert!(part_body.part_body.is_empty() && part_body.transform.is_none());
        let inst = model.bodies.iter().find(|b| b.name == "b/wheel").expect("the instance body");
        assert!(inst.visible);
        assert!(inst.mesh.is_none(), "an instance body carries no mesh of its own");
        assert_eq!(inst.part_body, part_body.id);
        assert_eq!(inst.instance, "b");
        let t = inst.transform.as_ref().unwrap();
        let tr = t.translation.as_ref().unwrap();
        assert!((tr.x - 100.0).abs() < 1e-9 && tr.y.abs() < 1e-9 && tr.z.abs() < 1e-9);
        let bb = inst.bbox.as_ref().unwrap();
        // Turned 90° about x, the 8 mm tall cylinder stands 8 mm along y.
        let (lo, hi) = (bb.min.as_ref().unwrap(), bb.max.as_ref().unwrap());
        assert!((hi.x - lo.x - 40.0).abs() < 1e-3 && (hi.z - lo.z - 40.0).abs() < 1e-3, "{lo:?} {hi:?}");
        assert!((hi.y - lo.y - 8.0).abs() < 1e-3, "{lo:?} {hi:?}");
        assert_eq!(model.parts.len(), 1);
        assert_eq!(model.instances.len(), 2);
        assert_eq!(model.instances[1].part, "hub");
        assert_eq!(model.instances[1].body_ids, vec![inst.id.clone()]);
        // The proto round-trips.
        use cadpole_proto::prost::Message;
        let bytes = model.encode_to_vec();
        let back = cadpole_proto::v1::Model::decode(bytes.as_slice()).unwrap();
        assert_eq!(back.instances.len(), 2);
        let b2 = back.bodies.iter().find(|b| b.name == "b/wheel").unwrap();
        assert_eq!(b2.part_body, part_body.id);
        let q = b2.transform.as_ref().unwrap().rotation.as_ref().unwrap();
        assert!((q.x - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9 && (q.w - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);

        // No assemblies: nothing new, and no invisible body.
        let mut plain = run("b = box(10, 10, 10, name=\"b\")\n");
        let m = plain.to_model(false, 0.0);
        assert_eq!(m.bodies.len(), 1);
        assert!(m.bodies[0].part_body.is_empty() && m.bodies[0].transform.is_none() && m.bodies[0].instance.is_empty());
        assert!(m.parts.is_empty() && m.instances.is_empty() && m.assemblies.is_empty());
    }

    /// An impeller assembly without mates: the
    /// shaft runs in the bore and its shoulder sits below the backplate, the
    /// nut seats on the hub nose — all by `place` between named faces, no
    /// coordinate for the shaft or the nut in the script, and the fit
    /// measured, not assumed.
    #[test]
    fn the_impeller_assembles_by_placing_named_faces() {
        let ctx = run(concat!(
            "bore = 12.0\n",
            "back_t = 6.0\n",
            "shaft_len = 20.0\n",
            "plate = extrude(sketch(XY).circle(40.0), back_t, name=\"backplate\")\n",
            "hub = extrude(sketch(XY.offset(back_t)).circle(10.0), 20.0, name=\"hub\")\n",
            "wheel_body = union(plate, hub, name=\"wheel_union\")\n",
            "bore_cut = extrude(sketch(XY.offset(-1.0)).circle(bore / 2.0), back_t + 22.0, name=\"bore_cut\")\n",
            "wheel_body = subtract(wheel_body, bore_cut, name=\"wheel_bored\")\n",
            "wheel_body.tessellation(tolerance=0.005)\n",
            "rod = extrude(sketch(XY).circle(bore / 2.0 - 0.05), shaft_len + back_t + 20.0 + 1.0, name=\"rod\")\n",
            "rod.tessellation(tolerance=0.005)\n",
            "wheel_p = part(wheel_body, name=\"wheel_part\")\n",
            "shaft_p = part(rod, name=\"shaft_part\")\n",
            "nut_p = part(extrude(sketch(XY).ngon(6, 9.0), 8.0, name=\"nut_hex\"), name=\"nut_part\")\n",
            "wheel = instance(wheel_p, name=\"wheel\")\n",
            "shaft = instance(shaft_p, name=\"shaft\", material=\"steel\")\n",
            "cap = instance(nut_p, name=\"cap\")\n",
            "asm = assembly([wheel, shaft, cap], name=\"pump\")\n",
            // The bore's wall keeps its name through the subtract; a
            // cylinder face's frame is its axis.
            "f = frame(wheel.face(\"bore_cut/side[0.0]\"))\n",
            "assert_close(abs(f.z[2]), 1.0, tol=1e-9, msg=\"a bore's frame is its axis\")\n",
            "assert_close(f.origin[0], 0.0, tol=1e-6)\n",
            "assert_close(f.origin[1], 0.0, tol=1e-6)\n",
            // Shaft in bore: axis to axis, then the shoulder.
            "place(shaft, shaft.face(\"rod/side[0.0]\"), wheel.face(\"bore_cut/side[0.0]\"), flush=True)\n",
            "place(shaft, shaft.face(\"rod/start\"), wheel.face(\"backplate/start\"), flush=True, offset=shaft_len)\n",
            // The cap nut seats on the hub's end cap: opposed normals.
            "place(cap, cap.face(\"nut_hex/start\"), wheel.face(\"hub/end\"))\n",
            "lo, hi = shaft.body().bbox()\n",
            "assert_close(lo[2], -shaft_len, tol=1e-6, msg=\"the shaft's bottom is shaft_len below the backplate\")\n",
            "assert_close(lo[0], -(bore / 2.0 - 0.05), tol=1e-3, msg=\"the shaft is on the axis\")\n",
            "nlo, nhi = cap.body().bbox()\n",
            "assert_close(nlo[2], back_t + 20.0, tol=1e-6, msg=\"the nut sits on the hub nose\")\n",
            "assert_close(nhi[2], back_t + 28.0, tol=1e-6, msg=\"and stands up from it\")\n",
            "c = distance(shaft, wheel)\n",
            "assert_true(not c.overlap, msg=\"the shaft does not cut the wheel\")\n",
            "assert_close(c.distance, 0.05, tol=0.015, msg=\"the radial fit is measured: %s\" % c.distance)\n",
            "assert_true(c.face_b.startswith(\"bore_cut/side\"), msg=c.face_b)\n",
            "assert_eq(shaft.bodies[0].name, \"shaft/rod\")\n",
            "assert_eq(asm.name, \"pump\")\n",
            "assert_eq(len(asm.instances), 3)\n",
            "assert_eq(len(asm.bodies), 3)\n",
            "assert_solid(shaft.body())\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let kinds: Vec<&str> = ctx.features.iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == "place").count(), 3);
        assert_eq!(kinds.iter().filter(|k| **k == "assembly").count(), 1);
        let shaft = ctx.instances.iter().find(|i| i.name == "shaft").unwrap();
        assert_eq!(shaft.material.as_ref().map(|m| m.name.as_str()), Some("steel"));
        assert_eq!(ctx.body(shaft.bodies[0]).unwrap().material.name, "steel");
    }

    /// `explode` moves along the chosen axis by rank times the distance,
    /// and only in the view: the bodies' geometry, and `distance()`, stay
    /// assembled. Swept over the distance through the parameter.
    #[test]
    fn explode_moves_the_view_along_the_axis_by_the_distance_and_nothing_else() {
        let src = concat!(
            "d = param(\"explode\", 0.0, unit=\"mm\", min=0, max=60)\n",
            "base_p = part(box(40, 40, 5, name=\"base\"), name=\"base_p\")\n",
            "post_p = part(cylinder(5, 30, name=\"post\"), name=\"post_p\")\n",
            "b = instance(base_p, name=\"b\")\n",
            "p = instance(post_p, at=(0, 0, 5), name=\"p\")\n",
            "asm = assembly([b, p], name=\"stack\")\n",
            "explode(asm, d, along=\"z\")\n",
            "c = distance(b, p)\n",
            "assert_true(c.touching, msg=\"the post stands on the base whatever the picture shows\")\n",
        );
        for d in [0.0, 10.0, 25.5] {
            let mut ctx = crate::evaluate(crate::EvalInput {
                scripts: vec![crate::ScriptFile { path: "main.star".into(), source: src.into() }],
                overrides: [("explode".to_owned(), cadpole_core::ParamValue::Number(d))].into_iter().collect(),
                ..Default::default()
            });
            assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
            let post = ctx.instances.iter().find(|i| i.name == "p").unwrap().id;
            let base = ctx.instances.iter().find(|i| i.name == "b").unwrap().id;
            let off = ctx.view_offset(post).unwrap();
            assert!((off.z - d).abs() < 1e-9 && off.x == 0.0 && off.y == 0.0, "explode={d}: post offset {off:?}");
            assert_eq!(ctx.view_offset(base).unwrap(), cadpole_core::Vec3::ZERO, "the lowest member stays");
            // The geometry did not move.
            let pb = body_named(&ctx, "p/post");
            let (lo, _) = ctx.body_mesh(pb).unwrap().0.bbox().unwrap();
            assert!((lo.z - 5.0).abs() < 1e-6, "explode={d}: the post's mesh is still on the base");
            // The scene did.
            let model = ctx.to_model(true, 0.0);
            let body = model.bodies.iter().find(|b| b.name == "p/post").unwrap();
            let z = body.transform.as_ref().unwrap().translation.as_ref().unwrap().z;
            assert!((z - (5.0 + d)).abs() < 1e-9, "explode={d}: displayed at z={z}");
            let bb = body.bbox.as_ref().unwrap().min.as_ref().unwrap().z;
            assert!((bb - (5.0 + d)).abs() < 1e-6, "explode={d}: bbox follows the view");
            assert_eq!(model.assemblies[0].explode_distance, d);
            assert_eq!(model.assemblies[0].explode_along, "z");
        }
        // Along an explicit direction, and per instance.
        let mut ctx = run(concat!(
            "base_p = part(box(40, 40, 5, name=\"base\"), name=\"base_p\")\n",
            "post_p = part(cylinder(5, 30, name=\"post\"), name=\"post_p\")\n",
            "b = instance(base_p, name=\"b\")\n",
            "p = instance(post_p, at=(20, 20, 5), name=\"p\")\n",
            "asm = assembly([b, p], name=\"stack\")\n",
            "explode(asm, 4, along=(1, 0, 0))\n",
            "explode(p, (0, 7, 0))\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let post = ctx.instances.iter().find(|i| i.name == "p").unwrap().id;
        let off = ctx.view_offset(post).unwrap();
        // Along x the base (40 wide, cornered at the origin) and the post
        // (10 wide, at its centre) share a centre; the wider base stays and
        // the post moves.
        assert!((off.x - 4.0).abs() < 1e-9 && (off.y - 7.0).abs() < 1e-9 && off.z == 0.0, "{off:?}");
    }

    /// Nested assemblies: a sub-assembly moves as one member of its parent
    /// and then spreads its own members.
    #[test]
    fn a_nested_assembly_explodes_as_one_member_and_then_within() {
        let mut ctx = run(concat!(
            "slab = part(box(20, 20, 2, name=\"slab\"), name=\"slab_p\")\n",
            "a = instance(slab, name=\"a\")\n",
            "b = instance(slab, at=(0, 0, 2), name=\"b\")\n",
            "c = instance(slab, at=(0, 0, 4), name=\"c\")\n",
            "top = assembly([b, c], name=\"top\")\n",
            "explode(top, 1, along=\"z\")\n",
            "whole = assembly([a, top], name=\"whole\")\n",
            "explode(whole, 10, along=\"z\")\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let of = |ctx: &mut Context, n: &str| {
            let id = ctx.instances.iter().find(|i| i.name == n).unwrap().id;
            ctx.view_offset(id).unwrap().z
        };
        assert_eq!(of(&mut ctx, "a"), 0.0);
        assert_eq!(of(&mut ctx, "b"), 10.0);
        assert_eq!(of(&mut ctx, "c"), 11.0);
    }

    /// `pose` is a displacement on top of the placement, about the
    /// instance's own origin by default, and `distance()` between two
    /// instances measures the posed geometry.
    #[test]
    fn pose_turns_an_instance_about_its_own_origin_and_distance_follows() {
        let ctx = run(concat!(
            "bar_p = part(box(30, 4, 4, name=\"bar_body\"), name=\"bar_p\")\n",
            "peg_p = part(cylinder(1, 10, name=\"peg_body\"), name=\"peg_p\")\n",
            "bar = instance(bar_p, at=(50, 0, 0), name=\"bar\")\n",
            "peg = instance(peg_p, at=(50, 20, 0), name=\"peg\")\n",
            "before = distance(bar, peg).distance\n",
            // A quarter turn about the bar's own origin (50, 0, 0) swings
            // its far end toward the peg.
            "pose(bar, rotate=90)\n",
            "after = distance(bar, peg).distance\n",
            "assert_true(after < before, msg=\"the turned bar is nearer the peg: %s vs %s\" % (after, before))\n",
            "lo, hi = bar.body().bbox()\n",
            "assert_close(lo[0], 46.0, tol=1e-6)\n",
            "assert_close(hi[0], 50.0, tol=1e-6)\n",
            "assert_close(hi[1], 30.0, tol=1e-6)\n",
            "pose(bar, translate=(0, 0, 3))\n",
            "lo2, hi2 = bar.body().bbox()\n",
            "assert_close(lo2[2], 3.0, tol=1e-6)\n",
            "f = bar.frame()\n",
            "assert_close(f.origin[2], 3.0, tol=1e-9)\n",
            "assert_close(f.x[1], 1.0, tol=1e-9, msg=\"the instance's x now points along y\")\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let kinds: Vec<&str> = ctx.features.iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == "pose").count(), 2);
    }

    /// The refusals: a body in two parts, an instance in two assemblies, a
    /// place whose first frame is not on the instance, an unknown
    /// direction.
    #[test]
    fn assembly_mistakes_are_diagnostics_that_say_what_to_do() {
        let cases: &[(&str, &str)] = &[
            (
                "b = box(1, 1, 1, name=\"b\")\npart(b, name=\"p\")\npart(b, name=\"q\")\n",
                "already belongs to part",
            ),
            (
                "p = part(box(1, 1, 1, name=\"b\"), name=\"p\")\ni = instance(p, name=\"i\")\nassembly([i], name=\"x\")\nassembly([i], name=\"y\")\n",
                "already in assembly",
            ),
            (
                "p = part(box(1, 1, 1, name=\"b\"), name=\"p\")\ni = instance(p, name=\"i\")\nj = instance(p, at=(5, 0, 0), name=\"j\")\nplace(i, j.face(\"b/top\"), i.face(\"b/bottom\"))\n",
                "is not on instance(i of p)",
            ),
            (
                "p = part(box(1, 1, 1, name=\"b\"), name=\"p\")\ni = instance(p, name=\"i\")\na = assembly([i], name=\"a\")\nexplode(a, 3, along=\"sideways\")\n",
                "valid: x, y, z, radial",
            ),
            (
                "p = part(box(1, 1, 1, name=\"b\"), name=\"p\")\ni = instance(p, name=\"i\")\ni.face(\"nowhere\")\n",
                "no face named \"nowhere\"",
            ),
        ];
        for (src, expect) in cases {
            let ctx = run(src);
            let errs = errors(&ctx);
            assert!(errs.iter().any(|e| e.contains(expect)), "{src}\nexpected {expect:?}, got {errs:?}");
        }
    }

    /// `along=` steers a *distance*; with an offset `(dx, dy, dz)` the
    /// offset is the whole answer. It used to be dropped without a word, so
    /// `explode(p, (1, 2, 3), along="bogus")` ran clean while the same
    /// spelling with a distance was refused.
    #[test]
    fn explode_refuses_along_with_an_offset() {
        let head = concat!(
            "post_p = part(cylinder(5, 30, name=\"post\"), name=\"post_p\")\n",
            "p = instance(post_p, name=\"p\")\n",
        );
        for along in ["\"x\"", "\"bogus\"", "(1, 0, 0)"] {
            let ctx = run(&format!("{head}explode(p, (1, 2, 3), along={along})\n"));
            let errs = errors(&ctx);
            assert!(
                errs.iter().any(|e| e.contains("explode") && e.contains("along=") && e.contains("offset")),
                "along={along}: {errs:?}"
            );
        }
        // A distance with along= is still the documented scalar form.
        let mut ctx = run(&format!("{head}explode(p, 5, along=\"x\")\n"));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let post = ctx.instances.iter().find(|i| i.name == "p").unwrap().id;
        assert_eq!(ctx.view_offset(post).unwrap(), cadpole_core::Vec3::new(5.0, 0.0, 0.0));
    }
}
