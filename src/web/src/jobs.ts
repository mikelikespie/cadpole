// The one place heavy work is scheduled in the browser.
//
// Every operation that can take real time -- evaluating a document, asking the
// engine for its API reference, writing a script back to the session -- is a
// *job* in a named *slot*. A slot runs at most one job at a time and starts
// its jobs in submission order, so completions cannot overtake each other and
// a late result can never be mistaken for a fresh one.
//
// What a *second* request to a busy slot means differs by operation, and that
// is the only thing a caller chooses:
//
//   supersede  The new request makes the old one worthless: same question,
//              different inputs. The running job is cancelled, at most one
//              request waits behind it, and a third displaces the waiting one.
//              This is `evaluate`: a slider drag produces one request per
//              frame and only the last is about the model on screen.
//
//   coalesce   The new request is the *same* question, so it joins the running
//              job rather than starting a second. This is `apiDoc`: the answer
//              depends only on which engine build is loaded.
//
//   queue      Every request must happen, in order, none dropped. This is a
//              write: `save`, `setScript`. A dropped write is lost work, not a
//              stale pixel.
//
// Cancellation is always *requested* -- every job body receives an
// `AbortSignal` -- and whether it reclaims CPU depends on who is running it.
// `src/rust/cadpole-ffi` is explicit that its cancellation "releases the caller,
// not the CPU": nothing can interrupt Starlark or the kernel mid-operation, so
// an abandoned native evaluation finishes in the background and its result is
// dropped. A dedicated Worker is the exception and can genuinely be
// terminated. Either way the runner guarantees the part that matters: **the
// result of a cancelled job is never delivered**.
//
// **A job produces a sequence, not a value.** `run` is handed an `emit` and
// may call it any number of times before returning its last result; every
// emission is delivered to `onResult` in order, and the returned value is
// delivered as the final one. One result is the common case (`apiDoc`,
// `save`); evaluation uses the sequence to show a coarse mesh while the fine
// one is still being computed, which is the reason `run` has this shape.
//
// The invariants are checked by the Quint model in `spec/jobs.qnt`.

/** Why a job's promise rejected without the work having failed. */
export class JobAbandoned extends Error {
  constructor(
    readonly slot: string,
    readonly reason: "superseded" | "cancelled",
  ) {
    super(reason === "superseded" ? `${slot} superseded by a newer request` : `${slot} cancelled`);
    this.name = "JobAbandoned";
  }
}

/** True for the rejection a caller should ignore rather than report. */
export function isAbandoned(err: unknown): err is JobAbandoned {
  return err instanceof JobAbandoned || (err instanceof Error && err.name === "AbortError");
}

export type JobPolicy = "supersede" | "coalesce" | "queue";

export interface JobStatus {
  readonly slot: string;
  /** A job is running right now. */
  readonly running: boolean;
  /** A request is waiting for the running one to finish. */
  readonly queued: boolean;
  /** `performance.now()` when the running job started, else 0. */
  readonly startedAt: number;
  /** The running job's key, for a status line. */
  readonly key: string;
  /** Generation of the most recent submission to this slot. */
  readonly generation: number;
}

/** Deliver an intermediate result. Later emissions supersede earlier ones. */
export type JobEmit<T> = (value: T) => void;

/** What a job body is: emits zero or more partials, returns the final one. */
export type JobBody<T> = (emit: JobEmit<T>, signal: AbortSignal) => Promise<T>;

export interface SubmitOptions<T> {
  /**
   * How long a running job must have been running before a superseding
   * request bothers to cancel it. Below this, waiting for it costs less than
   * whatever restarting the engine costs -- terminating and respawning a
   * worker is not free, and a fast model finishes inside the window anyway.
   */
  abortAfterMs?: number;
  /**
   * Every result of this job, in order, ending with the final one. Never
   * called after the job is abandoned, and never called out of order: that is
   * what lets a caller apply results without re-checking whether they are
   * still current.
   */
  onResult?: (value: T, final: boolean) => void;
}

interface Pending<T> {
  gen: number;
  key: string;
  run: JobBody<T>;
  resolve: (v: T) => void;
  reject: (e: unknown) => void;
  opts: SubmitOptions<T>;
}

interface Running {
  gen: number;
  key: string;
  startedAt: number;
  abort: AbortController;
  /** Everyone waiting on this exact job (`coalesce` adds to it). */
  settled: boolean;
}

