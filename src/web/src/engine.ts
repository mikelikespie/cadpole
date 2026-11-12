// The engine boundary. `EngineLike` is what the UI talks to; four things
// implement it:
//
//   WorkerEngine   the wasm engine in a dedicated module worker -- the default
//   RemoteEngine   `POST /api/ops/Evaluate` on a running `cadpole serve`
//   WasmEngine     the wasm engine on this thread, when there is no Worker
//   MockEngine     a hardcoded parametric model, so the UI runs with no wasm
//   BakedEngine    a published page's one model, evaluated by `cadpole bake`
//
// Two things about the shape are load-bearing:
//
// **Evaluation is asynchronous.** A thirteen-second model on the main thread
// is thirteen seconds of dead tab, so nothing here may offer a synchronous
// facade over a worker or a network call.
//
// **A result is a value, not engine state.** `evaluate` resolves to an
// `Evaluated` carrying both the model and its meshes. Reading meshes back
// from whatever the engine last computed cannot survive concurrency: a
// superseded evaluation landing late would overwrite what a fresh one just
// wrote, and the viewer would draw one evaluation's triangles against
// another's topology. Handing the meshes back with the model makes that
// unrepresentable.

import * as norm from "./normalize.js";
import type { ApiDoc, Model, ParamValue, ScriptFile } from "./types.js";

export type EngineKind = "worker" | "wasm" | "remote" | "mock" | "baked";

/** The default tessellation tolerance in mm, when a caller names none. */
export const DEFAULT_TOLERANCE = 0.05;

/** One body's triangles, as the viewer wants them. */
export interface MeshData {
  /** xyz triples. */
  positions: Float32Array;
  /** xyz triples, one per position. */
  normals: Float32Array;
  indices: Uint32Array;
  /** One topology face index per triangle. */
  faceIds: Uint32Array;
  /**
   * One colour per vertex, sRGB-encoded 8-bit RGB -- three bytes a vertex --
   * or absent: an imported scan's own colour (glTF `COLOR_0`, an OBJ's
   * `v x y z r g b`). Every mesh Cadpole builds itself has none, and the
   * browser's own engine never produces one (it has no assets to import);
   * it arrives from `cadpole serve` in `cadpole.v1.Mesh.colors`.
   */
  colors?: Uint8Array;
}

/** Everything one evaluation produced: `meshes[body.mesh_index]` is its mesh. */
export interface Evaluated {
  model: Model;
  meshes: MeshData[];
  /** The tessellation tolerance this was computed at, in mm. */
  tolerance: number;
  /** Which engine produced it. */
  kind: EngineKind;
  /** Wall clock the caller measured, end to end, in ms. */
  elapsedMs: number;
}

export interface EvaluateRequest {
  scripts: ScriptFile[];
  /** name -> number | bool | string | {x,y,z} */
  overrides: Record<string, ParamValue>;
  /** Tessellation tolerance in mm (0 / absent = `DEFAULT_TOLERANCE`). */
  tolerance?: number;
  /**
   * Wall-clock budget in ms, 0 for none. Passed to the engine, not merely
   * used as a client-side timer: `EvaluateRequest.deadline_ms` is checked by
   * the Starlark interpreter and by every builtin, so a runaway script stops
   * at the far end rather than being abandoned while it keeps running.
   */
  deadlineMs?: number;
  /**
   * A drag preview: sent while a slider is moving. The engine skips
   * every fuse and boolean whose inputs changed and shows the operands
   * unfused; the evaluation sent when the drag ends is full.
   */
  preview?: boolean;
}

export interface EngineLike {
  readonly kind: EngineKind;
  /**
   * Which build this is, for a crash report: "cadpole-wasm 0.1.0 opt", or "mock".
   * Two reports of the same panic from different builds are different bugs.
   */
  readonly buildId: string;
  /**
   * Evaluate. Script errors come back as `model.diagnostics`, never as a
   * rejection; a rejection means the engine itself could not answer.
   *
   * `signal` is honoured as far as the engine allows. A worker is terminated
   * outright; a `fetch` is aborted; the in-process wasm cannot be interrupted
   * at all and finishes in the background, exactly as `src/rust/cadpole-ffi`
   * says of `cadpole_cancel`. In every case the caller's promise rejects and the
   * result is dropped.
   */
  evaluate(req: EvaluateRequest, signal?: AbortSignal): Promise<Evaluated>;
  apiDoc(signal?: AbortSignal): Promise<ApiDoc>;
  /** Release whatever this engine holds (a worker, a pending request). */
  dispose(): void;
}

