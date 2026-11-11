//! JSON view of an evaluated model for the web UI (and `cadpole eval --json`).
//! Field names match `cadpole.v1.Model`; meshes are replaced by `mesh_index`
//! so the browser can fetch typed arrays straight from wasm memory.

use std::collections::BTreeMap;
use std::sync::Arc;

use cadpole_core::Context;
use cadpole_core::memo::Memo;
use cadpole_core::tessellate::Mesh;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ScriptIn {
    pub path: String,
    pub source: String,
}

/// Input accepted by `Engine.evaluate` in wasm and by the CLI's JSON mode.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct EvalIn {
    pub scripts: Vec<ScriptIn>,
    #[serde(default)]
    pub overrides: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub tolerance: f64,
    /// Boolean tolerance in mm (0 = engine default).
    #[serde(default)]
    pub boolean_tolerance: f64,
    #[serde(default)]
    pub selection: Vec<RefView>,
    #[serde(default)]
    pub skip_mesh: bool,
    /// The drag preview: fuses and booleans whose inputs changed
    /// are skipped and their operands shown unfused.
    #[serde(default)]
    pub preview: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct RefView {
    pub feature_id: String,
    #[serde(default)]
    pub body_id: String,
    pub kind: String,
    #[serde(default)]
    pub topo_name: String,
    /// What the entity looked like when it was picked. Optional; it is what
    /// lets a reference whose name is gone resolve as `moved` rather than
    /// `stale`. A front-end copies it out of the `TopoFace`
    /// or `TopoEdge` it clicked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<FingerprintView>,
}

/// `cadpole.v1.EntityFingerprint` as JSON.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct FingerprintView {
    #[serde(default)]
    pub surface_kind: String,
    #[serde(default)]
    pub centroid: [f64; 3],
    #[serde(default)]
    pub normal: [f64; 3],
    #[serde(default)]
    pub area: f64,
    #[serde(default)]
    pub length: f64,
}

impl FingerprintView {
    pub fn to_proto(&self) -> cadpole_proto::v1::EntityFingerprint {
        let v = |c: [f64; 3]| cadpole_proto::v1::Vec3 { x: c[0], y: c[1], z: c[2] };
        cadpole_proto::v1::EntityFingerprint {
            surface_kind: self.surface_kind.clone(),
            centroid: Some(v(self.centroid)),
            normal: Some(v(self.normal)),
            area: self.area,
            length: self.length,
        }
    }

