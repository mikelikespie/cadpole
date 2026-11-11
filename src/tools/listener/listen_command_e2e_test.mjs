// `cadpole listen --next` and `--follow` against a real server: the wait an
// assistant uses instead of polling.
//
//   bazel test //src/tools/listener:listen_command_e2e_test
//
// A real `cadpole serve` on an ephemeral loopback port with its own runtime
// directory, and the real `cadpole listen` as a child process, watched the
// way a harness watches it: by what it writes to stdout and by its exit. The
// test plays the user through the owner's token. What it shows:
//
//   * nothing is printed while nothing happens, nor for a parameter change
//     or a bare selection, nor while the Listen toggle is off;
//   * a message prints exactly one line, and `--next` then exits 0;
//   * a message sent while nobody waits is printed by the next `--next`;
//   * a reply written a piece at a time wakes neither `cadpole listen` nor
//     the channel of `cadpole mcp`: the pieces are not events, and the
//     finished message is the assistant's.

import { strict as assert } from "node:assert";
import test from "node:test";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";

/** The `cadpole` binary, from this test's runfiles. */
function cadpoleBinary() {
  const runfiles = process.env.JS_BINARY__RUNFILES ?? process.env.RUNFILES_DIR ?? "";
  for (const p of [
    join(runfiles, "_main", "src", "rust", "cadpole-cli", "cadpole"),
    join(runfiles, "cadpole", "src", "rust", "cadpole-cli", "cadpole"),
    resolve("bazel-bin/src/rust/cadpole-cli/cadpole"),
  ]) {
    if (p && existsSync(p)) return p;
  }
  throw new Error(`cannot find the cadpole binary (runfiles: ${runfiles || "unset"})`);
}

