// The listener against a real server.
//
//   bazel test //src/tools/listener:listener_e2e_test
//
// It starts a real `cadpole serve` on an ephemeral loopback port with its own
// runtime directory, so the tokens it mints are this test's and nobody else's.
// It then:
//
//   1. reads the owner token out of the runtime directory, the way a user's
//      `cadpole op` would, and uses it to turn Listen on and select a face with
//      a note -- i.e. it plays the human at the UI;
//   2. starts the listener, which spawns a real `cadpole mcp --listener`; that
//      child finds the *restricted* token by itself and this process never
//      sees it;
//   3. lets the listener take one turn, with a scripted driver standing in for
//      the model (driver_test.mjs covers the process a real one runs in);
//   4. asserts that the edit and the reply really landed in the session, and
//      that the same connection is refused `set_script`.
//
// The privilege boundary is checked twice over, from both sides: here from
// outside, through a real MCP child holding a real restricted token, and in
// `cadpole-cli/src/server.rs` from inside, against the router.

import { strict as assert } from "node:assert";
import test from "node:test";
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";

import { McpStdioClient } from "./mcp_client.mjs";
import { Listener } from "./listener.mjs";
import { scriptedDriver } from "./driver.mjs";
import { checkTools } from "./policy.mjs";

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
  /** @type {string[]} */
  const said = [];
  const base = await new Promise((resolveUrl, rejectUrl) => {
    const timer = setTimeout(() => rejectUrl(new Error(`cadpole serve did not start:\n${said.join("\n")}`)), 30_000);
    createInterface({ input: child.stderr }).on("line", (line) => {
      said.push(line);
      const m = /listening on (http:\/\/[^/]+)\//.exec(line);
      if (m) {
        clearTimeout(timer);
        resolveUrl(m[1]);
      }
    });
    child.on("exit", (code) => {
      clearTimeout(timer);
      rejectUrl(new Error(`cadpole serve exited with ${code}:\n${said.join("\n")}`));
    });
  });
  return { child, base, said };
}

/** Polls `cond` every 20 ms until it holds or `ms` have passed. */
async function waitFor(cond, ms, what) {
  const until = Date.now() + ms;
  while (!cond()) {
    if (Date.now() > until) throw new Error(what);
    await new Promise((r) => setTimeout(r, 20));
  }
}

