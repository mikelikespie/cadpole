//! The determinism probe: one function, compiled natively and to
//! wasm, that evaluates a document and writes down everything the
//! native-vs-wasm test compares.
//!
//! `//src/rust/cadpole-wasm:native_vs_wasm_test` feeds the same input string to
//! `native_probe` (a binary around [`probe_json`]) and to `Engine.probe` in
//! the wasm engine, and diffs the two answers category by category. Both
//! sides run *this* code, so a difference is a difference in what the engine
//! computed, never in how two front ends printed it. Every `f64` is written
//! as the hex of its bit pattern — the test's question is "identical to the
//! bit", not "close" — and nothing here is a wall-clock time.

use std::collections::BTreeMap;
use std::sync::Arc;

use cadpole_assets::{AssetStore, MemoryAssetStore};
use cadpole_core::context::Severity;
use cadpole_script::{EvalInput, ScriptFile};
use serde::{Deserialize, Serialize};

use crate::view::{ScriptIn, param_value_from_json};

/// What the probe evaluates: the `EvalIn` shape plus the document's assets,
/// which the JSON evaluate path does not carry (the examples that call
/// `asset(...)` need them on both sides).
#[derive(Deserialize, Debug, Clone, Default)]
pub struct ProbeIn {
    pub scripts: Vec<ScriptIn>,
    #[serde(default)]
    pub overrides: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub tolerance: f64,
    #[serde(default)]
    pub boolean_tolerance: f64,
    /// Asset name to its bytes, hex-encoded (two lowercase digits a byte).
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
    /// Also write every named value the `topology` digest covers, so a
    /// difference can be traced to the face, edge or vertex it starts at.
    #[serde(default)]
    pub detail: bool,
    /// Run the script's unswept `test(...)` declarations in the evaluation
    /// and report their outcomes. What a `stdlib/*_test.star` document is
    /// made of: its geometry is built inside its tests.
    #[serde(default)]
    pub run_tests: bool,
}

/// One test outcome (`run_tests: true` only): pass or fail, the message, the
/// sweep point it ran at, and what the function returned as `f64` bits.
#[derive(Serialize, Debug, Clone)]
pub struct ProbeTest {
    pub name: String,
    pub ok: bool,
    pub message: String,
    pub point: String,
    pub value: Option<String>,
}

