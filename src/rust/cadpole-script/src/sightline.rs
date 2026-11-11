//! Shadow studies and sightlines: `shadow_study(bodies, on=, sun=)`,
//! `sightline(from, to, obstacles=)`, `sightlines(from, target,
//! obstacles=)` and `view_cone(from, az=, fov=, el=)`. The geometry is
//! `cadpole_core::shadow`'s; this module names things — the receiver, the
//! casters, the body and face a ray stops at — and hands back values a
//! `test()` can assert on (docs/buildings.md).
//!
//! None of these is a feature. A shadow can be recorded as a sketch on its
//! receiver (`Shadow.sketch()`), which is a feature like any sketch, so a
//! script can extrude the footprint a millimetre and see it in a render.

use std::collections::BTreeMap;
use std::fmt;

use allocative::Allocative;
use cadpole_core::context::{Context, fmt_num};
use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::ids::BodyId;
use cadpole_core::shadow::{self, Footprint, Occluders, Shapes};
use cadpole_core::sketch::{Loop, Sketch};
use cadpole_core::sun::Site;
use starlark::environment::{GlobalsBuilder, Methods, MethodsBuilder, MethodsStatic};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::values::float::UnpackFloat;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::{Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, UnpackValue, Value, ValueError, ValueLike};
use starlark_derive::starlark_value;

use crate::check::{VecArg, num, opt_num, vec3};
use crate::clearance::{Meshed, meshed};
use crate::convert::alloc_vec3;
use crate::globals::collect_bodies;
use crate::store::{call_span, store};
use crate::sun::{KSun, date_of, hours_of, site_of, sun_at};
use crate::values::{KBody, KFace, KPlane, KSketch};

// ---------------------------------------------------------------- Shadow

/// One footprint: what `bodies` throw on the receiver under one sun.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KShadow {
    pub sun: KSun,
    pub receiver: String,
    pub casters: Vec<String>,
    /// The receiver plane, for a planar receiver.
    #[allocative(skip)]
    pub plane: Option<Plane>,
    #[allocative(skip)]
    pub shapes: Shapes,
    /// The footprint in space: one loop per contour on a plane, one
    /// triangle per shaded triangle on terrain.
    #[allocative(skip)]
    pub loops: Vec<Vec<Vec3>>,
    pub area: f64,
    /// The receiver faces the sun at all.
    pub lit: bool,
    /// Exact for the tessellation (a plane) rather than sampled (terrain).
    pub exact: bool,
}
starlark_simple_value!(KShadow);

impl fmt::Display for KShadow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shadow({} on {}: {} mm2{})", self.sun.spec.label, self.receiver, fmt_num(self.area), if self.lit { "" } else { ", receiver unlit" })
    }
}

fn alloc_loops<'v>(heap: Heap<'v>, loops: &[Vec<Vec3>]) -> Value<'v> {
    let out: Vec<Value<'v>> = loops.iter().map(|l| heap.alloc(l.iter().map(|p| alloc_vec3(heap, *p)).collect::<Vec<_>>())).collect();
    heap.alloc(out)
}

#[starlark_value(type = "Shadow")]
impl<'v> StarlarkValue<'v> for KShadow {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("shadow_methods", shadow_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        Some(match attribute {
            "sun" => heap.alloc(self.sun.clone()),
            "label" => heap.alloc(self.sun.spec.label.as_str()),
            "receiver" => heap.alloc(self.receiver.as_str()),
            "casters" => crate::convert::alloc_str_list(heap, &self.casters),
            "loops" => alloc_loops(heap, &self.loops),
            "lit" => heap.alloc(self.lit),
            "exact" => heap.alloc(self.exact),
            _ => return None,
        })
    }
    fn dir_attr(&self) -> Vec<String> {
        ["sun", "label", "receiver", "casters", "loops", "lit", "exact"].iter().map(|s| s.to_string()).collect()
    }
}

#[starlark_module]
fn shadow_methods(builder: &mut MethodsBuilder) {
    /// The shadowed area of the receiver, mm²: exact for the tessellation on
    /// a plane or a planar face, sampled on terrain. 0 when the receiver
    /// does not face the sun.
    fn area(this: &KShadow) -> anyhow::Result<f64> {
        Ok(this.area)
    }

    /// Record the footprint as a sketch on the receiver's plane, one loop
    /// per contour (holes included), so it can be extruded and rendered.
    /// Only a shadow on a plane or a planar face has one; a terrain shadow
    /// is triangles (`.loops`).
    fn sketch<'v>(this: &KShadow, #[starlark(require = named)] name: Option<&str>, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<KSketch> {
        let Some(plane) = this.plane else {
            anyhow::bail!("shadow.sketch(): a shadow on terrain is triangles, not a sketch; use .loops");
        };
        let span = call_span(eval);
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut args = BTreeMap::new();
        args.insert("plane".into(), format!("{}", KPlane(plane)));
        args.insert("on".into(), this.receiver.clone());
        args.insert("sun".into(), this.sun.spec.label.clone());
        let mut sketch = Sketch::new(plane);
        let mut n = 0;
        for (si, shape) in this.shapes.iter().enumerate() {
            for (ci, c) in shape.iter().enumerate() {
                let pts = shadow::contour_points(c);
                if pts.len() < 3 {
                    continue;
                }
                let label = if ci == 0 { format!("shadow[{si}]") } else { format!("shadow[{si}].hole[{}]", ci - 1) };
                sketch = sketch.with_named_loop(Loop::polygon(&pts), label);
                n += 1;
            }
        }
        args.insert("loops".into(), n.to_string());
        let fid = ctx.new_feature("sketch", name.map(str::to_owned), vec![], args, span)?;
        Ok(KSketch::new(fid.0, sketch))
    }
}

