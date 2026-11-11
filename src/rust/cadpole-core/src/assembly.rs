//! Display assemblies: parts, instances, assemblies, frames and
//! exploded views — the part of assemblies that needs no solver.
//!
//! The model in one paragraph. A **part** is a reusable definition: a set of
//! bodies, built once, that stop rendering on their own. An **instance** is
//! a placed copy of a part: a *pose* (one rigid transform) and nothing else
//! — it shares the part's geometry and, through [`BodyRecord`]'s rigid-copy
//! path, the part's tessellation, so ten instances of a wheel cost one
//! evaluation of the wheel and one tessellation of it, plus ten vertex
//! passes. An **assembly** groups instances (and assemblies) so a view can
//! be exploded as a whole; it never moves geometry. A **frame** is a
//! right-handed coordinate system (origin, z, x) read off a named face,
//! edge or vertex of a posed instance, or given explicitly; `place` maps a
//! frame on one instance onto a frame on another, and `pose` applies a
//! parameter-driven displacement on top. An exploded view is a per-instance
//! *view offset* computed from the assembly's `explode` record when a scene
//! is built — never geometry, so `distance()`, `print_check` and every
//! export see the assembled position whatever the picture shows.
//!
//! Everything here is a pose applied at scene time. The one place an
//! instance's transform reaches the kernel is [`Context::instance`] /
//! [`Context::set_pose`] baking the part's solid into the instance's own
//! [`Body`] (a rigid map of control points, `kernel::transformed`), which is
//! what lets a boolean, a STEP export or `assert_solid` take an instance's
//! body like any other. That bake is not a kernel operation — no boolean runs, nothing is
//! re-tessellated — and [`EvalStats`] counts what does.

use std::collections::BTreeMap;

use crate::context::{Context, Diagnostic, Material, Span};
use crate::fmath;
use crate::geom::{Matrix4, Transform, Vec3, Vector3};
use crate::ids::{BodyId, FeatureId};
use crate::memo::KeyBuilder;
use crate::surface_class::SurfaceClass;
use crate::tessellate::{EdgeInfo, FaceInfo, VertexInfo};
use crate::{CadpoleError, Result};
use cadpole_proto::v1 as pb;

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

/// A right-handed orthonormal frame: an origin, a `z` axis and an `x` axis
/// in the plane square to it (`y = z × x`).
///
/// **How a face gives one.** The origin is the face's centroid and
/// `z` its outward normal, for every face. A face of revolution — cylinder,
/// cone, torus — instead takes the point of its axis nearest the centroid
/// as the origin and the axis as `z`, so a shaft's side and a bore's side
/// hand `place` the axis a `concentric` mate would use; a sphere takes its
/// centre. `x` is the **stable in-plane direction**: the world axis with
/// the smallest component along `z` (ties X before Y before Z), projected
/// into the plane and normalised. It is a rule with no memory — nothing
/// about the mesh, the edge order or an earlier evaluation enters it — so
/// the same face gives the same frame after any edit that keeps its
/// normal, and two faces that look alike give frames that agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: Vec3,
    pub z: Vec3,
    pub x: Vec3,
}

/// Below this length a direction is "zero" and refused.
const TINY: f64 = 1e-9;

/// The world axis with the smallest component along `z`, projected into
/// the plane square to `z` and normalised. `z` must be unit.
pub fn stable_x(z: Vec3) -> Vec3 {
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let mut best = Vec3::X;
    let mut least = f64::MAX;
    for a in axes {
        let c = a.dot(z).abs();
        if c < least - 1e-12 {
            least = c;
            best = a;
        }
    }
    (best - z * best.dot(z)).normalized()
}

impl Frame {
    /// A frame at `origin` with the given `z`, and `x` either as given
    /// (projected square to `z`) or the stable direction when `None`.
    pub fn new(origin: Vec3, z: Vec3, x: Option<Vec3>) -> Result<Frame> {
        if z.length() < TINY {
            return Err(CadpoleError::invalid("a frame's z axis must be a non-zero direction"));
        }
        let z = z.normalized();
        let x = match x {
            Some(x) => {
                let p = x - z * x.dot(z);
                if p.length() < TINY {
                    return Err(CadpoleError::invalid("a frame's x axis must not be parallel to its z axis"));
                }
                p.normalized()
            }
            None => stable_x(z),
        };
        Ok(Frame { origin, z, x })
    }

    /// The world frame: origin at 0, axes X, Y, Z.
    pub fn world() -> Frame {
        Frame { origin: Vec3::ZERO, z: Vec3::Z, x: Vec3::X }
    }

    pub fn y(&self) -> Vec3 {
        self.z.cross(self.x)
    }

    /// The same frame with `z` reversed (`y` follows, `x` stays): a face's
    /// frame turned to look at the face it will sit on.
    pub fn flipped(&self) -> Frame {
        Frame { origin: self.origin, z: -self.z, x: self.x }
    }

    /// The frame moved `d` along its own `z`.
    pub fn offset(&self, d: f64) -> Frame {
        Frame { origin: self.origin + self.z * d, ..*self }
    }

    /// The frame turned `degrees` about its own `z` (right-hand rule).
    pub fn turned(&self, degrees: f64) -> Frame {
        let t = Transform::rotation(self.origin, self.z, degrees.to_radians());
        Frame { origin: self.origin, z: self.z, x: t.direction(self.x) }
    }

    /// The frame carried by `t`.
    pub fn transformed(&self, t: &Transform) -> Frame {
        Frame { origin: t.point(self.origin), z: t.direction(self.z), x: t.direction(self.x) }
    }

    /// The rigid transform that carries the world frame onto this one:
    /// columns `x`, `y`, `z`, `origin`.
    pub fn to_transform(&self) -> Transform {
        let y = self.y();
        let c = |v: Vec3, w: f64| truck_modeling::Vector4::new(v.x, v.y, v.z, w);
        Transform(Matrix4::from_cols(c(self.x, 0.0), c(y, 0.0), c(self.z, 0.0), c(self.origin, 1.0)))
    }

