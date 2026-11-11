//! WebAssembly bindings. The JS API is documented in docs/architecture.md.

use std::cell::RefCell;

use std::sync::Arc;

use cadpole_engine::cadpole_core::memo::Memo;
use cadpole_engine::cadpole_core::tessellate::Mesh;
use cadpole_engine::view::{EvalIn, evaluate_json_with};
use wasm_bindgen::prelude::*;

// The last Rust panic, kept so JavaScript can ask what actually happened.
//
// wasm32 has no unwinding: a panic runs the hook and then traps, and the
// caller in JavaScript catches `RuntimeError: unreachable`, which says nothing
// at all. `console_error_panic_hook` writes the real message to
// `console.error`, where the app cannot read it -- and a message only a
// developer with the console open ever sees is the bug this exists to fix. So
// the hook also *keeps* what it formatted, right here, and `take_panic` hands
// it to the `catch` block. Memory survives a trap, so the record is still
// there when the call site asks for it.
thread_local! {
    static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[wasm_bindgen(start)]
pub fn start() {
    install_panic_hook();
}

/// Install the panic hook: report to `console.error` as before, *and* keep the
/// message, the `file:line:column` and the JavaScript stack for [`take_panic`].
///
/// Idempotent, like `console_error_panic_hook::set_once`, so a second entry
/// point (a worker, a test) can call it without replacing what is there.
pub fn install_panic_hook() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_owned()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "panicked".to_owned()
            };
            // `Location` comes from `#[track_caller]`, not from debug info, so
            // it survives `-c opt` — which matters, because the wasm the web
            // UI loads is always the optimized one.
            let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
            // The same trick `console_error_panic_hook` uses: construct a JS
            // `Error` here and read its `stack`. It is a *JavaScript* stack —
            // `std::backtrace` does not exist on wasm32 — so it names the
            // wasm frames by index and the JS frames by name. That is still
            // the difference between "somewhere in the engine" and "under
            // Engine.evaluate".
            // `stack` is not in ECMAScript, so js-sys has no accessor for it;
            // every engine that matters has the property.
            let err = js_sys::Error::new("cadpole panic");
            let stack = js_sys::Reflect::get(&err, &JsValue::from_str("stack"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();
            let record = serde_json::json!({ "message": message, "location": location, "stack": stack }).to_string();
            // `try_borrow_mut`: a panic *inside* the hook would abort, and
            // losing the record is better than losing the process.
            LAST_PANIC.with(|slot| {
                if let Ok(mut slot) = slot.try_borrow_mut() {
                    *slot = Some(record);
                }
            });
            // Unchanged behaviour for anyone with the console open.
            console_error_panic_hook::hook(info);
        }));
    });
}

/// The last Rust panic as JSON — `{message, location, stack}` — and clear it.
/// `undefined` when nothing has panicked since the last call.
///
/// Call this from the `catch` around any engine call: the exception JavaScript
/// caught is a bare `RuntimeError: unreachable`, and this is where the Rust
/// message and the source location it happened at actually live.
#[wasm_bindgen]
pub fn take_panic() -> Option<String> {
    LAST_PANIC.with(|slot| slot.try_borrow_mut().ok().and_then(|mut s| s.take()))
}

/// Which engine build this is, for a crash report. Two reports of the same
/// panic from different builds are different bugs.
///
/// `CARGO_PKG_VERSION` is real here because `cadpole_wasm_lib` passes `version`
/// in its Bazel rule; a target that does not gets rules_rust's `"0.0.0"`.
/// There is no git revision in it because nothing in this build stamps one —
/// saying "unknown" would be more honest than inventing one.
#[wasm_bindgen]
pub fn build_id() -> String {
    let profile = if cfg!(debug_assertions) { "debug" } else { "opt" };
    let hooks = if cfg!(feature = "test_hooks") { " test-hooks" } else { "" };
    format!("cadpole-wasm {} {profile}{hooks}", env!("CARGO_PKG_VERSION"))
}

/// The string a script must contain for the test build to panic on it. It
/// only exists in the `test_hooks` build; in a shipped engine it is an
/// ordinary comment in an ordinary script.
#[cfg(feature = "test_hooks")]
pub const TEST_PANIC_MARKER: &str = "__cadpole_test_panic__";

/// True in the build that carries the deliberate-panic hook, so a test can
/// assert it is driving the engine it thinks it is, and the reported
/// `engine_build` says `test-hooks`.
#[cfg(feature = "test_hooks")]
#[wasm_bindgen]
pub fn has_test_hooks() -> bool {
    true
}

