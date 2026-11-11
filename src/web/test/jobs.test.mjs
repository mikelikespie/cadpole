// The job runner's supersede policy, on the clock.
//
// `submit` decides whether a running job is old enough to abort at the instant
// the superseding request arrives. That decision has to be revisited: a
// request that lands 100 ms into a thirteen-second evaluation is not worth
// interrupting the engine for *yet*, and is very much worth it 150 ms later.
// Without a second look the request waits out the whole evaluation, the
// stale result goes on screen first, and the picture does not match the
// slider for an evaluation's length. The runner's own invariant (a late
// result is never *mistaken* for a fresh one) holds throughout; this is the
// stronger claim, that the user is not shown the superseded answer at all.

import assert from "node:assert/strict";
import { test } from "node:test";
import { JobRunner, isAbandoned } from "../src/jobs.js";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** A job that takes `ms` unless its signal fires first, noting when it did. */
function slowJob(value, ms, log) {
  return (_emit, signal) =>
    new Promise((resolve, reject) => {
      const t0 = performance.now();
      const timer = setTimeout(() => resolve(value), ms);
      signal.addEventListener("abort", () => {
        clearTimeout(timer);
        log.abortedAfterMs = performance.now() - t0;
        reject(new DOMException("aborted", "AbortError"));
      });
    });
}

test("a request waiting behind a young job aborts it once the job is old enough", async () => {
  const runner = new JobRunner();
  const applied = [];
  const log = {};
  const a = runner.submit("eval", "supersede", "A", slowJob("A", 2_000, log), {
    abortAfterMs: 60,
    onResult: (v) => applied.push(v),
  });
  // B arrives well inside A's grace period, so nothing is aborted *now*...
  await sleep(10);
  const b = runner.submit("eval", "supersede", "B", async () => "B", {
    abortAfterMs: 60,
    onResult: (v) => applied.push(v),
  });
  // ...but A must not be allowed to run its two seconds with B waiting.
  await assert.rejects(a, isAbandoned);
  assert.equal(await b, "B");
  assert.deepEqual(applied, ["B"], "the superseded job's result reached the screen");
  assert.ok(
    log.abortedAfterMs >= 40 && log.abortedAfterMs < 1_000,
    `A was aborted ${log.abortedAfterMs} ms in; expected about 60 ms`,
  );
});

test("a young job that finishes in time is not aborted, and neither is its successor", async () => {
  const runner = new JobRunner();
  const applied = [];
  const logA = {};
  const logB = {};
  const a = runner.submit("eval", "supersede", "A", slowJob("A", 30, logA), {
    abortAfterMs: 60,
    onResult: (v) => applied.push(v),
  });
  await sleep(10);
  const b = runner.submit("eval", "supersede", "B", slowJob("B", 120, logB), {
    abortAfterMs: 60,
    onResult: (v) => applied.push(v),
  });
  assert.equal(await a, "A");
  assert.equal(await b, "B");
  assert.deepEqual(applied, ["A", "B"]);
  assert.equal(logA.abortedAfterMs, undefined, "A finished before the deadline and was aborted anyway");
  // The deadline armed for A must not fire on B, which started after A
  // finished and has nothing waiting behind it.
  assert.equal(logB.abortedAfterMs, undefined, "B inherited A's abort");
});

test("a displaced request is rejected as superseded and only the newest runs", async () => {
  const runner = new JobRunner();
  const applied = [];
  const log = {};
  const a = runner.submit("eval", "supersede", "A", slowJob("A", 2_000, log), {
    abortAfterMs: 30,
    onResult: (v) => applied.push(v),
  });
  const b = runner.submit("eval", "supersede", "B", async () => "B", { onResult: (v) => applied.push(v) });
  const c = runner.submit("eval", "supersede", "C", async () => "C", { onResult: (v) => applied.push(v) });
  await assert.rejects(b, isAbandoned);
  await assert.rejects(a, isAbandoned);
  assert.equal(await c, "C");
  assert.deepEqual(applied, ["C"]);
});