/** Start `cadpole serve` and wait for its banner. */
async function startServer(cadpole, runtimeDir) {
  // `--agent-tcp`: this test plays the user's side over the TCP port with
  // the agent token from the runtime directory.
  const child = spawn(cadpole, ["serve", "--port", "0", "--agent-tcp", "--web-dir", join(runtimeDir, "no-web")], {
    env: { ...process.env, CADPOLE_RUNTIME_DIR: runtimeDir, CADPOLE_TOKEN: "" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const base = await new Promise((resolveUrl, rejectUrl) => {
    const timer = setTimeout(() => rejectUrl(new Error("cadpole serve did not start")), 30_000);
    createInterface({ input: child.stderr }).on("line", (line) => {
      const m = /listening on (http:\/\/[^/]+)\//.exec(line);
      if (m) {
        clearTimeout(timer);
        resolveUrl(m[1]);
      }
    });
    child.on("exit", (code) => rejectUrl(new Error(`cadpole serve exited with ${code}`)));
  });
  return { child, base };
}

const pause = (ms) => new Promise((r) => setTimeout(r, ms));

/** Polls `cond` every 20 ms until it holds or `ms` have passed. */
async function waitFor(cond, ms, what) {
  const until = Date.now() + ms;
  while (!cond()) {
    if (Date.now() > until) throw new Error(what);
    await pause(20);
  }
}

test("cadpole listen is silent until there is a message to act on", async (t) => {
  const cadpole = cadpoleBinary();
  const runtimeDir = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "cadpole-listen-"));
  const { child: server, base } = await startServer(cadpole, runtimeDir);
  t.after(() => server.kill("SIGTERM"));
  await waitFor(() => existsSync(join(runtimeDir, "session-token")), 10_000, "no session token was written");
  const token = readFileSync(join(runtimeDir, "session-token"), "utf8").trim();

  // The user, at the UI.
  const op = async (name, body) => {
    const r = await fetch(`${base}/api/ops/${name}`, {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${token}`, "x-cadpole-client": "ui" },
      body: JSON.stringify(body),
    });
    assert.equal(r.status, 200, `${name}: ${await r.clone().text()}`);
    return r.json();
  };
  const setListen = (listening) =>
    fetch(`${base}/api/session/assistant`, {
      method: "PUT",
      headers: { "content-type": "application/json", authorization: `Bearer ${token}` },
      body: JSON.stringify({ listening }),
    });
  const say = (text) => op("PostMessage", { text });
  const fiddle = async () => {
    await op("SetParams", { overrides: { w: { number: 41 } } });
    await op("SetSelection", { selected: [] });
  };

  /** `cadpole listen <args>`, with what it has printed so far. */
  const listen = (...args) => listenAt(base, ...args);
  const listenAt = (url, ...args) => {
    const child = spawn(cadpole, ["listen", "--url", url, ...args], {
      env: { ...process.env, CADPOLE_RUNTIME_DIR: runtimeDir, CADPOLE_TOKEN: "" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    const run = { child, lines: /** @type {any[]} */ ([]), notes: /** @type {string[]} */ ([]), code: /** @type {number | null} */ (null), done: false };
    createInterface({ input: child.stdout }).on("line", (l) => run.lines.push(JSON.parse(l)));
    createInterface({ input: child.stderr }).on("line", (l) => run.notes.push(l));
    child.on("exit", (code) => {
      run.code = code;
      run.done = true;
    });
    t.after(() => child.kill("SIGKILL"));
    return run;
  };
  const position = async () => (await op("Ack", { client: "listen", cursor: 0 })).cursor ?? 0;

  // ---- Listen off: a message is not passed on, and the one note about it is
  // on stderr, where it wakes nobody.
  const off = listen("--next");
  await waitFor(() => off.notes.length > 0, 10_000, "no note about the Listen toggle");
  assert.match(off.notes[0], /Listen toggle is off/);
  await say("typed while Listen is off");
  await pause(400);
  assert.deepEqual(off.lines, [], "nothing is printed while the user has Listen off");
  assert.equal(off.done, false, "and it keeps waiting");

  // ---- Listen on. Idle, and the user moving things: still nothing.
  await setListen(true);
  await fiddle();
  await pause(600);
  assert.deepEqual(off.lines, [], "a parameter change and a bare selection are not printed");
  assert.equal(off.done, false);

  // ---- A message: one line, then exit 0.
  await say("make the plate 2 mm thicker");
  await waitFor(() => off.done, 10_000, "--next did not exit after a message");
  assert.equal(off.code, 0);
  assert.equal(off.lines.length, 1, JSON.stringify(off.lines));
  assert.equal(off.lines[0].kind, "message");
  assert.equal(off.lines[0].data.text, "make the plate 2 mm thicker");
  assert.equal(off.lines[0].data.from, "user");

  // ---- Sent while nobody waits (the assistant is busy): the next run has it
  // at once, and only it.
  await say("and round the corners");
  const next = listen("--next");
  await waitFor(() => next.done, 10_000, "the next --next did not pick up the waiting message");
  assert.deepEqual(next.lines.map((l) => l.data.text), ["and round the corners"]);
  assert.deepEqual(next.notes, [], "no notes when there is nothing to say");

  // ---- --follow: the same silence, one line per message, and no exit. A
  // selection with a note is worth a line; the assistant's own reply is not.
  const follow = listen("--follow");
  const start = await position();
  await fiddle();
  await op("Reply", { text: "done" });
  await pause(600);
  assert.deepEqual(follow.lines, [], "nothing for a parameter change, a bare selection or a reply");
  await op("SetSelection", { selected: [], note: "this edge" });
  await say("third");
  await waitFor(() => follow.lines.length >= 2, 10_000, `--follow printed ${JSON.stringify(follow.lines)}`);
  await pause(300);
  assert.deepEqual(follow.lines.map((l) => l.kind), ["selection_changed", "message"]);
  assert.equal(follow.done, false, "--follow keeps running");
  assert.ok((await position()) > start, "and its position is kept by the server");
  follow.child.kill("SIGTERM");

  // ---- No server: an error at once, not a silent wait.
  const nowhere = listenAt("http://127.0.0.1:9", "--next");
  await waitFor(() => nowhere.done, 10_000, "listen kept waiting for a server that was never there");
  assert.notEqual(nowhere.code, 0);
  assert.match(nowhere.notes.join("\n"), /no session at http:\/\/127\.0\.0\.1:9/);
});

test("a reply written a piece at a time wakes nobody who is waiting for the user", async (t) => {
  const cadpole = cadpoleBinary();
  const runtimeDir = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "cadpole-listen-stream-"));
  const { child: server, base } = await startServer(cadpole, runtimeDir);
  t.after(() => server.kill("SIGTERM"));
  await waitFor(() => existsSync(join(runtimeDir, "session-token")), 10_000, "no session token was written");
  const token = readFileSync(join(runtimeDir, "session-token"), "utf8").trim();
  const listenerToken = readFileSync(join(runtimeDir, "listener-token"), "utf8").trim();
  const env = { ...process.env, CADPOLE_RUNTIME_DIR: runtimeDir, CADPOLE_TOKEN: "" };

  const call = async (bearer, client, name, body) => {
    const r = await fetch(`${base}/api/ops/${name}`, {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${bearer}`, "x-cadpole-client": client },
      body: JSON.stringify(body),
    });
    assert.equal(r.status, 200, `${name}: ${await r.clone().text()}`);
    return r.json();
  };
  await fetch(`${base}/api/session/assistant`, {
    method: "PUT",
    headers: { "content-type": "application/json", authorization: `Bearer ${token}` },
    body: JSON.stringify({ listening: true }),
  });

  // The two ways an assistant waits: `cadpole listen --next`, and the
  // channel of `cadpole mcp`.
  const listen = spawn(cadpole, ["listen", "--url", base, "--next"], { env, stdio: ["ignore", "pipe", "pipe"] });
  /** @type {string[]} */
  const printed = [];
  let exited = false;
  createInterface({ input: listen.stdout }).on("line", (l) => printed.push(l));
  listen.on("exit", () => (exited = true));
  t.after(() => listen.kill("SIGKILL"));

  const mcp = spawn(cadpole, ["mcp", "--server", base], { env, stdio: ["pipe", "pipe", "pipe"] });
  t.after(() => mcp.kill("SIGKILL"));
  /** @type {any[]} */
  const notifications = [];
  let initialized = false;
  createInterface({ input: mcp.stdout }).on("line", (l) => {
    const m = JSON.parse(l);
    if (m.id === 1) initialized = true;
    if (m.method === "notifications/claude/channel") notifications.push(m.params);
  });
  mcp.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "t", version: "0" } } })}\n`);
  await waitFor(() => initialized, 10_000, "cadpole mcp did not answer initialize");
  mcp.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized", params: {} })}\n`);
  // Both are waiting on the event log now.
  await pause(800);

  // A page that asked for the pieces, for comparison: it does see them.
  const page = new AbortController();
  t.after(() => page.abort());
  let stream = "";
  void (async () => {
    const r = await fetch(`${base}/api/events?after=0&live=1`, { headers: { authorization: `Bearer ${token}` }, signal: page.signal });
    const reader = r.body.getReader();
    const decoder = new TextDecoder();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      stream += decoder.decode(value, { stream: true });
    }
  })().catch(() => {});
  await pause(300);

  // The reply: started, grown, an activity line, grown again, ended.
  const started = await call(listenerToken, "listener:1", "StreamReply", { activity: "thinking…" });
  const seq = started.seq;
  for (const body of [
    { seq, append: "Making the plate " },
    { seq, activity: "editing the script…" },
    { seq, append: "2 mm thicker" },
  ]) {
    await call(listenerToken, "listener:1", "StreamReply", body);
    await pause(250);
  }
  const before = await call(token, "ui", "GetSession", {});
  assert.deepEqual(
    before.messages.map((m) => [m.from, m.state, m.text]),
    [["assistant", "streaming", "Making the plate 2 mm thicker"]],
  );
  await call(listenerToken, "listener:1", "StreamReply", { seq, append: ".", finish: "REPLY_FINISH_DONE" });
  await pause(700);

  assert.deepEqual(printed, [], "cadpole listen --next printed nothing for the pieces or for the finished reply");
  assert.equal(exited, false, "and it is still waiting");
  assert.deepEqual(notifications, [], "the channel sent nothing either");
  const frames = stream.split("\n\n").filter((f) => f.includes("event: reply_stream"));
  assert.equal(frames.length, 4, `the page that asked got each piece: ${stream}`);
  assert.equal(stream.split("\n\n").filter((f) => f.includes("event: message")).length, 1, "and the finished message once, as an event");

  // The user's next message is what wakes both.
  await call(token, "ui", "PostMessage", { text: "now round the corners" });
  await waitFor(() => exited, 10_000, "--next did not exit on the user's message");
  assert.equal(printed.length, 1);
  assert.equal(JSON.parse(printed[0]).data.text, "now round the corners");
  await waitFor(() => notifications.length === 1, 10_000, `the channel sent ${JSON.stringify(notifications)}`);
  assert.match(notifications[0].content, /now round the corners/);
});