/// One engine instance keeps the meshes of its last evaluation so the page
/// can pull them out as typed arrays without re-serializing, and the
/// kernel-op memo that makes the next evaluation of the same
/// document cost only what changed.
#[wasm_bindgen]
pub struct Engine {
    meshes: Vec<Mesh>,
    memo: Arc<Memo>,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine { meshes: Vec::new(), memo: Arc::new(Memo::new()) }
    }

    /// Forget every memoised kernel op: what a page does when it opens
    /// another document, so one document's bodies never hold memory for
    /// another's session.
    pub fn clear_memo(&self) {
        self.memo.clear();
    }

    /// Evaluate. `input` is JSON: {scripts:[{path,source}], overrides:{}, tolerance, selection:[], skip_mesh}.
    /// Returns the model view as JSON (see cadpole-engine `view.rs`).
    pub fn evaluate(&mut self, input: &str) -> Result<String, JsError> {
        // A real panic, on the real evaluate path, on demand: this is how the
        // crash-reporting end-to-end test gets one without waiting for a bug.
        // Compiled only into `//src/rust/cadpole-wasm:cadpole_wasm_test_hooks`,
        // never into the wasm `//src/web:dist` ships, so the engine a user
        // runs has no way to be told to crash (docs/security.md).
        #[cfg(feature = "test_hooks")]
        if input.contains(TEST_PANIC_MARKER) {
            panic!("deliberate test panic from the wasm engine");
        }
        let input: EvalIn = serde_json::from_str(input).map_err(|e| JsError::new(&format!("bad input: {e}")))?;
        let (view, meshes) = evaluate_json_with(&input, Some(self.memo.clone()));
        self.meshes = meshes;
        serde_json::to_string(&view).map_err(|e| JsError::new(&e.to_string()))
    }

    /// The determinism probe: evaluate `input` (the `evaluate` JSON
    /// plus `assets: {name: hex}`) and return names, counts, volumes to the
    /// bit and the `Inspect` report, as `cadpole_engine::probe::probe_json`
    /// writes them. `//src/rust/cadpole-wasm:native_vs_wasm_test` runs the same
    /// input through the native `native_probe` and diffs the two.
    pub fn probe(&self, input: &str) -> Result<String, JsError> {
        cadpole_engine::probe::probe_json(input).map_err(|e| JsError::new(&e))
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    pub fn mesh_positions(&self, index: usize) -> js_sys::Float32Array {
        self.meshes.get(index).map(|m| js_sys::Float32Array::from(m.positions.as_slice())).unwrap_or_else(|| js_sys::Float32Array::new_with_length(0))
    }

    pub fn mesh_normals(&self, index: usize) -> js_sys::Float32Array {
        self.meshes.get(index).map(|m| js_sys::Float32Array::from(m.normals.as_slice())).unwrap_or_else(|| js_sys::Float32Array::new_with_length(0))
    }

    pub fn mesh_indices(&self, index: usize) -> js_sys::Uint32Array {
        self.meshes.get(index).map(|m| js_sys::Uint32Array::from(m.indices.as_slice())).unwrap_or_else(|| js_sys::Uint32Array::new_with_length(0))
    }

    pub fn mesh_face_ids(&self, index: usize) -> js_sys::Uint32Array {
        self.meshes.get(index).map(|m| js_sys::Uint32Array::from(m.face_ids.as_slice())).unwrap_or_else(|| js_sys::Uint32Array::new_with_length(0))
    }

    /// The Starlark API as JSON (ApiDocResponse shape).
    pub fn api_doc(&self) -> String {
        let doc = cadpole_engine::cadpole_script::api_doc();
        // prost types are not serde; hand-roll a small JSON view.
        let f = |f: &cadpole_engine::cadpole_proto::v1::ApiFunction| {
            serde_json::json!({
                "name": f.name, "doc": f.doc, "returns": f.returns, "on_type": f.on_type, "constant": f.constant,
                "params": f.params.iter().map(|p| serde_json::json!({
                    "name": p.name, "type": p.r#type, "doc": p.doc, "required": p.required, "default_value": p.default_value
                })).collect::<Vec<_>>()
            })
        };
        serde_json::json!({
            "functions": doc.functions.iter().map(f).collect::<Vec<_>>(),
            "types": doc.types.iter().map(|t| serde_json::json!({
                "name": t.name, "doc": t.doc,
                "methods": t.methods.iter().map(f).collect::<Vec<_>>(),
                "attrs": t.attrs.iter().map(|p| serde_json::json!({"name": p.name, "type": p.r#type, "doc": p.doc})).collect::<Vec<_>>()
            })).collect::<Vec<_>>()
        })
        .to_string()
    }

    /// The protobuf envelope: `cadpole.v1.Request` bytes in, `cadpole.v1.Response` bytes out.
    pub fn call(&self, request: &[u8]) -> js_sys::Uint8Array {
        js_sys::Uint8Array::from(cadpole_engine::call(request).as_slice())
    }

    /// Software-render the scripts to a PNG (for the static playground and tests).
    /// `input` is the same JSON as `evaluate` plus optional width/height/view.
    pub fn render_png(&self, input: &str) -> Result<js_sys::Uint8Array, JsError> {
        #[derive(serde::Deserialize)]
        struct RenderIn {
            #[serde(flatten)]
            eval: EvalIn,
            #[serde(default)]
            width: u32,
            #[serde(default)]
            height: u32,
            #[serde(default)]
            view: String,
        }
        let r: RenderIn = serde_json::from_str(input).map_err(|e| JsError::new(&format!("bad input: {e}")))?;
        let doc = cadpole_engine::document_from_scripts(
            "playground",
            r.eval.scripts.iter().map(|s| (s.path.clone(), s.source.clone())).collect(),
        );
        let mut req = cadpole_engine::cadpole_proto::v1::RenderRequest {
            document: Some(doc),
            width: r.width,
            height: r.height,
            camera: Some(cadpole_engine::cadpole_proto::v1::Camera { preset: r.view, ..Default::default() }),
            tolerance: r.eval.tolerance,
            ..Default::default()
        };
        for (k, v) in &r.eval.overrides {
            if let Some(pv) = cadpole_engine::view::param_value_from_json(v) {
                req.param_overrides.insert(k.clone(), pv.to_proto());
            }
        }
        let resp = cadpole_engine::render(&req).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(js_sys::Uint8Array::from(resp.png.as_slice()))
    }

    pub fn version(&self) -> String {
        cadpole_engine::VERSION.to_owned()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}
