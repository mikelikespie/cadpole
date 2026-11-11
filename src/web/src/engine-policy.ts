// Which engine runs an evaluation, and whether it runs twice.
//
// Two decisions live here and they are the same idea applied twice: **you
// cannot know what a model costs until you have paid for it once, so measure,
// remember the measurement against the document, and be willing to change your
// mind.**
//
//   local vs remote   Offloading to `cadpole serve` is worth about 2x on a heavy
//                     model and nothing at all on a light one, where the round
//                     trip is most of the time. So: evaluate locally first,
//                     promote the document to remote once a local evaluation
//                     has proved slow, and demote it again if remote turns out
//                     not to be faster end to end.
//
//   one pass or two   Showing a coarse mesh first and refining is a large win
//                     when the coarse pass really is much cheaper, and a
//                     straight loss when it is not. For a body the mesh kernel
//                     owns, tolerance feeds the booleans and the base lid runs
//                     1.3 s coarse against 6.6 s fine. For an exact B-rep body
//                     the solid is built once and tessellation is a rounding
//                     error, so a coarse pass costs a whole extra evaluation
//                     and saves nothing. The client is never told which kernel
//                     a body used: it measures whether the coarse pass
//                     actually arrived sooner, which gets the same answer and
//                     keeps working for a document that mixes the two -- as
//                     the base lid does.
//
// The alternative for both -- deciding from the model's size -- is a trap:
// size is not knowable until you have evaluated, and the
// first evaluation is the one that hurts.

/** What the user asked for, overriding the measurement. */
export type EnginePreference = "auto" | "local" | "remote";

export const ENGINE_PREFERENCE_KEY = "cadpole.enginePreference";
const VERDICT_KEY = "cadpole.engineVerdicts.v1";

/** A local evaluation at or above this promotes the document to remote. */
export const PROMOTE_MS = 750;
/** A local evaluation at or above this is worth showing a coarse pass first. */
export const LADDER_MS = 400;
/** The coarse rung, in mm. */
export const COARSE_TOLERANCE = 0.4;
/**
 * The coarse pass must arrive in at most this fraction of the full time, or
 * the ladder is not buying the user anything and is switched off for the
 * document. Half is deliberately generous: even a 2x head start on a
 * six-second evaluation is three seconds of looking at the part.
 */
const LADDER_GAIN = 0.5;

/** What has been measured about one document. All times in ms; 0 = unknown. */
export interface Verdict {
  /** The most recent full-detail local evaluation. */
  localMs: number;
  /** The most recent full-detail remote evaluation, end to end. */
  remoteMs: number;
  /** Cleared by a remote failure: this document stops trying the server. */
  remoteOk: boolean;
  /** The most recent coarse pass. */
  coarseMs: number;
  /** Cleared when a measured coarse pass did not arrive appreciably sooner. */
  ladderOk: boolean;
  /**
   * Set when a local evaluation stopped at an `opt` solve: the
   * solver is native, so the wasm engine builds the problem and stops at
   * `solve()`. From then on the document evaluates on the server.
   */
  needsSolver: boolean;
}

export function emptyVerdict(): Verdict {
  return { localMs: 0, remoteMs: 0, remoteOk: true, coarseMs: 0, ladderOk: true, needsSolver: false };
}

/**
 * What the wasm engine says when a script asks it to solve: the start of
 * `cadpole_core::opt::NOT_INSTALLED`, which every such diagnostic carries.
 */
export const NEEDS_NATIVE_SOLVER = "solving needs native Cadpole";

/** Whether an evaluation stopped because its engine has no solver. */
export function stoppedForSolver(diagnostics: readonly { message: string }[]): boolean {
  return diagnostics.some((d) => d.message.includes(NEEDS_NATIVE_SOLVER));
}

type Store = Record<string, Verdict>;

