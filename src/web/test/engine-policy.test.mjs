// What the engine policy remembers about a document, and where.
//
// The verdicts live in `localStorage`, and a running page reads them from
// there on every evaluation rather than from a copy it took on load. The
// apply-path benchmark depends on that: it clears the verdicts before
// every edit so that no edit is refined coarse-first, and while the page kept
// its own copy the clearing did nothing -- one evaluation that happened to take
// 400 ms on a loaded machine laddered every edit after it, and the counts the
// benchmark asserts doubled.

import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import {
  COARSE_TOLERANCE,
  EngineMemory,
  LADDER_MS,
  NEEDS_NATIVE_SOLVER,
  chooseEngine,
  emptyVerdict,
  ladderFor,
  stoppedForSolver,
} from "../src/engine-policy.js";

const KEY = "cadpole.engineVerdicts.v1";
const FINE = 0.05;

/** `localStorage`, as much of it as the policy touches; `broken` makes it throw like private mode. */
class FakeStorage {
  constructor() {
    this.items = new Map();
    this.broken = false;
  }
  getItem(k) {
    if (this.broken) throw new Error("SecurityError: storage is disabled");
    return this.items.has(k) ? this.items.get(k) : null;
  }
  setItem(k, v) {
    if (this.broken) throw new Error("QuotaExceededError");
    this.items.set(k, String(v));
  }
  removeItem(k) {
    if (this.broken) throw new Error("SecurityError: storage is disabled");
    this.items.delete(k);
  }
}

function install() {
  const storage = new FakeStorage();
  globalThis.localStorage = storage;
  return storage;
}

afterEach(() => {
  delete globalThis.localStorage;
});

test("a verdict cleared from storage is forgotten by a page that is already running", () => {
  const storage = install();
  const memory = new EngineMemory();
  memory.recordFull("doc", "local", LADDER_MS + 50);
  assert.deepEqual(ladderFor(memory.verdict("doc"), FINE), [COARSE_TOLERANCE, FINE], "a slow document is refined coarse-first");
  // What `forgetPolicy` in e2e/perf.spec.ts does between edits.
  storage.removeItem(KEY);
  assert.equal(memory.verdict("doc").localMs, 0);
  assert.deepEqual(ladderFor(memory.verdict("doc"), FINE), [FINE], "the next edit is one evaluation again");
});

test("a verdict another tab recorded is the one this tab uses", () => {
  install();
  const here = new EngineMemory();
  const there = new EngineMemory();
  assert.equal(here.verdict("doc").remoteOk, true);
  there.demoteRemote("doc");
  assert.equal(here.verdict("doc").remoteOk, false, "the server failed this document in the other tab");
  // And a write here keeps what the other tab wrote about the same document.
  here.recordFull("doc", "local", 120);
  assert.deepEqual({ ...there.verdict("doc") }, { localMs: 120, remoteMs: 0, remoteOk: false, coarseMs: 0, ladderOk: true, needsSolver: false });
});

test("with no working storage the page's own copy is the record", () => {
  const storage = install();
  storage.broken = true;
  const memory = new EngineMemory();
  memory.recordFull("doc", "local", 900);
  assert.equal(memory.verdict("doc").localMs, 900);
});

test("storage that stops taking writes leaves the page's copy as the record", () => {
  const storage = install();
  const memory = new EngineMemory();
  memory.recordFull("doc", "local", 100);
  storage.broken = true;
  memory.recordFull("doc", "local", 900);
  assert.equal(memory.verdict("doc").localMs, 900, "a measurement that could not be stored is still used");
  // Once storage has failed, what it says later does not override the copy.
  storage.broken = false;
  storage.removeItem(KEY);
  assert.equal(memory.verdict("doc").localMs, 900);
});

test("an unreadable record is an empty one", () => {
  const storage = install();
  storage.setItem(KEY, "{not json");
  const memory = new EngineMemory();
  assert.equal(memory.verdict("doc").localMs, 0);
  memory.recordFull("doc", "local", 42);
  assert.equal(JSON.parse(storage.getItem(KEY)).doc.localMs, 42, "and the next measurement replaces it");
});

// The browser holds no asset bytes, so a document that imports files
// is evaluated on the server from its first evaluation -- not measured
// locally first, which could only fail on the import.
test("a document that imports files goes to the server unless the user chose otherwise", () => {
  const fresh = emptyVerdict();
  assert.equal(chooseEngine("auto", fresh, true).engine, "local", "an ordinary document is measured locally first");
  const scan = chooseEngine("auto", fresh, true, true);
  assert.equal(scan.engine, "remote");
  assert.match(scan.why, /imports files/);
  // Even after the server once failed on it: local cannot do better.
  assert.equal(chooseEngine("auto", { ...fresh, remoteOk: false, localMs: 5 }, true, true).engine, "remote");
  // The user's own choice wins, and with no server there is nothing to choose.
  assert.equal(chooseEngine("local", fresh, true, true).engine, "local");
  assert.equal(chooseEngine("auto", fresh, false, true).engine, "local");
});

// The solver is native, so the wasm engine stops at `solve()`. A
// document that has done so once goes to the server from then on -- across
// reloads, since the verdict is stored -- unless the user chose local.
test("a document that solves goes to the server once a local evaluation has stopped at a solve", () => {
  install();
  const memory = new EngineMemory();
  const fresh = memory.verdict("plan");
  assert.equal(chooseEngine("auto", fresh, true).engine, "local", "measured locally first, like any document");
  const stopped = [{ message: `sizes: solve: ${NEEDS_NATIVE_SOLVER}: the CLI (\`cadpole eval\`, \`cadpole test\`), ...` }];
  assert.equal(stoppedForSolver(stopped), true);
  assert.equal(stoppedForSolver([{ message: "sizes: infeasible; these cannot all hold: ..." }]), false);
  memory.markNeedsSolver("plan");
  const later = new EngineMemory().verdict("plan");
  assert.equal(later.needsSolver, true, "remembered across pages");
  const choice = chooseEngine("auto", later, true);
  assert.equal(choice.engine, "remote");
  assert.match(choice.why, /solver/);
  // Even after the server failed it once, local cannot do better.
  assert.equal(chooseEngine("auto", { ...later, remoteOk: false }, true).engine, "remote");
  // The user's choice wins, and with no server (a published page, the
  // playground) the wasm engine still stops at solve() as before.
  assert.equal(chooseEngine("local", later, true).engine, "local");
  assert.equal(chooseEngine("auto", later, false).engine, "local");
});