    pub fn from_proto(p: &cadpole_proto::v1::EntityFingerprint) -> FingerprintView {
        let v = |c: &Option<cadpole_proto::v1::Vec3>| c.as_ref().map(|c| [c.x, c.y, c.z]).unwrap_or([0.0; 3]);
        FingerprintView {
            surface_kind: p.surface_kind.clone(),
            centroid: v(&p.centroid),
            normal: v(&p.normal),
            area: p.area,
            length: p.length,
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct SpanView {
    pub path: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Serialize, Debug, Clone)]
pub struct DiagnosticView {
    pub severity: &'static str,
    pub message: String,
    pub span: Option<SpanView>,
    pub feature_id: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct ParamView {
    pub name: String,
    pub default_value: serde_json::Value,
    pub value: serde_json::Value,
    pub overridden: bool,
    pub description: String,
    pub unit: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub choices: Vec<String>,
    pub span: Option<SpanView>,
}

#[derive(Serialize, Debug, Clone)]
pub struct FeatureView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub args: BTreeMap<String, String>,
    pub span: Option<SpanView>,
    pub diagnostics: Vec<DiagnosticView>,
    /// Every kernel op of this feature was served from the memo.
    #[serde(default)]
    pub cached: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct FaceView {
    /// Generated name, e.g. `extrude_1/side[0.2]`.
    pub name: String,
    /// Other names that resolve to this face.
    pub aliases: Vec<String>,
    /// Position in `topology.faces`; also addressable as `face:<index>`.
    pub index: u32,
    pub surface_kind: String,
    pub centroid: [f64; 3],
    pub normal: [f64; 3],
    pub area: f64,
    pub edges: Vec<String>,
    pub tri_start: u32,
    pub tri_count: u32,
}

#[derive(Serialize, Debug, Clone)]
pub struct EdgeView {
    /// Generated name: the pair of faces this edge bounds.
    pub name: String,
    /// Position in `topology.edges`; also addressable as `edge:<index>`.
    pub index: u32,
    pub curve_kind: String,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub length: f64,
    pub faces: Vec<String>,
    pub polyline: Vec<[f64; 3]>,
    /// 0 = tangent seam, 90 = box corner, 180 = fold, -1 = unknown.
    pub dihedral_degrees: f64,
    pub convex: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct VertexView {
    /// Generated name: the faces that meet here.
    pub name: String,
    /// Position in `topology.vertices`; also addressable as `vertex:<index>`.
    pub index: u32,
    pub faces: Vec<String>,
    pub position: [f64; 3],
}

#[derive(Serialize, Debug, Clone)]
pub struct TopologyView {
    pub faces: Vec<FaceView>,
    pub edges: Vec<EdgeView>,
    pub vertices: Vec<VertexView>,
}

#[derive(Serialize, Debug, Clone)]
pub struct MaterialView {
    pub name: String,
    pub base_color: [f64; 3],
    pub roughness: f64,
    pub metallic: f64,
    pub texture: String,
    pub texture_scale: f64,
    pub opacity: f64,
    pub ior: f64,
    pub emissive: [f64; 3],
    pub transmission: f64,
    pub attenuation_distance: f64,
    pub scatter_distance: f64,
    pub scatter_g: f64,
    pub anisotropy: f64,
    pub edge_break: f64,
    pub texture_depth: f64,
    /// `cadpole.v1.Material.shadow`: false for guide geometry the
    /// camera sees but light does not.
    pub shadow: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct BodyView {
    pub id: String,
    pub feature_id: String,
    pub name: String,
    pub bbox: Option<[[f64; 3]; 2]>,
    pub topology: TopologyView,
    pub material: MaterialView,
    /// For an instance body this is the index of the *part body's*
    /// mesh: the triangles are sent once and every instance points at them.
    pub mesh_index: Option<usize>,
    pub triangle_count: usize,
    /// `cadpole.v1.Body.visible`: false on a part body, which is in the list
    /// only to carry the mesh its instances share.
    pub visible: bool,
    /// `cadpole.v1.Body.part_body`: the part body this instance body shares a
    /// mesh with; empty on an ordinary body.
    pub part_body: String,
    /// `cadpole.v1.Body.transform`: how the shared mesh is displayed (pose plus
    /// exploded-view offset); `None` on an ordinary body.
    pub transform: Option<TransformView>,
    /// `cadpole.v1.Body.instance`.
    pub instance: String,
    /// `cadpole.v1.Body.layer`: the display layer this body is on;
    /// left out of the JSON when empty, so a document without layers reads
    /// exactly as before.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub layer: String,
}

/// `cadpole.v1.Layer`: one `layer(...)` declaration.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LayerView {
    pub name: String,
    pub doc: String,
    pub visible: bool,
    pub opacity: f64,
    pub color: Option<[f64; 3]>,
    pub order: u32,
}

/// `cadpole.v1.Layer`s as the JSON view spells them.
pub fn layer_views(layers: &[cadpole_proto::v1::Layer]) -> Vec<LayerView> {
    layers
        .iter()
        .map(|l| LayerView {
            name: l.name.clone(),
            doc: l.doc.clone(),
            visible: l.visible,
            opacity: l.opacity,
            color: l.color.as_ref().map(|c| [c.r, c.g, c.b]),
            order: l.order,
        })
        .collect()
}

/// `cadpole.v1.Transform`: a translation and a rotation quaternion `[x, y, z, w]`.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct TransformView {
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
}

/// `cadpole.v1.Part`.
#[derive(Serialize, Debug, Clone)]
pub struct PartView {
    pub id: String,
    pub name: String,
    pub feature_id: String,
    pub body_ids: Vec<String>,
}

/// `cadpole.v1.Instance`.
#[derive(Serialize, Debug, Clone)]
pub struct InstanceView {
    pub id: String,
    pub name: String,
    pub feature_id: String,
    pub part: String,
    pub pose: Option<TransformView>,
    pub explode: [f64; 3],
    pub body_ids: Vec<String>,
    pub assembly: String,
}

/// `cadpole.v1.Assembly`.
#[derive(Serialize, Debug, Clone)]
pub struct AssemblyView {
    pub id: String,
    pub name: String,
    pub feature_id: String,
    pub instances: Vec<String>,
    pub assemblies: Vec<String>,
    pub explode_distance: f64,
    pub explode_along: String,
}

/// `cadpole.v1.Plate`: one print plate of a Bambu Studio project, as
/// the export lays it out.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct PlateView {
    /// 1-based plate number.
    pub index: u32,
    /// `export(plate="...")`'s name, or empty.
    pub name: String,
    /// The `export()` path this plate belongs to.
    pub path: String,
    /// The bed, in mm.
    pub bed_x: f64,
    pub bed_y: f64,
    /// The bed's corner in the export's world space.
    pub origin: [f64; 3],
    pub bodies: Vec<PlateBodyView>,
}

/// `cadpole.v1.PlateBody`: a body on a plate, the transform that puts its
/// mesh there, and the filament it prints in.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct PlateBodyView {
    pub body_id: String,
    pub object: String,
    pub transform: TransformView,
    pub slot: u32,
    /// Linear RGB.
    pub color: [f64; 3],
    pub filament: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct ModelView {
    pub params: Vec<ParamView>,
    pub features: Vec<FeatureView>,
    pub bodies: Vec<BodyView>,
    pub diagnostics: Vec<DiagnosticView>,
    pub eval_millis: f64,
    pub ok: bool,
    /// What the kernel-op memo did for this evaluation; all zero
    /// without one.
    #[serde(default)]
    pub memo: MemoView,
    /// Display assemblies; all empty for a document without any.
    pub parts: Vec<PartView>,
    pub instances: Vec<InstanceView>,
    pub assemblies: Vec<AssemblyView>,
    /// The print plates of the document's Bambu projects, laid out
    /// as the export would; left out of the JSON when there are none, so a
    /// document without Bambu exports reads exactly as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plates: Vec<PlateView>,
    /// The display layers the script declared, in declaration
    /// order; left out of the JSON when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<LayerView>,
    /// The document's display convention, when the script declared
    /// one with `units(...)`; left out of the JSON otherwise, which a front
    /// end reads as millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_units: Option<DisplayUnitsView>,
}

/// `cadpole.v1.DisplayUnits` as the JSON view spells it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DisplayUnitsView {
    pub unit: String,
    pub fraction: u32,
}

/// `cadpole.v1.Plate`s as the JSON view spells them.
pub fn plate_views(plates: &[cadpole_proto::v1::Plate]) -> Vec<PlateView> {
    plates
        .iter()
        .map(|p| PlateView {
            index: p.index,
            name: p.name.clone(),
            path: p.path.clone(),
            bed_x: p.bed_x,
            bed_y: p.bed_y,
            origin: p.origin.as_ref().map(|v| [v.x, v.y, v.z]).unwrap_or([0.0; 3]),
            bodies: p
                .bodies
                .iter()
                .map(|b| PlateBodyView {
                    body_id: b.body_id.clone(),
                    object: b.object.clone(),
                    transform: transform_view(&b.transform).unwrap_or(TransformView {
                        translation: [0.0; 3],
                        rotation: [0.0, 0.0, 0.0, 1.0],
                    }),
                    slot: b.slot,
                    color: b.color.as_ref().map(|c| [c.r, c.g, c.b]).unwrap_or([0.75; 3]),
                    filament: b.filament.clone(),
                })
                .collect(),
        })
        .collect()
}

/// `cadpole.v1.MemoReport`: counts, never times.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct MemoView {
    pub ops_run: u32,
    pub ops_hit: u32,
    pub ops_deferred: u32,
    pub entries: u32,
    pub bytes: u64,
    pub limit_bytes: u64,
    pub preview: bool,
}