interface Slot {
  name: string;
  policy: JobPolicy;
  generation: number;
  running: Running | null;
  /**
   * `supersede`: at most one. `queue` and `coalesce`: a FIFO (a `coalesce`
   * request for a different key waits behind the running one).
   */
  waiting: Pending<unknown>[];
  /**
   * `coalesce` only: the newest live request's promise, keyed, and which
   * request (`gen`) it is. Cleared when that request settles or is
   * cancelled, never by another request that happens to share its key: a
   * cancelled job is not one a new request may join.
   */
  shared: { key: string; gen: number; promise: Promise<unknown> } | null;
  /**
   * `supersede` only: armed when a request arrives while the running job is
   * still inside its `abortAfterMs` grace period, to abort that job the
   * moment it leaves the period with the request still waiting.
   */
  abortTimer: ReturnType<typeof setTimeout> | null;
}

const DEFAULT_ABORT_AFTER_MS = 250;

/**
 * Runs jobs in named slots under one of three policies.
 *
 * One instance per page. `onChange` fires whenever a slot's busy state changes,
 * which is what the UI's pending indicator is wired to: the indicator is on
 * exactly when the slot has a job queued or running, and never because somebody remembered to set a flag.
 */
export class JobRunner {
  private readonly slots = new Map<string, Slot>();
  private readonly listeners = new Set<(s: JobStatus) => void>();

  constructor(private readonly now: () => number = () => performance.now()) {}

  subscribe(fn: (s: JobStatus) => void): () => void {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  }

  status(slot: string): JobStatus {
    const s = this.slots.get(slot);
    if (!s) return { slot, running: false, queued: false, startedAt: 0, key: "", generation: 0 };
    return {
      slot,
      running: s.running !== null,
      queued: s.waiting.length > 0,
      startedAt: s.running?.startedAt ?? 0,
      key: s.running?.key ?? "",
      generation: s.generation,
    };
  }

  /** The generation a `submit` to this slot would receive next. */
  generation(slot: string): number {
    return this.slots.get(slot)?.generation ?? 0;
  }

  /**
   * Submit work to `slot`.
   *
   * The returned promise resolves with the job's value, or rejects with
   * [`JobAbandoned`] when the policy dropped it (superseded, or cancelled).
   * Callers distinguish the two with [`isAbandoned`]: an abandoned job is not
   * a failure and must not be reported as one.
   */
  submit<T>(slot: string, policy: JobPolicy, key: string, run: JobBody<T>, opts: SubmitOptions<T> = {}): Promise<T> {
    let s = this.slots.get(slot);
    if (!s) {
      s = { name: slot, policy, generation: 0, running: null, waiting: [], shared: null, abortTimer: null };
      this.slots.set(slot, s);
    }
    // A slot's policy is a property of the operation, not of one call.
    if (s.policy !== policy) throw new Error(`slot ${slot} is ${s.policy}, not ${policy}`);

    if (policy === "coalesce" && s.shared && s.shared.key === key) {
      return s.shared.promise as Promise<T>;
    }

    s.generation += 1;
    const gen = s.generation;
    const before = this.busy(s);
    const promise = new Promise<T>((resolve, reject) => {
      const pending: Pending<T> = { gen, key, run, resolve, reject, opts };
      if (policy === "supersede") {
        // At most one request waits. A third displaces the second: only the
        // newest input is worth evaluating, and the one it displaced was never
        // started, so nothing is lost but a promise nobody awaits.
        for (const w of s.waiting) w.reject(new JobAbandoned(slot, "superseded"));
        s.waiting = [pending as Pending<unknown>];
        const r = s.running;
        if (r && !r.settled) {
          const after = opts.abortAfterMs ?? DEFAULT_ABORT_AFTER_MS;
          const age = this.now() - r.startedAt;
          if (age >= after) r.abort.abort();
          else this.abortWhenOldEnough(s, r, after - age);
        }
      } else {
        s.waiting.push(pending as Pending<unknown>);
      }
    });

    if (policy === "coalesce") s.shared = { key, gen, promise: promise as Promise<unknown> };
    this.pump(s);
    if (this.busy(s) !== before) this.emit(s);
    return promise;
  }

