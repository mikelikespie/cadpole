//! The evaluation context: everything a script builds up, and the conversion
//! of that state into a `cadpole.v1.Model`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use cadpole_proto::v1 as pb;

use crate::geom::{Plane, Transform, Vec3};
use crate::ids::{BodyId, FeatureId};
use crate::kernel::{self, BoolOp, ChordError, KSolid, KernelChoice, TessellationOptions};
use crate::memo::{BodyKey, KeyBuilder, Memo, MeshOutcome, Note, OpKey, Produced, ProducedBody};
use crate::mesh_kernel::{self, MeshBody};
use crate::naming::{self, NamedFace, Role};
use crate::params::{ParamDecl, ParamValue};
use crate::sketch::Sketch;
use crate::tessellate::{self, Mesh, Topology};
use crate::{CadpoleError, Result};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Span {
    pub path: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl Span {
    pub fn to_proto(&self) -> pb::SourceSpan {
        pb::SourceSpan {
            path: self.path.clone(),
            start_line: self.start_line,
            start_col: self.start_col,
            end_line: self.end_line,
            end_col: self.end_col,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
    pub feature: Option<FeatureId>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Diagnostic {
        Diagnostic { severity: Severity::Error, message: message.into(), span: None, feature: None }
    }
    pub fn warning(message: impl Into<String>) -> Diagnostic {
        Diagnostic { severity: Severity::Warning, message: message.into(), span: None, feature: None }
    }
    /// Something the script did not ask for but should be able to find out:
    /// which kernel ran a boolean, and why.
    pub fn info(message: impl Into<String>) -> Diagnostic {
        Diagnostic { severity: Severity::Info, message: message.into(), span: None, feature: None }
    }
    pub fn with_span(mut self, span: Option<Span>) -> Diagnostic {
        self.span = span;
        self
    }
    pub fn with_feature(mut self, feature: FeatureId) -> Diagnostic {
        self.feature = Some(feature);
        self
    }
    pub fn to_proto(&self) -> pb::Diagnostic {
        pb::Diagnostic {
            severity: match self.severity {
                Severity::Info => pb::Severity::Info,
                Severity::Warning => pb::Severity::Warning,
                Severity::Error => pb::Severity::Error,
            } as i32,
            message: self.message.clone(),
            span: self.span.as_ref().map(Span::to_proto),
            feature_id: self.feature.as_ref().map(|f| f.0.clone()).unwrap_or_default(),
        }
    }
}

/// A document's display convention, as the script declared it with
/// `units(...)`: a unit name the drawing pipeline's parser takes (`mm`,
/// `cm`, `m`, `in`, `ft`, `ft-in`) and, for the imperial ones, the fraction
/// of an inch a dimension rounds to (`16` for sixteenths, `0` for decimal
/// inches). Data only: `cadpole-core` holds it for whoever formats a number and
/// never converts anything itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayUnits {
    pub unit: String,
    pub fraction: u32,
}

#[derive(Clone, Debug)]
pub struct Feature {
    pub id: FeatureId,
    pub kind: String,
    pub name: String,
    pub inputs: Vec<FeatureId>,
    pub outputs: Vec<BodyId>,
    pub args: BTreeMap<String, String>,
    pub span: Option<Span>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Surface appearance for rendering. All values are linear RGB; the
/// rasterizer, the web viewer and the path tracer read the same
/// numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    /// Linear RGB, 0..1.
    pub base_color: [f64; 3],
    /// 0..1; shading clamps to [0.03, 1.0].
    pub roughness: f64,
    /// 0..1; `F0 = mix(dielectric_f0(ior), base_color, metallic)`.
    pub metallic: f64,
    /// Procedural texture name ("", "checker", "stripes", "noise", "wood",
    /// "dots") — see [`crate::render::texture_kind`]. Unknown names are a
    /// script diagnostic; the renderer treats them as none.
    pub texture: String,
    /// Texture repeat length in mm; `<= 0` is treated as 10.
    pub texture_scale: f64,
    /// 0..1; below 0.999 the body is translucent (`Item::is_translucent`).
    pub opacity: f64,
    /// Index of refraction, 1.0..2.5. Drives Fresnel only; the rasterizer
    /// does not refract.
    pub ior: f64,
    /// Linear radiance added after lighting (unbounded, >= 0).
    pub emissive: [f64; 3],

    // --- print-material terms (path tracer only; see docs/design.md).
    // All five default to 0, and 0 means "off", so every material that
    // predates them renders exactly as before.
    /// Fraction of the surface that refracts rather than diffuses, 0..1.
    /// 0 with `opacity < 1` means "derive from opacity": the tracer uses
    /// `1 - opacity`, so a script that only sets `opacity` still refracts.
    pub transmission: f64,
    /// Distance in mm over which the interior attenuates to `base_color`
    /// (Beer-Lambert). 0 = no absorption. This is what makes a 3 mm wall of
    /// translucent PETG read darker and more saturated than a 0.8 mm one.
    pub attenuation_distance: f64,
    /// Mean free path in mm between scattering events inside the solid.
    /// 0 = clear; a few mm gives a milky filament.
    pub scatter_distance: f64,
    /// Henyey-Greenstein asymmetry of that scattering, -1 (back) .. 1 (forward).
    pub scatter_g: f64,
    /// Layer-line anisotropic roughness, 0..1: roughness across the layer
    /// stacking direction (world Z) is raised and along the extrusion
    /// lowered. Faces whose normal is parallel to Z show none of it, because
    /// a top surface has no visible layer lines.
    pub anisotropy: f64,
    /// Edge-break radius in mm for the path tracer's bevel shading: within
    /// this distance of a crease edge the shading normal blends toward the
    /// neighbouring face, so the edge catches the light as the narrow
    /// bright line a deburred machined edge shows. 0 = use the render's
    /// `edge_break_mm` (itself 0 = off by default). No geometry changes.
    pub edge_break: f64,
    /// Groove depth in mm of the machined textures (`"turned"`, `"milled"`);
    /// 0 = the kind's default fraction of the pitch (`texture_scale`).
    pub texture_depth: f64,
    /// Whether the body casts a shadow. `false` makes it guide
    /// geometry — an envelope, a setback plane, a lot line — that the
    /// camera sees and light does not: shadow rays, sun visibility,
    /// indirect bounces and every occlusion query skip it in the path
    /// tracer, and the rasterizer leaves it out of its sun-shadow BVH and
    /// its contact shadow. Its own surface is still lit and shadowed by the
    /// real bodies. Copied with the rest of the material by every feature
    /// that carries one forward (transforms, booleans, copies).
    pub shadow: bool,
}

/// Texture repeat used when `texture_scale` is not positive.
pub const DEFAULT_TEXTURE_SCALE: f64 = 10.0;
/// `opacity` when a proto leaves it unset.
pub const DEFAULT_OPACITY: f64 = 1.0;
/// `ior` when a proto leaves it unset.
pub const DEFAULT_IOR: f64 = 1.5;

impl Default for Material {
    fn default() -> Self {
        Material {
            name: "default".into(),
            base_color: [0.72, 0.74, 0.78],
            roughness: 0.6,
            metallic: 0.0,
            texture: String::new(),
            texture_scale: DEFAULT_TEXTURE_SCALE,
            opacity: DEFAULT_OPACITY,
            ior: DEFAULT_IOR,
            emissive: [0.0; 3],
            transmission: 0.0,
            attenuation_distance: 0.0,
            scatter_distance: 0.0,
            scatter_g: 0.0,
            anisotropy: 0.0,
            edge_break: 0.0,
            texture_depth: 0.0,
            shadow: true,
        }
    }
}

impl Material {
    pub fn to_proto(&self) -> pb::Material {
        let color = |c: [f64; 3]| pb::Color { r: c[0], g: c[1], b: c[2] };
        pb::Material {
            name: self.name.clone(),
            base_color: Some(color(self.base_color)),
            roughness: self.roughness,
            metallic: self.metallic,
            texture: self.texture.clone(),
            texture_scale: self.texture_scale,
            opacity: Some(self.opacity),
            ior: Some(self.ior),
            emissive: Some(color(self.emissive)),
            transmission: self.transmission,
            attenuation_distance: self.attenuation_distance,
            scatter_distance: self.scatter_distance,
            scatter_g: self.scatter_g,
            anisotropy: self.anisotropy,
            edge_break: self.edge_break,
            texture_depth: self.texture_depth,
            // Absent means true, and it is written only when false, so a
            // document that never set the flag serialises as it always did.
            shadow: if self.shadow { None } else { Some(false) },
        }
    }

    /// Inverse of [`Material::to_proto`]. Absent optional fields take their
    /// defaults (opacity 1.0, ior 1.5, emissive black, shadow true); a
    /// `texture_scale` of 0 (proto3 cannot tell it from unset) becomes the
    /// 10 mm default.
    pub fn from_proto(p: &pb::Material) -> Material {
        let color = |c: &Option<pb::Color>, d: [f64; 3]| c.as_ref().map_or(d, |c| [c.r, c.g, c.b]);
        let d = Material::default();
        Material {
            name: p.name.clone(),
            base_color: color(&p.base_color, d.base_color),
            roughness: p.roughness,
            metallic: p.metallic,
            texture: p.texture.clone(),
            texture_scale: if p.texture_scale > 0.0 { p.texture_scale } else { DEFAULT_TEXTURE_SCALE },
            opacity: p.opacity.unwrap_or(DEFAULT_OPACITY),
            ior: p.ior.unwrap_or(DEFAULT_IOR),
            emissive: color(&p.emissive, [0.0; 3]),
            transmission: p.transmission.clamp(0.0, 1.0),
            attenuation_distance: p.attenuation_distance.max(0.0),
            scatter_distance: p.scatter_distance.max(0.0),
            scatter_g: p.scatter_g.clamp(-1.0, 1.0),
            anisotropy: p.anisotropy.clamp(0.0, 1.0),
            edge_break: p.edge_break.max(0.0),
            texture_depth: p.texture_depth.max(0.0),
            shadow: p.shadow.unwrap_or(true),
        }
    }
}

/// One body, in one of the two representations the kernel strategy defines
/// (docs/design.md). `Body.kind` in a script is exactly this choice.
///
/// * `Brep` is exact: faces and edges are surfaces and curves, STEP is
///   possible, and booleans go through truck.
/// * `Mesh` is triangles plus the face groups that name them, produced by the
///   mesh kernel (or by tessellating a B-rep). Booleans are robust and scale
///   with the touching region, at the cost of the document's mesh tolerance.
///
/// Conversion is one-way: `to_mesh` is cheap and loses exactness in the
/// documented direction, `to_brep` is refused.
#[derive(Clone)]
pub enum Body {
    Brep(KSolid),
    Mesh(MeshBody),
}

impl From<KSolid> for Body {
    fn from(s: KSolid) -> Body {
        Body::Brep(s)
    }
}

impl From<MeshBody> for Body {
    fn from(m: MeshBody) -> Body {
        Body::Mesh(m)
    }
}

impl Body {
    /// `"brep"` or `"mesh"` — the value a script sees as `Body.kind`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Body::Brep(_) => "brep",
            Body::Mesh(_) => "mesh",
        }
    }

    /// Face count: B-rep faces, or mesh face groups. The routing budget reads
    /// this, so the two kinds have to answer the same question.
    pub fn face_count(&self) -> usize {
        match self {
            Body::Brep(s) => kernel::face_count(s),
            Body::Mesh((_, t)) => t.faces.len(),
        }
    }

    /// Triangles and named topology. Tessellating a B-rep is the sanctioned
    /// lossy direction: within the document's mesh tolerance, and every face
    /// keeps its name.
    pub fn to_mesh(&self, tol: f64) -> Result<MeshBody> {
        self.to_mesh_reporting(tol).map(|(mb, _)| mb)
    }

    /// [`Body::to_mesh`], and the tolerance a B-rep was tessellated at when
    /// it is not `tol`. That is always `None`: the tessellator never moves
    /// off the requested tolerance, and a face that cannot be meshed at `tol`
    /// is refused by name instead. `None` for a mesh body too, which
    /// is its own mesh.
    pub fn to_mesh_reporting(&self, tol: f64) -> Result<(MeshBody, Option<f64>)> {
        match self {
            Body::Brep(s) => {
                let (mb, used) = tessellate::tessellate_reporting(s, tol)?;
                Ok((mb, (used != tol).then_some(used)))
            }
            Body::Mesh(m) => Ok((m.clone(), None)),
        }
    }

    /// [`Body::to_mesh`] with every bound spelled out, and what came of it:
    /// the tolerance the mesh was actually made at and the chord error
    /// measured on it ([`kernel::ChordError`]). A mesh body is its own mesh:
    /// it comes back as it is, at the tolerance asked for, with no chord
    /// error to measure.
    pub fn to_mesh_with(&self, options: &TessellationOptions) -> Result<(MeshBody, f64, Option<ChordError>)> {
        match self {
            Body::Brep(s) => tessellate::tessellate_reporting_with(s, options),
            Body::Mesh(m) => Ok((m.clone(), options.tolerance, None)),
        }
    }

    /// The exact solid, for the operations that only the B-rep kernel has.
    ///
    /// Refused for a mesh body: rebuilding surfaces from triangles is not a
    /// conversion, it is a fit, and the design allows it only for the all
    /// planar case, which is not implemented. The message says what to do instead.
    pub fn to_brep(&self) -> Result<&KSolid> {
        match self {
            Body::Brep(s) => Ok(s),
            Body::Mesh(_) => Err(CadpoleError::kernel(
                "this is a mesh body and cannot become a B-rep again (STEP export and exact \
                 faces need the B-rep path); keep the part on the B-rep path with kernel=\"brep\"",
            )),
        }
    }

    pub fn transformed(&self, t: &Transform) -> Body {
        match self {
            Body::Brep(s) => Body::Brep(kernel::transformed(s, t)),
            Body::Mesh((m, topo)) => Body::Mesh((m.transformed(t), topo.transformed(t))),
        }
    }
}

/// An override as the kind its param declares, when it spells one: what a
/// front end that parses `name=value` without knowing the declared kind
/// meant. A string param takes any number, bool or vector as its text
/// (`2`, `true`, `1,2,3`; written back from the parsed value, so `007`
/// arrives as `7`); a number, bool or vector param takes text that parses
/// as one. Anything else is returned as is, for the kind check to refuse.
fn coerce_override(v: &ParamValue, declared: &ParamValue) -> ParamValue {
    let num = |t: &str| t.trim().parse::<f64>().ok();
    match (declared, v) {
        (ParamValue::Text(_), ParamValue::Number(n)) => ParamValue::Text(format!("{n}")),
        (ParamValue::Text(_), ParamValue::Bool(b)) => ParamValue::Text(format!("{b}")),
        (ParamValue::Text(_), ParamValue::Vector(p)) => ParamValue::Text(format!("{},{},{}", p.x, p.y, p.z)),
        (ParamValue::Number(_), ParamValue::Text(t)) => num(t).map(ParamValue::Number).unwrap_or_else(|| v.clone()),
        (ParamValue::Bool(_), ParamValue::Text(t)) => match t.trim() {
            "true" | "True" => ParamValue::Bool(true),
            "false" | "False" => ParamValue::Bool(false),
            _ => v.clone(),
        },
        (ParamValue::Vector(_), ParamValue::Text(t)) => {
            let parts: Vec<Option<f64>> = t.trim().trim_matches(|c| c == '(' || c == ')').split(',').map(num).collect();
            match parts[..] {
                [Some(x), Some(y), Some(z)] => ParamValue::Vector(Vec3::new(x, y, z)),
                _ => v.clone(),
            }
        }
        _ => v.clone(),
    }
}

/// Where a body's face names come from — the element map of docs/design.md, in the
/// smallest form that answers "what is this face called?". It is stored on the
/// body rather than derived at tessellation time, because only the feature
/// knows *how* the geometry was made, and that is what the name says.
#[derive(Clone)]
pub enum NameSource {
    /// The feature that made the geometry names it: an extrude names its two
    /// caps and one side per sketch loop-and-segment, a revolve its swept
    /// surfaces and (for a partial turn) its two caps.
    Generator { feature: FeatureId, generator: Arc<naming::Generator> },
    /// A boolean: every result face takes the name of the source surface it
    /// lies on, so a surviving face keeps its name, a split one becomes
    /// `name#k` and one that covers two sources carries both. `feature` only
    /// supplies the `face[n]` fallback for a genuinely new surface.
    Inherited { feature: FeatureId, sources: Arc<naming::SurfaceCatalogue> },
    /// A body some of whose faces were built by hand and are known by the
    /// surface each lies on (a fillet's blends); every other face is
    /// matched back to the named surfaces of the body it was rebuilt from,
    /// as a boolean's result is. The generator's surfaces are tried first:
    /// a blend's samples all lie on its own quadric, a neighbour's do not.
    Rewired { feature: FeatureId, generator: Arc<naming::Generator>, sources: Arc<naming::SurfaceCatalogue> },
    /// The body already carries final names: a mesh body (manifold's
    /// provenance ids propagated them through the boolean) or a rigid copy of
    /// a named body, whose faces are the same faces.
    Intrinsic,
    /// The faces are named by their position in the solid: the `k`-th face
    /// of the B-rep is `names[k]`, a full name plus its aliases. What an
    /// import uses: the file's face order is the closest thing to
    /// construction order an imported body has, and `kernel::tessellate`
    /// keeps a solid's face order, so the k-th `FaceInfo` is the k-th face
    /// of the file's `CLOSED_SHELL`.
    Listed { names: Arc<Vec<(String, Vec<String>)>> },
    /// A rigid (or uniformly scaled) copy of a body named by `source`: a
    /// `translate`, `rotate`, `copy`, pattern member or posed instance.
    /// The copy's faces are the source's faces moved, so each face is named
    /// by mapping its probes back through `to_source` (the inverse of the
    /// move) and asking `source`. Without this a moved B-rep had no names
    /// of its own: a boolean re-tessellates its operands, and every face of
    /// a moved operand came out as `face:N`.
    Moved { source: Arc<NameSource>, to_source: Transform },
}

impl NameSource {
    /// How a copy of a body named by `self`, moved by `t`, is named. A
    /// mesh body's names travel with its triangles and an import's with its
    /// face order, so those need nothing; a copy of a copy maps straight
    /// back to the first source. A transform with no inverse (which no
    /// operation makes) leaves the copy unnamed rather than misnamed.
    pub fn moved(&self, t: &Transform) -> NameSource {
        use truck_modeling::SquareMatrix;
        let Some(inv) = t.0.invert().map(Transform) else {
            return NameSource::Intrinsic;
        };
        match self {
            NameSource::Intrinsic | NameSource::Listed { .. } => self.clone(),
            NameSource::Moved { source, to_source } => {
                NameSource::Moved { source: source.clone(), to_source: inv.then(*to_source) }
            }
            _ => NameSource::Moved { source: Arc::new(self.clone()), to_source: inv },
        }
    }
}

/// What [`Context::select_bodies`] picked, in model order, and how many
/// bodies it left out for being on a hidden layer (so "nothing to export"
/// can say why).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BodySelection {
    pub ids: Vec<BodyId>,
    pub on_hidden_layers: usize,
}

pub struct BodyRecord {
    pub id: BodyId,
    pub feature: FeatureId,
    pub name: String,
    pub body: Body,
    pub visible: bool,
    pub material: Material,
    /// The display layer this body is on: a name `layer(...)`
    /// declared, or empty for the implicit layer. Carried wherever
    /// `material` is carried — a transform, a copy, a pattern, a boolean's
    /// first operand, an instance's posed bodies — and read only by a
    /// picture, never by geometry.
    pub layer: String,
    /// `body.tessellation(...)`: this body's own bounds, when it has any.
    pub tessellation: TessellationOverride,
    /// How [`Context::body_mesh`] names this body's faces.
    pub names: NameSource,
    /// When this body is a rigid copy of another, the source and the
    /// transform: tessellating the source once and moving the result is
    /// orders of magnitude cheaper than tessellating every copy, and keeps
    /// face and edge names identical across the pattern.
    pub(crate) derived_from: Option<(BodyId, Transform)>,
    /// The display tessellation, shared with the memo's entry when one made
    /// it, so a hit costs a pointer.
    pub(crate) cache: Option<Arc<MeshBody>>,
    /// What `cache` was made to; `None` while there is no cache, and for a
    /// mesh body, whose triangles are the geometry.
    pub(crate) quality: Option<MeshQuality>,
    /// The body's identity for the memo: the digest of everything
    /// that made it, or [`BodyKey::Unkeyed`] when its producer said nothing,
    /// in which case nothing downstream of it is served from the memo.
    pub key: BodyKey,
    /// The instance this body is a posed copy for, if any: such a
    /// body is drawn as its part body's mesh under a transform, and an
    /// exploded view moves it without touching its geometry.
    pub instance: Option<crate::assembly::InstanceId>,
}

impl BodyRecord {
    pub fn id_string(&self) -> String {
        format!("{}#{}", self.feature, self.id.0)
    }

    /// `"brep"` or `"mesh"`.
    pub fn kind(&self) -> &'static str {
        self.body.kind_name()
    }

    /// True for a rigid copy of another body (a pattern member, a `copy`, a
    /// `translate`/`rotate`), which shares that body's tessellation.
    pub fn is_copy(&self) -> bool {
        self.derived_from.is_some()
    }

    /// For a rigid copy, the body it copies and the transform that moves
    /// it: an instance body's part body and its pose.
    pub fn source(&self) -> Option<(BodyId, Transform)> {
        self.derived_from
    }

    /// True for a body that was handed its mesh ready-made
    /// ([`Context::add_body_realised`] — a non-uniform `scale` remaps its
    /// source's triangles) rather than tessellated here: that mesh
    /// is kept as given, so no tessellation setting reaches it.
    pub fn has_given_mesh(&self) -> bool {
        self.cache.is_some() && self.quality.is_none()
    }

    /// The exact solid, or an error explaining that this is a mesh body.
    pub fn solid(&self) -> Result<&KSolid> {
        self.body.to_brep()
    }
}

/// How far a face of a boolean's result may sit from a source surface and
/// still be counted as lying on it. Both are chords of their true surfaces,
/// each up to the mesh tolerance away from it, and a triangle centroid adds a
/// little more — so four tolerances, with a floor for very fine tessellations.
pub(crate) fn match_tolerance(tol: f64) -> f64 {
    (tol.max(kernel::DEFAULT_TOLERANCE) * 4.0).max(1e-4)
}

/// Name the faces of an already-tessellated body. Returns how many faces
/// matched nothing (a new surface, which is a diagnostic).
fn name_mesh(mb: &mut MeshBody, names: &NameSource) -> usize {
    match names {
        // Already named: a mesh body, or a copy of one.
        NameSource::Intrinsic => 0,
        // Named by position (an import): the k-th face is names[k]. A face
        // past the end of the list keeps `face:k`, which `apply_names`
        // reports as unmatched, so a body whose face count disagrees with
        // its name list says so rather than misnaming silently.
        NameSource::Listed { names } => {
            let named: Vec<naming::NamedFace> = (0..mb.1.faces.len())
                .map(|i| match names.get(i) {
                    Some((n, aliases)) => {
                        let mut f = naming::NamedFace::new(n.clone(), naming::Role::Kept);
                        for a in aliases {
                            f = f.with_alias(a.clone());
                        }
                        f
                    }
                    None => naming::NamedFace::new(format!("face:{i}"), naming::Role::Unmatched),
                })
                .collect();
            mb.1.apply_names(named)
        }
        NameSource::Moved { source, .. } if matches!(**source, NameSource::Intrinsic | NameSource::Listed { .. }) => {
            name_mesh(mb, source)
        }
        _ => {
            let probes = mb.1.probes(&mb.0);
            let named = name_probes(names, probes);
            mb.1.apply_names(named)
        }
    }
}

/// The names `names` gives faces seen through their probes: every source
/// that names by geometry rather than by position.
fn name_probes(names: &NameSource, probes: Vec<naming::FaceProbe>) -> Vec<NamedFace> {
    match names {
        NameSource::Generator { feature, generator } => naming::name_faces(generator, &feature.0, &probes),
        NameSource::Inherited { feature, sources } => naming::name_inherited(sources, &feature.0, &probes),
        NameSource::Rewired { feature, generator, sources } => {
            let by_surface = naming::name_faces(generator, &feature.0, &probes);
            let inherited = naming::name_inherited(sources, &feature.0, &probes);
            by_surface
                .into_iter()
                .zip(inherited)
                .map(|(g, i)| if g.role == naming::Role::Unmatched { i } else { g })
                .collect()
        }
        NameSource::Moved { source, to_source } => {
            let s = to_source.scale_factor();
            let back = probes
                .into_iter()
                .map(|p| naming::FaceProbe {
                    surface_kind: p.surface_kind,
                    centroid: to_source.point(p.centroid),
                    normal: to_source.direction(p.normal),
                    area: p.area * s * s,
                    samples: p.samples.iter().map(|q| to_source.point(*q)).collect(),
                })
                .collect();
            name_probes(source, back)
        }
        // Positional and intrinsic names never reach here (`name_mesh`
        // handles them); were one to, its faces stay unmatched.
        NameSource::Intrinsic | NameSource::Listed { .. } => (0..probes.len())
            .map(|i| NamedFace::new(format!("face:{i}"), Role::Unmatched))
            .collect(),
    }
}

/// Tessellate a body and name what comes out. The names are a function of the
/// body and its [`NameSource`] alone — the same code runs for a truck solid
/// and for a manifold one, which is why the two kernels agree on names.
/// The mesh kernel's **exit** gate: what a boolean returns is a closed,
/// consistently oriented surface, or it is a diagnostic.
///
/// `to_manifold` gates what goes *in* and nothing gated what came
/// out, so three results that are not solids reached scripts silently, each
/// of them from operands that touch without crossing:
///
/// - `intersect` of two boxes sharing one face. The true answer is a square
///   — two dimensions, not three — and manifold returns it as a zero-volume
///   sheet of four triangles rather than as nothing, which `finish` did not
///   refuse because it only refuses an *empty* result.
/// - `union` of two boxes meeting along one edge, and of two cylinders
///   tangent along a line: both lumps are kept and the shared line ends up
///   on four triangles, which is a surface pinched to nothing there.
///
/// A pinched or zero-volume surface has no inside, so every consumer past
/// this point is working on a guess: a slicer's layer stops being a set of
/// closed loops, the volume is meaningless, and the next boolean's
/// `to_manifold` would refuse the body anyway, one step further from the
/// line of script that caused it. The exact kernel already refuses every one
/// of these contacts by name, so refusing here makes the two
/// kernels agree about what is a body, and `kernel="mesh"` stops being a way
/// to get an answer the B-rep kernel declined to guess at.
///
/// **Crossings are deliberately not judged here.** Only the topology —
/// closed, no non-manifold edge, no edge whose two triangles are wound the
/// same way, positive volume. Triangles that pass through each other are a
/// real defect that `cadpole check` reports, but they are also what the field
/// path's fillet band produces by construction, every slicer
/// resolves them by winding, and the search is the expensive half. This gate
/// costs one weld and one edge map per boolean.
fn check_mesh_result(op: BoolOp, out: &MeshBody) -> Result<()> {
    let opts = crate::solid::SolidOptions { pair_limit: 0, ..Default::default() };
    let report = crate::solid::validate_with(&out.0, &opts);
    if report.oriented() {
        return Ok(());
    }
    // Closed, wound consistently, and enclosing nothing: the operands met in
    // a surface rather than a volume. That is the exact kernel's "only touch"
    // case, and it gets the exact kernel's words.
    if report.closed() && report.flipped.is_empty() {
        return Err(CadpoleError::kernel(format!(
            "mesh {}: the operands touch without enclosing anything between them, so the answer \
             is a surface, not a solid, and Cadpole has no empty body (the mesh kernel returned \
             {} triangles bounding {:.4} mm³). Overlap them by at least 0.5 mm, or drop the {}",
            op.kind_name(),
            report.triangles,
            report.volume,
            op.kind_name()
        )));
    }
    let at = report
        .non_manifold
        .first()
        .or(report.flipped.first())
        .map(|p| format!(" near ({:.3}, {:.3}, {:.3})", p.x, p.y, p.z))
        .unwrap_or_default();
    Err(CadpoleError::kernel(format!(
        "mesh {} produced a surface that is not a solid{at}: {}. The operands touch along an edge \
         or a line rather than crossing, so the result is pinched to nothing there and has no \
         inside a slicer could find. Overlap them by at least 0.5 mm where they touch, keep them \
         as two bodies, or join them with a third that overlaps both",
        op.kind_name(),
        report.problems().join("; ")
    )))
}