/// What `shadow_study(...)` returns: one `Shadow` per sun.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KShadowStudy {
    pub shadows: Vec<KShadow>,
}
starlark_simple_value!(KShadowStudy);

impl fmt::Display for KShadowStudy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shadow_study({} suns, {} mm2 in all)", self.shadows.len(), fmt_num(self.shadows.iter().map(|s| s.area).sum::<f64>()))
    }
}

#[starlark_value(type = "ShadowStudy")]
impl<'v> StarlarkValue<'v> for KShadowStudy {
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new("shadow_study_methods", shadow_study_methods);
        Some(RES.methods())
    }
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        Some(match attribute {
            "shadows" => heap.alloc(self.shadows.clone()),
            "labels" => heap.alloc(self.shadows.iter().map(|s| s.sun.spec.label.as_str()).collect::<Vec<_>>()),
            _ => return None,
        })
    }
    fn dir_attr(&self) -> Vec<String> {
        ["shadows", "labels"].iter().map(|s| s.to_string()).collect()
    }
    fn length(&self) -> starlark::Result<i32> {
        Ok(self.shadows.len() as i32)
    }
    fn at(&self, index: Value<'v>, heap: Heap<'v>) -> starlark::Result<Value<'v>> {
        let i = index.unpack_i32().ok_or(ValueError::IncorrectParameterType)?;
        let n = self.shadows.len() as i32;
        let k = if i < 0 { i + n } else { i };
        if k < 0 || k >= n {
            return Err(ValueError::IndexOutOfBound(i).into());
        }
        Ok(heap.alloc(self.shadows[k as usize].clone()))
    }
    fn iterate_collect(&self, heap: Heap<'v>) -> starlark::Result<Vec<Value<'v>>> {
        Ok(self.shadows.iter().map(|s| heap.alloc(s.clone())).collect())
    }
}

#[starlark_module]
fn shadow_study_methods(builder: &mut MethodsBuilder) {
    /// The shadowed areas summed over every sun, mm².
    fn area(this: &KShadowStudy) -> anyhow::Result<f64> {
        Ok(this.shadows.iter().map(|s| s.area).sum())
    }

    /// The largest shadowed area over the suns, mm²: the worst case.
    fn max_area(this: &KShadowStudy) -> anyhow::Result<f64> {
        Ok(this.shadows.iter().map(|s| s.area).fold(0.0, f64::max))
    }
}

// ---------------------------------------------------------------- Sightline

/// What `sightline(from, to)` returns.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KSightline {
    #[allocative(skip)]
    pub from: Vec3,
    #[allocative(skip)]
    pub to: Vec3,
    /// The body and face the ray meets first, when it meets one.
    pub blocker: Option<(String, String)>,
    #[allocative(skip)]
    pub hit: Option<Vec3>,
    /// From the eye to the blocker, or to the target when clear, mm.
    pub distance: f64,
}
starlark_simple_value!(KSightline);

impl fmt::Display for KSightline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.blocker {
            None => write!(f, "sightline(clear, {} mm)", fmt_num(self.distance)),
            Some((b, face)) => write!(f, "sightline(blocked by {b} at {face}, {} mm out)", fmt_num(self.distance)),
        }
    }
}

#[starlark_value(type = "Sightline")]
impl<'v> StarlarkValue<'v> for KSightline {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        Some(match attribute {
            "clear" => heap.alloc(self.blocker.is_none()),
            "blocked" => heap.alloc(self.blocker.is_some()),
            "blocked_by" => match &self.blocker {
                Some((b, _)) => heap.alloc(b.as_str()),
                None => Value::new_none(),
            },
            "blocked_face" => match &self.blocker {
                Some((_, face)) => heap.alloc(face.as_str()),
                None => Value::new_none(),
            },
            "hit" => match self.hit {
                Some(p) => alloc_vec3(heap, p),
                None => Value::new_none(),
            },
            "distance" => heap.alloc(self.distance),
            "length" => heap.alloc((self.to - self.from).length()),
            "from" => alloc_vec3(heap, self.from),
            "to" => alloc_vec3(heap, self.to),
            _ => return None,
        })
    }
    fn dir_attr(&self) -> Vec<String> {
        ["clear", "blocked", "blocked_by", "blocked_face", "hit", "distance", "length", "from", "to"].iter().map(|s| s.to_string()).collect()
    }
}

/// What `sightlines(...)` and `view_cone(...)` return: a fan of rays and
/// what stopped them.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KFan {
    pub kind: &'static str,
    pub target: String,
    pub visible: usize,
    pub total: usize,
    /// Body name and the fraction of the rays it stopped, most first.
    pub blocked_by: Vec<(String, f64)>,
}
starlark_simple_value!(KFan);

