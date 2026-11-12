// `WorkerEngine`: the wasm engine, in a dedicated worker.
//
// The default engine everywhere -- session mode, the static playground,
// offline. It is what stops a thirteen-second evaluation from being thirteen
// seconds of frozen tab, and it needs no server, which is why it comes before
// the remote engine rather than after it.
//
// **Cancelling here is real.** Everywhere else in Cadpole, abandoning an
// evaluation releases the caller and leaves the work running: `cadpole-ffi` says
// so in as many words, because nothing can interrupt Starlark or the kernel
// mid-operation. A worker can be terminated. So this is the one front-end
// where dragging a slider does not leave a queue of dead evaluations eating
// the machine behind the user's finger.
//
// The cost of terminating is a fresh worker and a fresh wasm instantiation,
// which is why the job runner only cancels a job that has been running longer
// than its `abortAfterMs`: for a model that evaluates in five milliseconds,
// waiting is cheaper than restarting.

import {
  DEFAULT_TOLERANCE,
  EnginePanic,
  throwIfAborted,
  type EngineLike,
  type EvaluateRequest,
  type Evaluated,
  type MeshData,
} from "./engine.js";
import { ENGINE_WORKER_MESSAGE, type WorkerRequest, type WorkerResponse } from "./engine-protocol.js";
import * as norm from "./normalize.js";
import { span } from "./perf.js";
import type { ApiDoc } from "./types.js";

/**
 * Where the worker script lives: next to `app.js` in `dist/`. Marked pure so a
 * bundle that never starts this worker (the project site's 3D, which has its
 * own) carries no address of a file it does not ship.
 */
export const ENGINE_WORKER_URL = /* @__PURE__ */ besideThisModule("./engine-worker.js");

function besideThisModule(file: string): string {
  return new URL(file, import.meta.url).href;
}

interface Waiter {
  resolve: (r: WorkerResponse) => void;
  reject: (e: unknown) => void;
  operation: "evaluate" | "apiDoc";
}

/** A worker that died, so every caller waiting on it learns why. */
class WorkerGone extends Error {
  constructor(why: string) {
    super(why);
    this.name = "WorkerGone";
  }
}

export class WorkerEngine implements EngineLike {
  readonly kind = "worker" as const;
  readonly buildId: string;
  private worker: Worker | null;
  private nextId = 1;
  private readonly waiting = new Map<number, Waiter>();
  private disposed = false;

  private constructor(
    worker: Worker,
    buildId: string,
    private readonly spawn: () => Worker,
  ) {
    this.buildId = buildId;
    this.worker = worker;
    this.listen(worker);
  }

  /**
   * Start a worker and wait for it to report that the wasm is up.
   *
   * Rejects if there is no `Worker`, if the script will not load, or if the
   * engine inside it will not initialise -- all three are "use something
   * else", and the caller falls back rather than leaving a dead UI.
   */
  static async start(url: string, timeoutMs = 20_000): Promise<WorkerEngine> {
    if (typeof Worker === "undefined") throw new Error("this browser has no Worker");
    const spawn = (): Worker => new Worker(url, { type: "module", name: "cadpole-engine" });
    const first = spawn();
    const buildId = await new Promise<string>((resolve, reject) => {
      const timer = setTimeout(() => {
        cleanup();
        first.terminate();
        reject(new Error(`the engine worker did not start within ${timeoutMs} ms`));
      }, timeoutMs);
      const onMessage = (ev: MessageEvent): void => {
        const msg = ev.data as WorkerResponse | null;
        if (!msg || msg.__cadpole !== ENGINE_WORKER_MESSAGE) return;
        if (msg.ok && msg.op === "ready") {
          cleanup();
          resolve(msg.buildId);
        } else if (!msg.ok && msg.op === "init") {
          cleanup();
          first.terminate();
          reject(new Error(msg.message));
        }
      };
      const onError = (ev: ErrorEvent): void => {
        cleanup();
        first.terminate();
        reject(new Error(ev.message || "the engine worker failed to load"));
      };
      const cleanup = (): void => {
        clearTimeout(timer);
        first.removeEventListener("message", onMessage);
        first.removeEventListener("error", onError as EventListener);
      };
      first.addEventListener("message", onMessage);
      first.addEventListener("error", onError as EventListener);
    });
    return new WorkerEngine(first, buildId, spawn);
  }

  private listen(w: Worker): void {
    w.addEventListener("message", (ev: MessageEvent) => {
      const msg = ev.data as WorkerResponse | null;
      if (!msg || msg.__cadpole !== ENGINE_WORKER_MESSAGE) return;
      if (msg.ok && msg.op === "ready") return; // a respawn announcing itself
      const waiter = this.waiting.get(msg.id);
      if (!waiter) return;
      this.waiting.delete(msg.id);
      waiter.resolve(msg);
    });
    w.addEventListener("error", (ev) => {
      // A worker-level error kills everything in flight on it. Nothing here
      // can tell which call caused it, so everyone waiting is told.
      const why = (ev as ErrorEvent).message || "the engine worker failed";
      this.failAll(new WorkerGone(why));
    });
  }

  private failAll(err: unknown): void {
    const waiters = [...this.waiting.values()];
    this.waiting.clear();
    for (const w of waiters) w.reject(err);
  }

