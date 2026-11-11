// What a rebuilt bundle costs, and the cheapest thing that answers it.
//
// `cadpole serve` watches the built web UI and puts a fingerprint of it in the
// session view and in every `build_changed` event. The fingerprint names each
// file separately:
//
//   index.html:1417:9f3a…|app.js:1216260:…|styles.css:30209:…|
//   cadpole_wasm_bg.wasm:19884615:…|cadpole_wasm.js:18872:…
//
// so a page can tell *what* moved and do the least that suffices. Three
// answers, and only the last one costs the user anything:
//
//   styles.css          swap the `<link>`. Instant, and nothing is lost --
//                       not a pixel of the DOM, not the camera, not a job.
//   the wasm engine     start a second engine worker, prove it answers, then
//                       retire the first. The page keeps its DOM, its camera,
//                       its selection, its messages and its script; it pays
//                       one evaluation, because the triangles on screen came
//                       out of the engine that was just replaced.
//   app.js / index.html the UI itself. There is no honest way to swap the
//                       module graph of a page that is running it, so this
//                       still reloads -- but it asks first, and it puts back
//                       more than the camera.
//
// Anything this file does not recognise reloads. A page is always older than
// the server that is talking to it, so an unknown field is a file some later
// build added, and guessing about a file you have never heard of is how a
// hot-reload mechanism serves half of one build and half of another.

import { dropServiceWorkerCaches } from "./pwa.js";
import type { EngineLike } from "./engine.js";
import { ENGINE_WORKER_URL, WorkerEngine } from "./worker-engine.js";
import type { ApiDoc } from "./types.js";

/**
 * The files the fingerprint covers, and what each one implies.
 *
 * This is a contract with `WEB_BUILD_FILES` in `src/rust/cadpole-cli/src/server.rs`:
 * the server decides what is fingerprinted, this decides what that costs.
 */
const STYLE_FILES = ["styles.css"] as const;
/**
 * The engine, and the three files it is made of.
 *
 * `engine-worker.js` belongs here rather than with the files that force a
 * reload: it is the one bundle a running page genuinely *can* replace,
 * because replacing it is precisely what `startReplacementEngine` does. A
 * page that classed it with app.js would reload for a change it can take for
 * the price of one worker.
 */
const ENGINE_FILES = ["cadpole_wasm_bg.wasm", "cadpole_wasm.js", "engine-worker.js"] as const;
const RELOAD_FILES = ["app.js", "index.html"] as const;
/** The one whose bytes decide whether the model has to be computed again. */
const ENGINE_WASM = "cadpole_wasm_bg.wasm";

const KNOWN: ReadonlySet<string> = new Set<string>([...STYLE_FILES, ...ENGINE_FILES, ...RELOAD_FILES]);

/** What to do about one build change. */
export interface BuildPlan {
  /** Every file whose field differs, in fingerprint order. */
  changed: string[];
  /** Swap the stylesheet link. */
  styles: boolean;
  /** Start a new engine worker and retire the current one. */
  engine: boolean;
  /**
   * Re-run the evaluation after the swap.
   *
   * True exactly when `cadpole_wasm_bg.wasm` moved. A change to `cadpole_wasm.js`
   * alone is wasm-bindgen glue regenerated around the same engine, so the
   * answers cannot have changed and re-computing them is pure cost.
   */
  reevaluate: boolean;
  /** Nothing short of a page load will do. */
  reload: boolean;
  /** Which file forced the reload, for the bar. */
  reloadBecause: string;
  /** Cache-busting token for the stylesheet, from its own field. */
  stylesToken: string;
  /** Cache-busting token for the engine, from both of its fields. */
  engineToken: string;
}

const NOTHING: BuildPlan = {
  changed: [],
  styles: false,
  engine: false,
  reevaluate: false,
  reload: false,
  reloadBecause: "",
  stylesToken: "",
  engineToken: "",
};

/**
 * `name:size:hash|…` as a map of name to the rest of its field.
 *
 * Returns null for anything that is not that shape, including the empty
 * string a page gets when the server serves no web directory at all.
 */
export function parseBuild(id: string): Map<string, string> | null {
  if (!id) return null;
  const out = new Map<string, string>();
  for (const field of id.split("|")) {
    const at = field.indexOf(":");
    if (at <= 0) return null;
    out.set(field.slice(0, at), field.slice(at + 1));
  }
  return out.size > 0 ? out : null;
}