fn transform_view(t: &Option<cadpole_proto::v1::Transform>) -> Option<TransformView> {
    t.as_ref().map(|t| TransformView {
        translation: t.translation.as_ref().map(|v| [v.x, v.y, v.z]).unwrap_or([0.0; 3]),
        rotation: t.rotation.as_ref().map(|q| [q.x, q.y, q.z, q.w]).unwrap_or([0.0, 0.0, 0.0, 1.0]),
    })
}

fn assembly_views(
    parts: &[cadpole_proto::v1::Part],
    instances: &[cadpole_proto::v1::Instance],
    assemblies: &[cadpole_proto::v1::Assembly],
) -> (Vec<PartView>, Vec<InstanceView>, Vec<AssemblyView>) {
    (
        parts
            .iter()
            .map(|p| PartView { id: p.id.clone(), name: p.name.clone(), feature_id: p.feature_id.clone(), body_ids: p.body_ids.clone() })
            .collect(),
        instances
            .iter()
            .map(|i| InstanceView {
                id: i.id.clone(),
                name: i.name.clone(),
                feature_id: i.feature_id.clone(),
                part: i.part.clone(),
                pose: transform_view(&i.pose),
                explode: i.explode.as_ref().map(|v| [v.x, v.y, v.z]).unwrap_or([0.0; 3]),
                body_ids: i.body_ids.clone(),
                assembly: i.assembly.clone(),
            })
            .collect(),
        assemblies
            .iter()
            .map(|a| AssemblyView {
                id: a.id.clone(),
                name: a.name.clone(),
                feature_id: a.feature_id.clone(),
                instances: a.instances.clone(),
                assemblies: a.assemblies.clone(),
                explode_distance: a.explode_distance,
                explode_along: a.explode_along.clone(),
            })
            .collect(),
    )
}

fn v3(v: cadpole_core::Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn span(s: &Option<cadpole_core::context::Span>) -> Option<SpanView> {
    s.as_ref().map(|s| SpanView {
        path: s.path.clone(),
        start_line: s.start_line,
        start_col: s.start_col,
        end_line: s.end_line,
        end_col: s.end_col,
    })
}

fn diag(d: &cadpole_core::context::Diagnostic) -> DiagnosticView {
    use cadpole_core::context::Severity;
    DiagnosticView {
        severity: match d.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        },
        message: d.message.clone(),
        span: span(&d.span),
        feature_id: d.feature.as_ref().map(|f| f.0.clone()).unwrap_or_default(),
    }
}

