// The engine, off the main thread.
//
// This is the whole of the worker: load `cadpole_wasm.js`, answer `evaluate` and
// `apiDoc`, hand the meshes back as transferables. It is a *module* worker, so
// it can `import()` the wasm-bindgen glue the same way the page can.
//
// It runs in `//src/web:dist` as `engine-worker.js`, its own esbuild entry
// point beside `app.js` and `worker-report.js`, and it deliberately imports
// nothing from the UI: `engine-protocol.js` and the generated `cadpole_wasm.js`
// are all it knows about. A worker that imported `main.js` would run the app.
//
// Why it exists at all: evaluating `examples/gridfinity_baselid.star` takes
// about 13 seconds in the wasm build, and on the main thread that is 13
// seconds with no spinner, no camera and no way to give up. Moving it here is
// also the only place in Cadpole where cancelling actually reclaims the machine
// -- `src/rust/cadpole-ffi` is explicit that `cadpole_cancel` releases the caller
// and not the CPU, because nothing can interrupt Starlark or the kernel
// mid-operation. `worker.terminate()` can.

import { ENGINE_WORKER_MESSAGE, transferablesOf, type WorkerRequest, type WorkerResponse } from "./engine-protocol.js";

interface WasmEngineHandle {
  evaluate(json: string): string;
  mesh_positions(index: number): Float32Array;
  mesh_normals(index: number): Float32Array;
  mesh_indices(index: number): Uint32Array;
  mesh_face_ids(index: number): Uint32Array;
  api_doc(): string;
  free?(): void;
}

interface WasmModule {
  default: (input?: unknown) => Promise<unknown>;
  Engine: new () => WasmEngineHandle;
  take_panic?: () => string | undefined;
  build_id?: () => string;
}

/**
 * The slice of `DedicatedWorkerGlobalScope` this file uses.
 *
 * Declared rather than imported: `tsconfig.json` gives every module the `DOM`
 * lib, and adding `WebWorker` to it would put a second, conflicting set of
 * globals in front of every other file in the app for the sake of two methods.
 */
interface WorkerScope {
  postMessage(message: unknown, transfer: Transferable[]): void;
  addEventListener(type: "message", listener: (ev: MessageEvent) => void): void;
}

const ctx = self as unknown as WorkerScope;

let engine: WasmEngineHandle | null = null;
let mod: WasmModule | null = null;

function post(res: WorkerResponse): void {
  ctx.postMessage(res, transferablesOf(res));
}

function panicOf(): { message: string; location: string; stack: string } | undefined {
  try {
    const raw = mod?.take_panic?.();
    if (!raw) return undefined;
    const o = JSON.parse(raw) as Record<string, unknown>;
    const message = typeof o["message"] === "string" ? o["message"] : "";
    if (!message) return undefined;
    return {
      message,
      location: typeof o["location"] === "string" ? o["location"] : "",
      stack: typeof o["stack"] === "string" ? o["stack"] : "",
    };
  } catch {
    return undefined;
  }
}

function fail(id: number, op: "evaluate" | "apiDoc" | "init", err: unknown): void {
  const panic = panicOf();
  const base: Extract<WorkerResponse, { ok: false }> = {
    __cadpole: ENGINE_WORKER_MESSAGE,
    id,
    ok: false,
    op,
    message: err instanceof Error ? err.message : String(err),
    stack: err instanceof Error ? (err.stack ?? "") : "",
  };
  post(panic ? { ...base, panic } : base);
}

/**
 * Load the wasm next to this script.
 *
 * `import.meta.url` is the worker's own URL, and in `dist/` the worker sits in
 * the same directory as `cadpole_wasm.js`, so this is the same resolution the
 * page did before.
 *
 * The `?b=` on it, when there is one, is a hot swap in progress: the
 * page starts a second worker on a busted URL so the browser fetches the
 * rebuilt engine rather than the one it already has. Relative URL resolution
 * drops a base's query, and so does `cadpole_wasm.js`'s own default path to the
 * `.wasm`, so the token has to be put back on by hand at both hops --
 * otherwise the new worker loads the *old* engine out of the HTTP cache and
 * the swap silently does nothing, which is the one failure mode that would
 * look exactly like success.
 */