/// One named topology entity's geometry, every `f64` as hex bits
/// (`detail: true` only).
#[derive(Serialize, Debug, Clone)]
pub struct ProbeDetail {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProbeFeature {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub outputs: Vec<String>,
    pub args: BTreeMap<String, String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProbeDiagnostic {
    pub severity: String,
    pub feature: String,
    pub message: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProbeCounts {
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub mesh_vertices: usize,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProbeBody {
    pub id: String,
    pub feature: String,
    pub name: String,
    pub kind: String,
    /// Face names in topology order — the list `select` and a stored
    /// reference resolve against.
    pub faces: Vec<String>,
    pub edges: Vec<String>,
    pub vertices: Vec<String>,
    pub counts: ProbeCounts,
    /// `[lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]`, each the hex of the `f64`.
    pub bbox: Vec<String>,
    /// The enclosed volume by the divergence theorem over the triangles, as
    /// the hex of the `f64` (`Mesh::volume`).
    pub volume: String,
    /// One digest over every face's centroid, normal and area, every edge's
    /// endpoints and length, and every vertex position — the named
    /// topology's geometry, to the bit.
    pub topology: String,
    /// One digest over the mesh's `f32` positions and its `u32` indices — the
    /// tessellation itself, to the bit.
    pub mesh: String,
    /// `Inspect`'s report on this body (`cadpole_engine::inspect::inspect_body`),
    /// in its `Debug` form: the solid verdict, the folds, the thinnest
    /// triangle, manifold's opinion, the surface classes.
    pub inspect: String,
    /// The body's memo key as hex: the digest of everything that
    /// made it, or `unkeyed`. Pure arithmetic on the script's numbers, so
    /// byte-identical natively and in wasm.
    pub key: String,
    /// With `detail: true`: per face `[centroid xyz, normal xyz, area]`, per
    /// edge `[start xyz, end xyz, length, polyline points...]`, per vertex
    /// `[position xyz]` -- what `topology` digests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<(Vec<ProbeDetail>, Vec<ProbeDetail>, Vec<ProbeDetail>)>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProbeOut {
    pub ok: bool,
    pub features: Vec<ProbeFeature>,
    pub diagnostics: Vec<ProbeDiagnostic>,
    pub bodies: Vec<ProbeBody>,
    /// The outcomes of the tests `run_tests` asked for, in declaration order.
    pub tests: Vec<ProbeTest>,
}

fn bits(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn hex_bytes(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("bad hex at {i}: {e}")))
        .collect()
}

/// Evaluate `input` and report what the test compares.
pub fn probe(input: &ProbeIn) -> Result<ProbeOut, String> {
    let assets: Option<Arc<dyn AssetStore>> = if input.assets.is_empty() {
        None
    } else {
        let mut store = MemoryAssetStore::new();
        for (name, hex) in &input.assets {
            store.insert(name, hex_bytes(hex).map_err(|e| format!("asset {name:?}: {e}"))?);
        }
        Some(Arc::new(store))
    };
    let overrides = input
        .overrides
        .iter()
        .filter_map(|(k, v)| param_value_from_json(v).map(|v| (k.clone(), v)))
        .collect();
    let evaluation = cadpole_script::evaluate_full(EvalInput {
        scripts: input.scripts.iter().map(|s| ScriptFile { path: s.path.clone(), source: s.source.clone() }).collect(),
        entry: None,
        overrides,
        tolerance: input.tolerance,
        boolean_tolerance: input.boolean_tolerance,
        selection: Vec::new(),
        assets,
        limits: cadpole_script::Limits::structural(),
        tests: if input.run_tests { cadpole_script::TestMode::Unswept { only: None } } else { cadpole_script::TestMode::Skip },
        // A memo, so every body is keyed: `key` below is what the
        // native-vs-wasm test compares.
        memo: Some(Arc::new(cadpole_core::memo::Memo::new())),
        preview: false,
        sweep_results: None,
    });
    let tests: Vec<ProbeTest> = evaluation
        .outcomes
        .iter()
        .map(|o| ProbeTest {
            name: o.name.clone(),
            ok: o.ok,
            message: o.message.clone(),
            point: cadpole_script::point_label(&o.point),
            value: o.value.map(bits),
        })
        .collect();
    let mut ctx = evaluation.ctx;

    let ids: Vec<_> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
    let mut bodies = Vec::with_capacity(ids.len());
    for id in ids {
        let (id_string, name, feature, kind, key) = {
            let b = ctx.body(id).map_err(|e| e.to_string())?;
            (b.id_string(), b.name.clone(), b.feature.0.clone(), b.kind().to_owned(), b.key.hex())
        };
        let (mesh, topo) = ctx.body_mesh(id).map_err(|e| e.to_string())?;
        let mut t = FNV_OFFSET;
        for f in &topo.faces {
            for v in [f.centroid.x, f.centroid.y, f.centroid.z, f.normal.x, f.normal.y, f.normal.z, f.area] {
                t = fnv(t, &v.to_bits().to_le_bytes());
            }
        }
        for e in &topo.edges {
            for v in [e.start.x, e.start.y, e.start.z, e.end.x, e.end.y, e.end.z, e.length] {
                t = fnv(t, &v.to_bits().to_le_bytes());
            }
        }
        for v in &topo.vertices {
            for c in [v.position.x, v.position.y, v.position.z] {
                t = fnv(t, &c.to_bits().to_le_bytes());
            }
        }
        let mut m = FNV_OFFSET;
        for p in &mesh.positions {
            m = fnv(m, &p.to_bits().to_le_bytes());
        }
        for i in &mesh.indices {
            m = fnv(m, &i.to_le_bytes());
        }
        let bbox = mesh.bbox().map(|(lo, hi)| [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z].map(bits).to_vec()).unwrap_or_default();
        let inspection = crate::inspect::inspect_body(&id_string, &name, &feature, &kind, mesh, topo);
        bodies.push(ProbeBody {
            id: id_string,
            feature,
            name,
            kind,
            faces: topo.faces.iter().map(|f| f.name.clone()).collect(),
            edges: topo.edges.iter().map(|e| e.name.clone()).collect(),
            vertices: topo.vertices.iter().map(|v| v.name.clone()).collect(),
            counts: ProbeCounts {
                faces: topo.faces.len(),
                edges: topo.edges.len(),
                vertices: topo.vertices.len(),
                triangles: mesh.triangle_count(),
                mesh_vertices: mesh.vertex_count(),
            },
            bbox,
            volume: bits(mesh.volume()),
            topology: format!("{t:016x}"),
            mesh: format!("{m:016x}"),
            inspect: format!("{inspection:?}"),
            key,
            detail: input.detail.then(|| {
                let v3 = |v: cadpole_core::Vec3| [v.x, v.y, v.z].map(bits).to_vec();
                (
                    topo.faces
                        .iter()
                        .map(|f| ProbeDetail {
                            name: f.name.clone(),
                            values: [v3(f.centroid), v3(f.normal), vec![bits(f.area)]].concat(),
                        })
                        .collect(),
                    topo.edges
                        .iter()
                        .map(|e| ProbeDetail {
                            name: e.name.clone(),
                            values: [v3(e.start), v3(e.end), vec![bits(e.length)], e.polyline.iter().flat_map(|p| v3(*p)).collect()].concat(),
                        })
                        .collect(),
                    topo.vertices.iter().map(|v| ProbeDetail { name: v.name.clone(), values: v3(v.position) }).collect(),
                )
            }),
        });
    }
    let severity = |s: Severity| match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    };
    let mut diagnostics: Vec<ProbeDiagnostic> = Vec::new();
    for d in &ctx.diagnostics {
        diagnostics.push(ProbeDiagnostic {
            severity: severity(d.severity).to_owned(),
            feature: d.feature.as_ref().map(|f| f.0.clone()).unwrap_or_default(),
            message: d.message.clone(),
        });
    }
    for f in &ctx.features {
        for d in &f.diagnostics {
            diagnostics.push(ProbeDiagnostic {
                severity: severity(d.severity).to_owned(),
                feature: f.id.0.clone(),
                message: d.message.clone(),
            });
        }
    }
    Ok(ProbeOut {
        ok: !ctx.diagnostics.iter().any(|d| d.severity == Severity::Error),
        features: ctx
            .features
            .iter()
            .map(|f| ProbeFeature {
                id: f.id.0.clone(),
                kind: f.kind.clone(),
                name: f.name.clone(),
                outputs: f.outputs.iter().map(|b| format!("{}#{}", f.id, b.0)).collect(),
                args: f.args.clone(),
            })
            .collect(),
        diagnostics,
        bodies,
        tests,
    })
}

/// [`probe`] from and to JSON: what `native_probe` reads on stdin and the
/// wasm `Engine.probe` takes as its argument, and what both return.
pub fn probe_json(input: &str) -> Result<String, String> {
    let input: ProbeIn = serde_json::from_str(input).map_err(|e| format!("bad probe input: {e}"))?;
    let out = probe(&input)?;
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_probes_to_its_six_faces_and_its_volume_bits() {
        let input = ProbeIn {
            scripts: vec![ScriptIn { path: "main.star".into(), source: "b = box(10, 20, 30, name=\"b\")".into() }],
            ..Default::default()
        };
        let out = probe(&input).unwrap();
        assert!(out.ok, "{:?}", out.diagnostics);
        assert_eq!(out.bodies.len(), 1);
        let b = &out.bodies[0];
        assert_eq!(b.counts.faces, 6);
        assert_eq!(b.faces.len(), 6);
        assert_eq!(b.counts.triangles, 12);
        assert_eq!(b.volume, bits(6000.0));
        assert!(b.inspect.contains("is_solid: true"), "{}", b.inspect);
        // The same input twice is the same answer byte for byte.
        let again = serde_json::to_string(&probe(&input).unwrap()).unwrap();
        assert_eq!(serde_json::to_string(&out).unwrap(), again);
    }

    #[test]
    fn run_tests_reports_the_outcomes_with_values_as_bits() {
        let input = ProbeIn {
            scripts: vec![ScriptIn {
                path: "main.star".into(),
                source: "b = box(10, 20, 30, name=\"b\")\ndef v():\n    return 10 * 20 * 30.0\ntest(\"volume\", v)\n".into(),
            }],
            run_tests: true,
            ..Default::default()
        };
        let out = probe(&input).unwrap();
        assert!(out.ok, "{:?}", out.diagnostics);
        assert_eq!(out.tests.len(), 1, "{:?}", out.tests);
        assert_eq!(out.tests[0].name, "volume");
        assert!(out.tests[0].ok, "{}", out.tests[0].message);
        assert_eq!(out.tests[0].value.as_deref(), Some(bits(6000.0).as_str()));
        // Off by default: nothing runs, nothing is reported.
        let quiet = probe(&ProbeIn { run_tests: false, ..input }).unwrap();
        assert!(quiet.tests.is_empty());
    }

    #[test]
    fn assets_arrive_as_hex() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40"><path id="p" d="M0 0 H40 V40 H0 Z"/></svg>"#;
        let hex: String = svg.iter().map(|b| format!("{b:02x}")).collect();
        let mut assets = BTreeMap::new();
        assets.insert("badge.svg".to_owned(), hex);
        let input = ProbeIn {
            scripts: vec![ScriptIn {
                path: "main.star".into(),
                source: "s = import_svg(asset(\"badge.svg\"), name=\"s\")\nplate = extrude(s, 3, name=\"plate\")".into(),
            }],
            assets,
            ..Default::default()
        };
        let out = probe(&input).unwrap();
        assert!(out.ok, "{:?}", out.diagnostics);
        assert_eq!(out.bodies.len(), 1);
        assert_eq!(out.bodies[0].volume, bits(40.0 * 40.0 * 3.0));
    }
}
