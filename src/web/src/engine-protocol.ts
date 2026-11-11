// The messages between the page and the engine worker.
//
// Its own module so `engine-worker.ts` can have it without importing the UI:
// the worker bundle is a separate esbuild entry point and pulling `engine.ts`
// into it would drag in `normalize.ts`, `mock.ts` and the type surface of the
// whole app for the sake of four interfaces.
//
// Shape notes that matter on the wire:
//
// * The **model travels as a JSON string**, not as an object. The wasm engine
//   already produces JSON and the meshes are not in it (bodies carry
//   `mesh_index`), so the string is the small half; structured-cloning the
//   parsed object instead would mean deep-cloning tens of thousands of little
//   objects on the worker thread and again on the main one.
// * The **meshes travel as transferables**. Four typed arrays per body, moved
//   rather than copied, so a 1.4 MB base-lid mesh costs a pointer.

import type { ParamValue, ScriptFile } from "./types.js";

/** Marker so a stray `postMessage` from something else is ignored. */
export const ENGINE_WORKER_MESSAGE = "cadpole:engine";

export interface WorkerEvaluateRequest {
  scripts: ScriptFile[];
  overrides: Record<string, ParamValue>;
  tolerance: number;
  deadlineMs: number;
  /** A drag preview: fuses whose inputs changed are skipped. */
  preview: boolean;
}

export type WorkerRequest =
  | { __cadpole: typeof ENGINE_WORKER_MESSAGE; id: number; op: "evaluate"; req: WorkerEvaluateRequest }
  | { __cadpole: typeof ENGINE_WORKER_MESSAGE; id: number; op: "apiDoc" };

/** One body's triangles, as four buffers that can be transferred. */
export interface WorkerMesh {
  positions: Float32Array;
  normals: Float32Array;
  indices: Uint32Array;
  faceIds: Uint32Array;
}

export interface WorkerPanic {
  message: string;
  location: string;
  stack: string;
}

export type WorkerResponse =
  | {
      __cadpole: typeof ENGINE_WORKER_MESSAGE;
      id: number;
      ok: true;
      op: "evaluate";
      /** `cadpole.v1.Model` as the wasm engine's JSON, meshes excluded. */
      modelJson: string;
      /** Indexed by `Body.mesh_index`. */
      meshes: WorkerMesh[];
    }
  | { __cadpole: typeof ENGINE_WORKER_MESSAGE; id: number; ok: true; op: "apiDoc"; apiDocJson: string }
  | {
      __cadpole: typeof ENGINE_WORKER_MESSAGE;
      id: number;
      ok: false;
      op: "evaluate" | "apiDoc" | "init";
      message: string;
      stack: string;
      /** Set when the failure was a Rust panic the hook recorded. */
      panic?: WorkerPanic;
    }
  /** Sent once, unsolicited, when the wasm module is up and usable. */
  | { __cadpole: typeof ENGINE_WORKER_MESSAGE; id: 0; ok: true; op: "ready"; buildId: string };

/** Everything in a response that must be moved rather than copied. */
export function transferablesOf(res: WorkerResponse): Transferable[] {
  if (!res.ok || res.op !== "evaluate") return [];
  const out: Transferable[] = [];
  // `meshes` is indexed by `mesh_index`, which is dense in practice but does
  // not have to be; a hole clones as `undefined` and must not be dereferenced.
  for (const m of res.meshes) {
    if (!m) continue;
    out.push(m.positions.buffer, m.normals.buffer, m.indices.buffer, m.faceIds.buffer);
  }
  return out;
}