pub fn param_value_json(v: &cadpole_core::ParamValue) -> serde_json::Value {
    use cadpole_core::ParamValue as P;
    match v {
        P::Number(n) => serde_json::json!(n),
        P::Bool(b) => serde_json::json!(b),
        P::Text(s) => serde_json::json!(s),
        P::Vector(v) => serde_json::json!([v.x, v.y, v.z]),
    }
}

pub fn param_value_from_json(v: &serde_json::Value) -> Option<cadpole_core::ParamValue> {
    use cadpole_core::ParamValue as P;
    match v {
        serde_json::Value::Bool(b) => Some(P::Bool(*b)),
        serde_json::Value::Number(n) => n.as_f64().map(P::Number),
        serde_json::Value::String(s) => Some(P::Text(s.clone())),
        serde_json::Value::Array(a) if a.len() == 3 => {
            let f: Option<Vec<f64>> = a.iter().map(|x| x.as_f64()).collect();
            f.map(|f| P::Vector(cadpole_core::Vec3::new(f[0], f[1], f[2])))
        }
        _ => None,
    }
}

/// Build the view. Meshes of visible bodies are returned separately, in
/// `mesh_index` order, so the caller can hand them out as typed arrays.
pub fn model_view(ctx: &mut Context, eval_millis: f64, skip_mesh: bool) -> (ModelView, Vec<Mesh>) {
    let mut meshes = Vec::new();
    let ids: Vec<_> = ctx.bodies.iter().map(|b| b.id).collect();
    let mut bodies = Vec::new();
    // The part bodies the visible instances share: their mesh goes
    // out once, under the part body's own entry (invisible), and every
    // instance body's `mesh_index` points at it. Parts are made before
    // their instances, so the index is known by the time one is needed.
    let shared = ctx.shared_part_bodies();
    let mut shared_index: std::collections::HashMap<cadpole_core::BodyId, usize> = std::collections::HashMap::new();
    for id in ids {
        let (visible, name, feature, material, id_string, instance, layer) = {
            let b = ctx.body(id).unwrap();
            (b.visible, b.name.clone(), b.feature.0.clone(), b.material.clone(), b.id_string(), b.instance, b.layer.clone())
        };
        let is_shared = !visible && shared.contains(&id);
        if !visible && !is_shared {
            continue;
        }
        let display = if visible && instance.is_some() { ctx.display_transform(id).ok().flatten() } else { None };
        let offset = if display.is_some() { ctx.body_view_offset(id).unwrap_or(cadpole_core::Vec3::ZERO) } else { cadpole_core::Vec3::ZERO };
        let (part_body, transform) = match &display {
            Some((src, t)) => (ctx.body(*src).map(|b| b.id_string()).unwrap_or_default(), Some(t.to_proto())),
            None => (String::new(), None),
        };
        let instance_id = instance.and_then(|i| ctx.instance_record(i).ok().map(|r| r.feature.0.clone())).unwrap_or_default();
        let Ok((mesh, topo)) = ctx.body_mesh(id) else { continue };
        let shift = cadpole_core::Transform::translation(offset);
        let moved_topo;
        let topo = if offset.length() > 0.0 {
            moved_topo = topo.transformed(&shift);
            &moved_topo
        } else {
            topo
        };
        let mesh_index = if skip_mesh {
            None
        } else if let Some((src, _)) = &display {
            shared_index.get(src).copied()
        } else {
            meshes.push(mesh.clone());
            if is_shared {
                shared_index.insert(id, meshes.len() - 1);
            }
            Some(meshes.len() - 1)
        };
        bodies.push(BodyView {
            id: id_string,
            feature_id: feature,
            name,
            bbox: mesh.bbox().map(|(lo, hi)| [v3(lo + offset), v3(hi + offset)]),
            topology: TopologyView {
                faces: topo
                    .faces
                    .iter()
                    .enumerate()
                    .map(|(i, f)| FaceView {
                        name: f.name.clone(),
                        aliases: f.aliases.clone(),
                        index: i as u32,
                        surface_kind: f.surface_kind.to_owned(),
                        centroid: v3(f.centroid),
                        normal: v3(f.normal),
                        area: f.area,
                        edges: f.edges.clone(),
                        tri_start: f.tri_start,
                        tri_count: f.tri_count,
                    })
                    .collect(),
                edges: topo
                    .edges
                    .iter()
                    .enumerate()
                    .map(|(i, e)| EdgeView {
                        name: e.name.clone(),
                        index: i as u32,
                        curve_kind: e.curve_kind.to_owned(),
                        start: v3(e.start),
                        end: v3(e.end),
                        length: e.length,
                        faces: e.faces.clone(),
                        polyline: e.polyline.iter().map(|p| v3(*p)).collect(),
                        dihedral_degrees: e.dihedral_degrees,
                        convex: e.convex,
                    })
                    .collect(),
                vertices: topo
                    .vertices
                    .iter()
                    .enumerate()
                    .map(|(i, v)| VertexView {
                        name: v.name.clone(),
                        index: i as u32,
                        faces: v.faces.clone(),
                        position: v3(v.position),
                    })
                    .collect(),
            },
            material: MaterialView {
                name: material.name,
                base_color: material.base_color,
                roughness: material.roughness,
                metallic: material.metallic,
                texture: material.texture,
                texture_scale: material.texture_scale,
                opacity: material.opacity,
                ior: material.ior,
                emissive: material.emissive,
                transmission: material.transmission,
                attenuation_distance: material.attenuation_distance,
                scatter_distance: material.scatter_distance,
                scatter_g: material.scatter_g,
                anisotropy: material.anisotropy,
                edge_break: material.edge_break,
                texture_depth: material.texture_depth,
                shadow: material.shadow,
            },
            mesh_index,
            triangle_count: mesh.triangle_count(),
            visible,
            part_body,
            transform: transform_view(&transform),
            instance: instance_id,
            layer,
        });
    }
    let ok = !ctx.diagnostics.iter().any(|d| d.severity == cadpole_core::context::Severity::Error);
    let (parts, instances, assemblies) = ctx.assembly_lists();
    let (parts, instances, assemblies) = assembly_views(&parts, &instances, &assemblies);
    // After the bodies: the plates place the meshes just made, and make
    // none of their own.
    let plates = plate_views(&crate::print::document_plates(ctx));
    let layers = layer_views(&ctx.layers.iter().enumerate().map(|(i, l)| l.to_proto(i)).collect::<Vec<_>>());
    let view = ModelView {
        parts,
        instances,
        assemblies,
        plates,
        layers,
        display_units: ctx
            .display_units
            .as_ref()
            .map(|u| DisplayUnitsView { unit: u.unit.clone(), fraction: u.fraction }),
        params: ctx
            .params
            .iter()
            .map(|(d, v)| ParamView {
                name: d.name.clone(),
                default_value: param_value_json(&d.default),
                value: param_value_json(v),
                overridden: ctx.overrides.contains_key(&d.name),
                description: d.description.clone(),
                unit: d.unit.clone(),
                min: d.min,
                max: d.max,
                step: d.step,
                choices: d.choices.clone(),
                span: span(&d.span),
            })
            .collect(),
        features: ctx
            .features
            .iter()
            .map(|f| FeatureView {
                id: f.id.0.clone(),
                kind: f.kind.clone(),
                name: f.name.clone(),
                inputs: f.inputs.iter().map(|i| i.0.clone()).collect(),
                outputs: f.outputs.iter().map(|b| format!("{}#{}", f.id, b.0)).collect(),
                args: f.args.clone(),
                span: span(&f.span),
                diagnostics: f.diagnostics.iter().map(diag).collect(),
                cached: ctx.feature_cached(&f.id.0),
            })
            .collect(),
        bodies,
        diagnostics: ctx.diagnostics.iter().map(diag).collect(),
        eval_millis,
        ok,
        memo: {
            let r = ctx.memo_report();
            MemoView {
                ops_run: r.ops_run,
                ops_hit: r.ops_hit,
                ops_deferred: r.ops_deferred,
                entries: r.entries,
                bytes: r.bytes,
                limit_bytes: r.limit_bytes,
                preview: r.preview,
            }
        },
    };
    (view, meshes)
}