/** The wasm-bindgen `Engine` class surface (see docs/architecture.md). */
export interface WasmEngineHandle {
  evaluate(json: string): string;
  mesh_positions(index: number): Float32Array;
  mesh_normals(index: number): Float32Array;
  mesh_indices(index: number): Uint32Array;
  mesh_face_ids(index: number): Uint32Array;
  api_doc(): string;
  free?(): void;
}

export interface WasmModule {
  default: (input?: unknown) => Promise<unknown>;
  Engine: new () => WasmEngineHandle;
  /**
   * The last Rust panic as JSON, and clears it (src/rust/cadpole-wasm). Optional
   * so an older `cadpole_wasm.js` still loads.
   */
  take_panic?: () => string | undefined;
  /** Engine version and profile, e.g. "cadpole-wasm 0.1.0 opt". */
  build_id?: () => string;
}

/**
 * A Rust panic that reached JavaScript.
 *
 * What the `catch` actually sees when the engine panics is
 * `RuntimeError: unreachable`, which says nothing: wasm32 has no unwinding, so
 * the panic runs its hook and then traps. The hook keeps the real message and
 * the `file:line:column`, and the wasm engines collect them here on the way
 * out, so the thing that reaches the reporter is
 * "panicked at src/rust/cadpole-core/src/boolean.rs:412:9: time not implemented
 * on this platform" rather than "unreachable".
 */
export class EnginePanic extends Error {
  readonly location: string;
  /** The JS stack captured *inside* the panic hook, wasm frames included. */
  readonly engineStack: string;
  /** What JavaScript itself saw, usually `RuntimeError: unreachable`. */
  readonly thrown: string;

  constructor(panic: { message: string; location: string; stack: string }, thrown: string, operation: string) {
    super(panic.location ? `${panic.message} (at ${panic.location}, during ${operation})` : panic.message);
    this.name = "EnginePanic";
    this.location = panic.location;
    this.engineStack = panic.stack;
    this.thrown = thrown;
  }
}

export function parsePanic(raw: string | undefined): { message: string; location: string; stack: string } | null {
  if (!raw) return null;
  try {
    const o = JSON.parse(raw) as Record<string, unknown>;
    const message = typeof o["message"] === "string" ? o["message"] : "";
    if (!message) return null;
    return {
      message,
      location: typeof o["location"] === "string" ? o["location"] : "",
      stack: typeof o["stack"] === "string" ? o["stack"] : "",
    };
  } catch {
    return null;
  }
}

/** `{scripts, overrides, tolerance, deadline_ms, preview}` as the wasm engine wants it. */
export function wasmRequestJson(req: EvaluateRequest): string {
  return JSON.stringify({
    scripts: req.scripts,
    overrides: req.overrides,
    tolerance: req.tolerance ?? DEFAULT_TOLERANCE,
    deadline_ms: req.deadlineMs ?? 0,
    preview: req.preview === true,
  });
}

/** Rejects with the standard `AbortError` when `signal` is already aborted. */
export function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted) {
    const e = new Error("aborted");
    e.name = "AbortError";
    throw e;
  }
}

/**
 * The wasm engine on the main thread.
 *
 * Kept only for browsers with no `Worker` and for `?engine=main`, which is how
 * the difference is demonstrated rather than asserted. Its `evaluate` returns
 * a promise like everyone else's and blocks the thread solid while it runs;
 * that is not a flaw in the wrapper, it is the thing `WorkerEngine` exists to
 * avoid.
 */
export class WasmEngine implements EngineLike {
  readonly kind = "wasm" as const;
  readonly buildId: string;