/// The tangent classification of two realised operands, as a clause for a
/// routing diagnostic: `. By name: faces `a/top` and `b/bottom` are coplanar
/// within 0.0 µm …`, or nothing when no contact is found within the band.
fn cadpole_tangent_summary(op: BoolOp, am: &MeshBody, bm: &MeshBody, tol: f64, path: crate::tangent::Path) -> String {
    crate::tangent::summary_for_bodies(op, am, bm, tol, path).map(|c| format!(". By name: {c}")).unwrap_or_default()
}

fn realise(body: &Body, names: &NameSource, tol: f64) -> Result<(MeshBody, usize, Option<f64>)> {
    let (mb, unmatched, at, _) = realise_with(body, names, &TessellationOptions::new(tol))?;
    Ok((mb, unmatched, (at != tol).then_some(at)))
}

/// [`realise`] with every bound spelled out, reporting the tolerance the
/// mesh was made at and the chord error measured on it.
fn realise_with(
    body: &Body,
    names: &NameSource,
    options: &TessellationOptions,
) -> Result<(MeshBody, usize, f64, Option<ChordError>)> {
    let (mut mb, at, chord) = body.to_mesh_with(options)?;
    let unmatched = name_mesh(&mut mb, names);
    Ok((mb, unmatched, at, chord))
}

/// [`realise`] without cloning a mesh body that is already a mesh.
fn realise_owned(body: Body, names: &NameSource, tol: f64) -> Result<(MeshBody, usize)> {
    let mut mb = match body {
        Body::Mesh(m) => m,
        Body::Brep(s) => tessellate::tessellate(&s, tol)?,
    };
    let unmatched = name_mesh(&mut mb, names);
    Ok((mb, unmatched))
}

/// The named surfaces a boolean's two operands bring in, as the triangles they
/// cover: this is what a result face is matched against.
fn boolean_catalogue(
    a: &Body,
    a_names: &NameSource,
    b: &Body,
    b_names: &NameSource,
    tol: f64,
) -> Result<naming::SurfaceCatalogue> {
    let mut cat = naming::SurfaceCatalogue::new(match_tolerance(tol));
    for (body, names) in [(a, a_names), (b, b_names)] {
        let ((mesh, topo), _, _) = realise(body, names, tol)?;
        for f in &topo.faces {
            let tris: Vec<[Vec3; 3]> =
                (f.tri_start..f.tri_start + f.tri_count).filter_map(|t| mesh.triangle(t as usize)).collect();
            cat.add_face(
                NamedFace { name: f.name.clone(), aliases: f.aliases.clone(), role: Role::Kept },
                tris,
            );
        }
    }
    cat.finish();
    Ok(cat)
}

/// What [`Context::split`] cuts with.
#[derive(Clone, Copy, Debug)]
pub enum SplitTool {
    /// An infinite plane. The pieces come back in order along its normal.
    Plane(Plane),
    /// Another body's surface: the pieces are `a & b` then `a - b`. The cutter
    /// stays visible, because the same cutter usually cuts several bodies.
    Body(BodyId),
}

/// One side of `plane` as a solid that just covers `corners`.
///
/// Sized from the body's own bounding box rather than from some large
/// constant: truck's boolean is a numeric algorithm and intersecting a 60 mm
/// post with a 900 mm box makes it fail ("this shell is not oriented and
/// closed"), while the same cut against a box a few millimetres bigger than
/// the post succeeds. The box's only face that matters is the one *on* the
/// plane, which is what the piece ends up calling `cut`; the other five sit
/// `margin` clear of the body so no boolean ever sees a coincident face.
fn half_space(plane: &Plane, corners: &[Vec3], sign: f64) -> Result<Body> {
    let (x, y, n) = (plane.x_dir, plane.y_dir(), plane.normal);
    let project = |axis: Vec3| {
        corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
            let h = (*c - plane.origin).dot(axis);
            (lo.min(h), hi.max(h))
        })
    };
    let (u0, u1) = project(x);
    let (v0, v1) = project(y);
    let (h0, h1) = project(n);
    let span = (u1 - u0).max(v1 - v0).max(h1 - h0);
    if !span.is_finite() || span <= 0.0 {
        return Err(CadpoleError::invalid("split: the body has no extent to cut"));
    }
    let margin = (span * 0.05).max(1.0);
    let depth = if sign > 0.0 { h1 + margin } else { -(h0 - margin) };
    if depth <= 0.0 {
        return Err(CadpoleError::invalid("split: the plane does not cross the body"));
    }
    let rect = crate::sketch::Loop::rect(
        crate::sketch::P2::new((u0 + u1) / 2.0, (v0 + v1) / 2.0),
        (u1 - u0) + 2.0 * margin,
        (v1 - v0) + 2.0 * margin,
    );
    let sketch = Sketch::new(*plane).with_loop(rect);
    let mut solids = kernel::extrude(&sketch, depth * sign)?;
    if solids.is_empty() {
        return Err(CadpoleError::kernel("split: could not build the cutting half-space"));
    }
    Ok(Body::Brep(solids.remove(0)))
}

/// An `export(...)` call recorded by the script; the host decides what to do
/// with it (write a file, return bytes over the API).
#[derive(Clone, Debug)]
pub struct ExportSpec {
    pub bodies: Vec<BodyId>,
    pub path: String,
    pub format: String,
    /// `export(..., tolerance=)`: the tessellation tolerance in mm for every
    /// body of this call that has no `body.tessellation()` of its own — the
    /// middle of the three levels of precedence. `None` leaves the document's.
    pub tolerance: Option<f64>,
    /// `export(..., flavor=)`: the 3MF dialect. Only `.3mf` reads it.
    pub flavor: crate::export::Flavor,
    /// `export(..., plate=)`: which plate of a Bambu project this call's
    /// bodies land on, by number or by name. `None` is plate 1.
    pub plate: Option<PlateRef>,
    /// `export(..., name=)`: under the Bambu flavor, the name of the ONE
    /// object this call's bodies become, each body a part of it — how a
    /// print-in-place set stays together. `None` writes one object per
    /// body, named after the body.
    pub name: Option<String>,
    /// `export(..., extruder=)`: the filament slot (1-based) each body of
    /// this call prints in, one entry per body or one for all; empty means
    /// "from the body's colour".
    pub extruders: Vec<u32>,
    /// `export(..., printer=)`: the machine whose bed the Bambu plate grid is
    /// laid out for, by the name `print_check(printer=)` takes; empty means
    /// the 256 mm bed of the X1C/P1S/A1.
    pub printer: String,
    /// `export(..., bed=)`: a custom bed `[x, y, z]` in mm for the Bambu plate
    /// grid, replacing the printer's. `None` keeps the printer's.
    pub bed: Option<[f64; 3]>,
    /// `export(..., template=)`: a Bambu Studio project the user saved,
    /// whose printer, process and filament settings the project carries,
    /// read when the script ran.
    pub template: Option<BambuTemplate>,
}

/// What `export(..., template=)` takes from a saved Bambu Studio project,
/// as data: the asset it came from, and the two parts of it a
/// project is written with. Parsed and checked by the script layer; the
/// export maps the model's filaments onto it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BambuTemplate {
    /// The asset's name, as warnings name the template.
    pub asset: String,
    /// The asset's SHA-256.
    pub sha256: String,
    /// The root model's `Application`, e.g. "BambuStudio-02.06.00.51".
    pub application: String,
    /// `Metadata/project_settings.config`, the JSON text.
    pub settings: String,
}

/// How an `export()` call names its plate: `plate=2` or
/// `plate="Lid"`. Named plates are numbered in order of first appearance
/// after every numbered one; the name shows in Bambu Studio's plate tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlateRef {
    Number(u32),
    Name(String),
}

/// `plate_images(path, size=, background=, backend=)`: ask
/// for a standalone image of every plate of the Bambu project at `path`,
/// plus a contact sheet of them all, when the host runs the script's
/// exports. Data only, like [`ExportSpec`]: the host renders and writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlateImageSpec {
    /// The project's path, exactly as the `export(..., flavor=flavor.bambu)`
    /// calls spell it.
    pub path: String,
    pub width: u32,
    pub height: u32,
    /// `"transparent"`, or a backdrop name: `"studio"`, `"white"`,
    /// `"black"`, `"#rrggbb"`.
    pub background: String,
    /// `"raster"` or `"path"`.
    pub backend: String,
}

impl PlateImageSpec {
    /// The size a plate image is rendered at when nothing says otherwise.
    pub const DEFAULT_SIZE: (u32, u32) = (1600, 1200);
    pub const DEFAULT_BACKGROUND: &'static str = "transparent";
    pub const DEFAULT_BACKEND: &'static str = "raster";
}

/// A body's own tessellation settings, `body.tessellation(tolerance=,
/// angle=)`: the first of the three levels of precedence. Each is `None` until set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TessellationOverride {
    /// Chordal tolerance in mm.
    pub tolerance: Option<f64>,
    /// The normal-turn bound, in degrees; `Some(0.0)` switches it
    /// off for this body.
    pub max_normal_degrees: Option<f64>,
}


/// Which of the three levels a body's tessellation tolerance came from:
/// the body's own `tessellation(tolerance=)`, the `export(..., tolerance=)`
/// call, or the document (`--tolerance`, or the default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToleranceSource {
    Body,
    Call,
    Document,
}

impl ToleranceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ToleranceSource::Body => "body",
            ToleranceSource::Call => "call",
            ToleranceSource::Document => "document",
        }
    }
}

/// What a body's mesh was made to, kept beside the mesh so an export, an
/// inspection or a print check reports it without tessellating again.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshQuality {
    /// The bounds the tessellation was asked for.
    pub options: TessellationOptions,
    /// Where `options.tolerance` came from.
    pub source: ToleranceSource,
    /// The tolerance the mesh was made at: `options.tolerance`.
    pub tessellated_at: f64,
    /// The chord error the kernel measured on the mesh; `None` when no face
    /// had a surface to measure against.
    pub chord: Option<ChordError>,
    /// The generated name of the face `chord` is on (empty when `chord` is
    /// `None`).
    pub chord_face: String,
}

impl MeshQuality {
    /// The largest chord error in mm, when one was measured.
    pub fn chord_error(&self) -> Option<f64> {
        self.chord.map(|c| c.max)
    }
}

/// Something selected in the UI, as seen by a script via `selection()`.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityRef {
    pub feature_id: String,
    pub body_id: String,
    pub kind: String,
    pub topo_name: String,
}

pub struct Context {
    /// Tessellation tolerance in mm: how finely surfaces are triangulated for
    /// display and export.
    pub tolerance: f64,
    /// The document's display convention: the unit its readouts are
    /// written in — `cadpole inspect`, `distance()`, the web UI, the drawings'
    /// default — and the fraction of an inch an imperial length rounds to.
    /// `None` is millimetres. Set by the script's `units(...)`; the kernel
    /// never reads it: every length in this context is a millimetre.
    pub display_units: Option<DisplayUnits>,
    /// Geometric tolerance handed to the kernel's boolean operations, in mm.
    /// Deliberately separate from `tolerance`: the kernel's cost grows sharply
    /// as this shrinks (a bracket's three booleans cost 1.7 s at 0.05 and
    /// 4.3 s at 0.01), while the resulting solid's volume is unchanged to
    /// within 0.1%. Exporting a fine mesh should not make booleans slow.
    pub boolean_tolerance: f64,
    pub features: Vec<Feature>,
    pub bodies: Vec<BodyRecord>,
    pub params: Vec<(ParamDecl, ParamValue)>,
    pub overrides: HashMap<String, ParamValue>,
    /// The animations the script declared, in declaration order:
    /// data only, read by `Animate`. An ordinary evaluation records them
    /// and renders nothing.
    pub animations: Vec<crate::animation::AnimationDecl>,
    /// The display layers the script declared with `layer(...)`,
    /// in declaration order. Metadata: a viewer's switches, never geometry.
    pub layers: Vec<crate::layers::LayerDecl>,
    /// What `render_defaults(sun=, north=, camera=)` declared: the
    /// sun, where north is and the camera a render of this document takes
    /// unless the request says otherwise. Data only; nothing here renders.
    pub render_defaults: Option<RenderDefaults>,
    pub diagnostics: Vec<Diagnostic>,
    pub exports: Vec<ExportSpec>,
    /// The script's `plate_images(...)` declarations, one per Bambu project
    /// path, in declaration order.
    pub plate_images: Vec<PlateImageSpec>,
    pub selection: Vec<EntityRef>,
    /// Combined face count above which `kernel="auto"` skips the B-rep kernel:
    /// [`kernel::BREP_FACE_BUDGET`], unless a test lowers it. The same number
    /// for the whole document — routing is a function of the document, never
    /// of how long an earlier boolean happened to take.
    pub brep_face_budget: usize,
    /// Which of manifold's boolean engines the mesh kernel runs on: `None`
    /// is the policy (exact first, robust only when exact refuses) unless
    /// `CADPOLE_MESH_ENGINE` overrides it. A field so a test can force the
    /// robust engine for one context without a process-global variable.
    pub mesh_engine: Option<mesh_kernel::Engine>,
    /// The kernel-op memo this evaluation reads and fills: the
    /// session's, the wasm engine's, or none for a one-shot `cadpole eval`.
    pub memo: Option<Arc<Memo>>,
    /// The drag preview: while a parameter is being dragged, a fuse
    /// or boolean whose inputs changed is not run; the operands are shown
    /// unfused and the real op runs on the evaluation after the drag.
    pub preview: bool,
    /// What the memo did for this evaluation, by op and feature.
    pub memo_counts: MemoCounts,
    /// The ops being recorded for the memo, innermost last: every note and
    /// feature arg an op emits goes to each level, so an outer op's entry
    /// replays what its inner ops said.
    recording: Vec<Recorder>,
    /// How many memoised ops are running right now: 0 at the boundary,
    /// more inside an op that runs another (a pattern tessellating its
    /// source). Only boundary ops are counted.
    op_depth: usize,
    /// Display assemblies: the parts, the instances that place
    /// them and the assemblies that group them. Empty for a document that
    /// makes none, and nothing downstream then changes.
    pub parts: Vec<crate::assembly::PartRecord>,
    pub instances: Vec<crate::assembly::InstanceRecord>,
    pub assemblies: Vec<crate::assembly::AssemblyRecord>,
    /// What this evaluation cost in tessellations and rigid bakes
    /// ([`crate::assembly::EvalStats`]): what the instance-sharing test
    /// counts.
    pub stats: crate::assembly::EvalStats,
    /// What each `text()` call set, by its feature id: the string,
    /// the face and the frame it was laid out in. A drawing writes a label
    /// extruded from one as text rather than as the outlines of its
    /// glyphs. Data only; nothing in the kernel reads it.
    pub text_labels: BTreeMap<String, TextLabel>,
    counters: HashMap<String, u32>,
    next_body: u32,
}

/// One `text()` call's string and layout: enough to set it again,
/// as text, exactly where its glyph outlines lie.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLabel {
    pub text: String,
    /// `"builtin"`, an asset's name or `"system:<family>"`.
    pub font: String,
    /// The em size, and the cap height it gave, in mm.
    pub size: f64,
    pub cap_height: f64,
    /// `"left"`, `"center"` or `"right"`.
    pub align: String,
    /// `"alphabetic"`, `"middle"` or `"top"`.
    pub baseline: String,
    pub weight: f64,
    pub line_height: f64,
    /// True when nothing but the face, the size, the weight and the
    /// alignment shaped it -- no letter spacing, no feature list, no axes --
    /// so a PDF that sets the same string in the same face lands glyph for
    /// glyph on the outlines.
    pub plain: bool,
    /// The sketch plane and the anchor in it: the string's alignment point
    /// on its first baseline is `plane.to_world(at.x, at.y)`.
    pub plane: Plane,
    pub at: crate::sketch::P2,
}

/// A free camera a document asks for: where the eye stands and
/// what it looks at, in model space, the up vector and the vertical field
/// of view in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraSpec {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_degrees: f64,
}

/// What `render_defaults(...)` declared. Every field is optional;
/// a request's own value wins over each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderDefaults {
    pub sun: Option<crate::sun::SunSpec>,
    /// Degrees clockwise from `+y` to north, seen from above.
    pub north_degrees: Option<f64>,
    pub camera: Option<CameraSpec>,
}

impl RenderDefaults {
    pub fn to_proto(&self) -> pb::RenderDefaults {
        pb::RenderDefaults {
            sun: self.sun.as_ref().map(|s| pb::SunInfo { azimuth: s.azimuth, elevation: s.elevation, label: s.label.clone() }),
            north_degrees: self.north_degrees.unwrap_or(0.0),
            eye: self.camera.map(|c| c.eye.to_proto()),
            target: self.camera.map(|c| c.target.to_proto()),
            up: self.camera.map(|c| c.up.to_proto()),
            fov_degrees: self.camera.map(|c| c.fov_degrees).unwrap_or(0.0),
        }
    }
}

/// One kernel op this evaluation ran, hit in the memo, or deferred: which
/// feature and which op.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpRecord {
    pub feature: String,
    pub op: String,
}

/// What the memo did for one evaluation — counts, not times: a fact
/// about what the code did, which reads the same on an idle machine and a
/// loaded one.
#[derive(Clone, Debug, Default)]
pub struct MemoCounts {
    /// Ops that ran the kernel.
    pub runs: Vec<OpRecord>,
    /// Ops served from the memo.
    pub hits: Vec<OpRecord>,
    /// Ops the drag preview skipped.
    pub deferred: Vec<OpRecord>,
}

impl MemoCounts {
    /// The features whose ops ran, hit or were deferred, each once, in order.
    fn features(records: &[OpRecord]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in records {
            if !out.contains(&r.feature) {
                out.push(r.feature.clone());
            }
        }
        out
    }

    pub fn run_features(&self) -> Vec<String> {
        Self::features(&self.runs)
    }

    pub fn hit_features(&self) -> Vec<String> {
        Self::features(&self.hits)
    }

    pub fn deferred_features(&self) -> Vec<String> {
        Self::features(&self.deferred)
    }
}

#[derive(Default)]
struct Recorder {
    notes: Vec<Note>,
    args: Vec<(FeatureId, String, String)>,
}

impl Default for Context {
    fn default() -> Self {
        Context::new()
    }
}

impl Context {
    pub fn new() -> Context {
        // Kernel notes are scoped to an evaluation: anything a kernel call
        // outside a context left on this thread is not this document's.
        kernel::notes::drain();
        Context {
            tolerance: kernel::DEFAULT_TOLERANCE,
            display_units: None,
            boolean_tolerance: kernel::DEFAULT_BOOLEAN_TOLERANCE,
            features: Vec::new(),
            bodies: Vec::new(),
            params: Vec::new(),
            overrides: HashMap::new(),
            animations: Vec::new(),
            layers: Vec::new(),
            render_defaults: None,
            diagnostics: Vec::new(),
            exports: Vec::new(),
            plate_images: Vec::new(),
            selection: Vec::new(),
            brep_face_budget: kernel::BREP_FACE_BUDGET,
            mesh_engine: None,
            memo: None,
            preview: false,
            memo_counts: MemoCounts::default(),
            recording: Vec::new(),
            op_depth: 0,
            parts: Vec::new(),
            instances: Vec::new(),
            assemblies: Vec::new(),
            stats: crate::assembly::EvalStats::default(),
            text_labels: BTreeMap::new(),
            counters: HashMap::new(),
            next_body: 0,
        }
    }

    /// Attach the memo this evaluation reads and fills, and say
    /// whether it is a drag preview. Called once, before the script runs.
    pub fn use_memo(&mut self, memo: Option<Arc<Memo>>, preview: bool) {
        if let Some(m) = &memo {
            m.attach();
        }
        self.memo = memo;
        self.preview = preview;
    }

    /// Attach every note the kernel pushed since the last drain
    /// ([`kernel::notes`]) to `feature` as an info diagnostic with its span
    /// — the same shape as "the B-rep kernel could not union ...; retrying
    /// on the mesh kernel", and for the same reason: the kernel decided
    /// something the script did not ask about, and the user should be able
    /// to find out without a rebuild. Called after every feature whose
    /// evaluation reaches the mesh kernel or the tessellator, on the error
    /// path too, so a note is never left over for the next feature to claim.
    pub(crate) fn absorb_kernel_notes(&mut self, feature: &FeatureId, span: Option<Span>) {
        for note in kernel::notes::drain() {
            self.info(note, feature, span.clone());
        }
    }

    // ---- params

    /// Declare a parameter and return its effective value (override or default).
    pub fn declare_param(&mut self, decl: ParamDecl) -> Result<ParamValue> {
        if self.params.iter().any(|(d, _)| d.name == decl.name) {
            return Err(CadpoleError::invalid(format!("param {:?} declared twice", decl.name)));
        }
        // A front end that does not know the declared kind (the CLI reads
        // `-p s=2` as a number, `-p s=true` as a bool) hands over a value
        // that means the declared kind; take it as that kind.
        let given = self.overrides.get(&decl.name).map(|v| coerce_override(v, &decl.default));
        let value = match &given {
            Some(v) if v.kind_name() == decl.default.kind_name() => v.clone(),
            Some(v) => {
                self.diagnostics.push(
                    Diagnostic::warning(format!(
                        "override for param {:?} is a {} but the param is a {}; using default",
                        decl.name,
                        v.kind_name(),
                        decl.default.kind_name()
                    ))
                    .with_span(decl.span.clone()),
                );
                decl.default.clone()
            }
            None => decl.default.clone(),
        };
        // An override can carry NaN or infinity from outside the script —
        // `-p w=nan` parses as a number, and a proto double holds either — and
        // a script cannot defend itself against a value it was handed. The
        // builtins refuse a non-finite number where it would become geometry;
        // this stops it one step earlier, on the same warn-and-use-the-default
        // rule as a type mismatch.
        let finite = |v: &ParamValue| match v {
            ParamValue::Number(n) => n.is_finite(),
            ParamValue::Vector(v) => v.x.is_finite() && v.y.is_finite() && v.z.is_finite(),
            _ => true,
        };
        let value = if finite(&value) {
            value
        } else {
            self.diagnostics.push(
                Diagnostic::warning(format!(
                    "override for param {:?} is {value:?}, which is not a finite number; using the default",
                    decl.name
                ))
                .with_span(decl.span.clone()),
            );
            decl.default.clone()
        };
        // `choices` is the allowed set, not a hint for the UI: a script that
        // branches on the value (`if finish == "matte"`) silently took the
        // wrong branch when an override was outside it. The default was
        // already checked where the param is declared; this is the same rule
        // for the override, and it lives here because every front end reaches
        // it — `-p`, a document's param_overrides, SetParams, and a wasm or
        // FFI host — exactly as the min/max clamp does.
        let value = match &value {
            ParamValue::Text(t) if !decl.choices.is_empty() && !decl.choices.contains(t) => {
                let default = match &decl.default {
                    ParamValue::Text(d) => d.clone(),
                    other => format!("{other:?}"),
                };
                self.diagnostics.push(
                    Diagnostic::warning(format!(
                        "override for param {:?} is {t:?}, which is not one of choices {:?}; using the default {default:?}",
                        decl.name, decl.choices
                    ))
                    .with_span(decl.span.clone()),
                );
                decl.default.clone()
            }
            _ => value,
        };
        let value = match (&value, decl.min, decl.max) {
            (ParamValue::Number(n), min, max) => {
                let mut n = *n;
                if let Some(m) = min {
                    n = n.max(m);
                }
                if let Some(m) = max {
                    n = n.min(m);
                }
                ParamValue::Number(n)
            }
            _ => value,
        };
        // An override that was refused above (wrong kind, not finite, not
        // one of `choices`; each said "using the default") is not in force,
        // so it leaves the map every front end reads `overridden` from: a
        // refused value labelled "(override)" says the opposite of what
        // happened. A clamped one is still the override, clamped.
        if let Some(v) = given {
            let refused = !finite(&v)
                || v.kind_name() != decl.default.kind_name()
                || matches!(&v, ParamValue::Text(t) if !decl.choices.is_empty() && !decl.choices.contains(t));
            if refused {
                self.overrides.remove(&decl.name);
            }
        }
        self.params.push((decl, value.clone()));
        Ok(value)
    }

    // ---- features

    /// Allocate a feature id: the explicit name if given (must be unique),
    /// else `<kind>_<n>` numbered per kind.
    pub fn new_feature(
        &mut self,
        kind: &str,
        name: Option<String>,
        inputs: Vec<FeatureId>,
        args: BTreeMap<String, String>,
        span: Option<Span>,
    ) -> Result<FeatureId> {
        let id = match &name {
            Some(n) => {
                if n.is_empty() || n.contains(':') || n.contains('#') || n.contains('/') {
                    return Err(CadpoleError::invalid(format!(
                        "feature name {n:?} must be non-empty and not contain ':', '#' or '/'"
                    )));
                }
                if self.features.iter().any(|f| f.id.0 == *n) {
                    return Err(CadpoleError::invalid(format!("feature name {n:?} used twice")));
                }
                FeatureId(n.clone())
            }
            None => {
                let c = self.counters.entry(kind.to_owned()).or_insert(0);
                *c += 1;
                let mut candidate = format!("{kind}_{c}");
                while self.features.iter().any(|f| f.id.0 == candidate) {
                    *c += 1;
                    candidate = format!("{kind}_{c}");
                }
                FeatureId(candidate)
            }
        };
        self.features.push(Feature {
            id: id.clone(),
            kind: kind.to_owned(),
            name: name.unwrap_or_default(),
            inputs,
            outputs: Vec::new(),
            args,
            span,
            diagnostics: Vec::new(),
        });
        Ok(id)
    }

    /// The `text()` label a body is one glyph (or one merged run of
    /// glyphs) of, with the feature that made the body, which every glyph
    /// of the label shares: a body that a feature made directly
    /// from a text sketch -- `extrude(text(...), h)` -- and nothing since.
    /// A body moved, copied or cut afterwards is another feature's output
    /// and is not a label.
    pub fn text_label_of(&self, body: &BodyRecord) -> Option<(&FeatureId, &TextLabel)> {
        if self.text_labels.is_empty() || body.instance.is_some() {
            return None;
        }
        let f = self.feature(&body.feature).ok()?;
        if f.kind != "extrude" {
            return None;
        }
        let source = f.inputs.first()?;
        self.text_labels.get(&source.0).map(|l| (&f.id, l))
    }