const selfUrl = new URL(import.meta.url);
const bust = selfUrl.searchParams.get("b") ?? "";

function beside(name: string): URL {
  const url = new URL(`./${name}`, selfUrl);
  if (bust) url.searchParams.set("b", bust);
  return url;
}

const boot = (async (): Promise<void> => {
  const url = beside("cadpole_wasm.js").href;
  const m = (await import(/* @vite-ignore */ url)) as WasmModule;
  if (typeof m.default !== "function" || typeof m.Engine !== "function") {
    throw new Error("cadpole_wasm.js does not export init()/Engine");
  }
  // Without a token this is exactly what the glue does for itself; with one,
  // it is the same URL plus the token.
  await m.default(bust ? { module_or_path: beside("cadpole_wasm_bg.wasm") } : undefined);
  mod = m;
  engine = new m.Engine();
  let buildId = "cadpole-wasm (unknown build)";
  try {
    buildId = m.build_id?.() ?? buildId;
  } catch {
    /* an engine that cannot say which build it is still runs */
  }
  post({ __cadpole: ENGINE_WORKER_MESSAGE, id: 0, ok: true, op: "ready", buildId });
})();

boot.catch((err: unknown) => {
  // `id: 0` is the boot slot: the page is waiting on `ready` and this is the
  // answer instead. It then falls back rather than hanging.
  fail(0, "init", err);
});

ctx.addEventListener("message", (ev: MessageEvent) => {
  const msg = ev.data as WorkerRequest | null;
  if (!msg || msg.__cadpole !== ENGINE_WORKER_MESSAGE) return;
  void boot
    .then(() => {
      const e = engine;
      if (!e) throw new Error("the engine is not loaded");
      if (msg.op === "apiDoc") {
        post({ __cadpole: ENGINE_WORKER_MESSAGE, id: msg.id, ok: true, op: "apiDoc", apiDocJson: e.api_doc() });
        return;
      }
      const modelJson = e.evaluate(
        JSON.stringify({
          scripts: msg.req.scripts,
          overrides: msg.req.overrides,
          tolerance: msg.req.tolerance,
          deadline_ms: msg.req.deadlineMs,
          preview: msg.req.preview === true,
        }),
      );
      // The mesh indices are read straight out of the JSON rather than through
      // the UI's normalizer: the worker has no business knowing the UI's
      // shapes, and all it needs is `bodies[].mesh_index`.
      const parsed = JSON.parse(modelJson) as { error?: string; bodies?: { mesh_index?: number }[] };
      if (typeof parsed.error === "string" && parsed.error) throw new Error(parsed.error);
      const meshes: { positions: Float32Array; normals: Float32Array; indices: Uint32Array; faceIds: Uint32Array }[] = [];
      (parsed.bodies ?? []).forEach((b, i) => {
        const index = typeof b.mesh_index === "number" ? b.mesh_index : i;
        // Instances share their part body's mesh: copy it out once.
        if (meshes[index]) return;
        // `.slice()` copies out of wasm memory, which is invalidated when it
        // grows on the next evaluate -- and gives us a buffer we own, which is
        // what makes it transferable.
        meshes[index] = {
          positions: e.mesh_positions(index).slice(),
          normals: e.mesh_normals(index).slice(),
          indices: e.mesh_indices(index).slice(),
          faceIds: e.mesh_face_ids(index).slice(),
        };
      });
      post({ __cadpole: ENGINE_WORKER_MESSAGE, id: msg.id, ok: true, op: "evaluate", modelJson, meshes });
    })
    .catch((err: unknown) => fail(msg.id, msg.op, err));
});