    /// The frame `t` carries the world frame onto: the columns of `t`.
    pub fn from_transform(t: &Transform) -> Frame {
        Frame { origin: t.point(Vec3::ZERO), z: t.direction(Vec3::Z), x: t.direction(Vec3::X) }
    }

    /// The rigid transform that carries `from` onto `to`: `from`'s origin
    /// lands on `to`'s, its `x` on `to`'s `x`, and its `z` on `to`'s `z`
    /// reversed — two faces brought together look at each other — unless
    /// `flush`, which aligns the two `z` axes instead. `offset` then moves
    /// the result along `to`'s `z`: a positive offset lifts it off the
    /// target.
    pub fn placing(from: &Frame, to: &Frame, flush: bool, offset: f64) -> Transform {
        let target = if flush { *to } else { to.flipped() };
        let target = Frame { origin: target.origin + to.z * offset, ..target };
        from.to_transform().inverse_rigid().then(target.to_transform())
    }
}

impl Transform {
    /// The inverse of a rigid transform (rotation and translation only).
    pub fn inverse_rigid(&self) -> Transform {
        let m = &self.0;
        let x = Vec3::new(m.x.x, m.x.y, m.x.z);
        let y = Vec3::new(m.y.x, m.y.y, m.y.z);
        let z = Vec3::new(m.z.x, m.z.y, m.z.z);
        let t = Vec3::new(m.w.x, m.w.y, m.w.z);
        // R^T as columns: column i of R^T is row i of R.
        let c = |v: Vec3, w: f64| truck_modeling::Vector4::new(v.x, v.y, v.z, w);
        let r0 = Vec3::new(x.x, y.x, z.x);
        let r1 = Vec3::new(x.y, y.y, z.y);
        let r2 = Vec3::new(x.z, y.z, z.z);
        let nt = Vec3::new(-x.dot(t), -y.dot(t), -z.dot(t));
        Transform(Matrix4::from_cols(c(r0, 0.0), c(r1, 0.0), c(r2, 0.0), c(nt, 1.0)))
    }

    /// The translation part.
    pub fn translation_part(&self) -> Vec3 {
        Vec3::new(self.0.w.x, self.0.w.y, self.0.w.z)
    }

    /// The rotation part as a unit quaternion `[x, y, z, w]`, which is how
    /// `cadpole.v1.Transform` carries it. Shepperd's method: one square root,
    /// no transcendental.
    pub fn rotation_quaternion(&self) -> [f64; 4] {
        let m = &self.0;
        // R[row][col] from the column vectors.
        let (r00, r10, r20) = (m.x.x, m.x.y, m.x.z);
        let (r01, r11, r21) = (m.y.x, m.y.y, m.y.z);
        let (r02, r12, r22) = (m.z.x, m.z.y, m.z.z);
        let trace = r00 + r11 + r22;
        let q = if trace > 0.0 {
            let s = fmath::sqrt(trace + 1.0) * 2.0;
            [(r21 - r12) / s, (r02 - r20) / s, (r10 - r01) / s, 0.25 * s]
        } else if r00 > r11 && r00 > r22 {
            let s = fmath::sqrt(1.0 + r00 - r11 - r22) * 2.0;
            [0.25 * s, (r01 + r10) / s, (r02 + r20) / s, (r21 - r12) / s]
        } else if r11 > r22 {
            let s = fmath::sqrt(1.0 + r11 - r00 - r22) * 2.0;
            [(r01 + r10) / s, 0.25 * s, (r12 + r21) / s, (r02 - r20) / s]
        } else {
            let s = fmath::sqrt(1.0 + r22 - r00 - r11) * 2.0;
            [(r02 + r20) / s, (r12 + r21) / s, 0.25 * s, (r10 - r01) / s]
        };
        let n = fmath::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
        if n > 0.0 { [q[0] / n, q[1] / n, q[2] / n, q[3] / n] } else { [0.0, 0.0, 0.0, 1.0] }
    }

    /// A rigid transform from a unit quaternion `[x, y, z, w]` and a
    /// translation: the inverse of [`Transform::rotation_quaternion`].
    pub fn from_quaternion(q: [f64; 4], t: Vec3) -> Transform {
        let [x, y, z, w] = q;
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz) = (x * y, x * z, y * z);
        let (wx, wy, wz) = (w * x, w * y, w * z);
        let c = |a: f64, b: f64, d: f64, e: f64| truck_modeling::Vector4::new(a, b, d, e);
        Transform(Matrix4::from_cols(
            c(1.0 - 2.0 * (yy + zz), 2.0 * (xy + wz), 2.0 * (xz - wy), 0.0),
            c(2.0 * (xy - wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz + wx), 0.0),
            c(2.0 * (xz + wy), 2.0 * (yz - wx), 1.0 - 2.0 * (xx + yy), 0.0),
            c(t.x, t.y, t.z, 1.0),
        ))
    }

    /// `cadpole.v1.Transform`: translation plus rotation quaternion.
    pub fn to_proto(&self) -> cadpole_proto::v1::Transform {
        let q = self.rotation_quaternion();
        cadpole_proto::v1::Transform {
            translation: Some(self.translation_part().to_proto()),
            rotation: Some(cadpole_proto::v1::Quat { x: q[0], y: q[1], z: q[2], w: q[3] }),
        }
    }

    pub fn from_proto(p: &cadpole_proto::v1::Transform) -> Transform {
        let t = p.translation.as_ref().map(|v| Vec3::new(v.x, v.y, v.z)).unwrap_or(Vec3::ZERO);
        let q = p.rotation.as_ref().map(|q| [q.x, q.y, q.z, q.w]).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        Transform::from_quaternion(q, t)
    }

    /// The 3MF `<item transform="...">` form: the 3×3 rotation row by row,
    /// then the translation, twelve numbers.
    pub fn to_3mf(&self) -> [f64; 12] {
        let m = &self.0;
        [m.x.x, m.x.y, m.x.z, m.y.x, m.y.y, m.y.z, m.z.x, m.z.y, m.z.z, m.w.x, m.w.y, m.w.z]
    }

    /// True when this is the identity to within `eps`.
    pub fn is_identity(&self, eps: f64) -> bool {
        let m = &self.0;
        let id = Matrix4::from_scale(1.0);
        let cols = [(m.x, id.x), (m.y, id.y), (m.z, id.z), (m.w, id.w)];
        cols.iter().all(|(a, b)| (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps && (a.z - b.z).abs() < eps && (a.w - b.w).abs() < eps)
    }
}