impl KFan {
    fn fraction(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.visible as f64 / self.total as f64 }
    }
}

impl fmt::Display for KFan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({} of {} rays reach {}", self.kind, self.visible, self.total, self.target)?;
        for (b, frac) in &self.blocked_by {
            write!(f, ", {b} stops {}%", fmt_num((frac * 1000.0).round() / 10.0))?;
        }
        write!(f, ")")
    }
}

#[starlark_value(type = "SightlineFan")]
impl<'v> StarlarkValue<'v> for KFan {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        Some(match attribute {
            "fraction" | "visible" => heap.alloc(self.fraction()),
            "blocked" => heap.alloc(1.0 - self.fraction()),
            "visible_rays" => heap.alloc(self.visible as i32),
            "rays" => heap.alloc(self.total as i32),
            "target" => heap.alloc(self.target.as_str()),
            "blocked_by" => {
                let items: Vec<Value<'v>> = self.blocked_by.iter().map(|(b, f)| heap.alloc((b.as_str(), *f))).collect();
                heap.alloc(items)
            }
            _ => return None,
        })
    }
    fn dir_attr(&self) -> Vec<String> {
        ["fraction", "visible", "blocked", "visible_rays", "rays", "target", "blocked_by"].iter().map(|s| s.to_string()).collect()
    }
}

// ---------------------------------------------------------------- helpers

/// The bodies an `obstacles=`/`through=` argument names, or every visible
/// body of the document when it is left out.
fn obstacle_ids<'v>(v: Option<Value<'v>>, ctx: &Context, heap: Heap<'v>, what: &str) -> anyhow::Result<Vec<BodyId>> {
    match v {
        None => Ok(ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect()),
        Some(v) if v.is_none() => Ok(ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect()),
        Some(v) => {
            let items: Vec<Value<'v>> = match <UnpackListOrTuple<Value<'v>>>::unpack_value(v).map_err(|e| e.into_anyhow())? {
                Some(l) => l.items,
                None => vec![v],
            };
            Ok(collect_bodies(&items, heap, what)?.iter().map(|b| b.body_id()).collect())
        }
    }
}

fn face_name(m: &Meshed, tri: usize) -> String {
    m.mesh.face_ids.get(tri).and_then(|f| m.topo.faces.get(*f as usize)).map(|f| f.name.clone()).unwrap_or_else(|| format!("tri:{tri}"))
}

/// The receiver `on=` names: a planar face (its plane and outline), a
/// plane (unbounded), or a body (terrain: its whole mesh).
enum Receiver {
    Plane { plane: Plane, region: Option<Shapes>, oriented: bool, name: String },
    Mesh { mesh: cadpole_core::tessellate::Mesh, name: String },
}

fn receiver_of<'v>(v: Value<'v>, ctx: &mut Context) -> anyhow::Result<Receiver> {
    if let Some(p) = v.downcast_ref::<KPlane>() {
        return Ok(Receiver::Plane { plane: p.0, region: None, oriented: false, name: format!("{}", KPlane(p.0)) });
    }
    if let Some(f) = v.downcast_ref::<KFace>() {
        let plane = cadpole_core::select::face_plane(&f.kind, f.normal, f.centroid).map_err(|_| {
            anyhow::anyhow!("shadow_study: on= must be a planar face, a plane or a body (terrain); {} is a {} face", f.name, f.kind)
        })?;
        let (mesh, topo) = ctx.body_mesh(BodyId(f.body))?;
        let info = topo.faces.iter().find(|x| x.name == f.name).ok_or_else(|| anyhow::anyhow!("shadow_study: no face named {} on the body any more", f.name))?;
        let range = (info.tri_start as usize, (info.tri_start + info.tri_count) as usize);
        let region = shadow::face_region(mesh, range, &plane);
        return Ok(Receiver::Plane { plane, region: Some(region), oriented: true, name: f.name.clone() });
    }
    if let Some(b) = v.downcast_ref::<KBody>() {
        let (mesh, _) = ctx.body_mesh(b.body_id())?;
        return Ok(Receiver::Mesh { mesh: mesh.clone(), name: b.name.clone() });
    }
    anyhow::bail!("shadow_study: on= must be a planar face, a plane or a body (terrain), got {}", v.get_type())
}