/** An owner-token call to `POST /api/ops/<op>`: the user's half of the story. */
async function asOwner(base, token, op, body) {
  const res = await fetch(`${base}/api/ops/${op}`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${token}` },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  assert.equal(res.status, 200, `${op} -> ${res.status}: ${text}`);
  return JSON.parse(text);
}

test("a selection with a note gets an edit and a reply, and set_script stays refused", async (t) => {
  const cadpole = cadpoleBinary();
  const runtimeDir = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "cadpole-listener-"));
  const { child: server, base, said } = await startServer(cadpole, runtimeDir);
  t.after(() => server.kill("SIGTERM"));

  // The banner names both token files and prints no token at all: its output
  // is a pipe here, not a terminal, so even the browser's link goes to a
  // private file instead.
  const ownerToken = readFileSync(join(runtimeDir, "session-token"), "utf8").trim();
  const listenerToken = readFileSync(join(runtimeDir, "listener-token"), "utf8").trim();
  assert.notEqual(ownerToken, listenerToken);
  // `startServer` returns on the "listening on" line; the rest of the banner
  // comes after it, so wait for its last token line.
  await waitFor(() => said.some((line) => line.startsWith("listener:")), 10_000, "the banner was never finished");
  const banner = said.join("\n");
  assert.ok(!banner.includes(ownerToken), `the agent token must never be printed:\n${banner}`);
  assert.ok(!banner.includes(listenerToken), `the listener token must never be printed:\n${banner}`);
  assert.match(banner, /open: {2}the link with this run's token is in .*\.url \(0600\)/);

  // ---- the human: turn Listen on, then click a face and type a note.
  await fetch(`${base}/api/session/assistant`, {
    method: "PUT",
    headers: { "content-type": "application/json", authorization: `Bearer ${ownerToken}` },
    body: JSON.stringify({ listening: true }),
  });
  const before = await asOwner(base, ownerToken, "GetSession", {});
  await asOwner(base, ownerToken, "SetSelection", {
    selected: [{ feature_id: "blk", kind: "face", topo_name: "blk/top" }],
    note: "this wall is too thin",
  });

  // ---- the listener: a real `cadpole mcp --listener`, finding the server
  // and the restricted token on its own, over the agent socket. Note that
  // nothing below is given a token or an address.
  const mcp = new McpStdioClient({
    command: cadpole,
    args: ["mcp", "--listener"],
    env: { CADPOLE_RUNTIME_DIR: runtimeDir, CADPOLE_TOKEN: "", CADPOLE_LISTENER_TOKEN: "" },
    onLog: () => {},
  });
  const info = await mcp.start();
  // Registered before the assertions, so a failure below still tears the child
  // down and lets the test process exit.
  t.after(() => mcp.stop());
  assert.equal(info.serverInfo.name, "cadpole-listener");
  assert.match(info.instructions, /LIMITED listener/);

  const tools = await mcp.toolNames();
  assert.equal(checkTools(tools).ok, true, `unexpected tool list: ${tools.join(", ")}`);

  /** @type {string[]} */
  const prompts = [];
  const listener = new Listener({
    cadpole: mcp,
    client: "listener:e2e",
    pollMs: 2000,
    flushMs: 20,
    alwaysListen: false,
    driver: scriptedDriver({
      call: (n, a) => mcp.callTool(n, a),
      async turn(prompt, k, sink) {
        prompts.push(prompt);
        const session = await k.call("get_session", {});
        const sel = await k.call("get_selection", {});
        await k.call("edit_script", {
          path: "main.star",
          base_revision: session.revision,
          edits: [{ old_str: "box(20, 20, 10)", new_str: "box(20, 20, 14)" }],
        });
        // The reply, a piece at a time, as a model writes it.
        sink.activity("editing the script…");
        sink.text("Raised the box ");
        await new Promise((r) => setTimeout(r, 60));
        sink.text(`to 14 mm; you had ${sel.selected?.length ?? 0} thing(s) selected.`);
        return ["Raised the box to 14 mm"];
      },
    }),
  });

  // Resume from before the selection so this run sees it.
  await listener.resume();
  listener.cursor = Number(before.last_event_seq ?? 0);
  listener.listening = true;
  const acted = await listener.step();

  assert.equal(acted.length, 1, `expected one turn, got ${JSON.stringify(acted.map((e) => e.kind))}`);
  assert.equal(acted[0].kind, "selection_changed");
  assert.match(prompts[0], /this wall is too thin/);

  // ---- the result, read back as the owner.
  const after = await asOwner(base, ownerToken, "GetSession", {});
  assert.ok(after.revision > before.revision, "the edit bumped the revision");
  assert.ok(after.document.scripts[0].source.includes("14"), after.document.scripts[0].source);
  const replies = (after.messages ?? []).filter((m) => m.from === "assistant");
  assert.equal(replies.length, 1, `one assistant message: ${JSON.stringify(after.messages)}`);
  assert.match(replies[0].text, /^Raised the box to 14 mm; you had \d thing\(s\) selected\.$/);
  assert.equal(replies[0].state, "done", "the streamed reply settled");

  // The reply was written in pieces, and the event log holds it once: the
  // finished message. A piece is not an event.
  const log = await asOwner(base, ownerToken, "WaitEvents", { after: Number(before.last_event_seq ?? 0), timeout_ms: 10, client: "reader" });
  const messageEvents = (log.events ?? []).filter((e) => e.kind === "message");
  assert.deepEqual(messageEvents.map((e) => [e.data.from, e.data.state, e.data.text]), [["assistant", "done", replies[0].text]]);

  // Another client under the same token cannot write to it, in progress or
  // not; and it cannot speak as the user by any spelling.
  const stream = (body) =>
    fetch(`${base}/api/ops/StreamReply`, {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${listenerToken}`, "x-cadpole-client": "someone-else" },
      body: JSON.stringify(body),
    });
  const late = await stream({ seq: replies[0].seq, append: " And more." });
  assert.notEqual(late.status, 200);
  assert.match(await late.text(), /not in progress/);
  const mine = await mcp.callTool("stream_reply", { append: "Still writing" });
  const intruder = await stream({ seq: mine.seq, append: " -- not mine" });
  assert.notEqual(intruder.status, 200);
  assert.match(await intruder.text(), /another client/);
  await mcp.callTool("stream_reply", { seq: mine.seq, finish: "REPLY_FINISH_DONE" });

  // ---- and the boundary, from the outside. The same connection that just
  // edited the script cannot replace it.
  // ...and the refusal carries the *reason*, not just a number: the system
  // prompt tells the model to say what it could not do, which needs a
  // sentence.
  await assert.rejects(
    () => mcp.callTool("set_script", { path: "main.star", source: "b = box(1,1,1)\n", base_revision: after.revision }),
    /replaces a whole file/,
  );
  await assert.rejects(() => mcp.callTool("set_selection", { selected: [] }), /spoofing the UI/);
  await assert.rejects(() => mcp.callTool("add_asset", { name: "x.svg", data: "" }), /writes caller-chosen bytes/);

  // Those three were refused by `cadpole mcp --listener` before the request was
  // made, which is ergonomics, not the boundary. So ask the *server* the same
  // question with the same token and no MCP client in the way: this is the
  // assertion that would still hold if the client were replaced by a hostile
  // one.
  for (const [op, why] of [
    ["SetScript", /may not call SetScript: replaces a whole file/],
    ["SetSelection", /may not call SetSelection: moving the user's selection/],
    ["AddAsset", /may not call AddAsset: writes caller-chosen bytes/],
  ]) {
    const res = await fetch(`${base}/api/ops/${op}`, {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${listenerToken}` },
      body: JSON.stringify({}),
    });
    assert.equal(res.status, 403, `${op} straight to the server should be 403`);
    assert.match(await res.text(), why);
  }
  // The hand-written session routes are not a way round it either.
  const viaAlias = await fetch(`${base}/api/session/script`, {
    method: "PUT",
    headers: { "content-type": "application/json", authorization: `Bearer ${listenerToken}` },
    body: JSON.stringify({ path: "main.star", source: "b = box(1,1,1)\n", base_revision: after.revision }),
  });
  assert.equal(viaAlias.status, 403);

  // Nothing above touched the script.
  const final = await asOwner(base, ownerToken, "GetSession", {});
  assert.equal(final.document.scripts[0].source, after.document.scripts[0].source);
});

test("a listener started without its token refuses to run rather than falling back", async () => {
  const cadpole = cadpoleBinary();
  const runtimeDir = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "cadpole-notoken-"));
  const child = spawn(cadpole, ["mcp", "--server", "http://127.0.0.1:1", "--listener"], {
    env: { ...process.env, CADPOLE_RUNTIME_DIR: runtimeDir, CADPOLE_TOKEN: "", CADPOLE_LISTENER_TOKEN: "" },
    stdio: ["pipe", "pipe", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", (b) => (stderr += String(b)));
  // `close`, not `exit`: `exit` can fire before the stdio streams have been
  // drained, and the message is the whole point of this test.
  const code = await new Promise((r) => child.on("close", r));
  assert.notEqual(code, 0, "it must not start with no restricted token");
  assert.match(stderr, /refusing to fall back to the owner token/);
});