/** A short, filename-safe token that changes when `fields` do. */
function tokenOf(fields: (string | undefined)[]): string {
  const raw = fields.filter(Boolean).join("|");
  // FNV-1a, the same idea as the server's, over a string that is already a
  // digest. This only has to differ when the input differs; it is a cache
  // buster, not a checksum.
  let h = 0x811c9dc5;
  for (let i = 0; i < raw.length; i++) {
    h ^= raw.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(36);
}

/**
 * What this page should do about moving from build `from` to build `to`.
 *
 * Pure, and deliberately so: everything interesting about this feature is the
 * classification, and the classification is a function of two strings.
 */
export function planFor(from: string, to: string): BuildPlan {
  if (from === to) return NOTHING;
  const a = parseBuild(from);
  const b = parseBuild(to);
  if (!a || !b) {
    return {
      ...NOTHING,
      changed: [],
      reload: true,
      reloadBecause: "this page cannot read the server's build fingerprint",
    };
  }

  const names = new Set<string>([...a.keys(), ...b.keys()]);
  const changed: string[] = [];
  for (const name of names) {
    if (a.get(name) !== b.get(name)) changed.push(name);
  }
  if (changed.length === 0) return NOTHING;

  const unknown = changed.filter((n) => !KNOWN.has(n));
  const reloadFile = changed.find((n) => (RELOAD_FILES as readonly string[]).includes(n));
  const styles = changed.some((n) => (STYLE_FILES as readonly string[]).includes(n));
  const engine = changed.some((n) => (ENGINE_FILES as readonly string[]).includes(n));

  return {
    changed: changed.sort(),
    styles,
    engine,
    reevaluate: changed.includes(ENGINE_WASM),
    reload: unknown.length > 0 || reloadFile !== undefined,
    reloadBecause:
      unknown.length > 0
        ? `this build changed ${unknown.join(", ")}, which this page does not know how to replace`
        : reloadFile
          ? `${reloadFile} changed, and a page cannot replace the code it is running`
          : "",
    stylesToken: tokenOf([b.get("styles.css")]),
    // Every file the replacement worker will fetch, `engine-worker.js`
    // included: a build in which only the worker bundle moved would otherwise
    // produce the same token, and the swap would re-fetch the same URL and be
    // handed the copy it is trying to replace.
    engineToken: tokenOf([b.get(ENGINE_WASM), b.get("cadpole_wasm.js"), b.get("engine-worker.js")]),
  };
}

/** A one-line summary of a plan, for a status line or a screen reader. */
export function describePlan(plan: BuildPlan): string {
  if (plan.reload) return plan.reloadBecause;
  const bits: string[] = [];
  if (plan.styles) bits.push("styles");
  if (plan.engine) bits.push("the engine");
  return bits.length ? `${bits.join(" and ")} updated` : "nothing changed";
}

/**
 * `from`, with the named files' fields taken from `to`.
 *
 * What the page records as "the build I am running" has to be what it is
 * *actually* running, file by file, and after a partial application that is
 * neither fingerprint. Without this, a page holding an unanswered reload offer
 * keeps `from` -- so the next rebuild of anything diffs against a stylesheet it
 * replaced ten minutes ago and swaps it again, and again, once per rebuild.
 */
export function adoptFields(from: string, to: string, names: readonly string[]): string {
  const a = parseBuild(from);
  const b = parseBuild(to);
  if (!a || !b) return from;
  for (const name of names) {
    const field = b.get(name);
    if (field !== undefined) a.set(name, field);
    else a.delete(name);
  }
  return [...a].map(([name, rest]) => `${name}:${rest}`).join("|");
}

// ---------------------------------------------------------------- styles

/**
 * Point the stylesheet at the new build.
 *
 * The new `<link>` goes in *after* the old one and the old one is removed only
 * once the new one has loaded: two stylesheets briefly apply, the later one
 * wins, and there is no frame with no stylesheet at all. Removing first and
 * adding second is the version of this that flashes the whole app white.
 *
 * Resolves false if the new sheet will not load, in which case the old one is
 * still there and still applied -- a stale stylesheet is a cosmetic problem
 * and an unstyled page is not.
 */
export async function swapStylesheet(token: string, timeoutMs = 5_000): Promise<boolean> {
  const current = document.querySelector<HTMLLinkElement>('link[rel="stylesheet"]');
  if (!current) return false;
  const href = new URL(current.getAttribute("href") ?? "./styles.css", document.baseURI);
  href.searchParams.set("b", token);
  const next = document.createElement("link");
  next.rel = "stylesheet";
  next.href = href.href;

  const loaded = await new Promise<boolean>((resolve) => {
    let settled = false;
    const done = (ok: boolean): void => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      resolve(ok);
    };
    const timer = window.setTimeout(() => done(false), timeoutMs);
    next.addEventListener("load", () => done(true), { once: true });
    next.addEventListener("error", () => done(false), { once: true });
    current.after(next);
  });

  if (loaded) current.remove();
  else next.remove();
  return loaded;
}

