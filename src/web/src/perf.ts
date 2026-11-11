// Where the main thread's time goes when a result is applied.
//
// The engine runs off the main thread, but applying a result does not. The
// phases of the apply path name themselves here and the benchmark reads them
// back.
//
// Two things are recorded, and they answer different questions:
//
//   * **spans** -- named phases, nested or not, in the order they ended.
//     These say *where* the time went: normalising the model, rebuilding a
//     body, uploading to the GPU, re-rendering the panels.
//   * **long tasks** -- the browser's own `PerformanceObserver("longtask")`
//     view, which is the ground truth for "the page stopped responding" and
//     owes nothing to our own accounting. It only reports tasks of 50 ms or
//     more, so a healthy apply path produces none at all; that is the point.
//
// Recording is **off unless asked for** (`?perf=1`, or `cadpole:perf` in
// `localStorage`). Off, `span()` is a boolean test and a call; on, it is two
// `performance.now()` calls, a small object and -- so a DevTools profile shows
// the same breakdown the benchmark prints -- a `performance.measure`.
//
// Counters are different: `count()` is always on. A count of how many bodies
// were reused rather than rebuilt is not a measurement of this machine on this
// morning, it is a statement about what the code did, so a test can assert it
// and it cannot go flaky.

export interface PerfSpan {
  readonly name: string;
  /** Duration in milliseconds. */
  readonly ms: number;
  /** `performance.now()` when the span ended. */
  readonly at: number;
}

export interface PerfLongTask {
  readonly ms: number;
  readonly at: number;
  /**
   * What the browser blames: `window` for this page's own work, or an iframe's
   * container type. A long task that no span accounts for is the browser's own
   * work or the machine's -- garbage collection, style and layout, or simply
   * being descheduled on a busy box -- and telling those apart needs the
   * timestamps, which `CADPOLE_PERF_TRACE=1` prints.
   */
  readonly blame: string;
}

/** What one apply cost, as the benchmark reads it. */
export interface PerfReport {
  spans: PerfSpan[];
  longTasks: PerfLongTask[];
  counts: Record<string, number>;
}

/// Enough for a slider drag's worth of evaluations; older spans fall off the
/// front rather than growing without bound in a session left open all day.
const MAX_SPANS = 512;

const spans: PerfSpan[] = [];
const longTasks: PerfLongTask[] = [];
const counts = new Map<string, number>();
let on = false;
let observer: PerformanceObserver | null = null;

function push(name: string, ms: number): void {
  spans.push({ name, ms, at: nowMs() });
  if (spans.length > MAX_SPANS) spans.shift();
}

/**
 * Record `name` taking `ms`.
 *
 * Separate from `span` for the cases where the clock was already read for
 * another reason -- the render loop times its own frame for the HUD, and
 * `setModel` accumulates a phase across every body rather than per body.
 */
export function record(name: string, ms: number): void {
  if (!on) return;
  push(name, ms);
}

/** Time `fn` as `name`. Returns whatever `fn` returned; never swallows a throw. */
export function span<T>(name: string, fn: () => T): T {
  if (!on) return fn();
  const t0 = nowMs();
  try {
    return fn();
  } finally {
    const ms = nowMs() - t0;
    push(name, ms);
    measure(name, t0, ms);
  }
}

/**
 * Count something that happened, whether or not timing is on.
 *
 * These are what the regression test asserts: "39 of 40 bodies were reused"
 * is a fact about the code, not about the machine it ran on, so it is the
 * half of this module that cannot go flaky on a loaded CI box.
 */
export function count(name: string, n = 1): void {
  counts.set(name, (counts.get(name) ?? 0) + n);
}

/** Read and clear everything recorded so far. */
export function takeReport(): PerfReport {
  const report: PerfReport = {
    spans: spans.slice(),
    longTasks: longTasks.slice(),
    counts: Object.fromEntries(counts),
  };
  spans.length = 0;
  longTasks.length = 0;
  counts.clear();
  return report;
}

/**
 * Turn recording on, and hang a reader off `window` for the benchmark.
 *
 * Idempotent, and safe where `PerformanceObserver` does not know `longtask`
 * (Safari): the spans still work and `longTasks` is simply empty.
 */
export function enablePerf(): void {
  if (on) return;
  on = true;
  try {
    observer = new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        const attribution = (e as { attribution?: { containerType?: string }[] }).attribution ?? [];
        const blame = attribution.map((a) => a.containerType ?? "?").join(",") || e.name;
        longTasks.push({ ms: e.duration, at: e.startTime, blame });
        if (longTasks.length > MAX_SPANS) longTasks.shift();
      }
    });
    observer.observe({ entryTypes: ["longtask"] });
  } catch {
    observer = null;
  }
  (globalThis as { __cadpolePerf?: unknown }).__cadpolePerf = { take: takeReport, enabled: (): boolean => on };
}

/**
 * Whether this page was asked to measure.
 *
 * The URL is the benchmark's way in; `localStorage` is the developer's, so a
 * DevTools profile of a real session can carry the same phase marks without a
 * query string surviving in a bookmark.
 */
export function perfRequested(): boolean {
  try {
    if (new URLSearchParams(location.search).get("perf") === "1") return true;
    return localStorage.getItem("cadpole:perf") === "1";
  } catch {
    return false;
  }
}

function nowMs(): number {
  return performance.now();
}

/**
 * Mirror a span into the User Timing timeline.
 *
 * So that a DevTools performance recording of a real session shows the same
 * breakdown this module hands the benchmark, rather than an unlabelled block
 * of script. `clearMeasures` keeps the browser's own buffer from growing for
 * as long as the tab is open.
 */
function measure(name: string, startMs: number, ms: number): void {
  try {
    performance.measure(`cadpole:${name}`, { start: startMs, duration: ms });
    if (performance.getEntriesByType("measure").length > MAX_SPANS) performance.clearMeasures();
  } catch {
    /* User Timing L3 options are not everywhere; the span is still recorded */
  }
}