/**
 * The verdicts in `localStorage`, or `null` when there is no storage to read.
 *
 * A missing or unreadable entry is an empty record -- every document
 * unmeasured -- and is not the same answer as no storage at all: that one
 * leaves the page's own copy as the only record there is.
 */
function read(): Store | null {
  let raw: string | null;
  try {
    raw = localStorage.getItem(VERDICT_KEY);
  } catch {
    // Private mode, or storage disabled. Every document then starts unmeasured
    // and re-learns within one evaluation, which is the same behaviour as a
    // document nobody has opened before.
    return null;
  }
  if (!raw) return {};
  try {
    const o = JSON.parse(raw) as Record<string, unknown>;
    const out: Store = {};
    for (const [k, v] of Object.entries(o)) {
      const r = (v ?? {}) as Record<string, unknown>;
      out[k] = {
        localMs: Number(r["localMs"]) || 0,
        remoteMs: Number(r["remoteMs"]) || 0,
        remoteOk: r["remoteOk"] !== false,
        coarseMs: Number(r["coarseMs"]) || 0,
        ladderOk: r["ladderOk"] !== false,
        needsSolver: r["needsSolver"] === true,
      };
    }
    return out;
  } catch {
    return {};
  }
}

/** Whether the verdicts reached `localStorage`. */
function write(s: Store): boolean {
  try {
    localStorage.setItem(VERDICT_KEY, JSON.stringify(s));
    return true;
  } catch {
    return false;
  }
}

export function loadPreference(): EnginePreference {
  try {
    const v = localStorage.getItem(ENGINE_PREFERENCE_KEY);
    if (v === "auto" || v === "local" || v === "remote") return v;
  } catch {
    /* ignore */
  }
  return "auto";
}

export function savePreference(p: EnginePreference): void {
  try {
    localStorage.setItem(ENGINE_PREFERENCE_KEY, p);
  } catch {
    /* ignore */
  }
}

/**
 * The measurements for one document, kept across reloads.
 *
 * Keyed by document id so reopening a heavy document promotes it on its
 * *first* evaluation rather than paying the local price over again. A document
 * with no id (the playground's, before anything is loaded) gets an in-memory
 * verdict that is simply never persisted.
 *
 * **`localStorage` is the record, and every verdict is read from it**, not
 * from a copy taken when the page loaded. Otherwise a verdict cleared from
 * storage -- which is how the apply-path benchmark makes every edit
 * start from an unmeasured document -- or written by another tab on the same
 * document would go unseen until a reload. One small read per evaluation is
 * the price. The copy is kept for a browser with no working
 * storage, where it is the only record there is.
 */
export class EngineMemory {
  private store: Store;
  private readonly live = new Map<string, Verdict>();
  /** False once storage has refused a read or a write: the copy is the record. */
  private persisted: boolean;

  constructor() {
    const stored = read();
    this.persisted = stored !== null;
    this.store = stored ?? {};
  }

  verdict(docId: string): Verdict {
    if (!docId) {
      let v = this.live.get("");
      if (!v) {
        v = emptyVerdict();
        this.live.set("", v);
      }
      return v;
    }
    if (this.persisted) {
      const stored = read();
      if (stored === null) this.persisted = false;
      else this.store = stored;
    }
    return this.store[docId] ?? emptyVerdict();
  }

  private update(docId: string, patch: Partial<Verdict>): void {
    const next = { ...this.verdict(docId), ...patch };
    if (!docId) {
      this.live.set("", next);
      return;
    }
    this.store = { ...this.store, [docId]: next };
    if (this.persisted && !write(this.store)) this.persisted = false;
  }

  /** A full-detail evaluation finished. */
  recordFull(docId: string, kind: "local" | "remote", ms: number): void {
    this.update(docId, kind === "local" ? { localMs: ms } : { remoteMs: ms, remoteOk: true });
  }

