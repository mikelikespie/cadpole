// The shipped entry point, restarted: `main.mjs --driver echo --once` run
// twice against a fake `cadpole mcp` whose server-side state (acked cursors
// by client id, posted replies) survives between the two processes, the way
// a `cadpole serve` that stayed up does.
//
//   bazel test //src/tools/listener:restart_test
//
// A message posted while the listener was down must be acted on when it comes
// back.

import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const main = resolve(here, "main.mjs");
const fake = resolve(here, "fake_cadpole_mcp.mjs");

const msg = (seq, text) => ({ seq, at: "t", kind: "message", by: "web", data: { from: "user", text } });

/**
 * One listener process: attach, resume, handle one poll, exit.
 * @param {string} dir @param {any[]} events @param {number} lastSeq @param {string[]} extra
 */
function runOnce(dir, events, lastSeq, extra = []) {
  return execFileSync(process.execPath, [main, "--cadpole", join(dir, "cadpole"), "--driver", "echo", "--once", "--poll-ms", "10", ...extra], {
    env: {
      ...process.env,
      CADPOLE_FAKE_EVENTS: JSON.stringify(events),
      CADPOLE_FAKE_LAST_SEQ: String(lastSeq),
      CADPOLE_FAKE_LISTENING: "1",
      CADPOLE_FAKE_STATE: join(dir, "state.json"),
    },
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
}

function setup() {
  const dir = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "listener-restart-"));
  // `--cadpole` names one executable; this one is the fake.
  const bin = join(dir, "cadpole");
  writeFileSync(bin, `#!/bin/sh\nexec ${JSON.stringify(process.execPath)} ${JSON.stringify(fake)} "$@"\n`);
  chmodSync(bin, 0o755);
  return dir;
}

const state = (dir) => JSON.parse(readFileSync(join(dir, "state.json"), "utf8"));

test("a restarted listener resumes its cursor and answers what was posted while it was down", () => {
  const dir = setup();
  // Up: the session's tail is 7, the user asks something at 8.
  runOnce(dir, [msg(8, "first")], 7);
  assert.equal(state(dir).replies.length, 1, "the first run answered the message at seq 8");

  // Down; the user posts at 9. Up again, a new process.
  runOnce(dir, [msg(8, "first"), msg(9, "while you were away")], 9);
  const s = state(dir);
  assert.equal(Object.keys(s.cursors).length, 1, `both runs used one client id: ${JSON.stringify(s.cursors)}`);
  assert.equal(Object.values(s.cursors)[0], 9);
  assert.equal(s.replies.length, 2, "the message at seq 9 was answered after the restart, and seq 8 not twice");
});

test("--client names the cursor, so two listeners on one server keep separate places", () => {
  const dir = setup();
  runOnce(dir, [msg(8, "first")], 7, ["--client", "listener:a"]);
  runOnce(dir, [msg(8, "first")], 7, ["--client", "listener:b"]);
  assert.deepEqual(state(dir).cursors, { "listener:a": 8, "listener:b": 8 });
});

test("a first run that sees no events still keeps its place", () => {
  const dir = setup();
  // Up at tail 7 with nothing new; then down, and the user posts at 8.
  runOnce(dir, [], 7);
  assert.deepEqual(state(dir).cursors, { listener: 7 }, "the adopted tail is acked");
  runOnce(dir, [msg(8, "while you were away")], 8);
  const s = state(dir);
  assert.equal(s.replies.length, 1, "the message at seq 8 was answered after the restart");
  assert.deepEqual(s.cursors, { listener: 8 });
});