  /**
   * Abandon whatever `slot` is doing and anything waiting behind it.
   *
   * Idempotent: cancelling an idle slot, or one whose job has already settled,
   * does nothing at all. It never touches
   * anything a completed job already produced -- the caller keeps showing what
   * it was showing.
   */
  cancel(slot: string): void {
    const s = this.slots.get(slot);
    if (!s) return;
    const before = this.busy(s);
    for (const w of s.waiting) w.reject(new JobAbandoned(slot, "cancelled"));
    s.waiting = [];
    // Every request `shared` could name was just abandoned; a request made
    // from here on is new work, not a join onto a rejected promise.
    s.shared = null;
    this.disarm(s);
    if (s.running && !s.running.settled) s.running.abort.abort();
    if (this.busy(s) !== before) this.emit(s);
  }

  /**
   * Abort `r` once it is `inMs` older, if a request is still waiting behind it.
   *
   * `submit` judges "old enough to abort" at the instant the superseding
   * request arrives, and a job 100 ms into a thirteen-second evaluation is
   * not: restarting the engine costs more than the 150 ms left in the grace
   * period. Nothing about that judgement stays true, though. 150 ms later the
   * job *is* old enough and the request is still waiting, and without this
   * timer it would wait the whole thirteen seconds, watch the stale result go
   * on screen, and only then get its own. The runner's invariant -- a late
   * result is never mistaken for a fresh one -- held throughout; what the
   * user saw was a picture that did not match the slider for the length of
   * an evaluation.
   *
   * One timer per slot: a third request re-arms it for the same running job,
   * and `finish` and `cancel` disarm it, so it can never fire on a successor.
   */
  private abortWhenOldEnough(s: Slot, r: Running, inMs: number): void {
    this.disarm(s);
    s.abortTimer = setTimeout(() => {
      s.abortTimer = null;
      if (s.running === r && !r.settled && s.waiting.length > 0) r.abort.abort();
    }, inMs);
  }

  private disarm(s: Slot): void {
    if (s.abortTimer === null) return;
    clearTimeout(s.abortTimer);
    s.abortTimer = null;
  }

  /** Cancel every slot. Used when the page is going away. */
  cancelAll(): void {
    for (const name of this.slots.keys()) this.cancel(name);
  }

  private busy(s: Slot): boolean {
    return s.running !== null || s.waiting.length > 0;
  }

  private emit(s: Slot): void {
    const st = this.status(s.name);
    for (const l of this.listeners) l(st);
  }

  /** Start the next waiting job if nothing is running. */
  private pump(s: Slot): void {
    if (s.running !== null) return;
    const next = s.waiting.shift();
    if (!next) return;
    const abort = new AbortController();
    const running: Running = { gen: next.gen, key: next.key, startedAt: this.now(), abort, settled: false };
    s.running = running;

    const finish = (): void => {
      running.settled = true;
      if (s.running === running) {
        s.running = null;
        this.disarm(s);
      }
      if (s.shared?.gen === next.gen) s.shared = null;
      this.pump(s);
      this.emit(s);
    };

    // Partial results. Dropped the moment the job is abandoned, and dropped
    // after the final one, so `onResult` sees a prefix-closed, in-order
    // sequence ending in exactly one `final: true`.
    let done = false;
    const emit: JobEmit<unknown> = (value) => {
      if (done || abort.signal.aborted) return;
      next.opts.onResult?.(value as never, false);
    };

    // `run` may throw synchronously; Promise.resolve().then keeps both paths
    // identical and keeps `finish` off the caller's stack.
    void Promise.resolve()
      .then(() => (next.run as JobBody<unknown>)(emit, abort.signal))
      .then(
        (value) => {
          done = true;
          // A job that was aborted may still resolve -- an engine that cannot
          // be interrupted finishes anyway. Its answer is about inputs that
          // are no longer current, so it is dropped here rather than raced
          // upstream. This is what makes `monotonic_apply` hold without every
          // caller re-checking.
          if (abort.signal.aborted) {
            next.reject(new JobAbandoned(s.name, "cancelled"));
          } else {
            next.opts.onResult?.(value, true);
            next.resolve(value);
          }
          finish();
        },
        (err: unknown) => {
          done = true;
          if (abort.signal.aborted) next.reject(new JobAbandoned(s.name, "cancelled"));
          else next.reject(err);
          finish();
        },
      );
    this.emit(s);
  }
}