    pub fn feature(&self, id: &FeatureId) -> Result<&Feature> {
        self.features.iter().find(|f| &f.id == id).ok_or_else(|| CadpoleError::UnknownFeature(id.0.clone()))
    }

    pub fn feature_mut(&mut self, id: &FeatureId) -> Result<&mut Feature> {
        self.features
            .iter_mut()
            .find(|f| &f.id == id)
            .ok_or_else(|| CadpoleError::UnknownFeature(id.0.clone()))
    }

    pub fn set_feature_arg(&mut self, id: &FeatureId, key: &str, value: impl ToString) {
        let value = value.to_string();
        if let Ok(f) = self.feature_mut(id) {
            f.args.insert(key.to_owned(), value.clone());
        }
        for r in &mut self.recording {
            r.args.push((id.clone(), key.to_owned(), value.clone()));
        }
    }

    // ---- bodies

    pub fn add_body(&mut self, feature: &FeatureId, body: impl Into<Body>, name: Option<String>) -> Result<BodyId> {
        self.add_body_named(feature, body, name, NameSource::Intrinsic)
    }

    /// Add a body and say where its face names come from.
    pub fn add_body_named(
        &mut self,
        feature: &FeatureId,
        body: impl Into<Body>,
        name: Option<String>,
        names: NameSource,
    ) -> Result<BodyId> {
        let id = BodyId(self.next_body);
        self.next_body += 1;
        let name = name.unwrap_or_else(|| feature.0.clone());
        self.bodies.push(BodyRecord {
            id,
            feature: feature.clone(),
            name,
            body: body.into(),
            visible: true,
            material: Material::default(),
            layer: String::new(),
            names,
            tessellation: TessellationOverride::default(),
            derived_from: None,
            cache: None,
            quality: None,
            key: BodyKey::Unkeyed,
            instance: None,
        });
        self.feature_mut(feature)?.outputs.push(id);
        Ok(id)
    }

    /// [`Context::add_body_named`] with the body's identity for the memo:
    /// what a producer that keyed its inputs hands back.
    pub fn add_body_keyed(
        &mut self,
        feature: &FeatureId,
        body: impl Into<Body>,
        name: Option<String>,
        names: NameSource,
        key: BodyKey,
    ) -> Result<BodyId> {
        let id = self.add_body_named(feature, body, name, names)?;
        self.body_mut(id)?.key = key;
        Ok(id)
    }

    // ---- display layers

    /// Declare a display layer. Declaring the same name again with the same
    /// settings is the same layer (a library and the document that loads it
    /// may both declare `"fixture"`); with different settings it is an
    /// error, because two declarations that disagree would leave the panel
    /// to pick one. Returns the layer's position in declaration order.
    pub fn declare_layer(&mut self, decl: crate::layers::LayerDecl) -> Result<usize> {
        if decl.name.is_empty() {
            return Err(CadpoleError::invalid("layer: the name is empty"));
        }
        if !(0.0..=1.0).contains(&decl.opacity) || decl.opacity.is_nan() {
            return Err(CadpoleError::invalid(format!(
                "layer {:?}: opacity is between 0 and 1, got {}",
                decl.name,
                fmt_num(decl.opacity)
            )));
        }
        if let Some(i) = self.layers.iter().position(|l| l.name == decl.name) {
            if self.layers[i].same_settings(&decl) {
                return Ok(i);
            }
            let first = &self.layers[i];
            let at = first
                .span
                .as_ref()
                .map(|s| format!(" (first declared at line {})", s.start_line))
                .unwrap_or_default();
            return Err(CadpoleError::invalid(format!(
                "layer {:?} is already declared with different settings{at}: a layer is declared once, \
                 and a second declaration must repeat the first's doc, visible, opacity and color",
                decl.name
            )));
        }
        self.layers.push(decl);
        Ok(self.layers.len() - 1)
    }

    /// Put a body on a declared layer (`layer=` on a builtin, `paint(layer=)`,
    /// `Layer.add`). The empty name takes it off every layer. A name nothing
    /// declared is an error, so a typo cannot make a layer the panel does
    /// not list.
    pub fn set_layer(&mut self, id: BodyId, layer: &str) -> Result<()> {
        if !layer.is_empty() && !self.layers.iter().any(|l| l.name == layer) {
            let known: Vec<&str> = self.layers.iter().map(|l| l.name.as_str()).collect();
            return Err(CadpoleError::invalid(if known.is_empty() {
                format!("layer {layer:?} is not declared; declare it first with layer({layer:?})")
            } else {
                format!("layer {layer:?} is not declared; the declared layers are {} (declare it first with layer({layer:?}))", known.join(", "))
            }));
        }
        self.body_mut(id)?.layer = layer.to_owned();
        Ok(())
    }

    /// The bodies a whole-document operation takes: `cadpole
    /// export`, `cadpole check` and their rpcs.
    ///
    /// With neither `refs` nor `layers`, every body that is not consumed and
    /// not on a layer the script declares hidden, unless `include_hidden`
    /// -- what a viewer shows. With either, exactly the bodies they name,
    /// whatever their layer: a body by its id (`feature#n`) or its name, and
    /// every body on a named layer (`other` is the implicit layer, as `cadpole
    /// render --layer` spells it). The two add up rather than narrow, so
    /// `--layer walls --body slab#4` is the walls and the slab. A consumed
    /// body is never taken: a boolean made something else of it.
    ///
    /// A name that picks nothing is an error rather than an empty selection,
    /// so a typo never checks or exports less than was asked for: an unknown
    /// layer lists the declared ones, an unknown body says how bodies are
    /// named, and a body that only matches consumed bodies says so.
    pub fn select_bodies(&self, refs: &[String], layers: &[String], include_hidden: bool) -> Result<BodySelection> {
        let mut wanted: Vec<String> = Vec::with_capacity(layers.len());
        for l in layers {
            wanted.push(crate::layers::resolve_layer(l, &self.layers).map_err(CadpoleError::invalid)?);
        }
        let refs: Vec<&str> = refs.iter().map(|r| r.trim()).filter(|r| !r.is_empty()).collect();
        let narrowed = !refs.is_empty() || !wanted.is_empty();
        // Per ref: (matched any body, matched a body still in the model).
        let mut hits = vec![(false, false); refs.len()];
        let mut out = BodySelection::default();
        for b in &self.bodies {
            let id = b.id_string();
            let mut by_ref = false;
            for (k, r) in refs.iter().enumerate() {
                if *r == id || *r == b.name {
                    hits[k].0 = true;
                    hits[k].1 |= b.visible;
                    by_ref = true;
                }
            }
            if !b.visible {
                continue;
            }
            if narrowed {
                if by_ref || wanted.contains(&b.layer) {
                    out.ids.push(b.id);
                }
                continue;
            }
            if !include_hidden && !crate::layers::shown(&b.layer, &self.layers) {
                out.on_hidden_layers += 1;
                continue;
            }
            out.ids.push(b.id);
        }
        for (r, (any, live)) in refs.iter().zip(&hits) {
            if !any {
                return Err(CadpoleError::invalid(format!(
                    "no body {r:?}: name a body by its id (`feature#n`, as `cadpole eval` lists them) or by its name"
                )));
            }
            if !live {
                return Err(CadpoleError::invalid(format!(
                    "body {r:?} is not in the model: a boolean or a split consumed it, so name the body that \
                     operation made instead"
                )));
            }
        }
        Ok(out)
    }

    /// The body's identity for the memo.
    pub fn body_key(&self, id: BodyId) -> Result<BodyKey> {
        Ok(self.body(id)?.key)
    }

    /// Give a body its identity after the fact: what a producer outside this
    /// crate does when it keys a body by its content (a field's contour) —
    /// only worth the hashing when a memo is attached, which
    /// [`Context::wants_keys`] says.
    pub fn set_body_key(&mut self, id: BodyId, key: BodyKey) -> Result<()> {
        self.body_mut(id)?.key = key;
        Ok(())
    }

    /// True when a memo is attached, so a producer whose key costs something
    /// (hashing a mesh) knows whether it is worth computing.
    pub fn wants_keys(&self) -> bool {
        self.memo.is_some()
    }

    /// Add a body whose tessellation is already in hand: a mirrored or
    /// scaled copy, whose mesh is the source's cached, named mesh under the
    /// same map. `names` says how the body is named *again* when a
    /// later boolean tessellates it afresh — the catalogue of that mesh for
    /// a B-rep, `Intrinsic` for a mesh body, which is its own mesh.
    pub(crate) fn add_body_realised(
        &mut self,
        feature: &FeatureId,
        body: impl Into<Body>,
        name: Option<String>,
        mesh: MeshBody,
        names: NameSource,
    ) -> Result<BodyId> {
        let id = self.add_body_named(feature, body, name, names)?;
        self.body_mut(id)?.cache = Some(Arc::new(mesh));
        Ok(id)
    }

    pub fn body(&self, id: BodyId) -> Result<&BodyRecord> {
        self.bodies.iter().find(|b| b.id == id).ok_or(CadpoleError::UnknownBody(id))
    }

    pub fn body_mut(&mut self, id: BodyId) -> Result<&mut BodyRecord> {
        self.bodies.iter_mut().find(|b| b.id == id).ok_or(CadpoleError::UnknownBody(id))
    }

    /// The bounds a body is tessellated to, and where its tolerance came
    /// from — three levels, nearest first: the body's own
    /// `tessellation(...)`, then `call` (an `export(..., tolerance=)`), then
    /// the document's. A rigid copy answers with its source's own bounds,
    /// because it shares the source's mesh — and the
    /// source is the body at the *root* of the derivation, not the nearest
    /// link: `copy(b).translate(...)` derives the moved body from the copy,
    /// and an instance from a part body that is itself a copy, so the
    /// nearest link usually carries no `tessellation(...)` of its own.
    pub fn tessellation_for(&self, id: BodyId, call: Option<f64>) -> Result<(TessellationOptions, ToleranceSource)> {
        let own = self.body(self.derivation_root(id)?)?.tessellation;
        let mut options = TessellationOptions::new(self.tolerance);
        let mut source = ToleranceSource::Document;
        if let Some(t) = call.filter(|t| *t > 0.0) {
            options.tolerance = t;
            source = ToleranceSource::Call;
        }
        if let Some(t) = own.tolerance {
            options.tolerance = t;
            source = ToleranceSource::Body;
        }
        if let Some(a) = own.max_normal_degrees {
            options.max_normal_degrees = a;
        }
        Ok((options, source))
    }

    /// The body a rigid copy's mesh ultimately comes from: `id` itself
    /// when it is not derived from another body, else the first body up
    /// the `derived_from` chain that is not. Bounded by the body count, so
    /// a cycle (which no operation makes) cannot hang it.
    pub fn derivation_root(&self, id: BodyId) -> Result<BodyId> {
        let mut at = id;
        for _ in 0..self.bodies.len() {
            match self.body(at)?.derived_from {
                Some((src, _)) => at = src,
                None => return Ok(at),
            }
        }
        Ok(at)
    }

    /// What the body's current mesh was made to ([`MeshQuality`]): `None`
    /// until [`Context::body_mesh`] has made one, and for a mesh body.
    pub fn mesh_quality(&self, id: BodyId) -> Result<Option<MeshQuality>> {
        Ok(self.body(id)?.quality.clone())
    }

    /// The body's mesh as it stands, without making one: a mesh body's own
    /// triangles, or whatever [`Context::body_mesh`] last cached. `None` for
    /// a B-rep body that has not been tessellated yet.
    pub fn cached_mesh(&self, id: BodyId) -> Result<Option<&(Mesh, Topology)>> {
        let b = self.body(id)?;
        Ok(match &b.body {
            Body::Mesh(m) => Some(m),
            Body::Brep(_) => b.cache.as_deref(),
        })
    }

    /// Mesh and topology for a body, tessellated on first use at the body's
    /// own bounds ([`Context::tessellation_for`] with no call-level
    /// tolerance). A mesh body already *is* its mesh, so it needs neither
    /// the cache nor a copy; a B-rep body that is a rigid copy of another
    /// reuses the source's tessellation.
    pub fn body_mesh(&mut self, id: BodyId) -> Result<&(Mesh, Topology)> {
        self.body_mesh_with(id, None)
    }

    /// The body's mesh as it was last made, at whatever tolerance that was —
    /// or made now at the body's own bounds. What a check that must not
    /// disturb an export's meshes reads: after `export(..., tolerance=0.01)`
    /// has meshed a body, the chord-error check looks at *that* mesh,
    /// where [`Context::body_mesh`] would put the document's back.
    pub fn body_mesh_cached(&mut self, id: BodyId) -> Result<&(Mesh, Topology)> {
        if self.body(id)?.cache.is_some() {
            return Ok(self.body(id)?.cache.as_deref().unwrap());
        }
        self.body_mesh_with(id, None)
    }