// ---------------------------------------------------------------- engine

/**
 * The engine worker's URL for a particular build.
 *
 * The token is a query parameter rather than a path, because it has to defeat
 * three caches at once and only a query can: the browser's HTTP cache for the
 * worker script, the same cache for `cadpole_wasm.js` and `cadpole_wasm_bg.wasm`
 * (the worker reads `b` off its own URL and puts it on both -- see
 * `engine-worker.ts`), and the service worker's stale-while-revalidate.
 *
 * That last one only holds because `sw.js` is *told* what `b` means: its
 * cache lookup is `ignoreSearch` by default, which answered a busted URL out
 * of the precache, so it now treats a `b` parameter as "not what you have".
 * A query alone would not have defeated it.
 */
export function engineWorkerUrl(token: string): string {
  const url = new URL(ENGINE_WORKER_URL);
  if (token) url.searchParams.set("b", token);
  return url.href;
}

/** A healthy replacement engine, and the first answer it gave. */
export interface StartedEngine {
  engine: WorkerEngine;
  apiDoc: ApiDoc;
  /** How long start-up plus the health check took, in ms. */
  elapsedMs: number;
}

/**
 * Start an engine on the new build and prove it works, without touching the
 * one that is running.
 *
 * "Prove it works" is two things, and the second is the one that matters.
 * `WorkerEngine.start` resolves when the worker has instantiated the wasm and
 * constructed an `Engine`, which is necessary and not sufficient: a module
 * that instantiates and then traps on its first call would pass. So the new
 * engine is asked a real question -- `api_doc()`, which reads the same
 * registry `evaluate` walks and costs a few milliseconds -- and only an engine
 * that answers it is allowed to replace anything.
 *
 * Until that returns, the old engine is untouched and still answering. A swap
 * that can leave the page with no engine at all is worse than a stale engine,
 * because a stale engine computes the wrong thing and no engine computes
 * nothing.
 */
export async function startReplacementEngine(token: string, timeoutMs = 30_000): Promise<StartedEngine> {
  const t0 = performance.now();
  // Before the worker is spawned, not after: the service worker serves code
  // network-first but falls back to its cache, and the failure this whole
  // feature exists for is a device loading a broken engine over and over.
  await dropServiceWorkerCaches();
  const engine = await WorkerEngine.start(engineWorkerUrl(token), timeoutMs);
  try {
    const apiDoc = await withTimeout(engine.apiDoc(), timeoutMs, "the new engine did not answer its first call");
    return { engine, apiDoc, elapsedMs: performance.now() - t0 };
  } catch (err) {
    engine.dispose();
    throw err;
  }
}

function withTimeout<T>(p: Promise<T>, ms: number, why: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = window.setTimeout(() => reject(new Error(`${why} within ${ms} ms`)), ms);
    p.then(
      (v) => {
        window.clearTimeout(timer);
        resolve(v);
      },
      (e: unknown) => {
        window.clearTimeout(timer);
        reject(e instanceof Error ? e : new Error(String(e)));
      },
    );
  });
}

/** What the app has to lend this module to perform a swap. */
export interface HotReloadContext {
  /** The engine answering local evaluations right now. */
  currentEngine(): EngineLike;
  /**
   * Abandon everything in flight on the engine's job slots.
   *
   * Called *before* the replacement is installed, and that order is the whole
   * answer to "what happens to a job that is running when the swap starts".
   * The job runner rejects an abandoned job and drops its result even if the
   * work finishes anyway, so a result computed by the old engine can
   * never be applied against the new one. Nothing downstream has to compare
   * engine identities, because no such result ever arrives.
   */
  abandonInFlight(): void;
  /** Install a replacement that has already been proved healthy. */
  installEngine(next: WorkerEngine, apiDoc: ApiDoc, reevaluate: boolean): void;
  /**
   * Point the record of "what this page is running" at what it is running.
   *
   * Not necessarily the build that was announced: a page that swapped the
   * stylesheet and then stopped to ask about `app.js` is running a mixture,
   * and saying so is what stops the next rebuild of anything from replacing
   * that stylesheet all over again.
   */
  adoptBuild(build: string): void;
  /** A transient line: something was replaced and the user lost nothing. */
  note(text: string): void;
  /** The sticky bar: this build needs a page load, and here is why. */
  offerReload(plan: BuildPlan, build: string): void;
  /** Tell the session something went wrong. */
  report(err: unknown, operation: string): void;
}