/// The frame of a face (see [`Frame`] for the rule).
pub fn face_frame(f: &FaceInfo) -> Result<Frame> {
    match &f.surface {
        SurfaceClass::Cylinder { axis_point, axis, .. } => {
            let a = axis.normalized();
            let d = f.centroid - *axis_point;
            Frame::new(*axis_point + a * d.dot(a), a, None)
        }
        SurfaceClass::Cone { apex, axis, .. } => {
            let a = axis.normalized();
            let d = f.centroid - *apex;
            Frame::new(*apex + a * d.dot(a), a, None)
        }
        SurfaceClass::Torus { center, axis, .. } => Frame::new(*center, *axis, None),
        SurfaceClass::Sphere { center, .. } => Frame::new(*center, Vec3::Z, None),
        SurfaceClass::Plane { .. } | SurfaceClass::Other => {
            let n = if f.normal.length() < TINY { Vec3::Z } else { f.normal };
            Frame::new(f.centroid, n, None)
        }
    }
}

/// The frame of an edge: a circle or arc gives its centre and normal (the
/// axis of the hole it bounds); any other edge its midpoint, with `z`
/// along the chord from start to end.
pub fn edge_frame(e: &EdgeInfo) -> Result<Frame> {
    if let Some((centre, normal, _)) = crate::select::circle_fit_frame(&e.polyline) {
        return Frame::new(centre, normal, None);
    }
    let chord = e.end - e.start;
    let mid = if e.polyline.is_empty() { (e.start + e.end) * 0.5 } else { e.polyline[e.polyline.len() / 2] };
    if chord.length() < TINY {
        return Frame::new(mid, Vec3::Z, None);
    }
    Frame::new(mid, chord, None)
}

/// The frame of a vertex: its position, world axes.
pub fn vertex_frame(v: &VertexInfo) -> Frame {
    Frame { origin: v.position, z: Vec3::Z, x: Vec3::X }
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstanceId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssemblyId(pub u32);

/// A reusable definition: bodies built once, consumed from the scene.
#[derive(Clone, Debug)]
pub struct PartRecord {
    pub id: PartId,
    pub feature: FeatureId,
    pub name: String,
    pub bodies: Vec<BodyId>,
}

/// A placed copy of a part.
#[derive(Clone, Debug)]
pub struct InstanceRecord {
    pub id: InstanceId,
    pub feature: FeatureId,
    pub name: String,
    pub part: PartId,
    /// Where the part's origin sits in the world.
    pub pose: Transform,
    /// The posed body records, one per part body in the part's order. Each
    /// is a rigid copy of its part body (`derived_from`), so it shares the
    /// part's tessellation and its face names.
    pub bodies: Vec<BodyId>,
    /// A material given to this instance, over the part's.
    pub material: Option<Material>,
    pub assembly: Option<AssemblyId>,
    /// An explicit view offset from `explode(instance, offset)`, added to
    /// whatever the assemblies above it decide.
    pub explode: Vec3,
}

/// A member of an assembly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Member {
    Instance(InstanceId),
    Assembly(AssemblyId),
}

/// Which way `explode` spreads an assembly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Along {
    X,
    Y,
    Z,
    /// Away from the assembly's centre, each member along its own line.
    Radial,
    /// An explicit direction.
    Dir(Vec3),
}

impl Along {
    /// The named members, in the order `docs/parts.md` lists
    /// them — the `along` vocabulary.
    pub const ALL: [Along; 4] = [Along::X, Along::Y, Along::Z, Along::Radial];

    pub fn parse(s: &str) -> Option<Along> {
        Along::ALL.iter().copied().find(|a| a.name() == s)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Along::X => "x",
            Along::Y => "y",
            Along::Z => "z",
            Along::Radial => "radial",
            Along::Dir(_) => "direction",
        }
    }

    /// The direction, unit, or `None` for `Radial`.
    pub fn direction(&self) -> Option<Vec3> {
        match self {
            Along::X => Some(Vec3::X),
            Along::Y => Some(Vec3::Y),
            Along::Z => Some(Vec3::Z),
            Along::Radial => None,
            Along::Dir(d) => Some(d.normalized()),
        }
    }

    /// How the feature record spells it.
    pub fn label(&self) -> String {
        match self {
            Along::Dir(d) => crate::context::fmt_vec(*d),
            other => other.name().to_owned(),
        }
    }
}

/// An assembly's exploded-view record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Explode {
    pub distance: f64,
    pub along: Along,
}

/// A group of instances and assemblies.
#[derive(Clone, Debug)]
pub struct AssemblyRecord {
    pub id: AssemblyId,
    pub feature: FeatureId,
    pub name: String,
    pub members: Vec<Member>,
    pub parent: Option<AssemblyId>,
    pub explode: Option<Explode>,
}

/// What an evaluation cost in the two currencies an instance is meant to
/// save: tessellations of a body (the kernel triangulating a solid) and
/// rigid bakes of one (an instance or a copy re-expressing a solid where it
/// sits). Ten instances of one part are one of the first and ten of the
/// second, and a test says so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvalStats {
    pub tessellations: u32,
    pub bakes: u32,
}