fn suns_of<'v>(
    sun: Option<Value<'v>>,
    dates: Option<UnpackListOrTuple<String>>,
    times: Option<UnpackListOrTuple<Value<'v>>>,
    lat: Option<UnpackFloat>,
    lon: Option<UnpackFloat>,
    tz: Option<UnpackFloat>,
) -> anyhow::Result<Vec<KSun>> {
    let mut out = Vec::new();
    if let Some(v) = sun.filter(|v| !v.is_none()) {
        let items: Vec<Value<'v>> = match <UnpackListOrTuple<Value<'v>>>::unpack_value(v).map_err(|e| e.into_anyhow())? {
            Some(l) => l.items,
            None => vec![v],
        };
        for item in items {
            let s = item.downcast_ref::<KSun>().ok_or_else(|| anyhow::anyhow!("shadow_study: sun= takes a sun(...) or a list of them, got {}", item.get_type()))?;
            out.push(s.clone());
        }
    }
    match (dates, times) {
        (None, None) => {}
        (Some(dates), Some(times)) => {
            let (Some(lat), Some(lon)) = (lat, lon) else {
                anyhow::bail!("shadow_study: dates= and times= need the site too: lat=, lon= (and tz=)");
            };
            let site: Site = site_of(num(&lat, "shadow_study: lat")?, num(&lon, "shadow_study: lon")?, opt_num(&tz, "shadow_study: tz")?, "shadow_study")?;
            for d in &dates.items {
                let date = date_of(d, "shadow_study: dates")?;
                for t in &times.items {
                    out.push(sun_at(site, date, hours_of(*t, "shadow_study: times")?));
                }
            }
        }
        _ => anyhow::bail!("shadow_study: dates= and times= go together"),
    }
    if out.is_empty() {
        anyhow::bail!("shadow_study: give sun=sun(...) (or a list), or dates=[...] and times=[...] with lat=, lon=");
    }
    Ok(out)
}

fn north_of(north: Option<UnpackFloat>, ctx: &Context, what: &str) -> anyhow::Result<f64> {
    match opt_num(&north, what)? {
        Some(n) => Ok(n),
        None => Ok(ctx.render_defaults.as_ref().and_then(|d| d.north_degrees).unwrap_or(0.0)),
    }
}

/// The eye point of a sightline, a fan or a view cone: the first argument,
/// or `eye=`. It cannot be `from=`: `from` is a reserved word in Starlark
/// (the lexer refuses it wherever it appears), so the positional form is
/// the one the docs show and `eye=` is the keyword.
fn eye_point(first: Option<VecArg>, eye: Option<VecArg>, what: &str) -> anyhow::Result<Vec3> {
    match (first, eye) {
        (Some(p), None) | (None, Some(p)) => vec3(&p, &format!("{what}: eye")),
        (Some(_), Some(_)) => anyhow::bail!("{what}: the eye point was given twice, first and as eye="),
        (None, None) => anyhow::bail!(
            "{what}: needs an eye point, first or as eye= (`from` is a reserved word in Starlark, so it cannot be the keyword)"
        ),
    }
}

// ---------------------------------------------------------------- globals