/**
 * Apply one `build_changed` to a running page.
 *
 * Returns the plan it acted on, so a caller (and a test) can see what was
 * decided rather than inferring it from what happened.
 */
export async function applyBuild(from: string, to: string, ctx: HotReloadContext): Promise<BuildPlan> {
  const plan = planFor(from, to);
  if (plan.changed.length === 0) return plan;

  const done: string[] = [];
  // Which files this page genuinely took on, so that what it records as "the
  // build I am running" stays true even when only part of the build was taken.
  const adopted: string[] = [];

  if (plan.styles) {
    const ok = await swapStylesheet(plan.stylesToken);
    if (ok) {
      done.push("styles");
      adopted.push(...STYLE_FILES);
    } else {
      ctx.report(new Error("the rebuilt stylesheet would not load"), "hot_reload_styles");
    }
  }

  let engineSwapped = false;
  if (plan.engine && !plan.reload) {
    engineSwapped = await swapEngine(plan, ctx);
    if (engineSwapped) {
      done.push(plan.reevaluate ? "the engine" : "the engine bindings");
      adopted.push(...ENGINE_FILES);
    }
  }

  if (plan.reload) {
    // The stylesheet was still worth swapping on the way past: if the user
    // leaves the bar up for an hour, at least the part that could be current
    // is current. The engine was not, because the reload will fetch it again
    // and it is 20 MB.
    ctx.adoptBuild(adoptFields(from, to, adopted));
    if (done.length > 0) ctx.note(`${done.join(" and ")} updated without reloading`);
    ctx.offerReload(plan, to);
    return plan;
  }

  if (plan.engine && !engineSwapped) {
    // Everything else succeeded, but the engine is the one that matters and
    // it did not. Do not claim the page is current.
    ctx.adoptBuild(adoptFields(from, to, adopted));
    ctx.offerReload({ ...plan, reload: true, reloadBecause: "the rebuilt engine would not start here" }, to);
    return plan;
  }

  // Nothing is left over: this page is running the new build in every respect
  // the fingerprint covers.
  ctx.adoptBuild(to);
  if (done.length > 0) ctx.note(`${done.join(" and ")} updated without reloading`);
  return plan;
}

/**
 * Replace the local engine with one on the new build.
 *
 * The order below is the whole design, and each step exists because the one
 * before it can fail:
 *
 *   1. start a second engine and prove it answers. The old one is still
 *      running and still answering while this happens, which is why a failure
 *      here costs nothing but a bar.
 *   2. *retire* the old engine -- stop it respawning -- before anything is
 *      cancelled. `WorkerEngine` replaces a terminated worker on purpose, so
 *      cancelling first would spawn a worker on the old build a moment before
 *      we throw it away.
 *   3. abandon what is in flight. Anything the old engine still produces is
 *      dropped by the job runner, not raced against the new engine.
 *   4. install the replacement, then terminate the old worker.
 *
 * Returns false when the swap could not be done at all, in which case nothing
 * was touched and the caller falls back to offering a reload.
 */
async function swapEngine(plan: BuildPlan, ctx: HotReloadContext): Promise<boolean> {
  const old = ctx.currentEngine();
  // Only a worker can be replaced underneath a running page. A module worker
  // is a separate module graph with its own instance of the wasm; the
  // main-thread engine is an `import()` this page already performed, and
  // re-importing the same URL returns the same module. `?engine=main` and the
  // mock are therefore reload-only, and say so.
  if (!(old instanceof WorkerEngine)) return false;

  let started: StartedEngine;
  try {
    started = await startReplacementEngine(plan.engineToken);
  } catch (err) {
    ctx.report(err, "hot_reload_engine");
    return false;
  }

  old.retire();
  ctx.abandonInFlight();
  ctx.installEngine(started.engine, started.apiDoc, plan.reevaluate);
  old.dispose();
  return true;
}