/// Evaluate from the JSON input shape. Used by wasm and the CLI.
pub fn evaluate_json(input: &EvalIn) -> (ModelView, Vec<Mesh>) {
    evaluate_json_with(input, None)
}

/// [`evaluate_json`] with the engine's memo: the wasm `Engine`
/// keeps one for its lifetime and passes it here on every evaluation.
pub fn evaluate_json_with(input: &EvalIn, memo: Option<Arc<Memo>>) -> (ModelView, Vec<Mesh>) {
    let start = cadpole_core::time::Stopwatch::start();
    let overrides = input
        .overrides
        .iter()
        .filter_map(|(k, v)| param_value_from_json(v).map(|v| (k.clone(), v)))
        .collect();
    let mut ctx = cadpole_script::evaluate(cadpole_script::EvalInput {
        scripts: input
            .scripts
            .iter()
            .map(|s| cadpole_script::ScriptFile { path: s.path.clone(), source: s.source.clone() })
            .collect(),
        entry: None,
        overrides,
        tolerance: input.tolerance,
        boolean_tolerance: input.boolean_tolerance,
        selection: input
            .selection
            .iter()
            .map(|r| cadpole_core::context::EntityRef {
                feature_id: r.feature_id.clone(),
                body_id: r.body_id.clone(),
                kind: r.kind.clone(),
                topo_name: r.topo_name.clone(),
            })
            .collect(),
        // The JSON shape carries no assets; the protobuf path
        // (`evaluate_document_full`) is the one that does.
        assets: None,
        limits: cadpole_script::Limits::structural(),
        tests: cadpole_script::TestMode::Skip,
        memo,
        preview: input.preview,
        sweep_results: None,
    });
    let ms = start.elapsed_ms();
    model_view(&mut ctx, ms, input.skip_mesh)
}

