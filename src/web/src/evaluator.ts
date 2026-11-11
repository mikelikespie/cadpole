// Evaluation, as one job with a state.
//
// Everything the UI knows about evaluating is here: which engine runs it,
// whether it runs coarse-then-fine, what happens to the one already in flight
// when the user moves a slider, and what the screen says while it waits.
// `main.ts` asks for an evaluation and is told about results; it does not know
// there is a network, a worker or a ladder.
//
// The three behaviours worth naming:
//
// * **A burst does not queue.** The slot is `supersede`: one evaluation runs,
//   at most one waits, and a third displaces the second. Dragging a slider for
//   ten seconds leaves one outstanding evaluation, not six hundred.
// * **A superseded evaluation is cancelled, not merely ignored.** With the
//   worker engine that genuinely stops it. `jobs.ts` waits `abortAfterMs`
//   before bothering, because restarting a worker costs more than letting a
//   five-millisecond model finish.
// * **A result that arrives late is dropped.** The job runner delivers results
//   in start order and never after an abort, so nothing downstream has to
//   re-check whether what it is holding is still current.

import type { EngineLike, Evaluated, EvaluateRequest } from "./engine.js";
import { DEFAULT_TOLERANCE } from "./engine.js";
import { chooseEngine, ladderFor, EngineMemory, stoppedForSolver, type EnginePreference } from "./engine-policy.js";
import { isAbandoned, JobRunner, type JobStatus } from "./jobs.js";
import { RemoteUnavailable, type RemoteEngine } from "./remote-engine.js";
import type { ParamValue, ScriptFile } from "./types.js";

/** The slot name. One per operation; this is the only heavy one today. */
export const EVALUATE_SLOT = "evaluate";

export interface EvaluatorEngines {
  /**
   * The worker engine, or the main-thread one, or the mock. Always present.
   *
   * Mutable: a rebuilt wasm engine is swapped in underneath a
   * running page, and `setLocalEngine` is the only thing that may write it.
   */
  local: EngineLike;
  /** Present only in session mode. */
  remote: RemoteEngine | null;
}

export interface EvaluateInput {
  /** The document being evaluated; identity is what measurements are keyed by. */
  docId: string;
  scripts: ScriptFile[];
  overrides: Record<string, ParamValue>;
  /** The tolerance the document wants; the ladder's last rung. */
  tolerance?: number;
  /** A drag preview: the slider is still moving. */
  preview?: boolean;
  /**
   * The document imports files. Their bytes live on the server and
   * never in the browser, so neither local engine can evaluate it.
   */
  hasAssets?: boolean;
}

/** One result, and how it came about. */
export interface EvaluateStage {
  evaluated: Evaluated;
  /**
   * The engine object that produced this, by identity.
   *
   * Carried rather than looked up because the local engine can be *replaced*
   * while an evaluation is in flight. The job runner already guarantees that
   * an abandoned job's result is never delivered, which is what makes a
   * post-swap result from the old engine impossible; this is how the page
   * checks that rather than assuming it, and reports the day it is wrong.
   */
  producedBy: EngineLike;
  /** False for a coarse pass that a finer one is still coming for. */
  final: boolean;
  /** Which rung this is, from 0. */
  stage: number;
  /** How many rungs this evaluation will produce. */
  stages: number;
  /** Why this engine ran, for the status bar. */
  why: string;
  /** Set when this ran locally only because the server would not answer. */
  fellBack: string | null;
}

export interface EvaluatorCallbacks {
  onStage: (s: EvaluateStage) => void;
  onError: (err: unknown, operation: string) => void;
  onBusy: (status: JobStatus) => void;
  /** A note for the user that is not a failure of the evaluation itself. */
  onNotice: (text: string | null) => void;
}

export class Evaluator {
  readonly memory = new EngineMemory();
  private preference: EnginePreference;

  constructor(
    private readonly runner: JobRunner,
    private readonly engines: EvaluatorEngines,
    private readonly callbacks: EvaluatorCallbacks,
    preference: EnginePreference,
  ) {
    this.preference = preference;
    this.runner.subscribe((s) => {
      if (s.slot === EVALUATE_SLOT) this.callbacks.onBusy(s);
    });
  }

  get remoteAvailable(): boolean {
    return this.engines.remote !== null;
  }

  /** The local engine, for a caller that needs its identity. */
  get localEngine(): EngineLike {
    return this.engines.local;
  }

  /**
   * Install a rebuilt local engine.
   *
   * The caller must have abandoned the slot first. That is not a convention
   * this could check: `run` closes over the engine it chose when it started,
   * so a job already inside `engine.evaluate` keeps the old one whatever this
   * field says, and the guarantee that its answer is never displayed comes
   * from the job being cancelled -- not from anything here.
   */
  setLocalEngine(next: EngineLike): void {
    this.engines.local = next;
  }

  setPreference(p: EnginePreference, docId: string): void {
    this.preference = p;
    // Asking for the server by name is also asking for it to be retried: a
    // document demoted by one bad request should not be stuck local for ever.
    if (p !== "local") this.memory.clearRemoteDemotion(docId);
  }

  get pref(): EnginePreference {
    return this.preference;
  }