  /**
   * Throw this worker away and start another.
   *
   * Called when a job is cancelled: this is the only way to stop an
   * evaluation that is already inside the kernel. The replacement is spawned
   * immediately rather than lazily so its wasm instantiation overlaps whatever
   * the page does next -- the module itself is already compiled and cached by
   * the browser, so what is being paid for is an instantiate, not a compile.
   */
  private restart(why: string): void {
    const old = this.worker;
    this.worker = null;
    this.failAll(new WorkerGone(why));
    old?.terminate();
    if (this.disposed) return;
    const next = this.spawn();
    this.worker = next;
    this.listen(next);
  }

  private send(msg: WorkerRequest, operation: "evaluate" | "apiDoc", signal?: AbortSignal): Promise<WorkerResponse> {
    throwIfAborted(signal);
    const w = this.worker;
    if (!w) return Promise.reject(new Error("the engine worker is not running"));
    return new Promise<WorkerResponse>((resolve, reject) => {
      // Terminating is the point. The alternative -- letting it run and
      // dropping the answer -- is what every other Cadpole front-end has to do,
      // and it lets a slider drag on a heavy model bury the machine.
      const onAbort = (): void => this.restart("cancelled");
      const unlisten = (): void => signal?.removeEventListener("abort", onAbort);
      signal?.addEventListener("abort", onAbort, { once: true });
      this.waiting.set(msg.id, {
        operation,
        resolve: (r) => {
          unlisten();
          resolve(r);
        },
        reject: (e) => {
          unlisten();
          reject(e);
        },
      });
      w.postMessage(msg);
    });
  }

  /**
   * Turn a failed response into something to throw, and replace the worker if
   * the engine inside it panicked.
   *
   * wasm32 has no unwinding: a Rust panic runs its hook and then *traps*, and
   * the instance is not usable afterwards. On the main thread that meant the
   * page was finished until someone reloaded it. A worker can simply be
   * replaced, so a panic costs one evaluation rather than the session -- the
   * next edit runs against a fresh engine. The report still carries the Rust
   * message and `file:line:column` the hook recorded, because the
   * worker reads them before it dies.
   */
  private failure(res: Extract<WorkerResponse, { ok: false }>, operation: string): Error {
    if (res.panic) {
      this.restart("the engine panicked");
      return new EnginePanic(res.panic, res.message, operation);
    }
    const e = new Error(res.message);
    if (res.stack) e.stack = res.stack;
    return e;
  }

  async evaluate(req: EvaluateRequest, signal?: AbortSignal): Promise<Evaluated> {
    const t0 = performance.now();
    const tolerance = req.tolerance ?? DEFAULT_TOLERANCE;
    const res = await this.send(
      {
        __cadpole: ENGINE_WORKER_MESSAGE,
        id: this.nextId++,
        op: "evaluate",
        req: {
          scripts: req.scripts,
          overrides: req.overrides,
          tolerance,
          deadlineMs: req.deadlineMs ?? 0,
          preview: req.preview === true,
        },
      },
      "evaluate",
      signal,
    );
    if (!res.ok) throw this.failure(res, "evaluate");
    if (res.op !== "evaluate") throw new Error("the engine worker answered the wrong question");
    // On the page's thread, and inside the same task as the apply that
    // follows it: the message event, this parse and `setModel` are one
    // stall, which is why they are measured as three phases of one thing.
    const raw = span("engine.parse", () => JSON.parse(res.modelJson) as unknown);
    const model = span("engine.normalize", () => norm.model(raw));
    const meshes: MeshData[] = [];
    res.meshes.forEach((m, i) => {
      if (m) meshes[i] = m;
    });
    return { model, meshes, tolerance, kind: this.kind, elapsedMs: performance.now() - t0 };
  }

  async apiDoc(signal?: AbortSignal): Promise<ApiDoc> {
    const res = await this.send({ __cadpole: ENGINE_WORKER_MESSAGE, id: this.nextId++, op: "apiDoc" }, "apiDoc", signal);
    if (!res.ok) throw this.failure(res, "api_doc");
    if (res.op !== "apiDoc") throw new Error("the engine worker answered the wrong question");
    const raw = JSON.parse(res.apiDocJson) as { functions?: unknown; types?: unknown };
    return {
      functions: Array.isArray(raw.functions) ? (raw.functions as ApiDoc["functions"]) : [],
      types: Array.isArray(raw.types) ? (raw.types as ApiDoc["types"]) : [],
    };
  }

  /**
   * Stop replacing this worker, without stopping it.
   *
   * The step before a hot swap. `restart` exists so that cancelling an
   * evaluation, or surviving a panic, costs one worker rather than the
   * session -- but during a swap the replacement already exists, so respawning
   * this one would start an engine on the *old* build a moment before it is
   * thrown away. Retiring first turns the abort that follows into a plain
   * terminate.
   *
   * Nothing in flight is failed here: this engine is still the one answering
   * until the caller abandons its work, and a caller that retires and then
   * changes its mind has lost nothing.
   */
  retire(): void {
    this.disposed = true;
  }

  dispose(): void {
    this.disposed = true;
    const w = this.worker;
    this.worker = null;
    this.failAll(new WorkerGone("disposed"));
    w?.terminate();
  }
}
