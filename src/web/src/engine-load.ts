// Picking the local engine at start-up, and saying so when it is not the one
// we wanted.
//
// The order is worker, then main thread, then mock, and each step down is
// reported rather than merely noticed: falling back to a hardcoded model and
// putting the reason in a banner is how "the engine is not running" stays a
// secret between the user and the console.

import type { LoadedEngine } from "./engine.js";
import { loadWasmEngine } from "./engine.js";
import { MockEngine } from "./mock.js";
import { ENGINE_WORKER_URL, WorkerEngine } from "./worker-engine.js";

export type LocalEngineChoice = "worker" | "main" | "mock";

function describe(err: unknown): { message: string; stack: string } {
  return { message: err instanceof Error ? err.message : String(err), stack: err instanceof Error ? (err.stack ?? "") : "" };
}

/**
 * Load the local engine.
 *
 * `want` comes from the query string (`?mock=1`, `?engine=main`) and exists so
 * the difference between running the engine on this thread and running it in a
 * worker can be *demonstrated* -- by the e2e suite, and by anyone who does not
 * believe the freeze was real.
 */
export async function loadLocalEngine(want: LocalEngineChoice): Promise<LoadedEngine> {
  if (want === "mock") {
    return { engine: new MockEngine(), notice: null, failure: null };
  }

  if (want === "worker") {
    try {
      return { engine: await WorkerEngine.start(ENGINE_WORKER_URL), notice: null, failure: null };
    } catch (err) {
      const d = describe(err);
      console.warn("cadpole: the engine worker would not start, running the engine on this thread:", d.message);
      const main = await loadMainThread();
      if (main.engine.kind === "wasm") {
        return {
          engine: main.engine,
          // Worth a banner on its own: the app still works, and every heavy
          // evaluation will freeze the tab, which is exactly the complaint
          // the worker exists to answer.
          notice: `The engine could not start in a worker (${d.message.split("\n")[0]}); it is running on the main thread, so a slow model will freeze the page.`,
          failure: d,
        };
      }
      // Neither a worker nor the main thread: report the worker's failure,
      // since that is the first thing that went wrong.
      return { engine: main.engine, notice: main.notice, failure: d };
    }
  }

  return loadMainThread();
}

async function loadMainThread(): Promise<LoadedEngine> {
  try {
    return { engine: await loadWasmEngine(), notice: null, failure: null };
  } catch (err) {
    const d = describe(err);
    console.warn("cadpole: wasm engine unavailable, using MockEngine:", d.message);
    return {
      engine: new MockEngine(),
      notice: `Engine wasm not available (${d.message.split("\n")[0]}). Showing the built-in mock model.`,
      failure: d,
    };
  }
}

/** What `?engine=` / `?mock=` asked for. */
export function requestedEngine(params: URLSearchParams): LocalEngineChoice {
  if (params.get("mock") === "1") return "mock";
  const e = params.get("engine");
  if (e === "main" || e === "mock" || e === "worker") return e === "mock" ? "mock" : e === "main" ? "main" : "worker";
  return "worker";
}