  /**
   * A coarse pass finished, and then the full one did.
   *
   * If the coarse pass was not appreciably sooner it bought nothing and the
   * ladder goes away for this document -- that is the exact-B-rep case, where
   * the booleans dominate and re-tessellating is nearly free, so the coarse
   * pass costs a whole extra evaluation for a mesh the user sees for a few
   * hundred milliseconds.
   */
  recordLadder(docId: string, coarseMs: number, fullMs: number): void {
    const worth = fullMs > 0 && coarseMs <= fullMs * LADDER_GAIN;
    this.update(docId, { coarseMs, ladderOk: worth });
  }

  /** A remote evaluation failed: stop trying the server for this document. */
  demoteRemote(docId: string): void {
    this.update(docId, { remoteOk: false, remoteMs: 0 });
  }

  /** The user asked for remote explicitly, so give the server another chance. */
  clearRemoteDemotion(docId: string): void {
    this.update(docId, { remoteOk: true });
  }

  /** A local evaluation stopped at a solve: this document needs the server. */
  markNeedsSolver(docId: string): void {
    this.update(docId, { needsSolver: true });
  }
}

export interface Choice {
  engine: "local" | "remote";
  /** A sentence for the status bar, so "why is this slow" is answerable. */
  why: string;
}

/**
 * Which engine should run the next evaluation of `docId`.
 *
 * `remoteAvailable` is false in the static playground and anywhere there is no
 * session server, and that case is settled before anything is measured: there
 * is nothing to offload to.
 */
export function chooseEngine(pref: EnginePreference, v: Verdict, remoteAvailable: boolean, hasAssets = false): Choice {
  if (!remoteAvailable) {
    return { engine: "local", why: pref === "remote" ? "no server to offload to" : "no server" };
  }
  if (pref === "local") return { engine: "local", why: "you chose local" };
  if (pref === "remote") return { engine: "remote", why: "you chose remote" };
  // A document that imports files evaluates where the files are: the
  // browser holds no asset bytes, so a local evaluation could only fail on
  // its first `import_*`, and measuring that failure first would open the
  // document as an error. Before `remoteOk`, which a failure on some
  // other document's account must not override here.
  if (hasAssets) return { engine: "remote", why: "the document imports files the server holds" };
  // The same reasoning for a document that solves: only the server
  // has the solver, so local can only stop at the first `solve()`, and a
  // solve too long for one evaluation runs there as a job.
  if (v.needsSolver) return { engine: "remote", why: "the document solves an optimisation, which needs the server's solver" };
  if (!v.remoteOk) return { engine: "local", why: "the server failed on this document" };
  if (v.localMs === 0) return { engine: "local", why: "first evaluation, measuring" };
  if (v.localMs < PROMOTE_MS) return { engine: "local", why: `local is ${Math.round(v.localMs)} ms; not worth a round trip` };
  if (v.remoteMs > 0 && v.remoteMs >= v.localMs) {
    return { engine: "local", why: `remote was ${Math.round(v.remoteMs)} ms against ${Math.round(v.localMs)} ms local` };
  }
  return { engine: "remote", why: `local takes ${(v.localMs / 1000).toFixed(1)} s` };
}

/**
 * Whether the next evaluation should show a coarse mesh first.
 *
 * `requested` is the tolerance the document actually wants. A document asking
 * for something at or coarser than the coarse rung has nothing to refine
 * *from*, so there is no ladder.
 */
export function ladderFor(v: Verdict, requested: number): number[] {
  if (!v.ladderOk) return [requested];
  if (requested >= COARSE_TOLERANCE) return [requested];
  if (v.localMs === 0 && v.remoteMs === 0) return [requested];
  const known = v.remoteMs > 0 ? Math.min(v.localMs || Infinity, v.remoteMs) : v.localMs;
  if (known < LADDER_MS) return [requested];
  // Two rungs, never more. Each extra rung on the mesh-kernel path is a whole
  // extra evaluation, and one intermediate rung already collects most of the
  // latency: 1.3 s to something on screen instead of 6.6 s.
  return [COARSE_TOLERANCE, requested];
}
