// A document that solves evaluates on the server.
//
// The solver is native: the wasm engine builds an `opt` problem and
// stops at `solve()` with "solving needs native Cadpole". A local evaluation
// that stops at once is never slow enough to be promoted by timing alone, so
// the policy moves such a document to the server explicitly, where it can be
// solved -- and a long solve run as a job. These drive the evaluator with
// stand-in engines.

import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { NEEDS_NATIVE_SOLVER } from "../src/engine-policy.js";
import { Evaluator } from "../src/evaluator.js";
import { JobRunner } from "../src/jobs.js";
import { RemoteUnavailable } from "../src/remote-engine.js";

class FakeStorage {
  constructor() {
    this.items = new Map();
  }
  getItem(k) {
    return this.items.has(k) ? this.items.get(k) : null;
  }
  setItem(k, v) {
    this.items.set(k, String(v));
  }
  removeItem(k) {
    this.items.delete(k);
  }
}

afterEach(() => {
  delete globalThis.localStorage;
});

function model(diagnostics) {
  return { diagnostics, features: [], bodies: [], params: [], eval_millis: 1 };
}

/** An engine that answers with `diagnostics` and counts its calls. */
function engine(kind, answer) {
  return {
    kind,
    calls: 0,
    async evaluate(req) {
      this.calls += 1;
      if (answer instanceof Error) throw answer;
      return { model: model(answer), meshes: [], tolerance: req.tolerance, kind, elapsedMs: 3 };
    },
  };
}

function evaluator(local, remote) {
  globalThis.localStorage = new FakeStorage();
  const stages = [];
  const notices = [];
  const ev = new Evaluator(
    new JobRunner(),
    { local, remote },
    { onStage: (s) => stages.push(s), onError: () => {}, onBusy: () => {}, onNotice: (t) => notices.push(t) },
    "auto",
  );
  return { ev, stages, notices };
}

const STOPPED = [{ severity: "error", message: `sizes: solve: ${NEEDS_NATIVE_SOLVER}: the CLI...`, span: null, feature_id: "sizes" }];
const PENDING = [{ severity: "info", message: "sizes: pending: job 3f9a2c1b7e44 is solving it in cadpole serve", span: null, feature_id: "sizes" }];
const input = { docId: "plan", scripts: [{ path: "main.star", source: "..." }], overrides: {} };

test("a local evaluation that stops at a solve is run again on the server, and the next one goes there first", async () => {
  const local = engine("worker", STOPPED);
  const remote = engine("remote", PENDING);
  const { ev } = evaluator(local, remote);
  const first = await ev.request(input);
  assert.equal(first.evaluated.kind, "remote", "the server's answer is the one shown");
  assert.equal(first.evaluated.model.diagnostics[0].message, PENDING[0].message);
  assert.match(first.why, /solver/);
  assert.deepEqual([local.calls, remote.calls], [1, 1]);
  const second = await ev.request(input);
  assert.equal(second.evaluated.kind, "remote");
  assert.deepEqual([local.calls, remote.calls], [1, 2], "measured once; from then on the server");
});

test("a document that does not solve is left to the local engine", async () => {
  const local = engine("worker", [{ severity: "info", message: "print: hello", span: null, feature_id: "" }]);
  const remote = engine("remote", []);
  const { ev } = evaluator(local, remote);
  const r = await ev.request(input);
  assert.equal(r.evaluated.kind, "worker");
  assert.equal(remote.calls, 0);
});

test("with no server the wasm engine still stops at solve(), as on a published page", async () => {
  const local = engine("worker", STOPPED);
  const { ev } = evaluator(local, null);
  const r = await ev.request(input);
  assert.equal(r.evaluated.kind, "worker");
  assert.equal(r.evaluated.model.diagnostics[0].message, STOPPED[0].message);
});

test("a server that will not answer leaves the local result, and says why", async () => {
  const local = engine("worker", STOPPED);
  const remote = engine("remote", new RemoteUnavailable("the server could not be reached (ECONNREFUSED)", 0));
  const { ev, notices } = evaluator(local, remote);
  const r = await ev.request(input);
  assert.equal(r.evaluated.kind, "worker");
  assert.match(r.fellBack ?? "", /could not be reached/);
  assert.ok(notices.some((n) => n && /needs the server/.test(n)), JSON.stringify(notices));
});
