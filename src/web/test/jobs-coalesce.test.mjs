// The job runner's `coalesce` slots after a cancel.
//
// `coalesce` joins a request to the job already running for the same key. A
// cancelled job is not running: a request made after `cancel` is new work.
// The slot used to keep handing out the cancelled job's (rejected) promise --
// until the uninterruptible body settled, or for ever when the cancelled job
// had been waiting and so never reached `finish`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { JobRunner, isAbandoned } from "../src/jobs.js";

/** A body that ignores its signal and finishes when the test says so. */
function stubborn() {
  let release;
  const done = new Promise((r) => (release = r));
  return { body: async () => done, release };
}

test("a coalesce request after cancel runs, even while the cancelled body is still finishing", async () => {
  const runner = new JobRunner();
  const first = stubborn();
  const a = runner.submit("doc", "coalesce", "k", first.body);
  await new Promise((r) => setTimeout(r, 0));
  runner.cancel("doc");

  let ran = false;
  const b = runner.submit("doc", "coalesce", "k", async () => {
    ran = true;
    return "fresh";
  });
  first.release("stale");
  await assert.rejects(a, isAbandoned);
  assert.equal(await b, "fresh");
  assert.ok(ran, "the new request's body never ran");
});

test("a cancelled waiting coalesce job does not poison its key", async () => {
  const runner = new JobRunner();
  const a = stubborn();
  const pa = runner.submit("doc", "coalesce", "a", a.body);
  const pb = runner.submit("doc", "coalesce", "b", async () => "b-old");
  runner.cancel("doc");
  a.release("a");
  await assert.rejects(pa, isAbandoned);
  await assert.rejects(pb, isAbandoned);
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual(
    { running: runner.status("doc").running, queued: runner.status("doc").queued },
    { running: false, queued: false },
  );

  for (let i = 0; i < 2; i++) {
    let ran = false;
    const v = await runner.submit("doc", "coalesce", "b", async () => {
      ran = true;
      return `b-${i}`;
    });
    assert.equal(v, `b-${i}`);
    assert.ok(ran);
  }
});

test("a second request for the running key still joins it", async () => {
  const runner = new JobRunner();
  let runs = 0;
  const body = async () => {
    runs += 1;
    await new Promise((r) => setTimeout(r, 10));
    return "v";
  };
  const a = runner.submit("doc", "coalesce", "k", body);
  const b = runner.submit("doc", "coalesce", "k", body);
  assert.equal(a, b);
  assert.equal(await a, "v");
  assert.equal(runs, 1);
});