/// Per-member view offsets of one assembly's explode, in member order.
/// `bounds` is each member's posed bounding box (before any explode) and
/// `None` for a member with no geometry, which stays put.
pub fn explode_offsets(bounds: &[Option<(Vec3, Vec3)>], explode: &Explode) -> Vec<Vec3> {
    let n = bounds.len();
    let mut out = vec![Vec3::ZERO; n];
    if n == 0 || explode.distance == 0.0 {
        return out;
    }
    let centre = |b: &(Vec3, Vec3)| (b.0 + b.1) * 0.5;
    match explode.along.direction() {
        None => {
            // Radial: away from the mean of the members' centres, each by
            // the distance; a member at the centre has no direction and
            // stays.
            let cs: Vec<Vec3> = bounds.iter().flatten().map(centre).collect();
            if cs.is_empty() {
                return out;
            }
            let mut mean = Vec3::ZERO;
            for c in &cs {
                mean = mean + *c;
            }
            let mean = mean * (1.0 / cs.len() as f64);
            for (i, b) in bounds.iter().enumerate() {
                if let Some(b) = b {
                    let d = centre(b) - mean;
                    if d.length() > 1e-6 {
                        out[i] = d.normalized() * explode.distance;
                    }
                }
            }
        }
        Some(d) => {
            // Along an axis: members are ranked by their centre along it
            // (lowest first), ties — parts at the same height — by extent,
            // the larger first, because the part that encloses stays and the
            // part inside it comes out. Members with the same centre and
            // extent share a rank and move together: three planets at one
            // height lift as one. Rank k moves k × distance.
            let mut keyed: Vec<(usize, f64, f64)> = bounds
                .iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    let b = b.as_ref()?;
                    let e = b.1 - b.0;
                    let extent = e.x * d.x.abs() + e.y * d.y.abs() + e.z * d.z.abs();
                    Some((i, centre(b).dot(d), extent))
                })
                .collect();
            keyed.sort_by(|a, b| {
                a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(
                    b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal),
                ).then(a.0.cmp(&b.0))
            });
            let mut rank = 0usize;
            let mut prev: Option<(f64, f64)> = None;
            for (i, key, extent) in keyed {
                if let Some((pk, pe)) = prev {
                    if (key - pk).abs() > 1e-6 || (extent - pe).abs() > 1e-6 {
                        rank += 1;
                    }
                }
                prev = Some((key, extent));
                out[i] = d * (explode.distance * rank as f64);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

impl Context {
    pub fn part_record(&self, id: PartId) -> Result<&PartRecord> {
        self.parts.iter().find(|p| p.id == id).ok_or_else(|| CadpoleError::invalid(format!("no part #{}", id.0)))
    }

    pub fn instance_record(&self, id: InstanceId) -> Result<&InstanceRecord> {
        self.instances.iter().find(|i| i.id == id).ok_or_else(|| CadpoleError::invalid(format!("no instance #{}", id.0)))
    }

    fn instance_record_mut(&mut self, id: InstanceId) -> Result<&mut InstanceRecord> {
        self.instances.iter_mut().find(|i| i.id == id).ok_or_else(|| CadpoleError::invalid(format!("no instance #{}", id.0)))
    }

    pub fn assembly_record(&self, id: AssemblyId) -> Result<&AssemblyRecord> {
        self.assemblies.iter().find(|a| a.id == id).ok_or_else(|| CadpoleError::invalid(format!("no assembly #{}", id.0)))
    }

    fn assembly_record_mut(&mut self, id: AssemblyId) -> Result<&mut AssemblyRecord> {
        self.assemblies.iter_mut().find(|a| a.id == id).ok_or_else(|| CadpoleError::invalid(format!("no assembly #{}", id.0)))
    }

    /// `part(bodies, name=)`: a feature of kind `part` whose inputs are the
    /// bodies' features. The bodies are consumed — they stop rendering on
    /// their own and render only where an instance puts them — and keep
    /// their names. A body already inside a part, or one another instance
    /// already copies, is refused.
    pub fn part(&mut self, bodies: &[BodyId], name: Option<String>, span: Option<Span>) -> Result<PartId> {
        if bodies.is_empty() {
            return Err(CadpoleError::invalid("part: needs at least one body"));
        }
        let mut inputs = Vec::new();
        for (k, &b) in bodies.iter().enumerate() {
            let rec = self.body(b)?;
            // Listed twice, the body would be one part body twice: two
            // coincident copies in every instance and every export.
            if bodies[..k].contains(&b) {
                return Err(CadpoleError::invalid(format!("part: body {} is listed twice", rec.id_string())));
            }
            if let Some(p) = self.parts.iter().find(|p| p.bodies.contains(&b)) {
                return Err(CadpoleError::invalid(format!(
                    "part: body {} already belongs to part {:?}",
                    rec.id_string(),
                    p.name
                )));
            }
            if rec.instance.is_some() {
                return Err(CadpoleError::invalid(format!(
                    "part: body {} is an instance's copy; make the part from the original",
                    rec.id_string()
                )));
            }
            if !inputs.contains(&rec.feature) {
                inputs.push(rec.feature.clone());
            }
        }
        let mut args = BTreeMap::new();
        args.insert("bodies".into(), bodies.len().to_string());
        let fid = self.new_feature("part", name.clone(), inputs, args, span)?;
        for &b in bodies {
            self.consume_body(b)?;
        }
        let id = PartId(self.parts.len() as u32);
        let name = name.unwrap_or_else(|| fid.0.clone());
        self.parts.push(PartRecord { id, feature: fid, name, bodies: bodies.to_vec() });
        Ok(id)
    }

    /// `instance(part, at=, rotate=, name=)`: a feature of kind `instance`
    /// with one posed copy of every part body. Each copy is a rigid copy of
    /// its part body: it shares the part's
    /// tessellation and every face name, and `EvalStats::bakes` counts the
    /// one thing it costs.
    pub fn instance(
        &mut self,
        part: PartId,
        pose: Transform,
        name: Option<String>,
        material: Option<Material>,
        mut args: BTreeMap<String, String>,
        span: Option<Span>,
    ) -> Result<InstanceId> {
        let p = self.part_record(part)?.clone();
        args.insert("part".into(), p.name.clone());
        let fid = self.new_feature("instance", name.clone(), vec![p.feature.clone()], args, span)?;
        let id = InstanceId(self.instances.len() as u32);
        let inst_name = name.unwrap_or_else(|| fid.0.clone());
        let mut bodies = Vec::with_capacity(p.bodies.len());
        for &src in &p.bodies {
            let body_name = format!("{}/{}", inst_name, self.body(src)?.name);
            let bid = self.bake_copy(&fid, src, &pose, body_name)?;
            let b = self.body_mut(bid)?;
            b.instance = Some(id);
            if let Some(m) = &material {
                b.material = m.clone();
            }
            bodies.push(bid);
        }
        self.instances.push(InstanceRecord {
            id,
            feature: fid,
            name: inst_name,
            part,
            pose,
            bodies,
            material,
            assembly: None,
            explode: Vec3::ZERO,
        });
        Ok(id)
    }

    /// One posed copy of `src` under `feature`: the part's solid re-expressed
    /// where the instance sits, sharing the source's tessellation.
    fn bake_copy(&mut self, feature: &FeatureId, src: BodyId, pose: &Transform, name: String) -> Result<BodyId> {
        let (moved, material, key, layer, names) = {
            let s = self.body(src)?;
            (s.body.transformed(pose), s.material.clone(), rigid_copy_key(&s.key, pose), s.layer.clone(), s.names.moved(pose))
        };
        self.stats.bakes += 1;
        // Named as the part body moved, so a boolean against an instance
        // body (which re-tessellates it) keeps the part's face names.
        let bid = self.add_body_named(feature, moved, Some(name), names)?;
        let b = self.body_mut(bid)?;
        b.material = material;
        b.layer = layer;
        b.derived_from = Some((src, *pose));
        b.key = key;
        Ok(bid)
    }

    /// Re-pose an instance in place: its body records keep their ids and
    /// names and are re-baked from the part at the new pose, with their
    /// cached meshes dropped (the part's own cache, which they share, stays).
    /// Records a feature of `kind` (`place`, `pose`) with the instance's
    /// feature as its input, so the history says where the instance went.
    pub fn set_pose(
        &mut self,
        inst: InstanceId,
        pose: Transform,
        kind: &str,
        args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<()> {
        let rec = self.instance_record(inst)?.clone();
        let fid = self.new_feature(kind, name, vec![rec.feature.clone()], args, span)?;
        let part = self.part_record(rec.part)?.clone();
        for (&bid, &src) in rec.bodies.iter().zip(&part.bodies) {
            let (moved, key, names) = {
                let s = self.body(src)?;
                (s.body.transformed(&pose), rigid_copy_key(&s.key, &pose), s.names.moved(&pose))
            };
            self.stats.bakes += 1;
            let b = self.body_mut(bid)?;
            b.body = moved;
            b.names = names;
            b.derived_from = Some((src, pose));
            b.cache = None;
            b.quality = None;
            b.key = key;
        }
        // The move is this feature's doing: its outputs are the moved bodies.
        self.feature_mut(&fid)?.outputs = rec.bodies.clone();
        self.instance_record_mut(inst)?.pose = pose;
        Ok(())
    }

    /// Give every body of an instance a material, over the part's.
    pub fn set_instance_material(&mut self, inst: InstanceId, material: Material) -> Result<()> {
        let bodies = self.instance_record(inst)?.bodies.clone();
        for b in bodies {
            self.body_mut(b)?.material = material.clone();
        }
        self.instance_record_mut(inst)?.material = Some(material);
        Ok(())
    }

    /// `assembly(members, name=)`: a feature of kind `assembly`. A member
    /// belongs to one assembly; the first to claim it wins and a second is
    /// an error, because an exploded view has to know whose offset applies.
    pub fn assembly(&mut self, members: &[Member], name: Option<String>, span: Option<Span>) -> Result<AssemblyId> {
        if members.is_empty() {
            return Err(CadpoleError::invalid("assembly: needs at least one instance or assembly"));
        }
        let id = AssemblyId(self.assemblies.len() as u32);
        let mut inputs = Vec::new();
        for (k, m) in members.iter().enumerate() {
            // The one-assembly rule inside one list: a member listed twice
            // would count twice in `.bodies`, a BOM, and a radial explode's
            // centre. The claim loop below only checks other assemblies.
            if members[..k].contains(m) {
                let (kind, name) = match m {
                    Member::Instance(i) => ("instance", self.instance_record(*i)?.name.clone()),
                    Member::Assembly(a) => ("assembly", self.assembly_record(*a)?.name.clone()),
                };
                return Err(CadpoleError::invalid(format!("assembly: {kind} {name:?} is listed twice")));
            }
            match m {
                Member::Instance(i) => {
                    let r = self.instance_record(*i)?;
                    if let Some(a) = r.assembly {
                        return Err(CadpoleError::invalid(format!(
                            "assembly: instance {:?} is already in assembly {:?}",
                            r.name,
                            self.assembly_record(a)?.name
                        )));
                    }
                    inputs.push(r.feature.clone());
                }
                Member::Assembly(a) => {
                    let r = self.assembly_record(*a)?;
                    if let Some(p) = r.parent {
                        return Err(CadpoleError::invalid(format!(
                            "assembly: assembly {:?} is already in assembly {:?}",
                            r.name,
                            self.assembly_record(p)?.name
                        )));
                    }
                    inputs.push(r.feature.clone());
                }
            }
        }
        let mut args = BTreeMap::new();
        args.insert("members".into(), members.len().to_string());
        let fid = self.new_feature("assembly", name.clone(), inputs, args, span)?;
        for m in members {
            match m {
                Member::Instance(i) => self.instance_record_mut(*i)?.assembly = Some(id),
                Member::Assembly(a) => self.assembly_record_mut(*a)?.parent = Some(id),
            }
        }
        let name = name.unwrap_or_else(|| fid.0.clone());
        self.assemblies.push(AssemblyRecord { id, feature: fid, name, members: members.to_vec(), parent: None, explode: None });
        Ok(id)
    }

    /// `explode(assembly, distance, along=)`: recorded on the assembly and
    /// applied when a scene is built; a feature of kind `explode` so the
    /// history shows it. A second call replaces the first.
    pub fn explode_assembly(&mut self, asm: AssemblyId, explode: Explode, span: Option<Span>) -> Result<()> {
        let rec = self.assembly_record(asm)?.clone();
        let mut args = BTreeMap::new();
        args.insert("distance".into(), crate::context::fmt_num(explode.distance));
        args.insert("along".into(), explode.along.label());
        if explode.along.direction().map(|d| d.length() < TINY).unwrap_or(false) {
            return Err(CadpoleError::invalid("explode: along must be a non-zero direction"));
        }
        self.new_feature("explode", None, vec![rec.feature.clone()], args, span)?;
        self.assembly_record_mut(asm)?.explode = Some(explode);
        Ok(())
    }

    /// `explode(instance, offset)`: an explicit view offset for one instance.
    pub fn explode_instance(&mut self, inst: InstanceId, offset: Vec3, span: Option<Span>) -> Result<()> {
        let rec = self.instance_record(inst)?.clone();
        let mut args = BTreeMap::new();
        args.insert("offset".into(), crate::context::fmt_vec(offset));
        self.new_feature("explode", None, vec![rec.feature.clone()], args, span)?;
        self.instance_record_mut(inst)?.explode = offset;
        Ok(())
    }

    /// Every instance under an assembly, depth first in member order.
    pub fn assembly_instances(&self, asm: AssemblyId) -> Result<Vec<InstanceId>> {
        let mut out = Vec::new();
        self.walk_instances(asm, &mut out, 0)?;
        Ok(out)
    }

    fn walk_instances(&self, asm: AssemblyId, out: &mut Vec<InstanceId>, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(CadpoleError::invalid("assembly: nesting deeper than 64 levels"));
        }
        for m in &self.assembly_record(asm)?.members {
            match m {
                Member::Instance(i) => out.push(*i),
                Member::Assembly(a) => self.walk_instances(*a, out, depth + 1)?,
            }
        }
        Ok(())
    }

    /// The posed bodies of every instance under an assembly.
    pub fn assembly_bodies(&self, asm: AssemblyId) -> Result<Vec<BodyId>> {
        let mut out = Vec::new();
        for i in self.assembly_instances(asm)? {
            out.extend(self.instance_record(i)?.bodies.iter().copied());
        }
        Ok(out)
    }

    /// The instance a body belongs to, if it is an instance's copy.
    pub fn instance_of_body(&self, body: BodyId) -> Option<InstanceId> {
        self.body(body).ok().and_then(|b| b.instance)
    }

    /// The posed bounding box of a member, before any explode: the union of
    /// its instances' body meshes. `None` when nothing tessellates.
    fn member_bounds(&mut self, m: Member) -> Result<Option<(Vec3, Vec3)>> {
        let ids: Vec<BodyId> = match m {
            Member::Instance(i) => self.instance_record(i)?.bodies.clone(),
            Member::Assembly(a) => self.assembly_bodies(a)?,
        };
        let mut acc: Option<(Vec3, Vec3)> = None;
        for id in ids {
            if let Some((lo, hi)) = self.body_mesh(id)?.0.bbox() {
                acc = Some(match acc {
                    None => (lo, hi),
                    Some((a, b)) => (a.min(lo), b.max(hi)),
                });
            }
        }
        Ok(acc)
    }

    /// The exploded-view offset of one member of `asm`, from `asm`'s own
    /// explode record (zero when it has none).
    fn member_offset(&mut self, asm: AssemblyId, member: Member) -> Result<Vec3> {
        let rec = self.assembly_record(asm)?.clone();
        let Some(explode) = rec.explode else { return Ok(Vec3::ZERO) };
        let mut bounds = Vec::with_capacity(rec.members.len());
        for m in &rec.members {
            bounds.push(self.member_bounds(*m)?);
        }
        let offsets = explode_offsets(&bounds, &explode);
        Ok(rec.members.iter().position(|m| *m == member).map(|i| offsets[i]).unwrap_or(Vec3::ZERO))
    }

    /// The view offset of an instance: its own explicit offset, plus what
    /// every assembly above it decides for the member that contains it.
    /// This is the one thing an exploded view changes, and only a scene
    /// reads it.
    pub fn view_offset(&mut self, inst: InstanceId) -> Result<Vec3> {
        let rec = self.instance_record(inst)?.clone();
        let mut total = rec.explode;
        let mut member = Member::Instance(inst);
        let mut asm = rec.assembly;
        let mut depth = 0;
        while let Some(a) = asm {
            depth += 1;
            if depth > 64 {
                return Err(CadpoleError::invalid("assembly: nesting deeper than 64 levels"));
            }
            total = total + self.member_offset(a, member)?;
            member = Member::Assembly(a);
            asm = self.assembly_record(a)?.parent;
        }
        Ok(total)
    }

    /// How a body is *displayed*: for an instance's copy, its pose followed
    /// by the exploded-view offset, and the part body whose mesh it shares;
    /// `None` for an ordinary body, which is drawn where it is.
    pub fn display_transform(&mut self, body: BodyId) -> Result<Option<(BodyId, Transform)>> {
        let Some(inst) = self.body(body)?.instance else { return Ok(None) };
        let (src, pose) = self.body(body)?.derived_from.ok_or_else(|| CadpoleError::invalid("an instance body without a source"))?;
        let offset = self.view_offset(inst)?;
        Ok(Some((src, pose.then(Transform::translation(offset)))))
    }

    /// The exploded-view offset a body is displayed with (zero for a body
    /// that is not an instance's, or in no exploded assembly).
    pub fn body_view_offset(&mut self, body: BodyId) -> Result<Vec3> {
        match self.body(body)?.instance {
            Some(i) => self.view_offset(i),
            None => Ok(Vec3::ZERO),
        }
    }

    /// The bounding box of the scene as it is displayed: every visible
    /// body's mesh, an instance body's moved by its exploded-view offset.
    /// What a render frames its camera on; [`Context::bbox`] is the
    /// assembled position.
    pub fn display_bbox(&mut self) -> Option<(Vec3, Vec3)> {
        let ids: Vec<BodyId> = self.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
        let mut acc: Option<(Vec3, Vec3)> = None;
        for id in ids {
            let offset = self.body_view_offset(id).unwrap_or(Vec3::ZERO);
            if let Ok((mesh, _)) = self.body_mesh(id) {
                if let Some((lo, hi)) = mesh.bbox() {
                    let (lo, hi) = (lo + offset, hi + offset);
                    acc = Some(match acc {
                        None => (lo, hi),
                        Some((a, b)) => (a.min(lo), b.max(hi)),
                    });
                }
            }
        }
        acc
    }

    /// The part bodies whose meshes the visible instance bodies share:
    /// what a scene sends once, invisible, beside the instances that point
    /// at it.
    pub fn shared_part_bodies(&self) -> Vec<BodyId> {
        let mut out = Vec::new();
        for b in &self.bodies {
            if b.visible && b.instance.is_some() {
                if let Some((src, _)) = b.derived_from {
                    if !out.contains(&src) {
                        out.push(src);
                    }
                }
            }
        }
        out
    }

    /// The `Model.parts`, `.instances` and `.assemblies` lists. Ids are the
    /// records' feature ids, which are unique by construction; body ids are
    /// `feature#n` as everywhere.
    pub fn assembly_lists(&mut self) -> (Vec<pb::Part>, Vec<pb::Instance>, Vec<pb::Assembly>) {
        let id_string = |ctx: &Context, b: BodyId| ctx.body(b).map(|r| r.id_string()).unwrap_or_default();
        let parts = self
            .parts
            .iter()
            .map(|p| pb::Part {
                id: p.feature.0.clone(),
                name: p.name.clone(),
                feature_id: p.feature.0.clone(),
                body_ids: p.bodies.iter().map(|b| id_string(self, *b)).collect(),
            })
            .collect();
        let inst_ids: Vec<InstanceId> = self.instances.iter().map(|i| i.id).collect();
        let mut instances = Vec::with_capacity(inst_ids.len());
        for id in inst_ids {
            let offset = self.view_offset(id).unwrap_or(Vec3::ZERO);
            let r = self.instance_record(id).unwrap().clone();
            instances.push(pb::Instance {
                id: r.feature.0.clone(),
                name: r.name.clone(),
                feature_id: r.feature.0.clone(),
                part: self.part_record(r.part).map(|p| p.feature.0.clone()).unwrap_or_default(),
                pose: Some(r.pose.to_proto()),
                explode: Some(offset.to_proto()),
                body_ids: r.bodies.iter().map(|b| id_string(self, *b)).collect(),
                assembly: r.assembly.and_then(|a| self.assembly_record(a).ok()).map(|a| a.feature.0.clone()).unwrap_or_default(),
            });
        }
        let assemblies = self
            .assemblies
            .iter()
            .map(|a| pb::Assembly {
                id: a.feature.0.clone(),
                name: a.name.clone(),
                feature_id: a.feature.0.clone(),
                instances: a
                    .members
                    .iter()
                    .filter_map(|m| match m {
                        Member::Instance(i) => self.instance_record(*i).ok().map(|r| r.feature.0.clone()),
                        _ => None,
                    })
                    .collect(),
                assemblies: a
                    .members
                    .iter()
                    .filter_map(|m| match m {
                        Member::Assembly(s) => self.assembly_record(*s).ok().map(|r| r.feature.0.clone()),
                        _ => None,
                    })
                    .collect(),
                explode_distance: a.explode.map(|e| e.distance).unwrap_or(0.0),
                explode_along: a.explode.map(|e| e.along.label()).unwrap_or_default(),
            })
            .collect();
        (parts, instances, assemblies)
    }

    /// An info diagnostic on an assembly's feature.
    pub fn assembly_note(&mut self, asm: AssemblyId, message: impl Into<String>) {
        if let Ok(r) = self.assembly_record(asm) {
            let f = r.feature.clone();
            self.diagnostics.push(Diagnostic::info(message).with_feature(f));
        }
    }
}

/// The memo identity of a rigid copy: the source's key and the
/// matrix, under no feature, exactly as `Context::transform` keys a moved
/// body — so an instance's body is the same body to the memo as a `copy`
/// placed at the same pose, and a posed instance's tessellation entry is
/// keyed by where it is. The pose itself never enters a kernel op.
fn rigid_copy_key(source: &crate::memo::BodyKey, pose: &Transform) -> crate::memo::BodyKey {
    let mut k = KeyBuilder::new("transform");
    k.body(source).put(pose);
    k.finish().body_key(0)
}

/// Convert a cgmath column to a [`Vec3`] (for callers outside this module).
pub fn column(v: Vector3) -> Vec3 {
    Vec3::from_vector(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn the_stable_x_is_the_least_aligned_world_axis_projected() {
        assert_eq!(stable_x(Vec3::Z), Vec3::X);
        assert_eq!(stable_x(-Vec3::Z), Vec3::X);
        assert_eq!(stable_x(Vec3::X), Vec3::Y);
        assert_eq!(stable_x(Vec3::Y), Vec3::X);
        // A tilted normal: x is still square to it and unit.
        let z = Vec3::new(0.0, 0.6, 0.8);
        let x = stable_x(z);
        assert!(x.dot(z).abs() < 1e-12);
        assert!((x.length() - 1.0).abs() < 1e-12);
        assert!(close(x, Vec3::X));
    }

    #[test]
    fn a_frame_round_trips_through_its_transform() {
        let f = Frame::new(Vec3::new(3.0, -2.0, 5.0), Vec3::new(0.0, 1.0, 1.0), Some(Vec3::X)).unwrap();
        let t = f.to_transform();
        let g = Frame::from_transform(&t);
        assert!(close(f.origin, g.origin) && close(f.z, g.z) && close(f.x, g.x));
        let back = t.inverse_rigid().then(t);
        assert!(back.is_identity(1e-12));
    }

    #[test]
    fn placing_brings_two_faces_together_and_flush_aligns_them() {
        // A face looking +z at the origin, and a target face looking +z at
        // z = 10: placing opposes the normals, so the moved frame looks -z.
        let a = Frame::new(Vec3::ZERO, Vec3::Z, None).unwrap();
        let b = Frame::new(Vec3::new(0.0, 0.0, 10.0), Vec3::Z, None).unwrap();
        let t = Frame::placing(&a, &b, false, 0.0);
        let moved = a.transformed(&t);
        assert!(close(moved.origin, b.origin));
        assert!(close(moved.z, -b.z));
        assert!(close(moved.x, b.x));
        let t = Frame::placing(&a, &b, true, 2.5);
        let moved = a.transformed(&t);
        assert!(close(moved.origin, Vec3::new(0.0, 0.0, 12.5)));
        assert!(close(moved.z, b.z));
    }

    #[test]
    fn quaternions_round_trip_every_rotation_branch() {
        for (axis, deg) in [
            (Vec3::Z, 30.0),
            (Vec3::X, 170.0),
            (Vec3::Y, 190.0),
            (Vec3::new(1.0, 1.0, 0.0), 120.0),
            (Vec3::new(0.0, 1.0, 1.0), 200.0),
            (Vec3::new(1.0, 0.0, 1.0), 179.0),
        ] {
            let t = Transform::rotation(Vec3::new(1.0, 2.0, 3.0), axis, (deg as f64).to_radians()).then(Transform::translation(Vec3::new(4.0, 5.0, 6.0)));
            let q = t.rotation_quaternion();
            let back = Transform::from_quaternion(q, t.translation_part());
            for p in [Vec3::ZERO, Vec3::new(1.0, -2.0, 3.0), Vec3::new(-7.0, 0.5, 2.0)] {
                assert!(close(t.point(p), back.point(p)), "{axis:?} {deg}: {:?} vs {:?}", t.point(p), back.point(p));
            }
        }
    }

    #[test]
    fn explode_ranks_along_an_axis_and_ties_go_to_the_larger_part() {
        let b = |z0: f64, z1: f64| Some((Vec3::new(-1.0, -1.0, z0), Vec3::new(1.0, 1.0, z1)));
        // base 0..3, ring 3..15 (centre 9), planet 3..15 (centre 9, same
        // extent → same rank), pin 4..14 (centre 9, smaller → next rank),
        // cap 15..18.
        let bounds = [b(0.0, 3.0), b(3.0, 15.0), b(3.0, 15.0), b(4.0, 14.0), b(15.0, 18.0), None];
        let out = explode_offsets(&bounds, &Explode { distance: 10.0, along: Along::Z });
        assert_eq!(out[0], Vec3::ZERO);
        assert_eq!(out[1], Vec3::new(0.0, 0.0, 10.0));
        assert_eq!(out[2], Vec3::new(0.0, 0.0, 10.0));
        assert_eq!(out[3], Vec3::new(0.0, 0.0, 20.0));
        assert_eq!(out[4], Vec3::new(0.0, 0.0, 30.0));
        assert_eq!(out[5], Vec3::ZERO);
    }

    #[test]
    fn radial_explode_moves_each_member_away_from_the_centre() {
        let at = |x: f64| Some((Vec3::new(x - 1.0, -1.0, 0.0), Vec3::new(x + 1.0, 1.0, 1.0)));
        let bounds = [at(-10.0), at(0.0), at(10.0)];
        let out = explode_offsets(&bounds, &Explode { distance: 5.0, along: Along::Radial });
        assert!(close(out[0], Vec3::new(-5.0, 0.0, 0.0)));
        assert_eq!(out[1], Vec3::ZERO);
        assert!(close(out[2], Vec3::new(5.0, 0.0, 0.0)));
    }

    /// The same member twice in one list is the one-assembly rule broken
    /// inside a single call: `assembly([i, i])` listed one instance twice
    /// (two bodies, a doubled volume, a skewed radial explode) and
    /// `part([b, b])` exported the body twice, coincident. Both are refused
    /// by name, as a member already in another assembly is.
    #[test]
    fn a_member_listed_twice_is_refused() {
        use crate::geom::Plane;
        use crate::sketch::{Loop, P2, Sketch};
        let mut ctx = Context::new();
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let b = ctx.extrude("box", &s, None, 10.0, BTreeMap::new(), Some("b".into()), None).unwrap()[0];
        let e = ctx.part(&[b, b], Some("p2".into()), None).unwrap_err().to_string();
        assert!(e.contains("listed twice"), "{e}");
        assert!(ctx.parts.is_empty(), "nothing recorded");
        let p = ctx.part(&[b], Some("p".into()), None).unwrap();
        let i = ctx.instance(p, Transform::identity(), Some("i".into()), None, BTreeMap::new(), None).unwrap();
        let e = ctx.assembly(&[Member::Instance(i), Member::Instance(i)], Some("a".into()), None).unwrap_err().to_string();
        assert!(e.contains("listed twice") && e.contains("\"i\""), "{e}");
        let a = ctx.assembly(&[Member::Instance(i)], Some("a".into()), None).unwrap();
        let e = ctx.assembly(&[Member::Assembly(a), Member::Assembly(a)], Some("top".into()), None).unwrap_err().to_string();
        assert!(e.contains("listed twice") && e.contains("\"a\""), "{e}");
        assert_eq!(ctx.assembly_record(a).unwrap().members.len(), 1);
    }
}