  constructor(
    private readonly handle: WasmEngineHandle,
    private readonly mod: WasmModule,
  ) {
    let id = "cadpole-wasm (unknown build)";
    try {
      id = mod.build_id?.() ?? id;
    } catch {
      /* an engine that cannot say which build it is still runs */
    }
    this.buildId = id;
  }

  /**
   * Run one engine call and, if it traps, replace the useless
   * `RuntimeError: unreachable` with the panic the hook recorded.
   *
   * Every entry point goes through here, not just `evaluate`: a panic in
   * `mesh_positions` or `api_doc` is exactly as invisible as the one that
   * started all this.
   */
  private run<T>(operation: string, call: () => T): T {
    try {
      return call();
    } catch (err) {
      const panic = parsePanic(this.mod.take_panic?.());
      if (panic) throw new EnginePanic(panic, err instanceof Error ? err.message : String(err), operation);
      throw err;
    }
  }

  evaluate(req: EvaluateRequest, signal?: AbortSignal): Promise<Evaluated> {
    throwIfAborted(signal);
    const t0 = performance.now();
    const json = this.run("evaluate", () => this.handle.evaluate(wasmRequestJson(req)));
    const raw: unknown = JSON.parse(json);
    const err = (raw as { error?: unknown } | null)?.error;
    if (typeof err === "string" && err) return Promise.reject(new Error(err));
    const model = norm.model(raw);
    // wasm-bindgen hands back views into wasm memory, invalidated when the
    // memory grows on the next evaluate(); copy them out.
    const meshes: MeshData[] = [];
    for (const body of model.bodies) {
      // Instances share their part body's mesh: fetch it once.
      if (meshes[body.mesh_index]) continue;
      meshes[body.mesh_index] = {
        positions: this.run("mesh_positions", () => this.handle.mesh_positions(body.mesh_index).slice()),
        normals: this.run("mesh_normals", () => this.handle.mesh_normals(body.mesh_index).slice()),
        indices: this.run("mesh_indices", () => this.handle.mesh_indices(body.mesh_index).slice()),
        faceIds: this.run("mesh_face_ids", () => this.handle.mesh_face_ids(body.mesh_index).slice()),
      };
    }
    return Promise.resolve({
      model,
      meshes,
      tolerance: req.tolerance ?? DEFAULT_TOLERANCE,
      kind: this.kind,
      elapsedMs: performance.now() - t0,
    });
  }

  apiDoc(): Promise<ApiDoc> {
    const raw: unknown = JSON.parse(this.run("api_doc", () => this.handle.api_doc()));
    const o = (raw ?? {}) as { functions?: unknown; types?: unknown };
    return Promise.resolve({
      functions: Array.isArray(o.functions) ? (o.functions as ApiDoc["functions"]) : [],
      types: Array.isArray(o.types) ? (o.types as ApiDoc["types"]) : [],
    });
  }

  dispose(): void {
    try {
      this.handle.free?.();
    } catch {
      /* nothing useful to do about a failed free */
    }
  }
}

/**
 * `cadpole_wasm.js` is served next to index.html by the CLI (not bundled).
 * Pure, so a bundle that never loads the wasm on its own thread drops it.
 */
export const WASM_MODULE_URL = /* @__PURE__ */ besideThisModule("./cadpole_wasm.js");

function besideThisModule(file: string): string {
  return new URL(file, import.meta.url).href;
}

export interface LoadedEngine {
  engine: EngineLike;
  /** Human-readable reason when something is standing in for the default. */
  notice: string | null;
  /**
   * What went wrong, when a fallback is in use. The caller reports this:
   * falling back to a hardcoded model and saying so only in a banner is how
   * "the engine is not running" stays a secret between the user and the
   * console.
   */
  failure: { message: string; stack: string } | null;
}

/** Load the wasm module and wrap it for this thread. */
export async function loadWasmEngine(): Promise<WasmEngine> {
  const mod = (await import(/* @vite-ignore */ WASM_MODULE_URL)) as WasmModule;
  if (typeof mod.default !== "function" || typeof mod.Engine !== "function") {
    throw new Error("cadpole_wasm.js does not export init()/Engine");
  }
  await mod.default();
  return new WasmEngine(new mod.Engine(), mod);
}