    /// [`Context::body_mesh`] at the bounds [`Context::tessellation_for`]
    /// gives for `call`. The cache holds one mesh per body, made to one set
    /// of bounds; asking for different ones replaces it, so an export at a
    /// call-level tolerance and the display at the document's each get the
    /// mesh they asked for, and the last one asked for stays.
    pub fn body_mesh_with(&mut self, id: BodyId, call: Option<f64>) -> Result<&(Mesh, Topology)> {
        if matches!(self.body(id)?.body, Body::Mesh(_)) {
            let Body::Mesh(m) = &self.body(id)?.body else { unreachable!() };
            return Ok(m);
        }
        let (options, source) = self.tessellation_for(id, call)?;
        // A cached mesh is remade when it was made to other bounds. One
        // handed in ready-made (`add_body_realised`) has no bounds of its
        // own and is kept as given.
        let stale = match (&self.body(id)?.cache, &self.body(id)?.quality) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(_), Some(q)) => q.options != options,
        };
        if stale {
            let derived = self.body(id)?.derived_from.clone();
            match derived {
                // A rigid copy: the same faces, moved. Reusing the source's
                // tessellation keeps every name identical across a pattern.
                Some((src, t)) => {
                    let moved = {
                        let (mesh, topo) = self.body_mesh_with(src, call)?;
                        (mesh.transformed(&t), topo.transformed(&t))
                    };
                    let quality = self.body(src)?.quality.clone();
                    let b = self.body_mut(id)?;
                    b.cache = Some(Arc::new(moved));
                    b.quality = quality;
                }
                None => {
                    // A tessellation is its own memo entry, keyed by the
                    // body's identity and the bounds: a body that
                    // did not change is not tessellated again.
                    let (feature, span, key) = {
                        let b = self.body(id)?;
                        let span = self.feature(&b.feature).ok().and_then(|f| f.span.clone());
                        let mut k = KeyBuilder::new("tessellate");
                        k.body(&b.key).put(&options);
                        (b.feature.clone(), span, k.finish())
                    };
                    let produced = self.memoised("tessellate", &feature, span.clone(), key, |ctx| {
                        // Counted here, inside the memoised op, so a memo
                        // hit is not a tessellation (the sharing test reads
                        // this).
                        ctx.stats.tessellations += 1;
                        let built = {
                            let b = ctx.body(id)?;
                            realise_with(&b.body, &b.names, &options)
                        };
                        // This is the one place a body's mesh is made for
                        // display and export, so the kernel's notes land
                        // here, on the body's feature, whether or not the
                        // tessellation succeeded.
                        ctx.absorb_kernel_notes(&feature, span.clone());
                        let (mb, unmatched, at, chord) = built?;
                        if unmatched > 0 {
                            ctx.push_note(
                                Severity::Warning,
                                format!(
                                    "{unmatched} face(s) of {feature} lie on no surface of the feature's \
                                     inputs, so they are named {feature}/face[n] by kernel output order and \
                                     will not survive an edit"
                                ),
                                &feature,
                                None,
                                false,
                            );
                        }
                        Ok(Produced {
                            mesh: Some(MeshOutcome { mesh: Arc::new(mb), unmatched, at, chord }),
                            ..Default::default()
                        })
                    })?;
                    let m = produced.mesh.as_ref().expect("a tessellation entry carries its mesh");
                    let chord_face =
                        m.chord.and_then(|c| m.mesh.1.faces.get(c.face)).map(|f| f.name.clone()).unwrap_or_default();
                    let quality = MeshQuality { options, source, tessellated_at: m.at, chord: m.chord, chord_face };
                    let b = self.body_mut(id)?;
                    b.cache = Some(m.mesh.clone());
                    b.quality = Some(quality);
                }
            }
        }
        Ok(self.body(id)?.cache.as_deref().unwrap())
    }

    pub fn consume_body(&mut self, id: BodyId) -> Result<()> {
        self.body_mut(id)?.visible = false;
        Ok(())
    }

    // ---- modeling operations

    /// `kind` is the feature kind shown in the tree: "extrude" normally, or a
    /// primitive name such as "box" / "cylinder" when called from sugar.
    pub fn extrude(
        &mut self,
        kind: &str,
        sketch: &Sketch,
        sketch_feature: Option<&FeatureId>,
        distance: f64,
        mut args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        args.insert("distance".into(), fmt_num(distance));
        let inputs = sketch_feature.cloned().into_iter().collect();
        let fid = self.new_feature(kind, name.clone(), inputs, args, span.clone())?;
        let mut key = KeyBuilder::new("extrude");
        key.str(kind).put(&fid).put(sketch).f64(distance);
        let produced = self.memoised("extrude", &fid, span, key.finish(), |_| {
            let solids = kernel::extrude(sketch, distance)?;
            // The extrude, not the tessellator, names what it made: `start`,
            // `end` and one `side[l.s]` per sketch loop-and-segment.
            let generator = Arc::new(naming::Generator::extrude(sketch, distance, kind));
            Ok(Produced::bodies(
                solids
                    .into_iter()
                    .map(|s| {
                        ProducedBody::new(
                            Body::Brep(s),
                            NameSource::Generator { feature: fid.clone(), generator: generator.clone() },
                        )
                    })
                    .collect(),
            ))
        })?;
        self.add_produced(&fid, produced, name)
    }

    pub fn revolve(
        &mut self,
        kind: &str,
        sketch: &Sketch,
        sketch_feature: Option<&FeatureId>,
        origin: Vec3,
        axis: Vec3,
        angle_rad: f64,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        let mut args = BTreeMap::new();
        args.insert("angle_deg".into(), fmt_num(angle_rad.to_degrees()));
        args.insert("axis".into(), fmt_vec(axis));
        args.insert("origin".into(), fmt_vec(origin));
        let inputs = sketch_feature.cloned().into_iter().collect();
        let fid = self.new_feature(kind, name.clone(), inputs, args, span.clone())?;
        let mut key = KeyBuilder::new("revolve");
        key.str(kind).put(&fid).put(sketch).put(&origin).put(&axis).f64(angle_rad);
        let produced = self.memoised("revolve", &fid, span, key.finish(), |_| {
            let solids = kernel::revolve(sketch, origin, axis, angle_rad)?;
            // One revolved surface per profile segment, plus `start` / `end` when
            // the turn is partial.
            let generator = Arc::new(naming::Generator::revolve(sketch, origin, axis, angle_rad));
            Ok(Produced::bodies(
                solids
                    .into_iter()
                    .map(|s| {
                        ProducedBody::new(
                            Body::Brep(s),
                            NameSource::Generator { feature: fid.clone(), generator: generator.clone() },
                        )
                    })
                    .collect(),
            ))
        })?;
        self.add_produced(&fid, produced, name)
    }

    /// A ruled loft through `sections` (named as docs/design.md's `loft` row says).
    ///
    /// `sections` are the sketches, curves in space, or tip points in the order
    /// the script gave them; `sketch_features` names the features they came
    /// from (sketches or curves), so the loft depends on them in the tree.
    /// Sections correspond loop by loop and segment by segment in construction
    /// order — see [`crate::loft`] for why that, and not a geometric match, is
    /// the rule. `matching` is the one relaxation of it:
    /// [`crate::loft::SectionMatch::ArcLength`] subdivides sections whose
    /// segment counts differ until they agree. `caps` says how the
    /// start and the end are closed: a plane, or the loop ruled
    /// against itself for an end that lies in no plane.
    ///
    /// The result is an exact B-rep solid, never a mesh: a loft is not a
    /// boolean, so it cannot hit the coincident-face cases that route booleans
    /// to the mesh kernel, and the corner bands of a Gridfinity foot are exact
    /// cones only on this path ([`kernel::loft`]).
    #[allow(clippy::too_many_arguments)]
    pub fn loft(
        &mut self,
        kind: &str,
        sections: &[crate::loft::Section],
        sketch_features: &[FeatureId],
        ruled: bool,
        matching: crate::loft::SectionMatch,
        caps: [crate::loft::Cap; 2],
        mut args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        args.insert("ruled".into(), ruled.to_string());
        args.insert("match".into(), matching.name().to_owned());
        args.insert(
            "cap".into(),
            if caps[0] == caps[1] {
                caps[0].name().to_owned()
            } else {
                format!("({}, {})", caps[0].name(), caps[1].name())
            },
        );
        let fid = self.new_feature(kind, name.clone(), sketch_features.to_vec(), args, span.clone())?;
        // Keyed by the sections as given, not the plan: the plan is a pure
        // function of them, and its `describe()` has no coordinates in it.
        // The plan itself is built inside the memoised op — a ruled cap
        // splits and refits its curve, which on the compressor wheel's
        // blades is 200 ms — so its `sections` arg and its warnings are
        // recorded with the op and replayed on a hit like everything else.
        let mut key = KeyBuilder::new("loft");
        key.str(kind).put(&fid).put(sections).bool(ruled).put(&matching).put(&caps);
        let produced = self.memoised("loft", &fid, span.clone(), key.finish(), |ctx| {
            let plan = crate::loft::LoftPlan::with_caps(sections, ruled, matching, caps)?;
            ctx.set_feature_arg(&fid, "sections", plan.describe());
            for w in &plan.warnings {
                ctx.push_note(Severity::Warning, w.clone(), &fid, span.clone(), true);
            }
            let solids = kernel::loft(&plan)?;
            // The loft, not the tessellator, names what it made.
            let generator = Arc::new(naming::Generator::loft(&plan, kind));
            Ok(Produced::bodies(
                solids
                    .into_iter()
                    .map(|s| {
                        ProducedBody::new(
                            Body::Brep(s),
                            NameSource::Generator { feature: fid.clone(), generator: generator.clone() },
                        )
                    })
                    .collect(),
            ))
        })?;
        self.add_produced(&fid, produced, name)
    }

    /// A truncated cone (`r1 == 0` is a point), built as a two-section loft of
    /// concentric circles so its side is the same exact surface a lofted corner
    /// gets. Faces are `cone_1/bottom`, `cone_1/top` (absent when `r1 == 0`)
    /// and the two-face group `cone_1/side`.
    #[allow(clippy::too_many_arguments)]
    pub fn cone(
        &mut self,
        at: Vec3,
        r0: f64,
        r1: f64,
        h: f64,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<BodyId> {
        let sections = kernel::cone_sections(at, r0, r1, h)?;
        let plan = crate::loft::LoftPlan::new(&sections, true, crate::loft::SectionMatch::Index)?;
        let mut args = BTreeMap::new();
        args.insert("r0".into(), fmt_num(r0));
        args.insert("r1".into(), fmt_num(r1));
        args.insert("h".into(), fmt_num(h));
        args.insert("at".into(), fmt_vec(at));
        let fid = self.new_feature("cone", name.clone(), vec![], args, span.clone())?;
        let mut key = KeyBuilder::new("cone");
        key.put(&fid).put(&at).f64(r0).f64(r1).f64(h);
        let produced = self.memoised("cone", &fid, span, key.finish(), |_| {
            let mut solids = kernel::loft(&plan)?;
            let solid = solids.remove(0);
            let generator = Arc::new(naming::Generator::loft(&plan, "cone"));
            Ok(Produced::one(Body::Brep(solid), NameSource::Generator { feature: fid.clone(), generator }))
        })?;
        Ok(self.add_produced(&fid, produced, name)?[0])
    }

    /// A profile swept along a curve ([`crate::sweep`]): one body per
    /// region of `profile`, an exact B-rep. `profile_feature` and
    /// `path_feature` are what the sweep depends on in the tree; `twist_rad`
    /// is the extra turn over the whole length, `scale` the profile's scale
    /// at the end. The sweep's report goes into the feature's args (which
    /// segments were exact, how far the skinned ones were measured to sit
    /// from the true sweep) and, when anything was approximated, into an
    /// info diagnostic on the feature.
    #[allow(clippy::too_many_arguments)]
    pub fn sweep_along(
        &mut self,
        profile: &Sketch,
        profile_feature: Option<&FeatureId>,
        path: &crate::curve::Curve,
        path_feature: Option<&FeatureId>,
        frame: crate::sweep::Frame,
        up: Option<Vec3>,
        twist_rad: f64,
        scale: f64,
        mut args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        let spec = crate::sweep::SweepSpec { profile, path, frame, up, twist: twist_rad, scale, tolerance: self.tolerance };
        args.insert("frame".into(), frame.name().to_owned());
        args.insert("path".into(), path.describe());
        if let Some(u) = up {
            args.insert("up".into(), fmt_vec(u));
        }
        if twist_rad != 0.0 {
            args.insert("twist_deg".into(), fmt_num(twist_rad.to_degrees()));
        }
        if scale != 1.0 {
            args.insert("scale".into(), fmt_num(scale));
        }
        let mut inputs: Vec<FeatureId> = Vec::new();
        for f in [profile_feature, path_feature].into_iter().flatten() {
            if !inputs.contains(f) {
                inputs.push(f.clone());
            }
        }
        let fid = self.new_feature("sweep", name.clone(), inputs, args, span.clone())?;
        // Keyed by everything the sweep reads: the profile as drawn,
        // the path's segments as their numbers (a helix by its closed form),
        // the frame, the seed, the twist, the scale and the tolerance the
        // approximation target comes from. The report's args and its note
        // are set inside the op, so a hit replays them.
        let mut key = KeyBuilder::new("sweep");
        key.put(&fid).put(profile).put(path).str(frame.name()).bool(up.is_some());
        if let Some(u) = up {
            key.put(&u);
        }
        key.f64(twist_rad).f64(scale).f64(self.tolerance);
        let produced = self.memoised("sweep", &fid, span, key.finish(), |ctx| {
            let swept = crate::sweep::sweep(&spec)?;
            ctx.set_feature_arg(&fid, "segments", swept.report.describe());
            ctx.set_feature_arg(&fid, "length", fmt_num(swept.report.length));
            if let Some(d) = swept.report.max_deviation() {
                ctx.set_feature_arg(&fid, "approximation", format!("{d:.4} mm (target {:.4} mm)", swept.report.target));
            }
            if let Some(note) = swept.report.note() {
                kernel::notes::push(note);
            }
            // The sweep, not the tessellator, names what it made.
            let generator = Arc::new(naming::Generator::sweep(profile, &swept.layout));
            Ok(Produced::bodies(
                swept
                    .solids
                    .into_iter()
                    .map(|s| {
                        ProducedBody::new(
                            Body::Brep(s),
                            NameSource::Generator { feature: fid.clone(), generator: generator.clone() },
                        )
                    })
                    .collect(),
            ))
        })?;
        self.add_produced(&fid, produced, name)
    }

    /// A pattern of `body` at `transforms` (the first of which is the
    /// identity: the original is the first instance).
    ///
    /// With `merge = false` this is the historical shape — one `copy` feature
    /// per instance, N bodies back, and the script unions them itself.
    ///
    /// With `merge = true` the copies are fused into **one** body, and
    /// that is the whole point: the copies of a pattern usually *touch*, and
    /// touching is exactly what the exact B-rep kernel refuses (truck #57: a
    /// union whose operands meet on coincident faces or on an edge returns
    /// `None`). Gridfinity's feet are the case in hand — they meet only along
    /// their top edges on z = 0 and nowhere else.
    ///
    /// So a merging pattern **routes to the mesh kernel by default**:
    /// manifold's boolean is exact on touching input and its cost follows the
    /// touching region rather than the accumulated face count, which is the
    /// other reason a long union chain is the wrong job for shapeops.
    /// The result is a mesh body and an info diagnostic says so.
    /// `kernel="brep"` keeps the exact path, which is what a document that must
    /// export STEP asks for, and turns a failure back into a diagnostic.
    #[allow(clippy::too_many_arguments)]
    pub fn pattern(
        &mut self,
        kind: &str,
        body: BodyId,
        transforms: &[Transform],
        merge: bool,
        kernel_choice: KernelChoice,
        mut args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        if transforms.is_empty() {
            return Err(CadpoleError::invalid("pattern needs at least one instance"));
        }
        if !merge {
            let mut out = vec![body];
            for (i, t) in transforms.iter().enumerate().skip(1) {
                let n = name.as_ref().map(|n| format!("{n}_{i}"));
                out.push(self.duplicate(body, t, n, span.clone())?);
            }
            return Ok(out);
        }
        args.insert("count".into(), transforms.len().to_string());
        args.insert("merge".into(), "true".into());
        if kernel_choice != KernelChoice::Auto {
            args.insert("kernel".into(), kernel_choice.name().to_owned());
        }
        let material = self.body(body)?.material.clone();
        let layer = self.body(body)?.layer.clone();
        let inputs = vec![self.body(body)?.feature.clone()];
        let fid = self.new_feature(kind, name.clone(), inputs, args, span.clone())?;
        let choice = match kernel_choice {
            KernelChoice::Auto => {
                if transforms.len() > 1 {
                    self.info(
                        format!(
                            "{kind}(merge=True) fuses {} copies on the mesh kernel: copies of a \
                             pattern usually meet on coincident faces or touch along an edge, and \
                             the exact B-rep kernel refuses both. Pass \
                             kernel=\"brep\" to insist on the exact kernel",
                            transforms.len()
                        ),
                        &fid,
                        span.clone(),
                    );
                }
                KernelChoice::Mesh
            }
            other => other,
        };
        // One instance is a copy, not a fuse: keep it on the exact path so a
        // `merge=True` pattern of one does not quietly demote a B-rep body.
        let as_mesh = choice == KernelChoice::Mesh && transforms.len() > 1;
        // The fuse's key: the source's identity, every transform,
        // the kernel choice, the bounds the source is tessellated to and the
        // routing inputs. One key for the whole fuse, however many copies.
        let mut key = KeyBuilder::new("pattern");
        key.str(kind).put(&fid).body(&self.body(body)?.key).put(transforms).put(&choice);
        key.put(&self.tessellation_for(body, None)?.0);
        self.routing_key(&mut key);
        let key = key.finish();
        if self.deferred(&key) {
            // The drag preview: the copies are shown unfused —
            // rigid copies share the source's tessellation, so this costs
            // nothing — and the fuse runs when the drag ends.
            self.memo_counts.deferred.push(OpRecord { feature: fid.0.clone(), op: kind.to_owned() });
            self.info(
                format!(
                    "{kind}(merge=True) is deferred while a parameter is being dragged: its {} copies are \
                     shown unfused, and the fuse runs when the drag ends",
                    transforms.len()
                ),
                &fid,
                span,
            );
            // Mesh copies of the source's display mesh, not B-rep copies:
            // a preview only shows them, and moving a tessellation is a
            // pass over its vertices where mapping a truck solid is a deep
            // copy of every surface.
            let (mesh, topo) = {
                let (m, tp) = self.body_mesh(body)?;
                (m.clone(), tp.clone())
            };
            for t in transforms.iter().skip(1) {
                let moved = Body::Mesh((mesh.transformed(t), topo.transformed(t)));
                let id = self.add_body(&fid, moved, None)?;
                self.body_mut(id)?.material = material.clone();
                self.body_mut(id)?.layer = layer.clone();
            }
            return Ok(vec![body]);
        }
        let produced = self.memoised(kind, &fid, span.clone(), key, |ctx| {
            let mut acc: Option<(Body, NameSource)> = None;
            if as_mesh {
                // Copies that neither touch nor cross are fused by one
                // compose, no boolean at all; the pairwise fold
                // below is what touching or crossing copies still get.
                acc = ctx.pattern_compose(body, transforms)?.map(|mb| (Body::Mesh(mb), NameSource::Intrinsic));
            }
            if acc.is_none() {
                for t in transforms {
                    let inst = ctx.pattern_instance(body, t, as_mesh, &fid)?;
                    acc = Some(match acc {
                        None => inst,
                        Some((a, a_names)) => {
                            let (b, b_names) = inst;
                            ctx.boolean_pair(BoolOp::Union, a, &a_names, &b, &b_names, choice, &fid, span.clone())?
                        }
                    });
                }
            }
            let (fused, names) = acc.expect("at least one instance");
            if fused.kind_name() == "mesh" {
                ctx.set_feature_arg(&fid, "kind", "mesh");
            }
            Ok(Produced::one(fused, names))
        })?;
        self.consume_body(body)?;
        let id = self.add_produced(&fid, produced, name)?[0];
        self.body_mut(id)?.material = material;
        self.body_mut(id)?.layer = layer;
        Ok(vec![id])
    }

    /// A pattern's fuse in one step: every copy's mesh, from the
    /// source's one tessellation, composed into one manifold and checked —
    /// when the copies neither touch nor cross, their union *is* their
    /// concatenation, and no boolean runs at all. The compressor wheel's
    /// seven blade pairs are that case, where a pairwise fold pays for the
    /// growing accumulator at every step. `None` when the copies do touch or
    /// cross, which the pairwise path handles with its snaps and notes.
    fn pattern_compose(&mut self, body: BodyId, transforms: &[Transform]) -> Result<Option<MeshBody>> {
        let tol = self.tolerance;
        let copies: Vec<MeshBody> = {
            let (mesh, topo) = self.body_mesh(body)?;
            transforms.iter().map(|t| (mesh.transformed(t), topo.transformed(t))).collect()
        };
        let refs: Vec<&MeshBody> = copies.iter().collect();
        let Some(out) = mesh_kernel::compose_disjoint(&refs, tol)? else {
            return Ok(None);
        };
        check_mesh_result(BoolOp::Union, &out)?;
        kernel::notes::push(format!(
            "the {} copies neither touch nor cross each other, so the fuse is their concatenation: \
             composed in one step on the mesh kernel, no boolean",
            transforms.len()
        ));
        Ok(Some(out))
    }

    /// One instance of a pattern as a boolean operand: the source body moved,
    /// carrying the source's face names moved with it. The source is
    /// tessellated and named *once* per instance from the same cache, so every
    /// copy of a pattern agrees about what its faces are called,
    /// whichever kernel then fuses them.
    fn pattern_instance(
        &mut self,
        body: BodyId,
        t: &Transform,
        as_mesh: bool,
        feature: &FeatureId,
    ) -> Result<(Body, NameSource)> {
        let tol = self.tolerance;
        let (mesh, topo) = {
            let (m, tp) = self.body_mesh(body)?;
            (m.transformed(t), tp.transformed(t))
        };
        if as_mesh {
            return Ok((Body::Mesh((mesh, topo)), NameSource::Intrinsic));
        }
        let moved = self.body(body)?.body.transformed(t);
        // The exact path keeps a B-rep operand, so its names have to come back
        // through the element map: hand the boolean the moved copy's own named
        // surfaces as the catalogue every result face is matched against.
        let mut cat = naming::SurfaceCatalogue::new(match_tolerance(tol));
        for f in &topo.faces {
            let tris: Vec<[Vec3; 3]> =
                (f.tri_start..f.tri_start + f.tri_count).filter_map(|x| mesh.triangle(x as usize)).collect();
            cat.add_face(
                naming::NamedFace { name: f.name.clone(), aliases: f.aliases.clone(), role: Role::Kept },
                tris,
            );
        }
        cat.finish();
        Ok((moved, NameSource::Inherited { feature: feature.clone(), sources: Arc::new(cat) }))
    }

    /// Boolean of `a` with each of `others` in turn. Inputs are hidden.
    ///
    /// **Routing.** With `kernel == Auto` each pairwise step is
    /// decided *before* the call, because `truck-shapeops` has no cancellation
    /// hook:
    ///
    /// 1. either operand is already a mesh body → mesh kernel;
    /// 2. `faces(a) + faces(b)` over [`Context::brep_face_budget`] → mesh
    ///    kernel;
    /// 3. otherwise truck, and on failure (coincident faces, an open shell, a
    ///    panic) the same pair is retried on the mesh kernel.
    ///
    /// Nothing else decides: not how long the previous boolean took, not the
    /// machine, so the same document gets the same kernels on every run and
    /// in the browser (see [`kernel::BREP_FACE_BUDGET`]). Every route
    /// that is not
    /// plain B-rep emits an info diagnostic saying which rule fired.
    /// `kernel == Brep` disables the fallback entirely (the error is the
    /// answer — that is what a document that must export STEP wants);
    /// `kernel == Mesh` skips truck entirely.
    pub fn boolean(
        &mut self,
        op: BoolOp,
        a: BodyId,
        others: &[BodyId],
        kernel_choice: KernelChoice,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<BodyId> {
        let mut inputs = vec![self.body(a)?.feature.clone()];
        for o in others {
            inputs.push(self.body(*o)?.feature.clone());
        }
        let mut args = BTreeMap::new();
        args.insert("bodies".into(), {
            let mut v = vec![self.body(a)?.name.clone()];
            for o in others {
                v.push(self.body(*o)?.name.clone());
            }
            v.join(", ")
        });
        if kernel_choice != KernelChoice::Auto {
            args.insert("kernel".into(), kernel_choice.name().to_owned());
        }
        let fid = self.new_feature(op.kind_name(), name.clone(), inputs, args, span.clone())?;
        let material = self.body(a)?.material.clone();
        let layer = self.body(a)?.layer.clone();
        // The key: the op, the feature the names are minted under,
        // every operand's identity in order, the kernel choice and the
        // routing inputs.
        let mut key = KeyBuilder::new("boolean");
        key.put(&op).put(&fid).body(&self.body(a)?.key);
        key.tag("others").u64(others.len() as u64);
        for o in others {
            key.body(&self.body(*o)?.key);
        }
        key.put(&kernel_choice);
        self.routing_key(&mut key);
        // An operand's own `tessellation(tolerance=)` is the chord it crosses
        // into the mesh kernel at, so it is an input; keyed only where one is
        // set, so every other boolean's key is what it was.
        for (i, id) in std::iter::once(&a).chain(others).enumerate() {
            if let Some(t) = self.body(*id)?.tessellation.tolerance {
                key.tag("operand_chord").u64(i as u64).f64(t);
            }
        }
        let key = key.finish();
        if self.deferred(&key) {
            // The drag preview: a union shows its operands side by
            // side; a subtract or intersect shows `a` alone, which is less
            // wrong than a tool standing proud of the part.
            self.memo_counts.deferred.push(OpRecord { feature: fid.0.clone(), op: op.kind_name().to_owned() });
            self.info(
                format!(
                    "{} is deferred while a parameter is being dragged: {} shown, and the {} runs when the \
                     drag ends",
                    op.kind_name(),
                    if op == BoolOp::Union { "the operands are" } else { "only the first operand is" },
                    op.kind_name()
                ),
                &fid,
                span,
            );
            self.consume_body(a)?;
            if op != BoolOp::Union {
                for o in others {
                    self.consume_body(*o)?;
                }
            }
            return self.stand_in(&fid, a, name);
        }
        let produced = self.memoised(op.kind_name(), &fid, span.clone(), key, |ctx| {
            let (acc, acc_names) = ctx.boolean_fold(op, a, others, kernel_choice, &fid, span.clone())?;
            if acc.kind_name() == "mesh" {
                ctx.set_feature_arg(&fid, "kind", "mesh");
            }
            Ok(Produced::one(acc, acc_names))
        })?;
        self.consume_body(a)?;
        for o in others {
            self.consume_body(*o)?;
        }
        let id = self.add_produced(&fid, produced, name)?[0];
        self.body_mut(id)?.material = material;
        self.body_mut(id)?.layer = layer;
        Ok(id)
    }

    /// `union(..., weld=)`: join bodies the way a printer fuses the
    /// parts of one object. Every operand that is an axis-aligned box --
    /// the walls, slabs and posts of a model built out of them -- is joined
    /// exactly on a grid ([`crate::gridjoin`]): coordinates within `weld` of
    /// each other are one, so near misses touch and slivers thinner than
    /// the weld close; contacts along an edge or at a corner, which a union
    /// cannot hold, are bridged `weld` wide; and the result is closed,
    /// oriented and embedded by construction, a mesh body whose faces are
    /// named after the box sides they lie on. Every other operand is then
    /// unioned onto that result in order, exactly, by the usual routing
    /// (the weld does not reach it). Inputs are consumed; the result is on
    /// the first operand's layer with its material, as a boolean's is.
    #[allow(clippy::too_many_arguments)]
    pub fn union_welded(
        &mut self,
        a: BodyId,
        others: &[BodyId],
        weld: f64,
        kernel_choice: KernelChoice,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<BodyId> {
        if !(weld > 0.0) || !weld.is_finite() {
            return Err(CadpoleError::invalid(format!("union: weld= is a positive length, got {}", fmt_num(weld))));
        }
        if kernel_choice == KernelChoice::Brep {
            return Err(CadpoleError::invalid(
                "union: weld= joins boxes on a grid and returns a mesh body, which kernel=\"brep\" cannot; leave \
                 kernel= out or pass \"mesh\"",
            ));
        }
        let mut ids = vec![a];
        ids.extend_from_slice(others);
        let mut inputs = Vec::with_capacity(ids.len());
        let mut names = Vec::with_capacity(ids.len());
        for id in &ids {
            let b = self.body(*id)?;
            inputs.push(b.feature.clone());
            names.push(b.name.clone());
        }
        let mut args = BTreeMap::new();
        args.insert("bodies".into(), names.join(", "));
        args.insert("weld".into(), fmt_num(weld));
        if kernel_choice != KernelChoice::Auto {
            args.insert("kernel".into(), kernel_choice.name().to_owned());
        }
        let fid = self.new_feature("union", name.clone(), inputs, args, span.clone())?;
        let material = self.body(a)?.material.clone();
        let layer = self.body(a)?.layer.clone();
        let mut key = KeyBuilder::new("union_weld");
        key.put(&fid).f64(weld);
        key.tag("operands").u64(ids.len() as u64);
        for id in &ids {
            key.body(&self.body(*id)?.key);
        }
        key.put(&kernel_choice);
        self.routing_key(&mut key);
        let key = key.finish();
        if self.deferred(&key) {
            self.memo_counts.deferred.push(OpRecord { feature: fid.0.clone(), op: "union".to_owned() });
            self.info(
                "union is deferred while a parameter is being dragged: the operands are shown, and the union runs \
                 when the drag ends"
                    .to_owned(),
                &fid,
                span,
            );
            self.consume_body(a)?;
            return self.stand_in(&fid, a, name);
        }
        let produced = self.memoised("union", &fid, span.clone(), key, |ctx| {
            let (body, names) = ctx.welded_fold(&ids, weld, kernel_choice, &fid, span.clone())?;
            ctx.set_feature_arg(&fid, "kind", "mesh");
            Ok(Produced::one(body, names))
        })?;
        for id in &ids {
            self.consume_body(*id)?;
        }
        let id = self.add_produced(&fid, produced, name)?[0];
        self.body_mut(id)?.material = material;
        self.body_mut(id)?.layer = layer;
        Ok(id)
    }

    /// [`Context::union_welded`]'s work: the boxes on the grid, then the rest
    /// folded on in order.
    fn welded_fold(
        &mut self,
        ids: &[BodyId],
        weld: f64,
        kernel_choice: KernelChoice,
        fid: &FeatureId,
        span: Option<Span>,
    ) -> Result<(Body, NameSource)> {
        let mut groups: Vec<mesh_kernel::SourceFace> = Vec::new();
        let mut boxes: Vec<crate::gridjoin::GridBox> = Vec::new();
        let mut box_ids: Vec<BodyId> = Vec::new();
        let mut rest: Vec<BodyId> = Vec::new();
        for id in ids {
            let (mesh, topo) = self.body_mesh(*id)?;
            match crate::gridjoin::axis_box(mesh, topo) {
                Some((lo, hi, side_faces)) => {
                    let base = groups.len() as u32;
                    mesh_kernel::push_groups(&mut groups, topo, mesh_kernel::id_span_of(mesh, topo));
                    let sides = side_faces.map(|f| base + f);
                    boxes.push(crate::gridjoin::GridBox { lo, hi, sides });
                    box_ids.push(*id);
                }
                None => rest.push(*id),
            }
        }
        let bridge = groups.len() as u32;
        groups.push(mesh_kernel::SourceFace {
            name: format!("{}/weld", fid.0),
            aliases: Vec::new(),
            surface_kind: "group",
            surface: crate::surface_class::SurfaceClass::Other,
            geometry: None,
        });
        let mut acc: Option<(Body, NameSource)> = None;
        let mut grid: Option<([Vec<f64>; 3], Vec<([f64; 3], [f64; 3])>)> = None;
        if !boxes.is_empty() {
            let g = crate::gridjoin::union_boxes(&boxes, weld, bridge)?;
            grid = Some((g.lines.clone(), g.welded.clone()));
            let r = &g.report;
            let mut said = format!(
                "union with weld={}: {} box{} joined exactly on a grid of {} x {} x {} lines",
                fmt_num(weld),
                r.boxes,
                if r.boxes == 1 { "" } else { "es" },
                r.grid[0],
                r.grid[1],
                r.grid[2]
            );
            if r.snapped > 0 {
                said.push_str(&format!("; {} coordinate{} welded onto a neighbour", r.snapped, plural(r.snapped)));
            }
            if r.bridged_edges + r.bridged_corners > 0 {
                let at = r.first_bridge.map(|p| format!(", the first near ({:.4}, {:.4}, {:.4})", p.x, p.y, p.z)).unwrap_or_default();
                said.push_str(&format!(
                    "; {} contact{} along an edge and {} at a corner bridged {} wide{at}",
                    r.bridged_edges,
                    plural(r.bridged_edges),
                    r.bridged_corners,
                    fmt_num(weld)
                ));
            }
            if !rest.is_empty() {
                said.push_str(&format!(
                    "; {} bod{} that {} not an axis-aligned box unioned after, exactly",
                    rest.len(),
                    if rest.len() == 1 { "y" } else { "ies" },
                    if rest.len() == 1 { "is" } else { "are" }
                ));
            }
            self.info(said, fid, span.clone());
            if r.slivers > 0 {
                let first = r.first_sliver.and_then(|k| box_ids.get(k)).and_then(|id| self.body(*id).ok()).map(|b| b.name.clone());
                self.warn(
                    format!(
                        "union with weld={}: {} box{} thinner than the weld in some direction closed to nothing{}; a \
                         smaller weld keeps {}",
                        fmt_num(weld),
                        r.slivers,
                        if r.slivers == 1 { "" } else { "es" },
                        first.map(|n| format!(" (the first is {n})")).unwrap_or_default(),
                        if r.slivers == 1 { "it" } else { "them" }
                    ),
                    fid,
                    span.clone(),
                );
            }
            let mb = {
                let _span = kernel::trace::Span::start(format!("grid join: name {} triangles", g.triangles.len()));
                mesh_kernel::from_indexed(&g.positions, g.triangles, g.provenance, &groups, None, self.tolerance)?
            };
            acc = Some((Body::Mesh(mb), NameSource::Intrinsic));
        }
        for id in rest {
            let (b, b_names, b_name) = {
                let rec = self.body(id)?;
                (rec.body.clone(), rec.names.clone(), rec.name.clone())
            };
            acc = Some(match (acc.take(), &grid) {
                (None, _) => (b, b_names),
                (Some((a_body, a_names)), Some((lines, welded))) => self.weld_onto(
                    a_body,
                    &a_names,
                    &b,
                    &b_names,
                    &b_name,
                    (lines, welded),
                    weld,
                    kernel_choice,
                    fid,
                    span.clone(),
                )?,
                (Some((a_body, a_names)), None) => self
                    .boolean_pair(BoolOp::Union, a_body, &a_names, &b, &b_names, kernel_choice, fid, span.clone())
                    .map_err(|e| {
                        CadpoleError::kernel(format!(
                            "union with weld=: {b_name} is not an axis-aligned box and there are no boxes for the weld \
                             to join it to, so it was unioned exactly onto the operands before it, which failed: {e}"
                        ))
                    })?,
            });
        }
        acc.ok_or_else(|| CadpoleError::invalid("union: nothing to join"))
    }

    /// One operand of `union(weld=)` that is not an axis-aligned box,
    /// unioned exactly onto the boxes joined on the grid.
    ///
    /// The weld reaches it first: every vertex coordinate within `weld` of
    /// one of the grid's lines is moved onto that line
    /// ([`crate::gridjoin::snap_to_lines`]), when the body is still a solid
    /// afterwards. A corbel, a wedge or a sloped slab modelled against the
    /// boxes' faces then meets them exactly, and the mesh boolean resolves
    /// the contact as a tie instead of a sliver a float rounding wide, which
    /// it cannot -- before this, a wedge whose back stood 10 nm off a wall
    /// came out "passing through itself" and the union gate refused it. When
    /// the union still fails, the error says what to change, naming the
    /// nearest of its coordinates the weld did not reach.
    #[allow(clippy::too_many_arguments)]
    fn weld_onto(
        &mut self,
        acc: Body,
        acc_names: &NameSource,
        b: &Body,
        b_names: &NameSource,
        b_name: &str,
        (lines, boxes): (&[Vec<f64>; 3], &[([f64; 3], [f64; 3])]),
        weld: f64,
        kernel_choice: KernelChoice,
        fid: &FeatureId,
        span: Option<Span>,
    ) -> Result<(Body, NameSource)> {
        let tol = self.tolerance;
        let (mut bm, _, _) = realise(b, b_names, tol)?;
        let s = crate::gridjoin::snap_body(&mut bm, lines, weld);
        let (moved, declined) = ((s.snap.moved > 0).then_some(s.snap), s.declined);
        let miss = crate::gridjoin::nearest_miss(&bm.0, lines, weld, 20.0 * weld);
        let mut pushed_body = bm.clone();
        let pushed = crate::gridjoin::push_body(&mut pushed_body, lines, weld / 2.0, boxes);
        // In order, the first union that holds: the operand as the weld
        // leaves it (as modelled, when nothing was within the weld and off a
        // plane); then with its faces that rest on the boxes pushed into
        // them, where that exact tie did not hold; then as modelled -- the
        // weld's moves never cost a union that held
        // without them.
        let mut first_err: Option<CadpoleError> = None;
        let try_union = |ctx: &mut Self, operand: &mesh_kernel::MeshBody| -> Result<(Body, NameSource)> {
            let before = kernel::notes::drain();
            let out = realise_owned(acc.clone(), acc_names, tol).and_then(|(am, _)| ctx.mesh_boolean_prepared(BoolOp::Union, am, operand));
            let during = kernel::notes::drain();
            for n in before.into_iter().chain(if out.is_ok() { during } else { Vec::new() }) {
                kernel::notes::push(n);
            }
            out
        };
        let said = |moved: Option<crate::gridjoin::SnapReport>, pushed: usize| {
            let mut said = Vec::new();
            if let Some(r) = moved {
                said.push(format!(
                    "{} of its vertex coordinates within the weld of the boxes' planes moved onto them (the furthest by \
                     {})",
                    r.moved,
                    crate::tangent::um(r.furthest)
                ));
            }
            if pushed > 0 {
                said.push(format!(
                    "{pushed} face{} resting flat on the boxes pushed {} into them, since as an exact tie the union did \
                     not hold",
                    plural(pushed),
                    fmt_num(weld / 2.0)
                ));
            }
            format!("union with weld={}: {b_name} is not an axis-aligned box; {}", fmt_num(weld), said.join("; "))
        };
        let as_modelled_tried = moved.is_none();
        let tie = if moved.is_some() {
            match try_union(self, &bm) {
                Ok(o) => Some((o, said(moved, 0))),
                Err(e) => {
                    first_err = Some(e);
                    None
                }
            }
        } else {
            match self.boolean_pair(BoolOp::Union, acc.clone(), acc_names, b, b_names, kernel_choice, fid, span.clone()) {
                Ok(o) => return Ok(o),
                Err(e) => {
                    first_err = Some(e);
                    None
                }
            }
        };
        if let Some((o, note)) = tie {
            self.info(note, fid, span);
            return Ok(o);
        }
        if pushed > 0 {
            match try_union(self, &pushed_body) {
                Ok(o) => {
                    self.info(said(moved, pushed), fid, span);
                    return Ok(o);
                }
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        let out = if as_modelled_tried {
            Err(first_err.take().unwrap_or_else(|| CadpoleError::kernel("the union failed")))
        } else {
            let out = self.boolean_pair(BoolOp::Union, acc, acc_names, b, b_names, kernel_choice, fid, span.clone());
            if out.is_ok() {
                self.info(
                    format!(
                        "union with weld={}: {b_name} moved onto the boxes' planes within the weld did not union into a \
                         solid, so it was unioned as modelled, which did",
                        fmt_num(weld)
                    ),
                    fid,
                    span,
                );
            }
            out
        };
        let tried = moved.is_some() || pushed > 0;
        out.map_err(|e| {
            let how = if tried {
                "it was unioned exactly onto the joined boxes -- as modelled, with its vertex coordinates within the \
                 weld of the boxes' planes moved onto them, and with the faces resting on them pushed in"
            } else if declined {
                "moving its vertices onto the boxes' planes within the weld would have collapsed part of it, so it was \
                 unioned exactly as it is onto the joined boxes"
            } else {
                "it was unioned exactly onto the joined boxes"
            };
            // To the micrometre: a coordinate a float rounding off its
            // decimal is printed as the decimal.
            let near = miss
                .map(|(a, line, d)| {
                    format!(
                        " The nearest miss: a vertex {} from the boxes' plane {} = {}, beyond the weld -- move it onto \
                         the plane, or pass weld={} or more.",
                        fmt_num((d * 1e6).round() / 1e6),
                        ["x", "y", "z"][a],
                        fmt_num((line * 1e6).round() / 1e6),
                        fmt_num((d * 1.01 * 1e6).ceil() / 1e6)
                    )
                })
                .unwrap_or_default();
            CadpoleError::kernel(format!(
                "union with weld=: {b_name} is not an axis-aligned box, so {how}, which failed: {e}.{near} To join it: \
                 make the faces where it rests on or against the boxes lie on their planes (within the weld, which \
                 moves them there) or reach well into the boxes (0.05 mm or more), not a hair short or a hair past; \
                 or build it from boxes -- a corbel or a lead-in as steps -- which the weld joins exactly"
            ))
        })
    }

    /// [`Context::boolean`]'s fold over `others`: the result body and where
    /// its names come from.
    fn boolean_fold(
        &mut self,
        op: BoolOp,
        a: BodyId,
        others: &[BodyId],
        kernel_choice: KernelChoice,
        fid: &FeatureId,
        span: Option<Span>,
    ) -> Result<(Body, NameSource)> {
        let mut acc = self.body(a)?.body.clone();
        let mut acc_names = self.body(a)?.names.clone();
        // The chord each B-rep operand is tessellated at if the mesh kernel
        // takes it: its own setting, else the document's. A step's result
        // has no setting of its own, so the accumulator falls back to the
        // document's after the first step.
        let mut acc_tol = self.operand_tolerance(a)?;
        let mut rest: &[BodyId] = others;
        while let Some((first, tail)) = rest.split_first() {
            // A run of tools that cannot touch each other is one boolean
            // against their concatenation. `batched_run` returns the
            // realised operands when it finds one, and nothing otherwise, so
            // the pairwise fold below stays the general case.
            let tools = self.batched_run(op, &acc, rest, kernel_choice, fid, span.clone())?;
            if tools.len() > 1 {
                let n = tools.len();
                let (body, names) = self.mesh_boolean_many(op, acc, &acc_names, acc_tol, &tools)?;
                acc = body;
                acc_names = names;
                acc_tol = self.tolerance;
                rest = &rest[n..];
                continue;
            }
            let b = self.body(*first)?.body.clone();
            let b_names = self.body(*first)?.names.clone();
            let chords = (acc_tol, self.operand_tolerance(*first)?);
            let (body, names) = self.boolean_pair_at(
                op,
                acc,
                &acc_names,
                &b,
                &b_names,
                kernel_choice,
                fid,
                span.clone(),
                chords,
            )?;
            acc = body;
            acc_names = names;
            acc_tol = self.tolerance;
            rest = tail;
        }
        Ok((acc, acc_names))
    }

    /// The chord a body is tessellated at when a boolean hands it to the
    /// mesh kernel: its own `tessellation(tolerance=)` when it has one, as
    /// for every other reader of its triangles, else the document's.
    fn operand_tolerance(&self, id: BodyId) -> Result<f64> {
        Ok(self.body(id)?.tessellation.tolerance.unwrap_or(self.tolerance))
    }

    /// Join bodies that **touch** on coincident faces, by merging those faces
    /// (`glue`).
    ///
    /// `union` already does this on its own when the pre-pass finds that the
    /// operands touch — a script never has to know. What asking for it by name
    /// buys is the *refusal*: `glue` is a diagnostic when the bodies overlap,
    /// or touch only along an edge, or their faces coincide only in part,
    /// where `union` would quietly run a kernel boolean instead. A part that
    /// depends on two components meeting flush — the Gridfinity feet on the
    /// bin's floor, a lip on a wall top — is worth saying that about.
    ///
    /// It is always the exact kernel: a mesh body has no faces to merge, and
    /// gluing is not something the mesh kernel is needed for.
    pub fn glue(
        &mut self,
        a: BodyId,
        others: &[BodyId],
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<BodyId> {
        let mut inputs = vec![self.body(a)?.feature.clone()];
        for o in others {
            inputs.push(self.body(*o)?.feature.clone());
        }
        let mut args = BTreeMap::new();
        args.insert("bodies".into(), {
            let mut v = vec![self.body(a)?.name.clone()];
            for o in others {
                v.push(self.body(*o)?.name.clone());
            }
            v.join(", ")
        });
        let fid = self.new_feature("glue", name.clone(), inputs, args, span.clone())?;
        let mut key = KeyBuilder::new("glue");
        key.put(&fid).body(&self.body(a)?.key);
        key.tag("others").u64(others.len() as u64);
        for o in others {
            key.body(&self.body(*o)?.key);
        }
        key.f64(self.boolean_tolerance).f64(self.tolerance);
        let produced = self.memoised("glue", &fid, span, key.finish(), |ctx| {
            let tol = ctx.boolean_tolerance;
            let mut acc = ctx.body(a)?.body.clone();
            let mut acc_names = ctx.body(a)?.names.clone();
            for o in others {
                let b = ctx.body(*o)?.body.clone();
                let b_names = ctx.body(*o)?.names.clone();
                let merged = kernel::glue(acc.to_brep()?, b.to_brep()?, tol)?;
                // The merged solid's faces *are* the operands' faces, minus the
                // pair that became interior, so they are named the same way every
                // other boolean's result is: matched back to the named surfaces
                // the two operands brought in. No face is created
                // here, so no new name is either.
                let sources = boolean_catalogue(&acc, &acc_names, &b, &b_names, ctx.tolerance)?;
                acc = Body::Brep(merged);
                acc_names = NameSource::Inherited { feature: fid.clone(), sources: Arc::new(sources) };
            }
            Ok(Produced::one(acc, acc_names))
        })?;
        let material = self.body(a)?.material.clone();
        let layer = self.body(a)?.layer.clone();
        self.consume_body(a)?;
        for o in others {
            self.consume_body(*o)?;
        }
        let id = self.add_produced(&fid, produced, name)?[0];
        self.body_mut(id)?.material = material;
        self.body_mut(id)?.layer = layer;
        Ok(id)
    }

    /// The longest run of `rest`, from the front, that [`Context::boolean`] can
    /// do in **one** mesh boolean instead of one per tool, realised and
    /// named ready to hand to [`mesh_kernel::boolean_many`]. An empty vector
    /// means "no batch here", and the caller falls back to the pairwise fold.
    ///
    /// Three things have to hold, and each one is cheap to decide:
    ///
    /// 1. **The operation distributes.** `a − t1 − t2` is `a − (t1 ∪ t2)` and
    ///    `a ∪ t1 ∪ t2` is `a ∪ (t1 ∪ t2)`; `a ∩ t1 ∩ t2` is *not* an
    ///    intersection with a union, so `intersect` keeps the fold.
    /// 2. **Every step would route to the mesh kernel anyway**
    ///    ([`Context::run_is_all_mesh`]). The exact kernel has no batch to
    ///    offer — one truck solid is one outer shell plus its voids, and a
    ///    union of separated lumps is exactly what it refuses — so
    ///    batching there is not available, not merely unhelpful.
    /// 3. **The tools in the run cannot touch each other**
    ///    ([`mesh_kernel::separated`]). Then their union *is* their
    ///    concatenation, no boolean is needed to form it, and — the part that
    ///    matters for naming — no tool can cut another, so every tool's faces
    ///    reach the result under the names they already had.
    ///
    /// The run stops at the first tool that touches an earlier one, so the
    /// operand order is never reordered: a list of thirty-two sockets with one
    /// overlapping pair in the middle batches on both sides of it.
    ///
    /// A refused run costs the two tessellations it took to refuse it, and the
    /// fold then tessellates the first of them again. That is deliberate: the
    /// alternative is to hand the pairwise step a mesh operand and bypass
    /// [`Context::boolean_pair`], which would also bypass its routing
    /// diagnostic. Measured on every example with a mesh boolean in it — the
    /// bin's six feet, the baselid's four cups and four feet — the difference
    /// is inside the run-to-run noise.
    fn batched_run(
        &mut self,
        op: BoolOp,
        acc: &Body,
        rest: &[BodyId],
        choice: KernelChoice,
        feature: &FeatureId,
        span: Option<Span>,
    ) -> Result<Vec<mesh_kernel::MeshBody>> {
        if rest.len() < 2 || op == BoolOp::Intersect || !self.run_is_all_mesh(acc, rest, choice)? {
            return Ok(Vec::new());
        }
        let mut tools: Vec<mesh_kernel::MeshBody> = Vec::new();
        for id in rest {
            let tol = self.operand_tolerance(*id)?;
            let (mb, _, _) = {
                let rec = self.body(*id)?;
                realise(&rec.body, &rec.names, tol)?
            };
            if tools.iter().any(|t: &mesh_kernel::MeshBody| !mesh_kernel::separated(&t.0, &mb.0)) {
                break;
            }
            tools.push(mb);
        }
        if tools.len() < 2 {
            return Ok(Vec::new());
        }
        if choice == KernelChoice::Auto {
            self.info(
                format!(
                    "{} of {} tools that cannot touch each other: the mesh kernel takes them in \
                     one boolean rather than one each, because their union is their \
                     concatenation",
                    op.kind_name(),
                    tools.len()
                ),
                feature,
                span,
            );
        }
        Ok(tools)
    }

    /// Would *every* step of the fold over `rest` route to the mesh kernel?
    ///
    /// Only then is a batch equivalent to the fold, so this is deliberately
    /// conservative: a chain that starts on the exact kernel and demotes to the
    /// mesh kernel part way through keeps the fold, and so does anything the
    /// script pinned to `kernel="brep"`.
    fn run_is_all_mesh(&mut self, acc: &Body, rest: &[BodyId], choice: KernelChoice) -> Result<bool> {
        match choice {
            KernelChoice::Brep => Ok(false),
            KernelChoice::Mesh => Ok(true),
            // Rule 1 of the routing table: a mesh operand makes the result a
            // mesh body, and every step after it a mesh boolean.
            KernelChoice::Auto if acc.kind_name() == "mesh" => Ok(true),
            KernelChoice::Auto => {
                let mut all_mesh_tools = true;
                let mut all_over_budget = true;
                let faces = acc.face_count();
                let budget = self.brep_face_budget;
                for id in rest {
                    let b = &self.body(*id)?.body;
                    all_mesh_tools &= b.kind_name() == "mesh";
                    all_over_budget &= faces + b.face_count() > budget;
                    if !all_mesh_tools && !all_over_budget {
                        return Ok(false);
                    }
                }
                // Rule 2: a first pair over the face budget routes to the mesh
                // kernel, and the accumulator only grows, so the rest follow.
                Ok(all_mesh_tools || all_over_budget)
            }
        }
    }

    /// [`Context::mesh_boolean`] against a whole run of tools at once. The
    /// tools are already realised and named; `a` is taken by value so the
    /// accumulator moves into the kernel instead of being cloned.
    fn mesh_boolean_many(
        &mut self,
        op: BoolOp,
        a: Body,
        a_names: &NameSource,
        a_tol: f64,
        tools: &[mesh_kernel::MeshBody],
    ) -> Result<(Body, NameSource)> {
        let tol = self.tolerance;
        let (am, _) = realise_owned(a, a_names, a_tol)?;
        let refs: Vec<&mesh_kernel::MeshBody> = tools.iter().collect();
        // The cleanup pass wants the tolerance; the exit gate wants the result.
        let out = mesh_kernel::boolean_many_with(op, &am, &refs, tol, self.mesh_engine())?;
        check_mesh_result(op, &out)?;
        Ok((Body::Mesh(out), NameSource::Intrinsic))
    }

    /// The mesh kernel's engine choice for this document: the field when
    /// set, else `CADPOLE_MESH_ENGINE`, else the policy.
    fn mesh_engine(&self) -> Option<mesh_kernel::Engine> {
        self.mesh_engine.or_else(mesh_kernel::engine_from_env)
    }

    /// One pairwise boolean, routed per [`Context::boolean`].
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn boolean_pair(
        &mut self,
        op: BoolOp,
        a: Body,
        a_names: &NameSource,
        b: &Body,
        b_names: &NameSource,
        choice: KernelChoice,
        feature: &FeatureId,
        span: Option<Span>,
    ) -> Result<(Body, NameSource)> {
        let chords = (self.tolerance, self.tolerance);
        self.boolean_pair_at(op, a, a_names, b, b_names, choice, feature, span, chords)
    }

    /// [`Context::boolean_pair`] with the chord each B-rep operand is
    /// tessellated at should the mesh kernel take the pair (`chords`, for
    /// `a` and `b`): [`Context::operand_tolerance`] for a script's own
    /// bodies.
    #[allow(clippy::too_many_arguments)]
    fn boolean_pair_at(
        &mut self,
        op: BoolOp,
        a: Body,
        a_names: &NameSource,
        b: &Body,
        b_names: &NameSource,
        choice: KernelChoice,
        feature: &FeatureId,
        span: Option<Span>,
        chords: (f64, f64),
    ) -> Result<(Body, NameSource)> {
        let pair_faces = a.face_count() + b.face_count();
        let budget = self.brep_face_budget;
        let mesh_reason = match choice {
            KernelChoice::Mesh => None, // forced; no diagnostic needed
            KernelChoice::Brep => None,
            KernelChoice::Auto => {
                if a.kind_name() == "mesh" || b.kind_name() == "mesh" {
                    Some(format!(
                        "{} of a mesh body: the mesh kernel takes it (the B-rep side is \
                         tessellated at tolerance {}, so the result is approximate to that)",
                        op.kind_name(),
                        self.tolerance
                    ))
                } else if pair_faces > budget {
                    Some(format!(
                        "{} of {pair_faces} faces is over the B-rep budget of {budget}: \
                         routed to the mesh kernel, whose cost follows the touching region \
                         rather than the whole body",
                        op.kind_name()
                    ))
                } else {
                    None
                }
            }
        };
        if let Some(why) = mesh_reason {
            self.info(why, feature, span);
            return self.mesh_boolean(op, a, a_names, b, b_names, chords);
        }
        if choice == KernelChoice::Mesh {
            return self.mesh_boolean(op, a, a_names, b, b_names, chords);
        }

        let tol = self.boolean_tolerance;
        // No clock here, deliberately. The call is timed by `kernel::trace`
        // for `CADPOLE_TRACE=1` and `cadpole-bench`, and that is the only place the
        // number goes: nothing about the route may depend on it, or the same
        // script gets a different kernel on a loaded machine (and the
        // note at `kernel::BREP_FACE_BUDGET`).
        let brep = kernel::boolean(op, a.to_brep()?, b.to_brep()?, tol);
        match brep {
            Ok(s) => {
                // truck has no history, so the result's faces are matched back
                // to the operands' named surfaces. This is the
                // one place the B-rep route pays for naming: both operands are
                // tessellated, at the tolerance the result will use anyway.
                let sources = boolean_catalogue(&a, a_names, b, b_names, self.tolerance)?;
                Ok((
                    Body::Brep(s),
                    NameSource::Inherited { feature: feature.clone(), sources: Arc::new(sources) },
                ))
            }
            Err(e) if choice == KernelChoice::Auto => {
                // The exact kernel's refusal names the case by face index;
                // the operands are realised here anyway for the retry, and
                // they carry the names, so the diagnostic says the case in
                // the script's own vocabulary too.
                let (am, _) = realise_owned(a, a_names, chords.0)?;
                let (bm, _, _) = realise(b, b_names, chords.1)?;
                let named = cadpole_tangent_summary(op, &am, &bm, tol, crate::tangent::Path::Mesh);
                let why = e.to_string();
                self.info(
                    format!(
                        "the B-rep kernel could not {}: {}; retrying on the mesh kernel{named}",
                        op.kind_name(),
                        why.trim_end_matches('.')
                    ),
                    feature,
                    span,
                );
                // Keep truck's reason in the message: if the mesh kernel fails
                // too, the first failure is usually the one worth reading.
                self.mesh_boolean_prepared(op, am, &bm).map_err(|mesh_err| {
                    CadpoleError::kernel(format!("{e}; the mesh kernel could not do it either: {mesh_err}"))
                })
            }
            Err(e) => {
                // `kernel="brep"`: the refusal is the answer, with the faces
                // named. The tessellation this costs is on the error path only.
                let mesh_tol = self.tolerance;
                let named = match (realise(&a, a_names, mesh_tol), realise(b, b_names, mesh_tol)) {
                    (Ok((am, _, _)), Ok((bm, _, _))) => cadpole_tangent_summary(op, &am, &bm, tol, crate::tangent::Path::Exact),
                    _ => String::new(),
                };
                Err(match e {
                    CadpoleError::Kernel(why) if !named.is_empty() => CadpoleError::Kernel(format!("{why}{named}")),
                    other => other,
                })
            }
        }
    }

    /// `a` is taken by value so the accumulator of a long chain moves into the
    /// mesh kernel instead of being cloned once per step.
    ///
    /// Both operands are named *before* they cross into manifold, so the
    /// provenance id manifold carries through the boolean already stands for a
    /// generated name; the result needs no matching at all.
    fn mesh_boolean(
        &mut self,
        op: BoolOp,
        a: Body,
        a_names: &NameSource,
        b: &Body,
        b_names: &NameSource,
        chords: (f64, f64),
    ) -> Result<(Body, NameSource)> {
        let (am, _) = realise_owned(a, a_names, chords.0)?;
        let (bm, _, _) = realise(b, b_names, chords.1)?;
        self.mesh_boolean_prepared(op, am, &bm)
    }

    /// [`Context::mesh_boolean`] on operands already realised and named:
    /// the boolean, then the exit gate.
    fn mesh_boolean_prepared(&mut self, op: BoolOp, am: MeshBody, bm: &MeshBody) -> Result<(Body, NameSource)> {
        let out = mesh_kernel::boolean_with(op, &am, bm, self.tolerance, self.mesh_engine())?;
        check_mesh_result(op, &out)?;
        Ok((Body::Mesh(out), NameSource::Intrinsic))
    }

    /// An info diagnostic from an op, attached to `feature` and to the
    /// document, and replayed by the memo. Public so a binding that
    /// records a feature of its own — `wrap` — can attach the
    /// kernel's notes to it.
    pub fn info(&mut self, message: String, feature: &FeatureId, span: Option<Span>) {
        self.push_note(Severity::Info, message, feature, span, true);
    }

    pub(crate) fn warn(&mut self, message: String, feature: &FeatureId, span: Option<Span>) {
        self.push_note(Severity::Warning, message, feature, span, true);
    }

    /// One diagnostic from an op, attached to `feature` (and to the feature's
    /// own list when `on_feature`), and recorded for every memo entry being
    /// made, so a hit replays it.
    fn push_note(&mut self, severity: Severity, message: String, feature: &FeatureId, span: Option<Span>, on_feature: bool) {
        let d = Diagnostic { severity, message: message.clone(), span, feature: Some(feature.clone()) };
        if on_feature && let Ok(f) = self.feature_mut(feature) {
            f.diagnostics.push(d.clone());
        }
        self.diagnostics.push(d);
        if !self.recording.is_empty() {
            let note = Note { severity, message, feature: feature.clone(), on_feature };
            for r in &mut self.recording {
                r.notes.push(note.clone());
            }
        }
    }

    // ---- the kernel-op boundary

    /// Run one kernel op through the memo. `key` is the canonical bytes of
    /// everything that decides the op's answer; `run` is the op. On a hit
    /// the entry's diagnostics and feature args are replayed and its bodies
    /// come back without the kernel running; on a miss the op runs with its
    /// notes recorded, and what it produced is stored under the key. Either
    /// way the caller adds the bodies with [`Context::add_produced`], so an
    /// op is written once and the memo is not a second code path.
    ///
    /// Nested ops (a hull tessellating its inputs, a mirror fusing a copy)
    /// each get their own entry, and the outer entry records what the inner
    /// ones said, so a hit on the outer op replays all of it.
    pub(crate) fn memoised<F>(&mut self, op: &str, fid: &FeatureId, span: Option<Span>, key: OpKey, run: F) -> Result<Arc<Produced>>
    where
        F: FnOnce(&mut Context) -> Result<Produced>,
    {
        let record = OpRecord { feature: fid.0.clone(), op: op.to_owned() };
        // Only an op at the boundary is counted: one nested in another (a
        // pattern's display tessellation, a hull's) is part of the outer
        // op's entry, and is not reached at all when the outer op hits.
        let top = self.op_depth == 0;
        let memo = self.memo.clone();
        if let Some(m) = &memo
            && let Some(hit) = m.get(&key)
        {
            self.replay(&hit);
            if top {
                self.memo_counts.hits.push(record);
            }
            return Ok(hit);
        }
        if memo.is_some() {
            self.recording.push(Recorder::default());
        }
        self.op_depth += 1;
        let out = run(self);
        self.op_depth -= 1;
        // What the kernel had to say reaches the feature whether or not the
        // op succeeded, and is recorded with the rest.
        self.absorb_kernel_notes(fid, span);
        let recorded = if memo.is_some() { self.recording.pop() } else { None };
        let mut produced = out?;
        if let Some(r) = recorded {
            produced.notes = r.notes;
            produced.args = r.args;
        }
        for (i, b) in produced.bodies.iter_mut().enumerate() {
            b.key = key.body_key(i);
        }
        if top {
            self.memo_counts.runs.push(record);
        }
        Ok(match memo {
            Some(m) if !produced.transient => m.put(&key, produced),
            _ => Arc::new(produced),
        })
    }

    /// Run an op that makes bodies from outside this crate — an importer
    /// — through the memo, exactly as the kernel ops do: `key` is
    /// the canonical bytes of everything that decides the answer (for an
    /// import, the file's content hash, the options and the importer's
    /// version), `run` parses and converts, and what it produced is added
    /// as the bodies of `fid`, named `name`, `name#1`, … On a hit nothing
    /// is parsed: the bodies, the diagnostics and the feature args `run`
    /// set come back from the entry. Diagnostics inside `run` go through
    /// `kernel::notes::push` (info) and [`Context::set_feature_arg`], which
    /// the entry records and replays; anything a caller wants at warning
    /// level it reads back from the args after this returns.
    pub fn produce<F>(
        &mut self,
        op: &str,
        fid: &FeatureId,
        span: Option<Span>,
        key: OpKey,
        name: Option<String>,
        run: F,
    ) -> Result<Vec<BodyId>>
    where
        F: FnOnce(&mut Context) -> Result<Produced>,
    {
        let produced = self.memoised(op, fid, span, key, run)?;
        self.add_produced(fid, produced, name)
    }

    /// [`Context::produce`] for a producer whose answer is a value rather
    /// than bodies — an imported 2D drawing. `run` returns the value
    /// and its size in bytes for the memo's bound; on a hit the entry's
    /// `Arc` comes back untouched, with its diagnostics and feature args
    /// replayed as for any op. The value is typed by the caller: an entry
    /// under this key that holds another type is an internal error, since
    /// the key names the op.
    pub fn produce_value<T, F>(&mut self, op: &str, fid: &FeatureId, span: Option<Span>, key: OpKey, run: F) -> Result<Arc<T>>
    where
        T: std::any::Any + Send + Sync,
        F: FnOnce(&mut Context) -> Result<(T, usize)>,
    {
        let produced = self.memoised(op, fid, span, key, |ctx| {
            let (value, bytes) = run(ctx)?;
            Ok(Produced::payload(value, bytes))
        })?;
        let payload = produced.payload.clone().ok_or_else(|| CadpoleError::kernel(format!("{op}: the memo entry holds no value")))?;
        payload.downcast::<T>().map_err(|_| CadpoleError::kernel(format!("{op}: the memo entry holds a value of another type")))
    }

    /// [`Context::produce_value`] for a producer whose answer is a function
    /// of its key only sometimes: `run` also says whether to keep it. A solve
    /// that ended optimal, infeasible or unbounded is a property of
    /// the problem and is kept; one that a time limit stopped depends on the
    /// clock, so it is handed back and never stored, and the next evaluation
    /// solves again (deadline stops are not cached).
    pub fn produce_value_if<T, F>(&mut self, op: &str, fid: &FeatureId, span: Option<Span>, key: OpKey, run: F) -> Result<Arc<T>>
    where
        T: std::any::Any + Send + Sync,
        F: FnOnce(&mut Context) -> Result<(T, usize, bool)>,
    {
        let produced = self.memoised(op, fid, span, key, |ctx| {
            let (value, bytes, keep) = run(ctx)?;
            Ok(Produced { transient: !keep, ..Produced::payload(value, bytes) })
        })?;
        let payload = produced.payload.clone().ok_or_else(|| CadpoleError::kernel(format!("{op}: the memo entry holds no value")))?;
        payload.downcast::<T>().map_err(|_| CadpoleError::kernel(format!("{op}: the memo entry holds a value of another type")))
    }

    /// Say again what a memoised op said when it ran: its diagnostics under
    /// the same features, in the same order, and the args it set.
    fn replay(&mut self, hit: &Produced) {
        for n in &hit.notes {
            let span = self.feature(&n.feature).ok().and_then(|f| f.span.clone());
            self.push_note(n.severity, n.message.clone(), &n.feature, span, n.on_feature);
        }
        for (f, k, v) in &hit.args {
            self.set_feature_arg(f, k, v.clone());
        }
    }

    /// Add what an op produced as bodies of `fid`, named `name`, `name#1`,
    /// … as every multi-output op names them, each carrying its identity.
    /// The bodies are moved out when nothing else holds the entry — an
    /// evaluation with no memo — and cloned out of the memo's copy
    /// otherwise, so a one-shot `cadpole eval` copies no mesh it did not
    /// copy before.
    pub(crate) fn add_produced(&mut self, fid: &FeatureId, produced: Arc<Produced>, name: Option<String>) -> Result<Vec<BodyId>> {
        let bodies: Vec<ProducedBody> = match Arc::try_unwrap(produced) {
            Ok(p) => p.bodies,
            Err(shared) => {
                let _span = kernel::trace::Span::start(format!("memo hit: clone {} bod(ies) of {fid}", shared.bodies.len()));
                shared.bodies.clone()
            }
        };
        let mut ids = Vec::with_capacity(bodies.len());
        for (i, b) in bodies.into_iter().enumerate() {
            let n = if i == 0 { name.clone() } else { name.as_ref().map(|n| format!("{n}#{i}")) };
            let id = self.add_body_keyed(fid, b.body, n, b.names, b.key)?;
            if let Some(m) = b.mesh {
                self.body_mut(id)?.cache = Some(m);
            }
            ids.push(id);
        }
        Ok(ids)
    }

    /// The inputs that route a boolean between the kernels and decide what
    /// the mesh kernel sees: both tolerances, the face budget, the engine
    /// and the snap switch. Part of every boolean's key, because each one
    /// changes the answer.
    pub(crate) fn routing_key(&self, k: &mut KeyBuilder) {
        k.tag("routing")
            .f64(self.tolerance)
            .f64(self.boolean_tolerance)
            .u64(self.brep_face_budget as u64)
            .put(&self.mesh_engine())
            .bool(mesh_kernel::snap_enabled());
    }

    /// The drag preview's question: is this op, whose key this is,
    /// one the preview skips? Yes while a parameter is being dragged and the
    /// op's inputs changed — that is, the memo has no entry for them.
    fn deferred(&self, key: &OpKey) -> bool {
        self.preview && !self.memo.as_ref().is_some_and(|m| m.contains(key))
    }

    /// The stand-in for a deferred fuse: a rigid copy of `a` under
    /// `fid`, so the script gets a body back and everything downstream
    /// still runs, sharing `a`'s tessellation. Unkeyed on purpose, so
    /// nothing downstream of it is stored under a real key.
    fn stand_in(&mut self, fid: &FeatureId, a: BodyId, name: Option<String>) -> Result<BodyId> {
        let (body, names, material, layer) = {
            let src = self.body(a)?;
            (src.body.clone(), src.names.clone(), src.material.clone(), src.layer.clone())
        };
        let id = self.add_body_named(fid, body, name, names)?;
        let b = self.body_mut(id)?;
        b.material = material;
        b.layer = layer;
        b.derived_from = Some((a, Transform::identity()));
        b.key = BodyKey::Unkeyed;
        Ok(id)
    }

    /// Cut `body` in two with a plane, or with another body's surface.
    ///
    /// This is what makes a too-tall model printable: the bed-fit check names
    /// the plane to cut on ([`crate::naming`] and `cadpole_print::bed`), and this
    /// is the operation that does it.
    ///
    /// **How.** A plane cut intersects the body with each of the two
    /// half-spaces the plane divides space into, as a box big enough to
    /// swallow the body whole. A body cut is `a & b` and `a - b`. Either way
    /// the work is done by [`Context::boolean_pair`], so a split routes
    /// between the two kernels by exactly the same rules as a `union` and
    /// inherits the same naming machinery: every surviving face keeps its
    /// name, a face the cut divided becomes `name#k`, and the new face is
    /// `<feature>/cut` because the tool that made it says so
    /// ([`naming::Generator::Cut`]).
    ///
    /// **Order.** The pieces come back ordered along the plane normal: index 0
    /// is the piece on the *negative* side, index 1 the positive side. For the
    /// usual horizontal cut that reads as "bottom, then top". A cut by a body
    /// gives the inside piece first, then the outside.
    pub fn split(
        &mut self,
        body: BodyId,
        cut: &SplitTool,
        kernel_choice: KernelChoice,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        let source = self.body(body)?;
        let inputs = vec![source.feature.clone()];
        let material = source.material.clone();
        let layer = source.layer.clone();
        let source_names = source.names.clone();
        let source_body = source.body.clone();

        let mut args = BTreeMap::new();
        args.insert("body".into(), self.body(body)?.name.clone());
        match cut {
            SplitTool::Plane(p) => {
                args.insert("origin".into(), fmt_vec(p.origin));
                args.insert("normal".into(), fmt_vec(p.normal));
            }
            SplitTool::Body(id) => {
                args.insert("by".into(), self.body(*id)?.name.clone());
            }
        }
        if kernel_choice != KernelChoice::Auto {
            args.insert("kernel".into(), kernel_choice.name().to_owned());
        }
        if let SplitTool::Body(id) = cut {
            let f = self.body(*id)?.feature.clone();
            let mut inputs = inputs.clone();
            inputs.push(f);
            return self.split_by_body(body, *id, inputs, args, kernel_choice, name, span);
        }
        let SplitTool::Plane(plane) = cut else { unreachable!() };

        // The tool has to reach past the body on every side, and the cut has
        // to actually cross it: a plane that misses is a mistake worth saying
        // out loud rather than a body and an empty one.
        let (lo, hi) = {
            let (mesh, _) = self.body_mesh(body)?;
            mesh.bbox().ok_or_else(|| CadpoleError::invalid("split: the body has no geometry"))?
        };
        let height = |v: Vec3| (v - plane.origin).dot(plane.normal);
        let corners = [
            Vec3::new(lo.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, lo.z),
            Vec3::new(lo.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, lo.z),
            Vec3::new(lo.x, lo.y, hi.z),
            Vec3::new(hi.x, lo.y, hi.z),
            Vec3::new(lo.x, hi.y, hi.z),
            Vec3::new(hi.x, hi.y, hi.z),
        ];
        let (mut below, mut above) = (false, false);
        for c in corners {
            if height(c) < -1e-9 {
                below = true;
            }
            if height(c) > 1e-9 {
                above = true;
            }
        }
        if !(below && above) {
            return Err(CadpoleError::invalid(format!(
                "split: the plane through {} with normal {} does not cross the body (its bounding box is \
                 entirely on one side), so there is nothing to cut",
                fmt_vec(plane.origin),
                fmt_vec(plane.normal)
            )));
        }

        let fid = self.new_feature("split", name.clone(), inputs, args, span.clone())?;
        let generator = Arc::new(naming::Generator::Cut { plane: *plane });
        // The half-spaces are sized from the body's mesh, so its bounds are
        // an input too.
        let mut key = KeyBuilder::new("split");
        key.put(&fid).body(&self.body(body)?.key).put(plane).put(&kernel_choice);
        key.put(&self.tessellation_for(body, None)?.0);
        self.routing_key(&mut key);
        let produced = self.memoised("split", &fid, span.clone(), key.finish(), |ctx| {
            let mut pieces = Vec::new();
            // Negative side first, then positive: index 0 is "below the plane".
            for sign in [-1.0f64, 1.0] {
                let tool = half_space(plane, &corners, sign)?;
                let tool_names = NameSource::Generator { feature: fid.clone(), generator: generator.clone() };
                let (piece, names) = ctx.boolean_pair(
                    BoolOp::Intersect,
                    source_body.clone(),
                    &source_names,
                    &tool,
                    &tool_names,
                    kernel_choice,
                    &fid,
                    span.clone(),
                )?;
                pieces.push(ProducedBody::new(piece, names));
            }
            Ok(Produced::bodies(pieces))
        })?;

        self.consume_body(body)?;
        let ids = self.add_produced(&fid, produced, name)?;
        for id in &ids {
            self.body_mut(*id)?.material = material.clone();
            self.body_mut(*id)?.layer = layer.clone();
        }
        Ok(ids)
    }

    /// `split(body, by=other)`: the piece inside `other`, then the piece
    /// outside it. `other` is left alone — a cutter is not consumed, because
    /// the same cutter usually cuts several bodies.
    #[allow(clippy::too_many_arguments)]
    fn split_by_body(
        &mut self,
        body: BodyId,
        tool: BodyId,
        inputs: Vec<FeatureId>,
        args: BTreeMap<String, String>,
        kernel_choice: KernelChoice,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Vec<BodyId>> {
        let source = self.body(body)?;
        let material = source.material.clone();
        let layer = source.layer.clone();
        let source_names = source.names.clone();
        let source_body = source.body.clone();
        let tool_body = self.body(tool)?.body.clone();
        let tool_names = self.body(tool)?.names.clone();
        let fid = self.new_feature("split", name.clone(), inputs, args, span.clone())?;
        let mut key = KeyBuilder::new("split_by");
        key.put(&fid).body(&self.body(body)?.key).body(&self.body(tool)?.key).put(&kernel_choice);
        self.routing_key(&mut key);
        let produced = self.memoised("split", &fid, span.clone(), key.finish(), |ctx| {
            let mut pieces = Vec::new();
            for op in [BoolOp::Intersect, BoolOp::Subtract] {
                let (piece, names) = ctx.boolean_pair(
                    op,
                    source_body.clone(),
                    &source_names,
                    &tool_body,
                    &tool_names,
                    kernel_choice,
                    &fid,
                    span.clone(),
                )?;
                pieces.push(ProducedBody::new(piece, names));
            }
            Ok(Produced::bodies(pieces))
        })?;
        self.consume_body(body)?;
        let ids = self.add_produced(&fid, produced, name)?;
        for id in &ids {
            self.body_mut(*id)?.material = material.clone();
            self.body_mut(*id)?.layer = layer.clone();
        }
        Ok(ids)
    }

    pub fn transform(
        &mut self,
        body: BodyId,
        t: &Transform,
        kind: &str,
        args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<BodyId> {
        let src = self.body(body)?;
        let inputs = vec![src.feature.clone()];
        let material = src.material.clone();
        let layer = src.layer.clone();
        let moved = src.body.transformed(t);
        // A moved body is the source moved: its identity is the source's
        // and the matrix, under no feature, so a copy placed by another
        // feature at the same place is the same body to the memo.
        let key = {
            let mut k = KeyBuilder::new("transform");
            k.body(&src.key).put(t);
            k.finish().body_key(0)
        };
        let names = src.names.moved(t);
        let fid = self.new_feature(kind, name.clone(), inputs, args, span)?;
        self.consume_body(body)?;
        let id = self.add_body_named(&fid, moved, name, names)?;
        {
            let b = self.body_mut(id)?;
            b.material = material;
            b.layer = layer;
            b.derived_from = Some((body, *t));
            b.key = key;
        }
        Ok(id)
    }

    /// A copy of a body (the original stays visible).
    pub fn duplicate(&mut self, body: BodyId, t: &Transform, name: Option<String>, span: Option<Span>) -> Result<BodyId> {
        let src = self.body(body)?;
        let inputs = vec![src.feature.clone()];
        let material = src.material.clone();
        let layer = src.layer.clone();
        let moved = src.body.transformed(t);
        let key = {
            let mut k = KeyBuilder::new("transform");
            k.body(&src.key).put(t);
            k.finish().body_key(0)
        };
        let names = src.names.moved(t);
        let fid = self.new_feature("copy", name.clone(), inputs, BTreeMap::new(), span)?;
        let id = self.add_body_named(&fid, moved, name, names)?;
        {
            let b = self.body_mut(id)?;
            b.material = material;
            b.layer = layer;
            b.derived_from = Some((body, *t));
            b.key = key;
        }
        Ok(id)
    }

    // ---- output

    pub fn bbox(&mut self) -> Option<(Vec3, Vec3)> {
        let ids: Vec<BodyId> = self.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
        let mut acc: Option<(Vec3, Vec3)> = None;
        for id in ids {
            if let Ok((mesh, _)) = self.body_mesh(id) {
                if let Some((lo, hi)) = mesh.bbox() {
                    acc = Some(match acc {
                        None => (lo, hi),
                        Some((a, b)) => (a.min(lo), b.max(hi)),
                    });
                }
            }
        }
        acc
    }

    pub fn to_model(&mut self, skip_mesh: bool, eval_millis: f64) -> pb::Model {
        let ids: Vec<BodyId> = self.bodies.iter().map(|b| b.id).collect();
        // The part bodies the visible instances share: sent once,
        // invisible, with the mesh; every instance body then points at one
        // of them and carries no mesh of its own.
        let shared = self.shared_part_bodies();
        let mut bodies = Vec::new();
        for id in ids {
            let (visible, name, feature, material, instance, layer) = {
                let b = self.body(id).unwrap();
                (b.visible, b.name.clone(), b.feature.0.clone(), b.material.clone(), b.instance, b.layer.clone())
            };
            let is_shared = !visible && shared.contains(&id);
            if !visible && !is_shared {
                continue;
            }
            // An instance body is displayed under its pose plus the
            // exploded-view offset; its cached mesh is at the pose alone.
            let display = if visible && instance.is_some() {
                match self.display_transform(id) {
                    Ok(d) => d,
                    Err(e) => {
                        self.diagnostics.push(Diagnostic::error(e.to_string()));
                        continue;
                    }
                }
            } else {
                None
            };
            let offset = match instance {
                Some(_) if visible => match self.body_view_offset(id) {
                    Ok(o) => o,
                    Err(_) => Vec3::ZERO,
                },
                _ => Vec3::ZERO,
            };
            let (part_body, transform) = match &display {
                Some((src, t)) => (self.body(*src).map(|b| b.id_string()).unwrap_or_default(), Some(t.to_proto())),
                None => (String::new(), None),
            };
            let instance_id = instance
                .and_then(|i| self.instance_record(i).ok().map(|r| r.feature.0.clone()))
                .unwrap_or_default();
            let (mesh, topo) = match self.body_mesh(id) {
                Ok(x) => x,
                Err(e) => {
                    self.diagnostics.push(Diagnostic::error(e.to_string()));
                    continue;
                }
            };
            let shift = Transform::translation(offset);
            let moved_topo;
            let topo = if offset.length() > 0.0 {
                moved_topo = topo.transformed(&shift);
                &moved_topo
            } else {
                topo
            };
            let bbox = mesh.bbox().map(|(lo, hi)| {
                let (lo, hi) = if offset.length() > 0.0 { (lo + offset, hi + offset) } else { (lo, hi) };
                pb::BBox { min: Some(lo.to_proto()), max: Some(hi.to_proto()) }
            });
            bodies.push(pb::Body {
                id: format!("{feature}#{}", id.0),
                feature_id: feature,
                name,
                bbox,
                topology: Some(topo.to_proto()),
                mesh: if skip_mesh || display.is_some() { None } else { Some(mesh.to_proto()) },
                visible,
                material: Some(material.to_proto()),
                part_body,
                transform,
                instance: instance_id,
                layer,
            });
        }
        let (parts, instances, assemblies) = self.assembly_lists();
        pb::Model {
            parts,
            instances,
            assemblies,
            display_units: self.display_units.as_ref().map(|u| pb::DisplayUnits { unit: u.unit.clone(), fraction: u.fraction }),
            animations: self.animations.iter().map(|a| a.to_proto()).collect(),
            layers: self.layers.iter().enumerate().map(|(i, l)| l.to_proto(i)).collect(),
            render_defaults: self.render_defaults.as_ref().map(RenderDefaults::to_proto),
            // The plate layout is the engine's to fill (`cadpole_engine::print::
            // plate_plan`): it needs the export crate's grid and the filament
            // palette, neither of which this crate sees.
            plates: Vec::new(),
            params: self
                .params
                .iter()
                .map(|(d, v)| pb::ParamInfo {
                    name: d.name.clone(),
                    default_value: Some(d.default.to_proto()),
                    value: Some(v.to_proto()),
                    overridden: self.overrides.contains_key(&d.name),
                    description: d.description.clone(),
                    unit: d.unit.clone(),
                    min: d.min,
                    max: d.max,
                    step: d.step,
                    span: d.span.as_ref().map(Span::to_proto),
                    choices: d.choices.clone(),
                })
                .collect(),
            features: self
                .features
                .iter()
                .map(|f| pb::Feature {
                    id: f.id.0.clone(),
                    kind: f.kind.clone(),
                    name: f.name.clone(),
                    inputs: f.inputs.iter().map(|i| i.0.clone()).collect(),
                    outputs: f.outputs.iter().map(|b| format!("{}#{}", f.id, b.0)).collect(),
                    args: f.args.clone().into_iter().collect(),
                    span: f.span.as_ref().map(Span::to_proto),
                    suppressed: false,
                    diagnostics: f.diagnostics.iter().map(Diagnostic::to_proto).collect(),
                    cached: self.feature_cached(&f.id.0),
                })
                .collect(),
            bodies,
            diagnostics: self.diagnostics.iter().map(Diagnostic::to_proto).collect(),
            eval_millis,
            memo: Some(self.memo_report()),
        }
    }

    /// Whether every kernel op of this feature was served from the memo:
    /// what the tree shows as "cached".
    pub fn feature_cached(&self, feature: &str) -> bool {
        let c = &self.memo_counts;
        c.hits.iter().any(|r| r.feature == feature)
            && !c.runs.iter().any(|r| r.feature == feature)
            && !c.deferred.iter().any(|r| r.feature == feature)
    }

    /// What the memo did for this evaluation, and where the store stands.
    /// All zero without a memo.
    pub fn memo_report(&self) -> pb::MemoReport {
        let stats = self.memo.as_ref().map(|m| m.stats()).unwrap_or_default();
        pb::MemoReport {
            ops_run: self.memo_counts.runs.len() as u32,
            ops_hit: self.memo_counts.hits.len() as u32,
            ops_deferred: self.memo_counts.deferred.len() as u32,
            entries: stats.entries as u32,
            bytes: stats.bytes as u64,
            limit_bytes: stats.limit as u64,
            preview: self.preview,
        }
    }
}

pub fn fmt_num(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

pub fn fmt_vec(v: Vec3) -> String {
    format!("({}, {}, {})", fmt_num(v.x), fmt_num(v.y), fmt_num(v.z))
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmath;
    use crate::geom::Plane;
    use crate::sketch::{Loop, P2};
    use crate::tessellate::FaceInfo;

    #[test]
    fn feature_ids_are_numbered_per_kind_and_names_are_unique() {
        let mut ctx = Context::new();
        let a = ctx.new_feature("extrude", None, vec![], BTreeMap::new(), None).unwrap();
        let b = ctx.new_feature("extrude", None, vec![], BTreeMap::new(), None).unwrap();
        let c = ctx.new_feature("sketch", Some("base".into()), vec![], BTreeMap::new(), None).unwrap();
        assert_eq!(a.0, "extrude_1");
        assert_eq!(b.0, "extrude_2");
        assert_eq!(c.0, "base");
        assert!(ctx.new_feature("sketch", Some("base".into()), vec![], BTreeMap::new(), None).is_err());
    }

    fn prism(ctx: &mut Context, w: f64, d: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
        ctx.extrude("extrude", &s, None, h, BTreeMap::new(), Some(name.to_owned()), None).unwrap()[0]
    }

    #[test]
    fn split_by_a_plane_gives_two_pieces_whose_volumes_add_up() {
        let mut ctx = Context::new();
        let b = prism(&mut ctx, 20.0, 20.0, 40.0, "tall");
        let plane = Plane::new(Vec3::new(0.0, 0.0, 25.0), Vec3::Z, Vec3::X).unwrap();
        let pieces = ctx.split(b, &SplitTool::Plane(plane), KernelChoice::Brep, Some("half".into()), None).unwrap();
        assert_eq!(pieces.len(), 2);
        // Ordered along the normal: the lower piece first.
        let vols: Vec<f64> = pieces.iter().map(|p| ctx.body_mesh(*p).unwrap().0.volume()).collect();
        assert!((vols[0] - 20.0 * 20.0 * 25.0).abs() < 1.0, "lower piece {vols:?}");
        assert!((vols[1] - 20.0 * 20.0 * 15.0).abs() < 1.0, "upper piece {vols:?}");
        assert!(!ctx.body(b).unwrap().visible, "the source body is consumed");
        assert!(pieces.iter().all(|p| ctx.body(*p).unwrap().visible));
        assert_eq!(ctx.body(pieces[0]).unwrap().name, "half");
        assert_eq!(ctx.body(pieces[1]).unwrap().name, "half#1");
    }

    #[test]
    fn split_names_the_new_face_cut_and_keeps_the_old_names() {
        let mut ctx = Context::new();
        let b = prism(&mut ctx, 20.0, 20.0, 40.0, "tall");
        let source_face = {
            let (mesh, topo) = ctx.body_mesh(b).unwrap();
            let _ = mesh;
            topo.faces[0].name.clone()
        };
        assert!(source_face.starts_with("tall/"), "{source_face}");
        let plane = Plane::new(Vec3::new(0.0, 0.0, 25.0), Vec3::Z, Vec3::X).unwrap();
        let pieces = ctx.split(b, &SplitTool::Plane(plane), KernelChoice::Brep, None, None).unwrap();
        for p in &pieces {
            let (_, topo) = ctx.body_mesh(*p).unwrap();
            let names: Vec<String> = topo.faces.iter().map(|f| f.name.clone()).collect();
            assert!(names.iter().any(|n| n == "split_1/cut"), "no cut face in {names:?}");
            assert!(
                names.iter().filter(|n| n.starts_with("tall/")).count() >= 4,
                "the original faces must keep their names: {names:?}"
            );
            assert!(!names.iter().any(|n| n.contains("/tool[")), "the tool's own faces must not survive: {names:?}");
        }
    }

    #[test]
    fn a_plane_that_misses_the_body_is_a_diagnostic_not_two_pieces() {
        let mut ctx = Context::new();
        let b = prism(&mut ctx, 20.0, 20.0, 10.0, "flat");
        let plane = Plane::new(Vec3::new(0.0, 0.0, 500.0), Vec3::Z, Vec3::X).unwrap();
        let e = ctx.split(b, &SplitTool::Plane(plane), KernelChoice::Brep, None, None).unwrap_err();
        assert!(e.to_string().contains("does not cross the body"), "{e}");
        assert!(ctx.body(b).unwrap().visible, "a refused split leaves the body alone");
    }

    #[test]
    fn split_by_a_body_gives_the_inside_piece_then_the_outside_one() {
        let mut ctx = Context::new();
        let a = prism(&mut ctx, 20.0, 20.0, 20.0, "block");
        // A slab crossing the block: inside it, then outside it.
        let s = Sketch::new(Plane::XY.offset(-5.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 40.0, 40.0));
        let tool = ctx.extrude("extrude", &s, None, 13.0, BTreeMap::new(), Some("slab".into()), None).unwrap()[0];
        let pieces = ctx.split(a, &SplitTool::Body(tool), KernelChoice::Brep, Some("part".into()), None).unwrap();
        assert_eq!(pieces.len(), 2);
        let vols: Vec<f64> = pieces.iter().map(|p| ctx.body_mesh(*p).unwrap().0.volume()).collect();
        // The slab covers z in -5..8, so the inside piece is 8 mm tall and the
        // outside one 12 mm.
        assert!((vols[0] - 20.0 * 20.0 * 8.0).abs() < 1.0, "inside {vols:?}");
        assert!((vols[1] - 20.0 * 20.0 * 12.0).abs() < 1.0, "outside {vols:?}");
        assert!(ctx.body(tool).unwrap().visible, "a cutter body is not consumed");
    }

    #[test]
    fn extrude_then_subtract_produces_one_visible_body() {
        let mut ctx = Context::new();
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0));
        let a = ctx.extrude("extrude", &s, None, 10.0, BTreeMap::new(), Some("block".into()), None).unwrap()[0];
        let s2 = Sketch::new(Plane::XY.offset(-1.0)).with_loop(Loop::circle(P2::new(0.0, 0.0), 3.0));
        let b = ctx.extrude("extrude", &s2, None, 12.0, BTreeMap::new(), Some("hole".into()), None).unwrap()[0];
        let r = ctx.boolean(BoolOp::Subtract, a, &[b], KernelChoice::Auto, None, None).unwrap();
        let model = ctx.to_model(false, 0.0);
        assert_eq!(model.bodies.len(), 1);
        assert_eq!(model.bodies[0].feature_id, "subtract_1");
        assert_eq!(model.features.len(), 3);
        assert!(ctx.body(r).unwrap().visible);
        assert!(!ctx.body(a).unwrap().visible);
        let topo = model.bodies[0].topology.as_ref().unwrap();
        assert_eq!(topo.faces.len(), 8, "box with a through hole has 6 + 2 faces");
    }

    /// The four sections of a Gridfinity foot.
    fn foot_sections() -> Vec<crate::loft::Section> {
        [(-5.0, 3.2), (-4.2, 2.4), (-2.4, 2.4), (0.0, 0.0)]
            .into_iter()
            .map(|(z, inset): (f64, f64)| {
                crate::loft::Section::Profile(Sketch::new(Plane::XY.offset(z)).with_loop(
                    Loop::rounded_rect(P2::new(0.0, 0.0), 42.0 - 2.0 * inset, 42.0 - 2.0 * inset, 4.0 - inset),
                ))
            })
            .collect()
    }

    #[test]
    fn a_loft_names_its_caps_and_one_side_per_band_and_segment() {
        let mut ctx = Context::new();
        let id = ctx
            .loft(
                "loft",
                &foot_sections(),
                &[],
                true,
                crate::loft::SectionMatch::Index,
                [crate::loft::Cap::Plane; 2],
                BTreeMap::new(),
                Some("foot".into()),
                None,
            )
            .unwrap()[0];
        let names = names_of(&mut ctx, id);
        assert_eq!(names.len(), 26, "3 bands x 8 segments + 2 caps");
        assert!(names.contains(&"foot/start".to_string()), "{names:?}");
        assert!(names.contains(&"foot/end".to_string()), "{names:?}");
        // Corner 1's three bands: the bottom chamfer, the straight band and the
        // 45-degree top chamfer, each selectable on its own.
        for b in 0..3 {
            assert!(names.contains(&format!("foot/side[0.1]#{b}")), "{names:?}");
        }
        // ...and the unsuffixed name is the group alias for the whole column.
        let grouped: Vec<_> = ctx
            .body_mesh(id)
            .unwrap()
            .1
            .faces
            .iter()
            .filter(|f| f.aliases.iter().any(|a| a == "foot/side[0.1]"))
            .collect();
        assert_eq!(grouped.len(), 3, "the group alias covers every band of that corner");
    }

    #[test]
    fn the_loft_names_a_band_by_where_it_is_not_by_the_order_faces_came_out() {
        // The band a face is named for is decided against points the loft
        // planted on its own surfaces, so the name says which pair of sections
        // the face lies between — checkable from the face's own height.
        let mut ctx = Context::new();
        let id = ctx
            .loft(
                "loft",
                &foot_sections(),
                &[],
                true,
                crate::loft::SectionMatch::Index,
                [crate::loft::Cap::Plane; 2],
                BTreeMap::new(),
                Some("foot".into()),
                None,
            )
            .unwrap()[0];
        let bounds = [(-5.0, -4.2), (-4.2, -2.4), (-2.4, 0.0)];
        for f in &ctx.body_mesh(id).unwrap().1.faces {
            let Some(rest) = f.name.strip_prefix("foot/side[") else { continue };
            let band: usize = rest.split('#').nth(1).unwrap().parse().unwrap();
            let (lo, hi) = bounds[band];
            assert!(
                f.centroid.z >= lo - 1e-6 && f.centroid.z <= hi + 1e-6,
                "{} has centroid z {} but band {band} spans {lo}..{hi}",
                f.name,
                f.centroid.z
            );
        }
    }

    #[test]
    fn a_cone_names_its_faces_like_the_other_primitives() {
        let mut ctx = Context::new();
        let id = ctx.cone(Vec3::ZERO, 3.275, 3.075, 0.2, Some("mouth".into()), None).unwrap();
        let names = names_of(&mut ctx, id);
        assert!(names.contains(&"mouth/bottom".to_string()), "{names:?}");
        assert!(names.contains(&"mouth/top".to_string()), "{names:?}");
        // A circle is two arcs, so the wall is two faces under one group name.
        let wall: Vec<_> = names.iter().filter(|n| n.starts_with("mouth/side")).collect();
        assert_eq!(wall.len(), 2, "{names:?}");

        let mut ctx = Context::new();
        let id = ctx.cone(Vec3::ZERO, 5.0, 0.0, 9.0, Some("tip".into()), None).unwrap();
        let names = names_of(&mut ctx, id);
        assert!(names.contains(&"tip/bottom".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n == "tip/top"), "a cone to a point has no top: {names:?}");
    }

    #[test]
    fn a_merging_pattern_returns_one_body_that_holds_every_copy() {
        let mut ctx = Context::new();
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let a = ctx.extrude("box", &s, None, 4.0, BTreeMap::new(), Some("tile".into()), None).unwrap()[0];
        let steps: Vec<Transform> = (0..3)
            .map(|i| Transform::translation(Vec3::new(30.0 * i as f64, 0.0, 0.0)))
            .collect();
        let out = ctx
            .pattern("pattern_linear", a, &steps, true, KernelChoice::Auto, BTreeMap::new(), Some("row".into()), None)
            .unwrap();
        assert_eq!(out.len(), 1, "merge=True is one body");
        assert_eq!(ctx.body(out[0]).unwrap().kind(), "mesh", "the fuse goes to the mesh kernel");
        assert!(!ctx.body(a).unwrap().visible, "the source is consumed");
        let (mesh, _) = ctx.body_mesh(out[0]).unwrap();
        let v = mesh.welded(1e-4).volume().abs();
        assert!((v - 3.0 * 400.0).abs() < 1e-3, "volume {v}");
        // Each copy keeps the source's face names; three copies of one name
        // become the canonical pieces of that name.
        let names = names_of(&mut ctx, out[0]);
        let ends: Vec<_> = names.iter().filter(|n| n.starts_with("tile/top")).collect();
        assert_eq!(ends.len(), 3, "{names:?}");
    }

    #[test]
    fn a_merging_pattern_fuses_copies_that_only_touch() {
        // The Gridfinity case: two feet that meet along one edge only, which
        // the exact kernel refuses outright. The merge must still produce one
        // watertight body of the right volume.
        let mut ctx = Context::new();
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let a = ctx.extrude("box", &s, None, 4.0, BTreeMap::new(), Some("tile".into()), None).unwrap()[0];
        let steps = vec![
            Transform::identity(),
            Transform::translation(Vec3::new(10.0, 0.0, 0.0)),
            Transform::translation(Vec3::new(20.0, 0.0, 0.0)),
        ];
        let out = ctx
            .pattern("pattern_linear", a, &steps, true, KernelChoice::Auto, BTreeMap::new(), Some("row".into()), None)
            .unwrap();
        let (mesh, _) = ctx.body_mesh(out[0]).unwrap();
        let welded = mesh.welded(1e-4);
        assert!(welded.is_watertight(), "touching copies must still fuse into one closed body");
        assert!((welded.volume().abs() - 1200.0).abs() < 1e-3, "volume {}", welded.volume());
    }

    #[test]
    fn a_pattern_without_merge_is_unchanged() {
        let mut ctx = Context::new();
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let a = ctx.extrude("box", &s, None, 4.0, BTreeMap::new(), Some("tile".into()), None).unwrap()[0];
        let steps: Vec<Transform> =
            (0..3).map(|i| Transform::translation(Vec3::new(30.0 * i as f64, 0.0, 0.0))).collect();
        let out = ctx
            .pattern("pattern_linear", a, &steps, false, KernelChoice::Auto, BTreeMap::new(), Some("row".into()), None)
            .unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], a, "the original is the first instance");
        assert!(ctx.body(a).unwrap().visible, "nothing is consumed");
    }

    #[test]
    fn copies_reuse_the_source_tessellation_and_keep_names() {
        let mut ctx = Context::new();
        let s = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 5.0));
        let a = ctx.extrude("cylinder", &s, None, 10.0, BTreeMap::new(), Some("post".into()), None).unwrap()[0];
        let t = Transform::translation(Vec3::new(30.0, 0.0, 0.0));
        let b = ctx.duplicate(a, &t, Some("post2".into()), None).unwrap();
        let (m0, t0) = ctx.body_mesh(a).unwrap().clone();
        let (m1, t1) = ctx.body_mesh(b).unwrap().clone();
        assert_eq!(m0.triangle_count(), m1.triangle_count());
        assert_eq!(m0.indices, m1.indices, "copy reuses the source triangulation");
        // Names are identical, positions are shifted by exactly the transform.
        assert_eq!(
            t0.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            t1.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>()
        );
        for (f0, f1) in t0.faces.iter().zip(t1.faces.iter()) {
            assert!((f1.centroid.x - f0.centroid.x - 30.0).abs() < 1e-9);
            assert!((f1.area - f0.area).abs() < 1e-9, "area is preserved by a rigid move");
        }
        let (lo0, hi0) = m0.bbox().unwrap();
        let (lo1, hi1) = m1.bbox().unwrap();
        assert!((lo1.x - lo0.x - 30.0).abs() < 1e-4 && (hi1.x - hi0.x - 30.0).abs() < 1e-4);
        // A rotated copy keeps its normals unit length.
        let r = Transform::rotation(Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_3);
        let c = ctx.duplicate(a, &r, Some("post3".into()), None).unwrap();
        let (mc, tc) = ctx.body_mesh(c).unwrap().clone();
        for n in mc.normals.chunks_exact(3) {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((l - 1.0).abs() < 1e-5, "normal length {l}");
        }
        let top = tc.faces.iter().find(|f| f.surface_kind == "plane" && f.area > 70.0).unwrap();
        assert!(top.normal.length() > 0.99);
    }

    fn block(ctx: &mut Context, at: P2, w: f64, d: f64, z: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY.offset(z)).with_loop(Loop::rect(at, w, d));
        ctx.extrude("box", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    fn volume(ctx: &mut Context, id: BodyId) -> f64 {
        ctx.body_mesh(id).unwrap().0.volume()
    }

    /// The identity the B-rep path is held to holds on the mesh path too.
    #[test]
    fn mesh_route_keeps_the_boolean_volume_identities() {
        for (dx, dz) in [(8.0, 2.0), (12.0, -3.0), (3.0, 0.0)] {
            let mut ctx = Context::new();
            let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
            let b = block(&mut ctx, P2::new(dx, 6.0), 15.0, 12.0, dz, 8.0, "b");
            let (va, vb) = (volume(&mut ctx, a), volume(&mut ctx, b));
            let a2 = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a2");
            let b2 = block(&mut ctx, P2::new(dx, 6.0), 15.0, 12.0, dz, 8.0, "b2");
            let u = ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Mesh, None, None).unwrap();
            let i = ctx.boolean(BoolOp::Intersect, a2, &[b2], KernelChoice::Mesh, None, None).unwrap();
            assert_eq!(ctx.body(u).unwrap().kind(), "mesh");
            let (vu, vi) = (volume(&mut ctx, u), volume(&mut ctx, i));
            assert!(((vu + vi) - (va + vb)).abs() < 1e-6 * (va + vb), "{vu} + {vi} != {va} + {vb}");
            // And the result is a closed surface, not a pile of triangles.
            let m = ctx.body_mesh(u).unwrap().0.welded(crate::mesh_kernel::WELD_EPS);
            assert!(m.is_watertight(), "mesh union is not watertight");
        }
    }

    /// Everything a selection, an export or a print check reads off a body.
    fn observable(ctx: &mut Context, id: BodyId) -> String {
        let (mesh, topo) = ctx.body_mesh(id).unwrap();
        let mut out = format!("{:.9} {}t {}f", mesh.volume(), mesh.triangle_count(), topo.faces.len());
        for f in &topo.faces {
            out.push_str(&format!("|{} {:?} {} {}", f.name, f.aliases, f.surface_kind, f.tri_count));
        }
        for e in &topo.edges {
            out.push_str(&format!("|{}", e.name));
        }
        for v in &topo.vertices {
            out.push_str(&format!("|{}", v.name));
        }
        out
    }

    /// A fan of tools that cannot touch each other in one `subtract` call.
    /// The batch has to answer exactly what subtracting them one at a
    /// time answers: the same volume, the same faces in the same order under
    /// the same names, and the same edges and vertices derived from them —
    /// because that is what a saved selection resolves through.
    #[test]
    fn a_fan_of_separated_tools_answers_what_one_at_a_time_answers() {
        let make = |ctx: &mut Context, tag: &str| {
            let plate = block(ctx, P2::new(0.0, 0.0), 200.0, 40.0, 0.0, 10.0, &format!("{tag}plate"));
            let tools: Vec<BodyId> = (0..4)
                .map(|i| {
                    block(
                        ctx,
                        P2::new(-60.0 + 30.0 * i as f64, 0.0),
                        6.0,
                        6.0,
                        -5.0,
                        20.0,
                        &format!("{tag}t{i}"),
                    )
                })
                .collect();
            (plate, tools)
        };
        for op in [BoolOp::Subtract, BoolOp::Union] {
            let mut ctx = Context::new();
            let (plate, tools) = make(&mut ctx, "");
            let batched = ctx.boolean(op, plate, &tools, KernelChoice::Mesh, None, None).unwrap();

            // The same model, one tool per call: four `boolean` calls instead
            // of one. Face *names* are generated by the feature that made the
            // geometry, so the step names have to be stripped of the tag for
            // the comparison — nothing else may differ.
            let mut step = Context::new();
            let (mut acc, tools) = make(&mut step, "");
            for t in tools {
                acc = step.boolean(op, acc, &[t], KernelChoice::Mesh, None, None).unwrap();
            }
            assert_eq!(observable(&mut step, acc), observable(&mut ctx, batched), "{}", op.kind_name());
        }
    }

    /// One overlapping pair in the middle of a fan must not cost the rest
    /// their batch, and must not reorder the operands: the run stops at the
    /// tool that touches an earlier one, that one is done pairwise, and the
    /// run starts again after it.
    #[test]
    fn a_fan_with_one_overlapping_pair_batches_on_both_sides_of_it() {
        let build = |ctx: &mut Context| {
            let plate = block(ctx, P2::new(0.0, 0.0), 200.0, 40.0, 0.0, 10.0, "plate");
            // t1 and t2 overlap each other; t0, t3 and t4 are clear of everything.
            let xs = [-80.0, -40.0, -37.0, 20.0, 60.0];
            let tools: Vec<BodyId> = xs
                .iter()
                .enumerate()
                .map(|(i, x)| block(ctx, P2::new(*x, 0.0), 6.0, 6.0, -5.0, 20.0, &format!("t{i}")))
                .collect();
            (plate, tools)
        };
        let mut ctx = Context::new();
        let (plate, tools) = build(&mut ctx);
        let batched = ctx.boolean(BoolOp::Subtract, plate, &tools, KernelChoice::Mesh, None, None).unwrap();

        let mut step = Context::new();
        let (mut acc, tools) = build(&mut step);
        for t in tools {
            acc = step.boolean(BoolOp::Subtract, acc, &[t], KernelChoice::Mesh, None, None).unwrap();
        }
        assert_eq!(observable(&mut step, acc), observable(&mut ctx, batched));
    }

    /// `intersect` is not a boolean against a union, so it keeps the fold —
    /// and still has to be right.
    #[test]
    fn intersect_of_several_tools_is_still_every_tool_in_turn() {
        let mut ctx = Context::new();
        let a = block(&mut ctx, P2::new(0.0, 0.0), 40.0, 40.0, 0.0, 40.0, "a");
        let b = block(&mut ctx, P2::new(5.0, 0.0), 40.0, 40.0, 0.0, 40.0, "b");
        let c = block(&mut ctx, P2::new(0.0, 5.0), 40.0, 40.0, 0.0, 40.0, "c");
        let i = ctx.boolean(BoolOp::Intersect, a, &[b, c], KernelChoice::Mesh, None, None).unwrap();
        // The common block is 35 x 35 x 40.
        assert!((volume(&mut ctx, i) - 35.0 * 35.0 * 40.0).abs() < 1e-6, "{}", volume(&mut ctx, i));
    }

    /// Two boxes sharing part of a face: truck answers `None` or an open shell
    /// (its #57 / #114), and `auto` has to survive that. (`b` overhangs
    /// `a`'s edge: a boss wholly inside `a`'s top is glued exactly now.)
    #[test]
    fn coincident_faces_fall_back_to_the_mesh_kernel() {
        let mut ctx = Context::new();
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(8.0, 0.0), 10.0, 10.0, 10.0, 6.0, "b");
        let brep_says = {
            let (sa, sb) = (ctx.body(a).unwrap().solid().unwrap().clone(), ctx.body(b).unwrap().solid().unwrap().clone());
            kernel::boolean(BoolOp::Union, &sa, &sb, ctx.boolean_tolerance)
        };
        assert!(brep_says.is_err(), "this case is only interesting while truck fails it");
        let u = ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Auto, None, None).unwrap();
        assert_eq!(ctx.body(u).unwrap().kind(), "mesh");
        assert!((volume(&mut ctx, u) - 4600.0).abs() < 1e-6, "{}", volume(&mut ctx, u));
        let info: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Info)
            .map(|d| d.message.as_str())
            .collect();
        assert!(info.iter().any(|m| m.contains("retrying on the mesh kernel")), "{info:?}");
        // The same call with kernel="brep" is an error, not a silent demotion.
        let mut ctx = Context::new();
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(8.0, 0.0), 10.0, 10.0, 10.0, 6.0, "b");
        assert!(ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Brep, None, None).is_err());
    }

    /// The exact kernel refuses a flush pair by face index, because a bare
    /// solid has no names; the context has the names, so the routing info
    /// under `auto` and the error under `brep` both add the case in the
    /// script's own vocabulary (stage 2). The pair: a block inside
    /// another with three faces flush.
    #[test]
    fn a_brep_refusal_names_the_faces_in_the_scripts_vocabulary() {
        let named = "faces `a/bottom` and `b/bottom` are coplanar within 0.00 µm (normals 0.0 µrad apart), facing \
                     the same way: the faces are flush over 100.000 mm²; and 2 more contacts (coplanar faces, flush ×2). ";
        let mut ctx = Context::new();
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(5.0, 0.0), 10.0, 10.0, 0.0, 10.0, "b");
        let u = ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Auto, Some("u".into()), None).unwrap();
        assert_eq!(ctx.body(u).unwrap().kind(), "mesh");
        let info = ctx.diagnostics.iter().find(|d| d.message.contains("retrying on the mesh kernel")).unwrap_or_else(|| panic!("{:?}", ctx.diagnostics));
        assert_eq!(info.severity, Severity::Info);
        assert!(info.message.starts_with("the B-rep kernel could not union: kernel operation failed: union: coplanar faces, flush — the first body's face #"), "{}", info.message);
        assert!(
            info.message.ends_with(&format!(
                "; retrying on the mesh kernel. By name: {named}The mesh kernel makes the tie exact and keeps both \
                 `a/bottom` and `b/bottom` on one plane with a 0° seam between them"
            )),
            "{}",
            info.message
        );

        let mut ctx = Context::new();
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(5.0, 0.0), 10.0, 10.0, 0.0, 10.0, "b");
        let e = ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Brep, Some("u".into()), None).unwrap_err().to_string();
        assert!(e.starts_with("kernel operation failed: union: coplanar faces, flush — "), "{e}");
        assert!(
            e.ends_with(&format!(
                ". By name: {named}Overlap the bodies by at least 0.5 mm so the contact is a crossing, cut the flush \
                 part back so `a/bottom` and `b/bottom` only meet along an edge, or pass kernel=\"mesh\", where the \
                 two faces stay two faces on one plane with a 0° seam between them"
            )),
            "{e}"
        );
    }

    /// What the mesh kernel decides reaches the user as an info diagnostic on
    /// the feature, with the feature's span — the same channel as the B-rep
    /// fallback notice. Forced through the context's own field rather than
    /// `CADPOLE_MESH_ENGINE`, which is process-global and would flip every
    /// other test in this binary onto the robust engine.
    #[test]
    fn kernel_notes_become_infos_on_the_feature() {
        let mut ctx = Context::new();
        ctx.mesh_engine = Some(mesh_kernel::Engine::Robust);
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(8.0, 6.0), 15.0, 12.0, 2.0, 8.0, "b");
        let span = Span { path: "main.star".into(), start_line: 7, start_col: 1, end_line: 7, end_col: 30 };
        let u = ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Mesh, Some("u".into()), Some(span.clone())).unwrap();
        assert_eq!(ctx.body(u).unwrap().kind(), "mesh");
        let notes: Vec<&Diagnostic> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Info && d.message.contains("robust engine"))
            .collect();
        assert_eq!(notes.len(), 1, "{:?}", ctx.diagnostics);
        assert_eq!(notes[0].feature.as_ref().map(|f| f.0.as_str()), Some("u"));
        assert_eq!(notes[0].span.as_ref().map(|s| s.start_line), Some(7));
        assert!(notes[0].message.contains("do not survive"), "{}", notes[0].message);
        // The feature carries it too, so the tree shows it on the feature.
        assert!(ctx.feature(&FeatureId("u".into())).unwrap().diagnostics.iter().any(|d| d.message.contains("robust engine")));
        // And nothing is left in the sink for the next feature to claim.
        assert!(kernel::notes::is_empty());

        // The policy engine says nothing on a clean pair.
        let mut ctx = Context::new();
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(8.0, 6.0), 15.0, 12.0, 2.0, 8.0, "b");
        ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Mesh, None, None).unwrap();
        assert!(!ctx.diagnostics.iter().any(|d| d.message.contains("robust engine")), "{:?}", ctx.diagnostics);
    }

    #[test]
    fn a_pair_over_the_face_budget_skips_the_brep_kernel() {
        let mut ctx = Context::new();
        // Lower the budget instead of building a 200-face body: the rule under
        // test is "combined face count over the budget", not the constant.
        ctx.brep_face_budget = 8;
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(8.0, 6.0), 15.0, 12.0, 2.0, 8.0, "b");
        let u = ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Auto, None, None).unwrap();
        assert_eq!(ctx.body(u).unwrap().kind(), "mesh", "12 faces is over a budget of 8");
        let info: Vec<&str> = ctx.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(info.iter().any(|m| m.contains("over the B-rep budget of 8")), "{info:?}");
        // Under the budget, and with no coincident faces, the exact kernel
        // keeps the work.
        let mut ctx = Context::new();
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(8.0, 6.0), 15.0, 12.0, 2.0, 4.0, "b");
        let u = ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Auto, None, None).unwrap();
        assert_eq!(ctx.body(u).unwrap().kind(), "brep");
        // No *routing* diagnostic: the pair stayed on the exact kernel. Only
        // the two sentences that mean *this pair went to the mesh kernel* are
        // asserted against, so a future informational note cannot fail this.
        let routed: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Info
                    && (d.message.contains("routed to the mesh kernel") || d.message.contains("the mesh kernel takes it"))
            })
            .map(|d| d.message.as_str())
            .collect();
        assert!(routed.is_empty(), "no fallback expected, got {routed:?}");
    }

    /// A B-rep operand crosses into the mesh kernel at its own
    /// `tessellation(tolerance=)`, as every other reader of its triangles
    /// does, not at the document's: a bore that was set fine stays round
    /// through `kernel="mesh"`. Without one, the document's applies, and the
    /// setting is part of the boolean's memo key.
    #[test]
    fn a_mesh_boolean_tessellates_each_operand_at_its_own_tolerance() {
        fn hub_union(fine: Option<f64>) -> (usize, Vec<f32>) {
            let mut ctx = Context::new();
            let plate = block(&mut ctx, P2::new(0.0, 0.0), 60.0, 60.0, 0.0, 6.0, "plate");
            let hub_sketch = Sketch::new(Plane::XY.offset(5.5)).with_loop(Loop::circle(P2::new(30.0, 30.0), 12.0));
            let hub =
                ctx.extrude("cylinder", &hub_sketch, None, 4.0, BTreeMap::new(), Some("hub".into()), None).unwrap()[0];
            ctx.body_mut(hub).unwrap().tessellation.tolerance = fine;
            let u = ctx.boolean(BoolOp::Union, plate, &[hub], KernelChoice::Mesh, None, None).unwrap();
            let Body::Mesh((mesh, _)) = &ctx.body(u).unwrap().body else { panic!("kernel=\"mesh\" makes a mesh body") };
            (mesh.triangle_count(), mesh.positions.clone())
        }
        let (coarse, _) = hub_union(None);
        let (fine, positions) = hub_union(Some(0.005));
        assert!(2 * fine > 3 * coarse, "the hub's own 0.005 mm chord reaches the result: {fine} vs {coarse} triangles");
        // Every vertex on the hub's rim lies on the true circle, and the
        // rim's chords sag no more than the hub asked for.
        let mut rim: Vec<f64> = positions
            .chunks(3)
            .filter(|p| (p[2] as f64 - 9.5).abs() < 1e-3)
            .map(|p| (p[1] as f64 - 30.0).atan2(p[0] as f64 - 30.0))
            .collect();
        rim.sort_by(f64::total_cmp);
        rim.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        assert!(rim.len() > 16, "the rim is sampled: {} vertices", rim.len());
        let widest = rim.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
        let sag = 12.0 * (1.0 - (widest / 2.0).cos());
        assert!(sag <= 0.005 + 1e-4, "the rim's widest chord sags {sag} mm, over the hub's 0.005 mm");
    }

    /// The route is decided by the document, never by the clock.
    ///
    /// A plate ∪ boss union followed by a bolt hole — `examples/bracket.star`'s
    /// first two booleans, and a plain box ∪ cylinder that truck takes 124 to
    /// 171 ms over at the default tolerance on a quiet machine, a second or
    /// more in a debug build or under load. Under the wall-clock rule this
    /// test replaces, the union's timing decided whether the *hole* was cut
    /// by truck or by manifold: the same script came out `brep` on a quiet
    /// box and `mesh` on a loaded one, and `cadpole export --format step` of an
    /// unchanged document succeeded or was refused with the load average.
    /// Every step here is under the face budget, every operand is a B-rep,
    /// so every step stays exact — however long any of them took.
    #[test]
    fn routing_is_a_function_of_the_document_not_of_the_clock() {
        let mut ctx = Context::new();
        let plate = block(&mut ctx, P2::new(0.0, 0.0), 60.0, 30.0, -4.0, 8.0, "plate");
        let boss_sketch = Sketch::new(Plane::XY.offset(3.0)).with_loop(Loop::circle(P2::new(0.0, 0.0), 8.0));
        let boss =
            ctx.extrude("cylinder", &boss_sketch, None, 10.0, BTreeMap::new(), Some("boss".into()), None).unwrap()[0];
        let body = ctx.boolean(BoolOp::Union, plate, &[boss], KernelChoice::Auto, None, None).unwrap();
        assert_eq!(ctx.body(body).unwrap().kind(), "brep");
        let hole_sketch = Sketch::new(Plane::XY.offset(-5.0)).with_loop(Loop::circle(P2::new(-22.0, 0.0), 2.75));
        let hole =
            ctx.extrude("cylinder", &hole_sketch, None, 10.0, BTreeMap::new(), Some("hole".into()), None).unwrap()[0];
        let part = ctx.boolean(BoolOp::Subtract, body, &[hole], KernelChoice::Auto, None, None).unwrap();
        assert_eq!(ctx.body(part).unwrap().kind(), "brep", "the hole was re-routed by a timing");
        assert_eq!(ctx.brep_face_budget, kernel::BREP_FACE_BUDGET, "the budget is a constant of the document");
        // And nothing in the diagnostics mentions a measurement: the only
        // numbers a routing note may carry are face counts.
        let timed: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.message.contains(" ms") || d.message.contains("budget"))
            .map(|d| d.message.as_str())
            .collect();
        assert!(timed.is_empty(), "{timed:?}");
    }

    #[test]
    fn provenance_names_survive_a_mesh_union_and_a_copy() {
        let mut ctx = Context::new();
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(8.0, 6.0), 15.0, 12.0, 2.0, 8.0, "b");
        let before: Vec<String> = ctx.body_mesh(a).unwrap().1.faces.iter().map(|f| f.name.clone()).collect();
        let u = ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Mesh, None, None).unwrap();
        let after: Vec<String> = ctx.body_mesh(u).unwrap().1.faces.iter().map(|f| f.name.clone()).collect();
        let kept: Vec<&String> = before.iter().filter(|n| after.contains(n)).collect();
        assert!(kept.len() >= 5, "a's faces should survive the union: {before:?} -> {after:?}");
        // A copy of a mesh body keeps every name and moves the geometry.
        let t = Transform::translation(Vec3::new(50.0, 0.0, 0.0));
        let c = ctx.duplicate(u, &t, Some("copy".into()), None).unwrap();
        assert_eq!(ctx.body(c).unwrap().kind(), "mesh");
        let copied: Vec<String> = ctx.body_mesh(c).unwrap().1.faces.iter().map(|f| f.name.clone()).collect();
        assert_eq!(after, copied);
        let (lo, _) = ctx.body_mesh(c).unwrap().0.bbox().unwrap();
        assert!((lo.x - 40.0).abs() < 1e-4, "the copy moved: {lo:?}");
    }

    #[test]
    fn a_mesh_body_refuses_to_become_a_brep_again() {
        let mut ctx = Context::new();
        let a = block(&mut ctx, P2::new(0.0, 0.0), 20.0, 20.0, 0.0, 10.0, "a");
        let b = block(&mut ctx, P2::new(8.0, 6.0), 15.0, 12.0, 2.0, 8.0, "b");
        let u = ctx.boolean(BoolOp::Union, a, &[b], KernelChoice::Mesh, None, None).unwrap();
        let e = ctx.body(u).unwrap().solid().err().unwrap().to_string();
        assert!(e.contains("mesh body"), "{e}");
        // A model containing a mesh body still exports, renders and measures.
        let model = ctx.to_model(false, 0.0);
        assert_eq!(model.bodies.len(), 1);
        let body = &model.bodies[0];
        assert!(body.mesh.as_ref().unwrap().indices.len() > 30);
        assert!(!body.topology.as_ref().unwrap().faces.is_empty());
        assert!(body.bbox.is_some());
        assert!(ctx.bbox().is_some());
    }

    // ------------------------------------------------ generator naming

    fn names_of(ctx: &mut Context, id: BodyId) -> Vec<String> {
        ctx.body_mesh(id).unwrap().1.faces.iter().map(|f| f.name.clone()).collect()
    }

    fn sorted_names(ctx: &mut Context, id: BodyId) -> Vec<String> {
        let mut n = names_of(ctx, id);
        n.sort();
        n
    }

    fn a_box(ctx: &mut Context, w: f64, d: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
        ctx.extrude("box", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    fn a_cylinder(ctx: &mut Context, at: P2, z: f64, r: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY.offset(z)).with_loop(Loop::circle(at, r));
        ctx.extrude("cylinder", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    /// An extrude names what it made: two caps and one side per sketch
    /// loop-and-segment, never an index into the kernel's output order.
    #[test]
    fn an_extrude_names_caps_and_one_side_per_sketch_segment() {
        let mut ctx = Context::new();
        let s = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0))
            .with_loop(Loop::circle(P2::new(0.0, 0.0), 4.0));
        let id = ctx.extrude("extrude", &s, None, 6.0, BTreeMap::new(), Some("plate".into()), None).unwrap()[0];
        assert_eq!(
            sorted_names(&mut ctx, id),
            vec![
                "plate/end",
                "plate/side[0.0]",
                "plate/side[0.1]",
                "plate/side[0.2]",
                "plate/side[0.3]",
                "plate/side[1.0]",
                "plate/side[1.1]",
                "plate/start",
            ]
        );
        // The hole's wall came from loop 1, the outside from loop 0, and the
        // caps are planar and at the two ends of the sweep.
        let topo = ctx.body_mesh(id).unwrap().1.clone();
        let hole: Vec<&FaceInfo> = topo.faces.iter().filter(|f| f.name.starts_with("plate/side[1.")).collect();
        assert_eq!(hole.len(), 2);
        assert!(hole.iter().all(|f| f.centroid.x.abs() < 4.1 && f.centroid.y.abs() < 4.1));
        let end = topo.faces.iter().find(|f| f.name == "plate/end").unwrap();
        assert!((end.centroid.z - 6.0).abs() < 1e-9 && end.normal.z > 0.99);
        let start = topo.faces.iter().find(|f| f.name == "plate/start").unwrap();
        assert!(start.centroid.z.abs() < 1e-9 && start.normal.z < -0.99);
    }

    /// The whole point: the name of the top face of a box does not change when
    /// an unrelated feature is inserted before it, even though every kernel
    /// index does.
    #[test]
    fn names_are_unchanged_by_an_unrelated_feature_inserted_before() {
        let mut plain = Context::new();
        let a = a_box(&mut plain, 10.0, 10.0, 10.0, "blk");
        let before = names_of(&mut plain, a);

        let mut edited = Context::new();
        let _pin = a_cylinder(&mut edited, P2::new(40.0, 0.0), 0.0, 3.0, 4.0, "pin");
        let b = a_box(&mut edited, 10.0, 10.0, 10.0, "blk");
        assert_eq!(before, names_of(&mut edited, b));
        assert!(before.contains(&"blk/top".to_owned()), "{before:?}");

        // A parameter change moves the face; the name stays put.
        let mut taller = Context::new();
        let c = a_box(&mut taller, 10.0, 10.0, 40.0, "blk");
        assert_eq!(before, names_of(&mut taller, c));
        let topo = taller.body_mesh(c).unwrap().1.clone();
        let top = topo.faces.iter().find(|f| f.name == "blk/top").unwrap();
        assert!((top.centroid.z - 40.0).abs() < 1e-9);
    }

    /// A primitive's faces have names that mean something, and the generic
    /// `start` / `end` / `side[l.s]` forms keep resolving to the same faces.
    #[test]
    fn primitives_name_their_faces_meaningfully() {
        let mut ctx = Context::new();
        let b = a_box(&mut ctx, 10.0, 20.0, 5.0, "blk");
        assert_eq!(
            sorted_names(&mut ctx, b),
            vec!["blk/back", "blk/bottom", "blk/front", "blk/left", "blk/right", "blk/top"]
        );
        let topo = ctx.body_mesh(b).unwrap().1.clone();
        let top = topo.faces.iter().find(|f| f.name == "blk/top").unwrap();
        assert_eq!(top.aliases, vec!["blk/end".to_owned()]);
        assert!(top.normal.z > 0.99);
        let front = topo.faces.iter().find(|f| f.name == "blk/front").unwrap();
        assert!(front.normal.y < -0.99);
        assert_eq!(front.aliases, vec!["blk/side[0.0]".to_owned()]);

        // A cylinder's wall is two kernel faces (a circle is two arcs), so
        // `side` is the group and the pieces are `side#k`.
        let mut ctx = Context::new();
        let c = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 0.0, 5.0, 10.0, "post");
        assert_eq!(sorted_names(&mut ctx, c), vec!["post/bottom", "post/side#0", "post/side#1", "post/top"]);
        let topo = ctx.body_mesh(c).unwrap().1.clone();
        for f in topo.faces.iter().filter(|f| f.name.starts_with("post/side")) {
            assert!(f.aliases.contains(&"post/side".to_owned()), "{:?}", f.aliases);
            assert!(f.aliases.iter().any(|a| a.starts_with("post/side[")), "{:?}", f.aliases);
        }
    }

    /// Edges are named by the ordered pair of faces they bound, with the
    /// feature prefix written once when both come from the same feature.
    #[test]
    fn edges_are_named_by_the_faces_they_bound() {
        let mut ctx = Context::new();
        let b = a_box(&mut ctx, 10.0, 10.0, 10.0, "blk");
        let topo = ctx.body_mesh(b).unwrap().1.clone();
        let mut names: Vec<String> = topo.edges.iter().map(|e| e.name.clone()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "blk/back|bottom",
                "blk/back|left",
                "blk/back|right",
                "blk/back|top",
                "blk/bottom|front",
                "blk/bottom|left",
                "blk/bottom|right",
                "blk/front|left",
                "blk/front|right",
                "blk/front|top",
                "blk/left|top",
                "blk/right|top",
            ]
        );
        // The two faces an edge names are the two faces that list it.
        for e in &topo.edges {
            assert_eq!(e.faces.len(), 2, "{}", e.name);
            for fname in &e.faces {
                let f = topo.faces.iter().find(|f| &f.name == fname).unwrap();
                assert!(f.edges.contains(&e.name), "{} does not list {}", f.name, e.name);
            }
        }
        // Vertices are the faces that meet there.
        assert_eq!(topo.vertices.len(), 8);
        for v in &topo.vertices {
            assert_eq!(v.faces.len(), 3, "{}: {:?}", v.name, v.faces);
            assert_eq!(v.name.matches('|').count(), 2, "{}", v.name);
        }
    }

    /// A partial revolve names its swept surfaces per profile segment and its
    /// two caps; a full turn has no caps.
    #[test]
    fn a_revolve_names_its_swept_surfaces_and_caps() {
        let profile = Sketch::new(Plane::XZ).with_loop(Loop::rect(P2::new(20.0, 5.0), 8.0, 10.0));
        let mut ctx = Context::new();
        let id = ctx
            .revolve("revolve", &profile, None, Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2, Some("arc".into()), None)
            .unwrap()[0];
        // truck splits each swept surface into quarter turns, so one profile
        // segment can come back as several faces; they are the `#k` pieces of
        // one `seg[l.s]`, and the unsuffixed name is the group.
        let names = sorted_names(&mut ctx, id);
        assert_eq!(names.len(), 10, "{names:?}");
        assert!(names.contains(&"arc/start".to_owned()) && names.contains(&"arc/end".to_owned()));
        for l in 0..4 {
            let want = format!("arc/seg[0.{l}]");
            let pieces: Vec<&String> = names.iter().filter(|n| n.starts_with(&want)).collect();
            assert!(!pieces.is_empty(), "no face from profile segment {l}: {names:?}");
            let topo = ctx.body_mesh(id).unwrap().1.clone();
            for p in pieces {
                let f = topo.faces.iter().find(|f| &f.name == p).unwrap();
                assert!(f.name == want || f.aliases.contains(&want), "{}: {:?}", f.name, f.aliases);
            }
        }
        let mut ctx = Context::new();
        let full = ctx
            .revolve("revolve", &profile, None, Vec3::ZERO, Vec3::Z, std::f64::consts::TAU, Some("ring".into()), None)
            .unwrap()[0];
        let names = sorted_names(&mut ctx, full);
        assert!(names.iter().all(|n| n.starts_with("ring/seg[")), "a full turn has no caps: {names:?}");
    }

    /// Propagation through a boolean: the faces the boolean did not touch keep
    /// their names, and the wall of the hole keeps the *tool's* name, because
    /// that is the surface it lies on.
    #[test]
    fn a_boolean_carries_both_operands_names_onto_the_result() {
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 20.0, 20.0, 10.0, "blk");
        let hole = a_cylinder(&mut ctx, P2::new(0.0, 0.0), -1.0, 3.0, 12.0, "hole");
        let r = ctx.boolean(BoolOp::Subtract, blk, &[hole], KernelChoice::Brep, Some("part".into()), None).unwrap();
        assert_eq!(ctx.body(r).unwrap().kind(), "brep");
        assert_eq!(
            sorted_names(&mut ctx, r),
            vec![
                "blk/back",
                "blk/bottom",
                "blk/front",
                "blk/left",
                "blk/right",
                "blk/top",
                "hole/side#0",
                "hole/side#1",
            ]
        );
        // "where the hole meets the top" is an edge with both features in it.
        let topo = ctx.body_mesh(r).unwrap().1.clone();
        let names: Vec<&str> = topo.edges.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.iter().any(|n| n.starts_with("blk/top|hole/side")),
            "the hole rim names both parents: {names:?}"
        );
        // No face fell back to the kernel-order form.
        assert!(topo.faces.iter().all(|f| !f.name.contains("/face[")), "{:?}", sorted_names(&mut ctx, r));
    }

    /// A body moved before a boolean keeps its generator's names through it
    /// (names come from the generator): a translated tool's wall is still the tool's,
    /// a moved block's faces are still the block's, and the same holds for a
    /// rotated copy and a pattern member. Before `NameSource::Moved`, a
    /// moved B-rep was named `Intrinsic`, so the boolean re-tessellated it
    /// as `face:N` and every result face it brought in lost its name.
    #[test]
    fn a_moved_operand_keeps_its_names_through_a_boolean() {
        let want = vec![
            "blk/back",
            "blk/bottom",
            "blk/front",
            "blk/left",
            "blk/right",
            "blk/top",
            "hole/side#0",
            "hole/side#1",
        ];
        let t = Transform::translation(Vec3::new(2.0, 1.0, 0.0));
        let r = Transform::rotation(Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_6);
        // A translated tool.
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 20.0, 20.0, 10.0, "blk");
        let hole = a_cylinder(&mut ctx, P2::new(0.0, 0.0), -1.0, 3.0, 12.0, "hole");
        let moved = ctx.transform(hole, &t, "translate", BTreeMap::new(), None, None).unwrap();
        let part = ctx.boolean(BoolOp::Subtract, blk, &[moved], KernelChoice::Brep, Some("part".into()), None).unwrap();
        assert_eq!(sorted_names(&mut ctx, part), want, "a translated tool");
        // A rotated block and a copied tool.
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 20.0, 20.0, 10.0, "blk");
        let blk = ctx.transform(blk, &r, "rotate", BTreeMap::new(), None, None).unwrap();
        let hole = a_cylinder(&mut ctx, P2::new(0.0, 0.0), -1.0, 3.0, 12.0, "hole");
        let copy = ctx.duplicate(hole, &t, None, None).unwrap();
        // And a copy of a move, so the chain composes.
        let copy = ctx.duplicate(copy, &r, None, None).unwrap();
        let part = ctx.boolean(BoolOp::Subtract, blk, &[copy], KernelChoice::Brep, Some("part".into()), None).unwrap();
        assert_eq!(sorted_names(&mut ctx, part), want, "a rotated block and a copied tool");
        // A uniformly scaled tool goes through `transform` too.
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 20.0, 20.0, 10.0, "blk");
        let hole = a_cylinder(&mut ctx, P2::new(0.0, 0.0), -1.0, 2.0, 12.0, "hole");
        let big = ctx.scale(hole, Vec3::ZERO, Vec3::new(1.5, 1.5, 1.5), None, None).unwrap();
        let part = ctx.boolean(BoolOp::Subtract, blk, &[big], KernelChoice::Brep, Some("part".into()), None).unwrap();
        assert_eq!(sorted_names(&mut ctx, part), want, "a scaled tool");
    }

    /// Two evaluations of the same model, in one process, must produce the
    /// same bytes: the same triangles in the same order, and the same names
    /// out of `cadpole_core::naming`. Before [`kernel::canonical`] this failed on
    /// the second or third run, because `truck_shapeops` orders its result's
    /// faces by the heap addresses of their surfaces.
    #[test]
    fn twenty_evaluations_of_one_boolean_are_byte_identical() {
        let once = || {
            let mut ctx = Context::new();
            let blk = a_box(&mut ctx, 20.0, 20.0, 10.0, "blk");
            let hole = a_cylinder(&mut ctx, P2::new(2.0, 1.0), -1.0, 3.0, 12.0, "hole");
            let r = ctx
                .boolean(BoolOp::Subtract, blk, &[hole], KernelChoice::Brep, Some("part".into()), None)
                .unwrap();
            let (mesh, topo) = ctx.body_mesh(r).unwrap().clone();
            let mut s = String::new();
            for p in mesh.positions.iter().chain(&mesh.normals) {
                s.push_str(&format!("{:08x} ", p.to_bits()));
            }
            for i in mesh.indices.iter().chain(&mesh.face_ids) {
                s.push_str(&format!("{i} "));
            }
            for f in &topo.faces {
                s.push_str(&format!("|{} {:?} {} {}", f.name, f.aliases, f.tri_start, f.tri_count));
            }
            for e in &topo.edges {
                s.push_str(&format!("|{}", e.name));
            }
            for v in &topo.vertices {
                s.push_str(&format!("|{}", v.name));
            }
            s
        };
        let first = once();
        for i in 1..20 {
            assert_eq!(first, once(), "evaluation {i} differs from the first");
        }
    }

    /// A subtrahend with nothing but material around it becomes a void in the
    /// result: one body, two shells, and the volume goes *down*.
    #[test]
    fn a_fully_enclosed_subtraction_is_a_body_with_a_void() {
        let mut ctx = Context::new();
        let outer = a_box(&mut ctx, 20.0, 20.0, 20.0, "outer");
        let inner = {
            let s = Sketch::new(Plane::XY.offset(2.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 16.0, 16.0));
            ctx.extrude("box", &s, None, 16.0, BTreeMap::new(), Some("inner".into()), None).unwrap()[0]
        };
        let r = ctx
            .boolean(BoolOp::Subtract, outer, &[inner], KernelChoice::Brep, Some("shell".into()), None)
            .unwrap();
        assert_eq!(ctx.body(r).unwrap().kind(), "brep", "the exact kernel keeps it");
        let (mesh, topo) = ctx.body_mesh(r).unwrap().clone();
        let want = fmath::powi(20.0f64, 3) - fmath::powi(16.0f64, 3);
        assert!((mesh.volume() - want).abs() < want * 1e-6, "volume {}, want {want}", mesh.volume());
        assert!(mesh.welded(1e-4).is_watertight());
        // Both shells are named by the feature that made them, cavity included.
        let names = sorted_names(&mut ctx, r);
        assert_eq!(names.len(), 12, "6 outer faces and 6 cavity walls: {names:?}");
        assert!(names.iter().filter(|n| n.starts_with("inner/")).count() == 6, "{names:?}");
        assert!(names.iter().filter(|n| n.starts_with("outer/")).count() == 6, "{names:?}");
        // The 3MF/STL path sees one watertight mesh, cavity and all.
        let stl = crate::export::stl(&[&mesh]);
        assert_eq!(u32::from_le_bytes(stl[80..84].try_into().unwrap()) as usize, mesh.triangle_count());
        assert_eq!(topo.faces.iter().map(|f| f.tri_count as usize).sum::<usize>(), mesh.triangle_count());
    }

    /// Cutting a body that already has a cavity is past truck's boolean, so
    /// `auto` routes it to the mesh kernel and says so rather than answering
    /// wrongly.
    #[test]
    fn cutting_a_hollow_body_falls_back_to_the_mesh_kernel() {
        let mut ctx = Context::new();
        let outer = a_box(&mut ctx, 20.0, 20.0, 20.0, "outer");
        let inner = {
            let s = Sketch::new(Plane::XY.offset(2.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 16.0, 16.0));
            ctx.extrude("box", &s, None, 16.0, BTreeMap::new(), Some("inner".into()), None).unwrap()[0]
        };
        let shell = ctx
            .boolean(BoolOp::Subtract, outer, &[inner], KernelChoice::Auto, Some("shell".into()), None)
            .unwrap();
        let post = {
            let s = Sketch::new(Plane::XY.offset(-2.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 4.0, 4.0));
            ctx.extrude("box", &s, None, 30.0, BTreeMap::new(), Some("post".into()), None).unwrap()[0]
        };
        let cut = ctx
            .boolean(BoolOp::Subtract, shell, &[post], KernelChoice::Auto, Some("cut".into()), None)
            .unwrap();
        assert_eq!(ctx.body(cut).unwrap().kind(), "mesh", "the mesh kernel took it");
        assert!(
            ctx.diagnostics.iter().any(|d| d.message.contains("already has a cavity")),
            "the diagnostic says why: {:?}",
            ctx.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        // 20^3 - 16^3 shell, less the part of the 4x4x30 post inside its walls.
        let v = ctx.body_mesh(cut).unwrap().0.volume();
        let want = fmath::powi(20.0f64, 3) - fmath::powi(16.0f64, 3) - 4.0 * 4.0 * 4.0;
        assert!((v - want).abs() < want * 1e-3, "volume {v}, want {want}");
    }

    /// A face a boolean cuts in two becomes `name#0` / `name#1`, ordered by
    /// `naming::order_pieces` rather than by the kernel's output order, and the
    /// unsuffixed name stays on as a group alias.
    #[test]
    fn a_face_split_by_a_boolean_gets_ordered_piece_names() {
        // A slab right across the box's +X wall cuts it into a low piece and a
        // high one.
        let build = |choice: KernelChoice| {
            let mut ctx = Context::new();
            let blk = a_box(&mut ctx, 20.0, 20.0, 10.0, "blk");
            let s = Sketch::new(Plane::XY.offset(3.0)).with_loop(Loop::rect(P2::new(10.0, 0.0), 20.0, 30.0));
            let slab = ctx.extrude("box", &s, None, 3.0, BTreeMap::new(), Some("slab".into()), None).unwrap()[0];
            let r = ctx.boolean(BoolOp::Union, blk, &[slab], choice, Some("part".into()), None).unwrap();
            (ctx, r)
        };
        let (mut ctx, r) = build(KernelChoice::Brep);
        let topo = ctx.body_mesh(r).unwrap().1.clone();
        let pieces: Vec<&FaceInfo> = topo.faces.iter().filter(|f| f.name.starts_with("blk/right#")).collect();
        assert_eq!(pieces.len(), 2, "{:?}", sorted_names(&mut ctx, r));
        let low = topo.faces.iter().find(|f| f.name == "blk/right#0").unwrap();
        let high = topo.faces.iter().find(|f| f.name == "blk/right#1").unwrap();
        assert!(low.centroid.z < high.centroid.z, "#0 is the lower piece: {low:?} {high:?}");
        for f in pieces {
            assert!(f.aliases.contains(&"blk/right".to_owned()), "{}: {:?}", f.name, f.aliases);
        }
        // Deterministic: building it again names the same geometry the same
        // way. truck's *output order* is not reproducible run to run (which is
        // exactly why `face:N` is not a name), so the claim is about the map
        // from name to geometry, not about the order the faces arrive in.
        let (mut again, r2) = build(KernelChoice::Brep);
        let place = |ctx: &mut Context, id: BodyId| {
            let mut v: Vec<(String, [i64; 3])> = ctx
                .body_mesh(id)
                .unwrap()
                .1
                .faces
                .iter()
                .map(|f| {
                    let c = f.centroid;
                    (f.name.clone(), [(c.x * 1e6) as i64, (c.y * 1e6) as i64, (c.z * 1e6) as i64])
                })
                .collect();
            v.sort();
            v
        };
        assert_eq!(place(&mut ctx, r), place(&mut again, r2));
    }

    /// Routing the same model through both kernels must produce the same
    /// names: the mesh path carries them on manifold's provenance ids and the
    /// B-rep path matches result faces back to the operands' surfaces, but
    /// both end up in `naming`.
    #[test]
    fn the_two_kernels_agree_on_the_names_of_the_same_model() {
        let build = |choice: KernelChoice| {
            let mut ctx = Context::new();
            let blk = a_box(&mut ctx, 20.0, 20.0, 10.0, "blk");
            let hole = a_cylinder(&mut ctx, P2::new(0.0, 0.0), -1.0, 3.0, 12.0, "hole");
            let r = ctx.boolean(BoolOp::Subtract, blk, &[hole], choice, Some("part".into()), None).unwrap();
            let kind = ctx.body(r).unwrap().kind();
            let mut names = sorted_names(&mut ctx, r);
            names.dedup();
            (kind, names)
        };
        let (brep_kind, brep) = build(KernelChoice::Brep);
        let (mesh_kind, mesh) = build(KernelChoice::Mesh);
        assert_eq!(brep_kind, "brep");
        assert_eq!(mesh_kind, "mesh");
        assert_eq!(brep, mesh, "the two kernels must name the same model the same way");
    }

    /// A copy (and therefore a pattern, which is a copy per instance) keeps
    /// the source's names: they are the same faces, moved.
    #[test]
    fn copies_and_patterns_keep_the_source_names() {
        let mut ctx = Context::new();
        let src = a_box(&mut ctx, 10.0, 10.0, 10.0, "blk");
        let want = names_of(&mut ctx, src);
        assert!(want.contains(&"blk/top".to_owned()));
        let mut copies = Vec::new();
        for i in 1..4 {
            let t = Transform::translation(Vec3::new(20.0 * i as f64, 0.0, 0.0));
            copies.push(ctx.duplicate(src, &t, Some(format!("copy{i}")), None).unwrap());
        }
        // A rotation as well, so this is not only about translation.
        let r = Transform::rotation(Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_4);
        copies.push(ctx.duplicate(src, &r, Some("turned".into()), None).unwrap());
        for c in copies {
            assert_eq!(names_of(&mut ctx, c), want, "a copy keeps the source's names");
        }
        // And a move consumes the original but keeps the names too.
        let moved = ctx
            .transform(src, &Transform::translation(Vec3::new(0.0, 50.0, 0.0)), "translate", BTreeMap::new(), None, None)
            .unwrap();
        assert_eq!(names_of(&mut ctx, moved), want);
        let topo = ctx.body_mesh(moved).unwrap().1.clone();
        let top = topo.faces.iter().find(|f| f.name == "blk/top").unwrap();
        assert!((top.centroid.y - 50.0).abs() < 1e-9, "the geometry moved, the name did not");
    }

    #[test]
    fn param_override_wins_and_is_clamped() {
        let mut ctx = Context::new();
        ctx.overrides.insert("w".into(), ParamValue::Number(500.0));
        let v = ctx
            .declare_param(ParamDecl {
                name: "w".into(),
                default: ParamValue::Number(10.0),
                description: String::new(),
                unit: "mm".into(),
                min: Some(1.0),
                max: Some(100.0),
                step: None,
                choices: vec![],
                span: None,
            })
            .unwrap();
        assert_eq!(v, ParamValue::Number(100.0));
    }
    fn decl(name: &str, default: ParamValue, choices: &[&str]) -> ParamDecl {
        ParamDecl {
            name: name.into(),
            default,
            description: String::new(),
            unit: String::new(),
            min: None,
            max: None,
            step: None,
            choices: choices.iter().map(|s| s.to_string()).collect(),
            span: None,
        }
    }

    fn overridden(ctx: &mut Context, name: &str) -> bool {
        ctx.to_model(true, 0.0).params.iter().find(|p| p.name == name).unwrap().overridden
    }

    /// A front end that cannot know the declared kind (`-p s=2` reads `2`
    /// as a number) hands over a value that means the declared kind: a
    /// string param takes the text, a number or bool param takes text that
    /// spells one. Before, the override was dropped with "using default"
    /// and no `-p` spelling could set a string param to `2` or `true`.
    #[test]
    fn an_override_is_coerced_to_the_declared_kind() {
        let mut ctx = Context::new();
        ctx.overrides.insert("s".into(), ParamValue::Number(2.0));
        ctx.overrides.insert("t".into(), ParamValue::Bool(true));
        ctx.overrides.insert("u".into(), ParamValue::Number(0.25));
        ctx.overrides.insert("w".into(), ParamValue::Text("12.5".into()));
        ctx.overrides.insert("b".into(), ParamValue::Text("false".into()));
        ctx.overrides.insert("v".into(), ParamValue::Vector(Vec3::new(1.0, 2.0, 3.0)));
        let txt = |s: &str| ParamValue::Text(s.into());
        assert_eq!(ctx.declare_param(decl("s", txt("1"), &["1", "2", "true"])).unwrap(), txt("2"));
        assert_eq!(ctx.declare_param(decl("t", txt("1"), &["1", "2", "true"])).unwrap(), txt("true"));
        assert_eq!(ctx.declare_param(decl("u", txt("abc"), &[])).unwrap(), txt("0.25"));
        assert_eq!(ctx.declare_param(decl("w", ParamValue::Number(1.0), &[])).unwrap(), ParamValue::Number(12.5));
        assert_eq!(ctx.declare_param(decl("b", ParamValue::Bool(true), &[])).unwrap(), ParamValue::Bool(false));
        assert_eq!(ctx.declare_param(decl("v", txt("abc"), &[])).unwrap(), txt("1,2,3"));
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
        for n in ["s", "t", "u", "w", "b", "v"] {
            assert!(overridden(&mut ctx, n), "{n} took its override");
        }
    }

    /// An override the param refused — wrong kind, not finite, outside
    /// `choices` — leaves the default in force, so the model must not call
    /// that param overridden: `param width = 60 mm (override)` after
    /// "using default" told the user the opposite of what happened.
    #[test]
    fn a_rejected_override_is_not_reported_as_an_override() {
        let mut ctx = Context::new();
        ctx.overrides.insert("w".into(), ParamValue::Text("abc".into()));
        ctx.overrides.insert("h".into(), ParamValue::Number(f64::NAN));
        ctx.overrides.insert("c".into(), ParamValue::Text("blue".into()));
        ctx.overrides.insert("k".into(), ParamValue::Number(500.0));
        let num = ParamValue::Number(10.0);
        let mut clamped = decl("k", num.clone(), &[]);
        clamped.max = Some(100.0);
        assert_eq!(ctx.declare_param(decl("w", num.clone(), &[])).unwrap(), num);
        assert_eq!(ctx.declare_param(decl("h", num.clone(), &[])).unwrap(), num);
        let red = ParamValue::Text("red".into());
        assert_eq!(ctx.declare_param(decl("c", red.clone(), &["red", "green"])).unwrap(), red);
        assert_eq!(ctx.declare_param(clamped).unwrap(), ParamValue::Number(100.0));
        assert_eq!(ctx.diagnostics.len(), 3, "one warning per refused override");
        assert!(!overridden(&mut ctx, "w"));
        assert!(!overridden(&mut ctx, "h"));
        assert!(!overridden(&mut ctx, "c"));
        assert!(overridden(&mut ctx, "k"), "a clamped override is still the override, clamped");
    }

    #[test]
    fn material_round_trips_through_proto() {
        let m = Material {
            name: "glassy".into(),
            base_color: [0.1, 0.2, 0.3],
            roughness: 0.05,
            metallic: 0.0,
            texture: "checker".into(),
            texture_scale: 12.5,
            opacity: 0.4,
            ior: 1.57,
            emissive: [1.0, 0.0, 2.0],
            transmission: 1.0,
            attenuation_distance: 14.0,
            scatter_distance: 3.0,
            scatter_g: 0.4,
            anisotropy: 0.5,
            edge_break: 0.3,
            texture_depth: 0.01,
            shadow: false,
        };
        let p = m.to_proto();
        assert_eq!(p.opacity, Some(0.4));
        assert_eq!(p.shadow, Some(false), "a guide body writes the flag");
        assert_eq!(Material::default().to_proto().shadow, None, "a body that casts shadows writes nothing");
        assert_eq!(p.ior, Some(1.57));
        assert_eq!(p.emissive.as_ref().map(|c| [c.r, c.g, c.b]), Some([1.0, 0.0, 2.0]));
        assert_eq!(p.transmission, 1.0);
        assert_eq!(p.attenuation_distance, 14.0);
        assert_eq!(p.scatter_distance, 3.0);
        assert_eq!(p.scatter_g, 0.4);
        assert_eq!(p.anisotropy, 0.5);
        assert_eq!(p.edge_break, 0.3);
        assert_eq!(p.texture_depth, 0.01);
        assert_eq!(Material::from_proto(&p), m);

        // Absent optionals take the documented defaults, and the
        // print-material terms are all 0 = off, so a Material written
        // before they existed decodes to the appearance it always had.
        let bare = pb::Material { name: "bare".into(), ..Default::default() };
        let back = Material::from_proto(&bare);
        assert_eq!(back.opacity, 1.0);
        assert_eq!(back.ior, 1.5);
        assert_eq!(back.emissive, [0.0; 3]);
        assert_eq!(back.transmission, 0.0);
        assert_eq!(back.attenuation_distance, 0.0);
        assert_eq!(back.scatter_distance, 0.0);
        assert_eq!(back.scatter_g, 0.0);
        assert_eq!(back.anisotropy, 0.0);
        assert_eq!(back.edge_break, 0.0);
        assert!(back.shadow, "absent means it casts a shadow");
        assert_eq!(back.texture_scale, DEFAULT_TEXTURE_SCALE);
        assert_eq!(back.base_color, Material::default().base_color);

        // The defaults survive a round trip too.
        let d = Material::default();
        assert_eq!(Material::from_proto(&d.to_proto()), d);
        assert_eq!(d.texture_scale, 10.0);
        assert_eq!(d.opacity, 1.0);
        assert_eq!(d.ior, 1.5);
        assert_eq!(d.emissive, [0.0; 3]);
    }
}