#[starlark_module]
pub(crate) fn sightline_globals(builder: &mut GlobalsBuilder) {
    /// Where `bodies` throw their shadow on a receiver, under one sun or
    /// several: `on=` a planar face (a neighbour's light court, a roof), a
    /// plane, or a body (terrain, sampled). `sun=` is a `sun(...)` or a
    /// list of them; or `dates=["YYYY-MM-DD", ...]` and `times=["HH:MM",
    /// ...]` with `lat=`, `lon=` (and `tz=`) draw every combination —
    /// `dates=solstices(2026) + equinoxes(2026)[:1], times=["09:00",
    /// "12:00", "15:00"]` is the nine-tile study. `north=` says where north
    /// is in model space (degrees clockwise from +y; default the document's
    /// `render_defaults(north=)`, else +y). The answer is a `ShadowStudy`:
    /// one `Shadow` per sun (`.shadows`, or index it), each with `.area()`
    /// in mm², `.loops` in space, `.lit` (the receiver faces that sun),
    /// `.exact`, and `.sketch()` to record the footprint as a sketch on the
    /// receiver; `.area()` and `.max_area()` over the whole study. On a
    /// plane or a face the footprint is exact for the tessellation: the
    /// bodies' triangles projected along the sun and unioned, clipped to
    /// the face. A receiver facing away from the sun is in its own shadow,
    /// and the answer for it is 0 with `.lit` False. docs/buildings.md.
    fn shadow_study<'v>(
        #[starlark(require = pos)] bodies: Value<'v>,
        #[starlark(require = named)] on: Value<'v>,
        #[starlark(require = named)] sun: Option<Value<'v>>,
        #[starlark(require = named)] dates: Option<UnpackListOrTuple<String>>,
        #[starlark(require = named)] times: Option<UnpackListOrTuple<Value<'v>>>,
        #[starlark(require = named)] lat: Option<UnpackFloat>,
        #[starlark(require = named)] lon: Option<UnpackFloat>,
        #[starlark(require = named)] tz: Option<UnpackFloat>,
        #[starlark(require = named)] north: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KShadowStudy> {
        let heap = eval.heap();
        let items: Vec<Value<'v>> = match <UnpackListOrTuple<Value<'v>>>::unpack_value(bodies).map_err(|e| e.into_anyhow())? {
            Some(l) => l.items,
            None => vec![bodies],
        };
        let casters = collect_bodies(&items, heap, "shadow_study")?;
        if casters.is_empty() {
            anyhow::bail!("shadow_study: no bodies to cast a shadow");
        }
        let suns = suns_of(sun, dates, times, lat, lon, tz)?;
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let north = north_of(north, &ctx, "shadow_study: north")?;
        let receiver = receiver_of(on, &mut ctx)?;
        let ids: Vec<BodyId> = casters.iter().map(|b| b.body_id()).collect();
        let meshes = meshed(&mut ctx, &ids)?;
        let caster_names: Vec<String> = meshes.iter().map(|m| m.name.clone()).collect();
        let mesh_refs: Vec<&cadpole_core::tessellate::Mesh> = meshes.iter().map(|m| &m.mesh).collect();
        let tolerance = crate::clearance::coarsest(&meshes);
        let mut shadows = Vec::with_capacity(suns.len());
        for s in suns {
            let dir = s.spec.direction(north);
            let shadow = match &receiver {
                Receiver::Plane { plane, region, oriented, name } => {
                    let Footprint { shapes, area, lit } = shadow::footprint_on_plane(&mesh_refs, dir, plane, *oriented, region.as_ref(), tolerance);
                    let loops = shadow::shapes_to_world(&shapes, plane);
                    KShadow { sun: s, receiver: name.clone(), casters: caster_names.clone(), plane: Some(*plane), shapes, loops, area, lit, exact: true }
                }
                Receiver::Mesh { mesh, name } => {
                    let ms = shadow::footprint_on_mesh(&mesh_refs, dir, mesh);
                    let loops = ms.shaded.iter().filter_map(|(t, _)| mesh.triangle(*t).map(|tri| tri.to_vec())).collect();
                    KShadow {
                        sun: s,
                        receiver: name.clone(),
                        casters: caster_names.clone(),
                        plane: None,
                        shapes: Vec::new(),
                        loops,
                        area: ms.area,
                        lit: ms.lit_area > 0.0,
                        exact: false,
                    }
                }
            };
            shadows.push(shadow);
        }
        Ok(KShadowStudy { shadows })
    }

    /// One line of sight, from an eye point to a target point, through the
    /// bodies in `obstacles=` (a body, a list, an instance or an assembly;
    /// every visible body when left out): `.clear` when nothing stands
    /// between, else `.blocked_by` (the first body the ray meets),
    /// `.blocked_face` (its face), `.hit` (the point) and `.distance` (from
    /// the eye to it; to the target when clear). A neighbour's window to
    /// the skyline, a sidewalk to a lantern. The eye comes first, or as
    /// `eye=` (`from` is a reserved word in Starlark, so it cannot be the
    /// keyword); the target second, or as `to=`. One ray through the
    /// renderer's BVH, on the bodies' tessellations. docs/buildings.md.
    fn sightline<'v>(
        #[starlark(require = pos)] from: Option<VecArg>,
        to: Option<VecArg>,
        #[starlark(require = named)] eye: Option<VecArg>,
        #[starlark(require = named)] obstacles: Option<Value<'v>>,
        #[starlark(require = named)] through: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KSightline> {
        let heap = eval.heap();
        let from = eye_point(from, eye, "sightline")?;
        let Some(to) = to else {
            anyhow::bail!("sightline: needs a target point, second or as to=");
        };
        let to = vec3(&to, "sightline: to")?;
        if (to - from).length() <= 0.0 {
            anyhow::bail!("sightline: from and to coincide");
        }
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let ids = obstacle_ids(obstacles.or(through), &ctx, heap, "sightline")?;
        let meshes = meshed(&mut ctx, &ids)?;
        let refs: Vec<&cadpole_core::tessellate::Mesh> = meshes.iter().map(|m| &m.mesh).collect();
        let occ = Occluders::new(&refs);
        Ok(match occ.cast(from, to) {
            None => KSightline { from, to, blocker: None, hit: None, distance: (to - from).length() },
            Some(b) => {
                let m = &meshes[b.body];
                KSightline { from, to, blocker: Some((m.name.clone(), face_name(m, b.tri))), hit: Some(b.point), distance: b.distance }
            }
        })
    }

    /// A fan of sightlines from an eye point to a target — a body, a face,
    /// or a list of points — and the fraction of it the eye can see:
    /// `samples=` points (default 256) are spread over the target by area,
    /// and a ray to each counts as visible when nothing in `obstacles=`
    /// (every visible body when left out) and nothing of the target itself
    /// stands before it. `.fraction` (0..1), `.visible_rays`, `.rays`, and
    /// `.blocked_by` as `[(body, fraction), ...]`, most first. "How much of
    /// the studio's front does the sidewalk still see with the passage at 8
    /// ft rather than 12?" is this, twice. The eye comes first, or as
    /// `eye=` (`from` is a reserved word in Starlark); the target second,
    /// or as `target=`.
    fn sightlines<'v>(
        #[starlark(require = pos)] from: Option<VecArg>,
        target: Option<Value<'v>>,
        #[starlark(require = named)] eye: Option<VecArg>,
        #[starlark(require = named)] obstacles: Option<Value<'v>>,
        #[starlark(require = named)] samples: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KFan> {
        let heap = eval.heap();
        let from = eye_point(from, eye, "sightlines")?;
        let Some(target) = target else {
            anyhow::bail!("sightlines: needs a target, second or as target=: a body, a face or a list of points");
        };
        let samples = match samples {
            Some(v) => crate::check::count(&v, "sightlines: samples", 1, 100_000)?,
            None => 256,
        };
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let mut ids = obstacle_ids(obstacles, &ctx, heap, "sightlines")?;
        // The target: a body or a face joins the occluders (its own far
        // side hides its points) and is sampled by area; a list of points
        // is used as given.
        let (points, target_name, target_id): (Vec<Vec3>, String, Option<BodyId>) = if let Some(b) = target.downcast_ref::<KBody>() {
            let id = b.body_id();
            let (mesh, _) = ctx.body_mesh(id)?;
            (shadow::surface_samples(mesh, None, samples), b.name.clone(), Some(id))
        } else if let Some(f) = target.downcast_ref::<KFace>() {
            let id = BodyId(f.body);
            let (mesh, topo) = ctx.body_mesh(id)?;
            let info = topo.faces.iter().find(|x| x.name == f.name).ok_or_else(|| anyhow::anyhow!("sightlines: no face named {}", f.name))?;
            let range = (info.tri_start as usize, (info.tri_start + info.tri_count) as usize);
            (shadow::surface_samples(mesh, Some(range), samples), f.name.clone(), Some(id))
        } else if let Some(list) = <UnpackListOrTuple<VecArg>>::unpack_value(target).map_err(|e| e.into_anyhow())? {
            let pts: Vec<Vec3> = list.items.iter().map(|p| vec3(p, "sightlines: target")).collect::<anyhow::Result<_>>()?;
            if pts.is_empty() {
                anyhow::bail!("sightlines: the target list is empty");
            }
            (pts, format!("{} points", list.items.len()), None)
        } else {
            anyhow::bail!("sightlines: target must be a body, a face or a list of points, got {}", target.get_type());
        };
        if points.is_empty() {
            anyhow::bail!("sightlines: {target_name} has no surface to sample");
        }
        let target_index = match target_id {
            Some(id) => {
                if let Some(i) = ids.iter().position(|x| *x == id) {
                    Some(i)
                } else {
                    ids.push(id);
                    Some(ids.len() - 1)
                }
            }
            None => None,
        };
        let meshes = meshed(&mut ctx, &ids)?;
        let refs: Vec<&cadpole_core::tessellate::Mesh> = meshes.iter().map(|m| &m.mesh).collect();
        let occ = Occluders::new(&refs);
        let fan = shadow::fan(&occ, from, &points, target_index);
        Ok(KFan {
            kind: "sightlines",
            target: target_name,
            visible: fan.visible,
            total: fan.total,
            blocked_by: fan.blocked_by.iter().map(|(b, n)| (meshes[*b].name.clone(), *n as f64 / fan.total.max(1) as f64)).collect(),
        })
    }

    /// How much sky a window sees: a grid of `rays=` × `rays=` rays (default
    /// 32) from an eye point over a window of sky `fov=` degrees wide about
    /// the compass azimuth `az=` (0 north, 90 east; `north=` as
    /// `shadow_study`) and between the two elevation angles `el=(lo, hi)`
    /// (the skyline band; default `(0, 30)`), through `obstacles=` (every
    /// visible body when left out). `.fraction` of the rays that reach the
    /// sky, `.blocked`, and `.blocked_by` as `[(body, fraction), ...]`: run
    /// it with and without the proposal and the difference is the view it
    /// takes. `range=` (mm) is how far a ray looks (default: unbounded).
    /// The eye comes first, or as `eye=` (`from` is a reserved word in
    /// Starlark).
    fn view_cone<'v>(
        #[starlark(require = pos)] from: Option<VecArg>,
        #[starlark(require = named)] eye: Option<VecArg>,
        #[starlark(require = named)] az: UnpackFloat,
        #[starlark(require = named)] fov: Option<UnpackFloat>,
        #[starlark(require = named)] el: Option<VecArg>,
        #[starlark(require = named)] obstacles: Option<Value<'v>>,
        #[starlark(require = named)] rays: Option<UnpackFloat>,
        #[starlark(require = named)] range: Option<UnpackFloat>,
        #[starlark(require = named)] north: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KFan> {
        let heap = eval.heap();
        let from = eye_point(from, eye, "view_cone")?;
        let az = num(&az, "view_cone: az")?;
        let fov = opt_num(&fov, "view_cone: fov")?.unwrap_or(60.0);
        if !(0.0 < fov && fov <= 360.0) {
            anyhow::bail!("view_cone: fov must be between 0 and 360 degrees, got {}", fmt_num(fov));
        }
        let (lo, hi) = match el {
            None => (0.0, 30.0),
            Some(v) => match v.items.as_slice() {
                [a, b] => (crate::check::finite(a.0, "view_cone: el")?, crate::check::finite(b.0, "view_cone: el")?),
                _ => anyhow::bail!("view_cone: el= is a pair of elevation angles (lo, hi)"),
            },
        };
        if !(-90.0..=90.0).contains(&lo) || !(-90.0..=90.0).contains(&hi) || hi <= lo {
            anyhow::bail!("view_cone: el=(lo, hi) needs lo < hi within -90..90, got ({}, {})", fmt_num(lo), fmt_num(hi));
        }
        let rays = match rays {
            Some(v) => crate::check::count(&v, "view_cone: rays", 1, 1024)?,
            None => 32,
        };
        let range = opt_num(&range, "view_cone: range")?.unwrap_or(f64::MAX);
        if range <= 0.0 {
            anyhow::bail!("view_cone: range must be positive");
        }
        let st = store(eval)?;
        let mut ctx = st.ctx.borrow_mut();
        let north = north_of(north, &ctx, "view_cone: north")?;
        let ids = obstacle_ids(obstacles, &ctx, heap, "view_cone")?;
        let meshes = meshed(&mut ctx, &ids)?;
        let refs: Vec<&cadpole_core::tessellate::Mesh> = meshes.iter().map(|m| &m.mesh).collect();
        let occ = Occluders::new(&refs);
        let fan = shadow::view_cone(&occ, from, (az - fov / 2.0, az + fov / 2.0), (lo, hi), (rays, rays), range, north);
        Ok(KFan {
            kind: "view_cone",
            target: format!("the sky {}-{} up, {} wide about az {}", fmt_num(lo), fmt_num(hi), fmt_num(fov), fmt_num(az)),
            visible: fan.visible,
            total: fan.total,
            blocked_by: fan.blocked_by.iter().map(|(b, n)| (meshes[*b].name.clone(), *n as f64 / fan.total.max(1) as f64)).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    /// A tower and a neighbour's court: at noon on the winter solstice the
    /// tower's shadow reaches the court, at midsummer it does not; the
    /// footprint on the ground plane is exact; a study over the nine
    /// date/time tiles reads back.
    #[test]
    fn a_shadow_study_measures_what_reaches_the_court() {
        let ctx = run(
            "tower = box(10, 10, 30, name=\"tower\")\n\
             court = box(20, 20, 1, at=(-40, -5, -1), name=\"court\")\n\
             top = court.faces(normal=(0, 0, 1)).one()\n\
             low = sun(az=90, el=45)\n\
             high = sun(az=90, el=71.6)\n\
             s = shadow_study(tower, on=top, sun=[low, high])\n\
             assert_eq(len(s), 2)\n\
             assert_true(abs(s[0].area() - 100) < 0.01, msg=str(s[0].area()))\n\
             assert_true(s[1].area() < 1e-6, msg=str(s[1].area()))\n\
             assert_true(s[0].lit and s[1].lit)\n\
             assert_true(s[0].exact)\n\
             assert_eq(s[0].receiver, top.name)\n\
             assert_eq(s[0].casters, [\"tower\"])\n\
             assert_true(abs(s.area() - 100) < 0.01)\n\
             assert_true(abs(s.max_area() - 100) < 0.01)\n\
             assert_eq(s.labels, [\"az 90 el 45\", \"az 90 el 71.6\"])\n\
             g = shadow_study([tower], on=XY, sun=low)\n\
             assert_true(abs(g[0].area() - 400) < 0.01, msg=str(g[0].area()))\n\
             assert_eq(len(g[0].loops), 1)\n\
             sk = g[0].sketch(name=\"footprint\")\n\
             assert_eq(sk.loops, 1)\n\
             slab = extrude(sk, 1, name=\"slab\")\n\
             assert_true(abs(slab.volume() - 400) < 0.5, msg=str(slab.volume()))\n\
             below = shadow_study(tower, on=top, sun=sun(az=90, el=-10))\n\
             assert_true(not below[0].lit)\n\
             assert_eq(below[0].area(), 0)\n\
             t = shadow_study(tower, on=court, sun=low)\n\
             assert_true(not t[0].exact)\n\
             assert_true(t[0].area() > 95 and t[0].area() < 125, msg=str(t[0].area()))\n\
             assert_true(len(t[0].loops) >= 1)\n\
             nine = shadow_study(tower, on=top, dates=solstices(2026) + equinoxes(2026)[:1], times=[\"09:00\", \"12:00\", \"15:00\"], lat=38.0, lon=-121.0, tz=-8)\n\
             assert_eq(len(nine), 9)\n\
             assert_eq(nine.labels[0], \"2026-06-21 09:00\")\n\
             assert_true(all([sh.area() >= 0 for sh in nine]))\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// The sidewalk case: an eye on the sidewalk at the
    /// property line, 5.5 ft up, looks down the passage between the new house
    /// and the east line to the old studio's lantern 50 ft back and 8 ft
    /// lower. With an 8 ft passage the house's east wall crosses the line of
    /// sight; with 12 ft it does not. A fan over the studio's front reads
    /// the fraction the sidewalk still sees; a view cone to the west loses
    /// sky to the house.
    #[test]
    fn sightlines_from_the_sidewalk() {
        let ctx = run(
            "ft = 304.8\n\
             passage = param(\"passage\", 8.0, unit=\"ft\", min=4, max=20)\n\
             house = box(30 * ft, 40 * ft, 30 * ft, at=(-30 * ft - passage * ft, 10 * ft, 0), name=\"house\")\n\
             studio = box(20 * ft, 20 * ft, 20 * ft, at=(-20 * ft, 50 * ft, -22.5 * ft), name=\"studio\")\n\
             lantern = (-10 * ft, 50 * ft, -2.5 * ft)\n\
             eye = (-passage * ft / 2, 0, 5.5 * ft)\n\
             narrow = sightline(eye, lantern, obstacles=[house])\n\
             assert_true(narrow.blocked, msg=str(narrow))\n\
             assert_eq(narrow.blocked_by, \"house\")\n\
             assert_true(\"house\" in narrow.blocked_face)\n\
             assert_true(narrow.hit[1] > 10 * ft and narrow.distance < narrow.length, msg=str(narrow))\n\
             wide_eye = (-6 * ft, 0, 5.5 * ft)\n\
             wide_house = box(30 * ft, 40 * ft, 30 * ft, at=(-42 * ft, 10 * ft, 0), name=\"wide_house\")\n\
             wide = sightline(wide_eye, lantern, obstacles=[wide_house])\n\
             assert_true(wide.clear, msg=str(wide))\n\
             assert_true(abs(wide.distance - wide.length) < 1e-6)\n\
             assert_eq(wide.hit, None)\n\
             inside = sightline(eye, (-10 * ft, 60 * ft, -12 * ft), obstacles=[studio])\n\
             assert_eq(inside.blocked_by, \"studio\")\n\
             front = studio.faces(normal=(0, -1, 0)).one()\n\
             fan = sightlines(eye, front, obstacles=[house], samples=400)\n\
             assert_eq(fan.rays, 400)\n\
             assert_true(fan.fraction < 1.0 and fan.fraction > 0.0, msg=str(fan))\n\
             assert_eq(fan.blocked_by[0][0], \"house\")\n\
             open = sightlines(wide_eye, front, obstacles=[wide_house], samples=100)\n\
             assert_true(open.fraction > fan.fraction, msg=str(open))\n\
             pts = sightlines(eye, [lantern, (-10 * ft, 50 * ft, -12 * ft)], obstacles=[house])\n\
             assert_eq(pts.rays, 2)\n\
             named = sightline(eye=eye, to=lantern, obstacles=[house])\n\
             assert_eq(named.blocked_by, \"house\")\n\
             assert_eq(named.hit, narrow.hit)\n\
             half = sightline(eye, to=lantern, obstacles=[house])\n\
             assert_eq(half.distance, narrow.distance)\n\
             fan2 = sightlines(eye=eye, target=front, obstacles=[house], samples=400)\n\
             assert_eq(fan2.fraction, fan.fraction)\n\
             sky = view_cone(eye, az=270, fov=90, el=(0, 30), obstacles=[house], rays=16)\n\
             assert_eq(sky.rays, 256)\n\
             assert_true(sky.blocked > 0 and sky.blocked < 1, msg=str(sky))\n\
             assert_eq(sky.blocked_by[0][0], \"house\")\n\
             none = view_cone(eye, az=90, fov=90, el=(0, 30), obstacles=[house], rays=8)\n\
             assert_eq(none.fraction, 1.0)\n\
             sky2 = view_cone(eye=eye, az=270, fov=90, el=(0, 30), obstacles=[house], rays=16)\n\
             assert_eq(sky2.fraction, sky.fraction)\n\
             everything = view_cone(eye, az=270, fov=90, rays=8)\n\
             assert_true(everything.rays == 64)\n\
             assert_true(everything.blocked >= sky.blocked - 0.2)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    #[test]
    fn refusals() {
        for (src, needle) in [
            ("b = box(1, 1, 1)\nshadow_study(b, on=3, sun=sun(az=0, el=10))", "on= must be"),
            ("b = box(1, 1, 1)\nshadow_study(b, on=XY)", "give sun="),
            ("b = box(1, 1, 1)\nshadow_study(b, on=XY, dates=[\"2026-01-01\"])", "go together"),
            ("b = box(1, 1, 1)\nshadow_study(b, on=XY, dates=[\"2026-01-01\"], times=[\"12:00\"])", "need the site"),
            ("b = box(1, 1, 1)\nshadow_study(b, on=XY, sun=5)", "takes a sun"),
            ("sightline((0, 0, 0), (0, 0, 0))", "coincide"),
            ("sightline(from=(0, 0, 0), to=(1, 1, 1))", "reserved keyword `from`"),
            ("sightline(to=(1, 1, 1))", "reserved word"),
            ("sightline((0, 0, 0), (1, 1, 1), eye=(2, 2, 2))", "twice"),
            ("sightline((0, 0, 0))", "needs a target point"),
            ("b = box(1, 1, 1)\nsightlines(target=b)", "reserved word"),
            ("b = box(1, 1, 1)\nsightlines((0, 0, 5))", "needs a target"),
            ("b = box(1, 1, 1)\nview_cone(az=0)", "reserved word"),
            ("b = box(1, 1, 1)\nsightlines((0, 0, 5), 7)", "target must be"),
            ("b = box(1, 1, 1)\nview_cone((0, 0, 5), az=0, el=(30, 10))", "lo < hi"),
            ("b = box(1, 1, 1)\nview_cone((0, 0, 5), az=0, fov=0)", "fov must be"),
        ] {
            let ctx = run(src);
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{src}: {e:?}");
            assert!(e[0].contains(needle), "{src}: {} does not mention {needle:?}", e[0]);
        }
    }
}