/// The same view, built from an already-evaluated `cadpole.v1.Model`.
///
/// The CLI's `eval --json` goes through the operation registry (`cadpole-ops`),
/// which hands back a proto `Model`; this converts it to the JSON shape the
/// web UI and existing scripts expect. `mesh_index` is always `None` here
/// because the meshes travelled inside the proto rather than alongside it.
pub fn model_view_from_proto(m: &cadpole_proto::v1::Model) -> ModelView {
    use cadpole_proto::v1 as pb;
    let v3 = |v: &Option<pb::Vec3>| v.as_ref().map(|v| [v.x, v.y, v.z]).unwrap_or([0.0; 3]);
    let span = |s: &Option<pb::SourceSpan>| {
        s.as_ref().map(|s| SpanView {
            path: s.path.clone(),
            start_line: s.start_line,
            start_col: s.start_col,
            end_line: s.end_line,
            end_col: s.end_col,
        })
    };
    let diag = |d: &pb::Diagnostic| DiagnosticView {
        severity: match d.severity() {
            pb::Severity::Error => "error",
            pb::Severity::Warning => "warning",
            _ => "info",
        },
        message: d.message.clone(),
        span: span(&d.span),
        feature_id: d.feature_id.clone(),
    };
    let value = |v: &Option<pb::ParamValue>| {
        v.as_ref()
            .and_then(cadpole_core::ParamValue::from_proto)
            .map(|v| param_value_json(&v))
            .unwrap_or(serde_json::Value::Null)
    };
    ModelView {
        params: m
            .params
            .iter()
            .map(|p| ParamView {
                name: p.name.clone(),
                default_value: value(&p.default_value),
                value: value(&p.value),
                overridden: p.overridden,
                description: p.description.clone(),
                unit: p.unit.clone(),
                min: p.min,
                max: p.max,
                step: p.step,
                choices: p.choices.clone(),
                span: span(&p.span),
            })
            .collect(),
        features: m
            .features
            .iter()
            .map(|f| FeatureView {
                id: f.id.clone(),
                kind: f.kind.clone(),
                name: f.name.clone(),
                inputs: f.inputs.clone(),
                outputs: f.outputs.clone(),
                args: f.args.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                span: span(&f.span),
                diagnostics: f.diagnostics.iter().map(diag).collect(),
                cached: f.cached,
            })
            .collect(),
        bodies: m
            .bodies
            .iter()
            .map(|b| {
                let topo = b.topology.clone().unwrap_or_default();
                let mat = b.material.clone().unwrap_or_default();
                BodyView {
                    id: b.id.clone(),
                    feature_id: b.feature_id.clone(),
                    name: b.name.clone(),
                    bbox: b.bbox.as_ref().map(|bb| [v3(&bb.min), v3(&bb.max)]),
                    topology: TopologyView {
                        faces: topo
                            .faces
                            .iter()
                            .map(|f| FaceView {
                                name: f.name.clone(),
                                aliases: f.aliases.clone(),
                                index: f.index,
                                surface_kind: f.surface_kind.clone(),
                                centroid: v3(&f.centroid),
                                normal: v3(&f.normal),
                                area: f.area,
                                edges: f.edges.clone(),
                                tri_start: f.tri_start,
                                tri_count: f.tri_count,
                            })
                            .collect(),
                        edges: topo
                            .edges
                            .iter()
                            .map(|e| EdgeView {
                                name: e.name.clone(),
                                index: e.index,
                                curve_kind: e.curve_kind.clone(),
                                start: v3(&e.start),
                                end: v3(&e.end),
                                length: e.length,
                                faces: e.faces.clone(),
                                polyline: e.polyline.iter().map(|p| [p.x, p.y, p.z]).collect(),
                                dihedral_degrees: e.dihedral_degrees,
                                convex: e.convex,
                            })
                            .collect(),
                        vertices: topo
                            .vertices
                            .iter()
                            .map(|v| VertexView {
                                name: v.name.clone(),
                                index: v.index,
                                faces: v.faces.clone(),
                                position: v3(&v.position),
                            })
                            .collect(),
                    },
                    material: MaterialView {
                        name: mat.name.clone(),
                        base_color: mat.base_color.as_ref().map(|c| [c.r, c.g, c.b]).unwrap_or([0.0; 3]),
                        roughness: mat.roughness,
                        metallic: mat.metallic,
                        texture: mat.texture.clone(),
                        texture_scale: mat.texture_scale,
                        opacity: mat.opacity.unwrap_or(1.0),
                        ior: mat.ior.unwrap_or(1.5),
                        emissive: mat.emissive.as_ref().map(|c| [c.r, c.g, c.b]).unwrap_or([0.0; 3]),
                        transmission: mat.transmission,
                        attenuation_distance: mat.attenuation_distance,
                        scatter_distance: mat.scatter_distance,
                        scatter_g: mat.scatter_g,
                        anisotropy: mat.anisotropy,
                        edge_break: mat.edge_break,
                        texture_depth: mat.texture_depth,
                        shadow: mat.shadow.unwrap_or(true),
                    },
                    mesh_index: None,
                    triangle_count: b.mesh.as_ref().map(|m| m.indices.len() / 3).unwrap_or(0),
                    visible: b.visible,
                    part_body: b.part_body.clone(),
                    transform: transform_view(&b.transform),
                    instance: b.instance.clone(),
                    layer: b.layer.clone(),
                }
            })
            .collect(),
        diagnostics: m.diagnostics.iter().map(diag).collect(),
        eval_millis: m.eval_millis,
        ok: !m.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error),
        memo: m
            .memo
            .as_ref()
            .map(|r| MemoView {
                ops_run: r.ops_run,
                ops_hit: r.ops_hit,
                ops_deferred: r.ops_deferred,
                entries: r.entries,
                bytes: r.bytes,
                limit_bytes: r.limit_bytes,
                preview: r.preview,
            })
            .unwrap_or_default(),
        parts: assembly_views(&m.parts, &m.instances, &m.assemblies).0,
        instances: assembly_views(&m.parts, &m.instances, &m.assemblies).1,
        assemblies: assembly_views(&m.parts, &m.instances, &m.assemblies).2,
        plates: plate_views(&m.plates),
        layers: layer_views(&m.layers),
        display_units: m
            .display_units
            .as_ref()
            .map(|u| DisplayUnitsView { unit: u.unit.clone(), fraction: u.fraction }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_view_has_bodies_and_mesh_indices() {
        let (view, meshes) = evaluate_json(&EvalIn {
            scripts: vec![ScriptIn { path: "main.star".into(), source: "b = box(1, 2, 3)".into() }],
            ..Default::default()
        });
        assert!(view.ok, "{:?}", view.diagnostics);
        assert_eq!(view.bodies.len(), 1);
        assert_eq!(view.bodies[0].mesh_index, Some(0));
        assert_eq!(meshes.len(), 1);
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"feature_id\":\"box_1\""));
        // No Bambu exports: no plates, and no `plates` key in the JSON at
        // all, so the view of such a document is the bytes it always was.
        assert!(view.plates.is_empty());
        assert!(!json.contains("plates"), "{json}");
    }

    /// The toy's shape in miniature: three Bambu calls to one path, a named
    /// print-in-place object, a pinned extruder, and an instance body. The
    /// view lists every plate with its bed, its name, and each body's
    /// placement -- the plate's grid position plus the centring shift, an
    /// instance's pose underneath -- and the filament slot and colour the
    /// export would give it.
    const PLATED: &str = concat!(
        "a = box(20, 20, 10, name=\"a\")\n",
        "a.material(color=\"#e8b84a\", label=\"PLA gold\")\n",
        "b = box(10, 10, 8, at=(30, 0, 0), name=\"b\")\n",
        "b.material(color=\"#2f8f8f\")\n",
        "c = box(15, 15, 5, at=(0, 40, 0), name=\"c\")\n",
        "c.material(color=\"#d94a3d\")\n",
        "p = part(c, name=\"pc\")\n",
        "i = instance(p, at=(100, 0, 0), name=\"ci\")\n",
        "export([a, b], \"proj.3mf\", flavor=flavor.bambu, plate=\"Core\", name=\"core\")\n",
        "export(i.body(), \"proj.3mf\", flavor=flavor.bambu, plate=\"Lid\", extruder=2)\n",
        "export([a, c], \"proj.3mf\", flavor=flavor.bambu, plate=3, extruder=[4, 1])\n",
        "export([a, b, c], \"plain.3mf\")\n",
    );

    #[test]
    fn the_view_lists_the_plates_as_the_export_lays_them_out() {
        let (view, _) = evaluate_json(&EvalIn {
            scripts: vec![ScriptIn { path: "main.star".into(), source: PLATED.into() }],
            ..Default::default()
        });
        assert!(view.ok, "{:?}", view.diagnostics);
        let plates = &view.plates;
        assert_eq!(plates.len(), 3, "{plates:?}");
        assert_eq!(plates.iter().map(|p| (p.index, p.name.as_str())).collect::<Vec<_>>(), [(1, "Core"), (2, "Lid"), (3, "")]);
        assert!(plates.iter().all(|p| p.path == "proj.3mf" && p.bed_x == 256.0 && p.bed_y == 256.0));
        // Bambu's grid for three plates: two to a row.
        assert_eq!(plates[0].origin, [0.0, 0.0, 0.0]);
        assert_eq!(plates[1].origin, [307.2, 0.0, 0.0]);
        assert_eq!(plates[2].origin, [0.0, -307.2, 0.0]);
        // Plate 1: a and b as one object "core", footprint x 0..40, y 0..20,
        // centred on the bed: moved by (108, 118).
        let core = &plates[0].bodies;
        assert_eq!(core.iter().map(|b| (b.body_id.as_str(), b.object.as_str(), b.slot)).collect::<Vec<_>>(), [("a#0", "core", 1), ("b#1", "core", 2)]);
        assert_eq!(core[0].transform.translation, [108.0, 118.0, 0.0]);
        assert_eq!(core[0].transform.rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(cadpole_core::paint::hex(core[0].color), "#E8B84A");
        assert_eq!(core[0].filament, "PLA gold");
        // A material the script gave no label keeps the default one's name,
        // as the export's basematerials do.
        assert_eq!(core[1].filament, "default");
        assert_eq!(cadpole_core::paint::hex(core[1].color), "#2F8F8F");
        // Plate 2: the instance body, pinned to slot 2, whose posed mesh
        // stands at x 100..115, y 40..55: the plate's transform is the pose
        // (100, 0, 0) then the placement, so the *part's* mesh lands where
        // the export puts the posed one -- at the centre of plate 2.
        let lid = &plates[1].bodies;
        assert_eq!(lid.len(), 1);
        assert_eq!((lid[0].body_id.as_str(), lid[0].object.as_str()), ("ci#3", "ci/c"));
        assert_eq!(lid[0].slot, 2);
        let t = lid[0].transform.translation;
        let shift = [307.2 + 128.0 - 107.5, 128.0 - 47.5];
        assert!((t[0] - (100.0 + shift[0])).abs() < 1e-9 && (t[1] - shift[1]).abs() < 1e-9 && t[2] == 0.0, "{t:?}");
        // Plate 3: a pinned to 4 (a slot opened with a's colour), c to 1.
        let three = &plates[2].bodies;
        assert_eq!(three.iter().map(|b| (b.body_id.as_str(), b.slot)).collect::<Vec<_>>(), [("a#0", 4), ("c#2", 1)]);
        assert_eq!(three[0].color, core[0].color);
        assert_eq!(three[0].transform.translation, [118.0, -307.2 + 100.5, 0.0]);
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"plates\":[{\"index\":1,\"name\":\"Core\",\"path\":\"proj.3mf\",\"bed_x\":256.0,\"bed_y\":256.0"), "{json}");
    }

    /// The plates are placement only: after the view
    /// has made its meshes, laying the plates out tessellates and bakes
    /// nothing, and a model without Bambu exports encodes to the same bytes
    /// with the plates filled in as without.
    #[test]
    fn the_plates_cost_no_tessellation_and_a_plain_model_is_byte_identical() {
        let mut ctx = cadpole_script::evaluate(cadpole_script::EvalInput {
            scripts: vec![cadpole_script::ScriptFile { path: "main.star".into(), source: PLATED.into() }],
            entry: None,
            overrides: Default::default(),
            tolerance: 0.0,
            boolean_tolerance: 0.0,
            selection: Vec::new(),
            assets: None,
            limits: cadpole_script::Limits::structural(),
            tests: cadpole_script::TestMode::Skip,
            memo: None,
            preview: false,
            sweep_results: None,
        });
        let (view, _) = model_view(&mut ctx, 0.0, false);
        assert_eq!(view.plates.len(), 3);
        let before = ctx.stats;
        let plates = crate::print::document_plates(&mut ctx);
        assert_eq!(plates.len(), 3);
        assert_eq!(ctx.stats, before, "the plates must not tessellate or bake anything");
        assert_eq!(plate_views(&plates), view.plates, "the proto and the JSON view are one layout");

        let mut plain = cadpole_script::evaluate(cadpole_script::EvalInput {
            scripts: vec![cadpole_script::ScriptFile {
                path: "main.star".into(),
                source: "a = box(20, 20, 10, name=\"a\")\nexport(a, \"a.3mf\")\nexport(a, \"a.stl\")\n".into(),
            }],
            entry: None,
            overrides: Default::default(),
            tolerance: 0.0,
            boolean_tolerance: 0.0,
            selection: Vec::new(),
            assets: None,
            limits: cadpole_script::Limits::structural(),
            tests: cadpole_script::TestMode::Skip,
            memo: None,
            preview: false,
            sweep_results: None,
        });
        // As messages, not as bytes: a feature's `args` is a map, and prost
        // writes a map in whatever order the hash table walks it, so two
        // encodings of one model differ between themselves. An empty
        // repeated field encodes to nothing, so equal messages with no
        // plates are equal bytes as far as the plates are concerned.
        let bare = plain.to_model(false, 0.0);
        let with = crate::print::model_with_plates(&mut plain, false, 0.0);
        assert!(with.plates.is_empty());
        assert_eq!(bare, with);
        assert!(crate::print::document_plates(&mut plain).is_empty());
        let (view, _) = model_view(&mut plain, 0.0, false);
        assert!(!serde_json::to_string(&view).unwrap().contains("plates"));
    }
}