  /** Abandon whatever is running. The displayed model is left alone. */
  cancel(): void {
    this.runner.cancel(EVALUATE_SLOT);
  }

  /**
   * Ask for an evaluation of `input`.
   *
   * Resolves when this request's own evaluation finished, and rejects with a
   * `JobAbandoned` when a newer request displaced it -- which callers ignore,
   * because being superseded is the system working.
   */
  request(input: EvaluateInput): Promise<EvaluateStage> {
    const requested = input.tolerance ?? DEFAULT_TOLERANCE;
    const key = `${input.docId}:${requested}`;
    return this.runner.submit<EvaluateStage>(
      EVALUATE_SLOT,
      "supersede",
      key,
      (emit, signal) => this.run(input, requested, emit, signal),
      {
        // Below a quarter-second, stopping the engine costs more than letting
        // it finish: terminating the worker means instantiating the wasm again.
        abortAfterMs: 250,
        // Every result of this job, coarse and final, in order. The job runner
        // is what guarantees it is in order and that nothing arrives after the
        // job is abandoned, so this can apply results without re-checking.
        onResult: (stage) => this.callbacks.onStage(stage),
      },
    );
  }

  private async run(
    input: EvaluateInput,
    requested: number,
    emit: (s: EvaluateStage) => void,
    signal: AbortSignal,
  ): Promise<EvaluateStage> {
    const verdict = this.memory.verdict(input.docId);
    const choice = chooseEngine(this.preference, verdict, this.remoteAvailable, input.hasAssets === true);
    const rungs = ladderFor(verdict, requested);
    // Everything `onStage` needs travels *with* the result rather than in a
    // field on this object: one evaluator serves every request, and a value
    // that describes the job it came from cannot be read against the wrong one.
    let why = choice.why;
    let fellBack: string | null = null;
    let coarseMs = 0;

    let engine: EngineLike = choice.engine === "remote" && this.engines.remote ? this.engines.remote : this.engines.local;
    let ran: "local" | "remote" = engine === this.engines.remote ? "remote" : "local";

    let last: EvaluateStage | null = null;
    for (let i = 0; i < rungs.length; i++) {
      const tolerance = rungs[i] as number;
      const isFinal = i === rungs.length - 1;
      const req: EvaluateRequest = {
        scripts: input.scripts,
        overrides: input.overrides,
        tolerance,
        // A runaway script must not hang the tab or the server. The engine
        // checks this, so the far end stops too rather than finishing an
        // evaluation nobody will read.
        deadlineMs: 120_000,
        preview: input.preview === true,
      };
      let result: Evaluated;
      try {
        result = await engine.evaluate(req, signal);
      } catch (err) {
        if (signal.aborted || isAbandoned(err)) throw err;
        if (ran === "remote" && err instanceof RemoteUnavailable) {
          // Fall back exactly once, at the same generation, on the same input.
          // Local is the floor: a fallback that could fall back again is a
          // loop, so `ran` is flipped before the retry and never flipped back.
          this.memory.demoteRemote(input.docId);
          fellBack = err.message;
          why = "the server would not answer";
          this.callbacks.onNotice(`Remote evaluation failed: ${err.message} Evaluating locally.`);
          this.callbacks.onError(err, "remote_evaluate");
          engine = this.engines.local;
          ran = "local";
          result = await engine.evaluate(req, signal);
        } else {
          throw err;
        }
      }
      // The wasm engine has no solver: it builds an `opt` problem and stops
      // at `solve()`. Where there is a server the document is
      // evaluated there instead, now and from now on -- once, at the
      // same generation; a server that will not answer leaves the local
      // result, which says what happened.
      if (ran === "local" && this.engines.remote && stoppedForSolver(result.model.diagnostics)) {
        this.memory.markNeedsSolver(input.docId);
        try {
          const remote = await this.engines.remote.evaluate(req, signal);
          engine = this.engines.remote;
          ran = "remote";
          why = "the document solves an optimisation, which needs the server's solver";
          result = remote;
        } catch (err) {
          if (signal.aborted || isAbandoned(err)) throw err;
          if (!(err instanceof RemoteUnavailable)) throw err;
          this.memory.demoteRemote(input.docId);
          fellBack = err.message;
          this.callbacks.onNotice(`The document solves an optimisation, which needs the server, and the server would not answer: ${err.message}`);
          this.callbacks.onError(err, "remote_evaluate");
        }
      }
      if (signal.aborted) {
        // Between rungs is the cheapest place to notice: a finer mesh of a
        // superseded model is wasted work, and the runner would drop it anyway.
        const e = new Error("aborted");
        e.name = "AbortError";
        throw e;
      }
      const stage: EvaluateStage = { evaluated: result, producedBy: engine, final: isFinal, stage: i, stages: rungs.length, why, fellBack };
      if (!isFinal) {
        coarseMs = result.elapsedMs;
        emit(stage);
      }
      last = stage;
    }

    const final = last as EvaluateStage;
    this.memory.recordFull(input.docId, ran, final.evaluated.elapsedMs);
    if (rungs.length > 1) this.memory.recordLadder(input.docId, coarseMs, final.evaluated.elapsedMs);
    return final;
  }
}
