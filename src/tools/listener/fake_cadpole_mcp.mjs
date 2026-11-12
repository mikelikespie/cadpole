// A stand-in for `cadpole mcp --listener`, for the hermetic listener test.
//
// It speaks the same JSON-RPC subset over stdio and enforces the same tool
// list, so the test exercises the real `McpStdioClient`, the real event loop
// and the real policy check against something that behaves like the server --
// including refusing a denied operation the way the server's 403 arrives here,
// as an `isError` result.
//
// It is *not* a second implementation of Cadpole: it holds three fields and a
// queue of events supplied through CADPOLE_FAKE_EVENTS. `stream_reply` is
// callable and not listed, as on the real server; CADPOLE_FAKE_NO_STREAM=1
// makes it an unknown tool (an older server), and CADPOLE_FAKE_THROTTLE=N
// answers every Nth continuing call `throttled` without storing it. The thing it is standing
// in for (the capability decision) is tested for real in
// `cadpole-cli/src/server.rs::a_listener_token_may_only_call_granted_operations`.

//
// CADPOLE_FAKE_CONTROL names a loopback port where a test plays the server
// instead (spec/replay): every `tools/call` is passed there as one JSON line,
// `{"id": n, "call": name, "args": {...}}`, and answered when the test sends
// `{"id": n, "result": {...}}` or `{"id": n, "error": "..."}`, which lets the
// test hold a long poll for as long as it likes.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { connect } from "node:net";
import { createInterface } from "node:readline";

const GRANTED = [
  "evaluate",
  "render",
  "sweep",
  "animate",
  "export",
  "print_check",
  "drawing",
  "api_doc",
  "get_session",
  "get_selection",
  "set_params",
  "reply",
  "wait_events",
  "explain_ref",
  "edit_script",
  "get_attachment",
  "ack",
  "list_assets",
  "get_asset",
  "run_tests",
  "inspect",
  "list_jobs",
  "wait_jobs",
];
const DENIED = {
  set_script: "replaces a whole file; use edit_script",
  set_selection: "moving the user's selection is spoofing the UI",
  add_asset: "writes caller-chosen bytes into the document",
  solve: "starts long solve jobs; spending the machine's time is the owner's call",
  cancel_jobs: "stops solve jobs the owner started",
};

/** @type {any[]} */
const queued = JSON.parse(process.env.CADPOLE_FAKE_EVENTS ?? "[]");
// What the server keeps between listener processes: acked cursors (keyed by
// client id, as the server keys them) and the replies posted. Only when
// CADPOLE_FAKE_STATE names a file, which is how a test restarts the listener
// against "the same server".
const statePath = process.env.CADPOLE_FAKE_STATE ?? "";
/** @type {{cursors: Record<string, number>, replies: string[], calls?: string[]}} */
const persisted =
  statePath && existsSync(statePath) ? JSON.parse(readFileSync(statePath, "utf8")) : { cursors: {}, replies: [] };
const persist = () => {
  if (statePath) writeFileSync(statePath, JSON.stringify(persisted));
};
const session = {
  id: "fake",
  revision: 3,
  last_event_seq: Number(process.env.CADPOLE_FAKE_LAST_SEQ ?? 7),
  assistant_listening: process.env.CADPOLE_FAKE_LISTENING === "1",
  messages: [],
  document: { id: "d", name: "fake", scripts: [{ path: "main.star", source: 'w = param("w", 40)\n' }] },
};
/** Callable, not in `tools/list`. */
const HIDDEN = process.env.CADPOLE_FAKE_NO_STREAM === "1" ? [] : ["stream_reply"];
const throttleEvery = Number(process.env.CADPOLE_FAKE_THROTTLE ?? 0);
let continued = 0;
/** The cursor most recently acked by any client, for the transcript. */
let cursor = 0;
/** Everything the listener did, so the test can read it back at the end. */
const calls = [];

const write = (o) => process.stdout.write(`${JSON.stringify(o)}\n`);
const text = (v) => ({ content: [{ type: "text", text: JSON.stringify(v) }] });

function callTool(name, args) {
  calls.push({ name, args });
  if (statePath) {
    persisted.calls = [...(persisted.calls ?? []), name];
    persist();
  }
  if (DENIED[name]) {
    return { content: [{ type: "text", text: `${name}: refused by the server: ${DENIED[name]}` }], isError: true };
  }
  if (!GRANTED.includes(name) && !HIDDEN.includes(name)) {
    return { content: [{ type: "text", text: `unknown tool ${name}` }], isError: true };
  }
  switch (name) {
    case "get_session":
      return text(session);
    case "ack": {
      const client = String(args?.client ?? "");
      const c = Number(args?.cursor ?? 0);
      const held = Math.max(persisted.cursors[client] ?? 0, c);
      persisted.cursors[client] = held;
      persist();
      if (held > cursor) cursor = held;
      return text({ client, cursor: held, last_event_seq: session.last_event_seq });
    }
    case "wait_events": {
      const after = Number(args?.after ?? 0);
      const events = queued.filter((e) => e.seq > after);
      const next = events.length ? events[events.length - 1].seq : after;
      // One batch, then nothing: the test drives `step()` itself.
      return text({ events, cursor: next });
    }
    case "reply": {
      const seq = session.messages.length + 1;
      session.messages.push({ seq, from: "assistant", text: String(args?.text ?? "") });
      persisted.replies.push(String(args?.text ?? ""));
      persist();
      return text({ seq });
    }
    case "stream_reply": {
      const finish = String(args?.finish ?? "");
      let m = session.messages.find((x) => x.seq === Number(args?.seq ?? 0));
      if (Number(args?.seq ?? 0) === 0) {
        m = { seq: session.messages.length + 1, from: "assistant", text: "", state: "streaming", activity: "" };
        session.messages.push(m);
      } else if (!m || m.state !== "streaming") {
        return { content: [{ type: "text", text: `stream_reply: message ${args?.seq} is not in progress` }], isError: true };
      } else if (!finish && throttleEvery > 0 && ++continued % throttleEvery === 0) {
        return text({ seq: m.seq, state: m.state, length: m.text.length, throttled: true });
      }
      m.text += String(args?.append ?? "");
      m.activity = finish ? "" : String(args?.activity ?? "");
      if (finish === "REPLY_FINISH_DONE") m.state = "done";
      if (finish === "REPLY_FINISH_FAILED") m.state = "failed";
      if (finish) {
        persisted.replies.push(m.text);
        persist();
      }
      return text({ seq: m.seq, state: m.state, length: m.text.length, throttled: false });
    }
    case "edit_script":
      session.revision += 1;
      return text({ revision: session.revision, applied: (args?.edits ?? []).length, diff: "--- fake diff" });
    case "get_selection":
      return text({ selected: [{ feature_id: "blk", kind: "face", topo_name: "blk/top", status: "REF_STATUS_OK" }] });
    default:
      return text({ ok: true });
  }
}

/** The test that plays the server, when there is one. */
const control = process.env.CADPOLE_FAKE_CONTROL ? connect({ host: "127.0.0.1", port: Number(process.env.CADPOLE_FAKE_CONTROL) }) : null;
/** Calls passed to the test and not answered yet, by the id they were passed under. */
const passedOn = new Map();
let nextCall = 1;
if (control) {
  // Written at once: a call must not overtake this line.
  control.write(`${JSON.stringify({ hello: "mcp", pid: process.pid })}\n`);
  control.on("error", () => process.exit(3));
  control.on("close", () => process.exit(0));
  createInterface({ input: control }).on("line", (line) => {
    const answer = JSON.parse(line);
    const reply = passedOn.get(answer.id);
    passedOn.delete(answer.id);
    if (!reply) return;
    reply(answer.error ? { content: [{ type: "text", text: String(answer.error) }], isError: true } : text(answer.result));
  });
}

createInterface({ input: process.stdin }).on("line", (line) => {
  if (!line.trim()) return;
  const msg = JSON.parse(line);
  const reply = (result) => write({ jsonrpc: "2.0", id: msg.id, result });
  if (control && msg.method === "tools/call") {
    const id = nextCall++;
    passedOn.set(id, reply);
    control.write(`${JSON.stringify({ id, call: msg.params?.name, args: msg.params?.arguments ?? {} })}\n`);
    return;
  }
  switch (msg.method) {
    case "initialize":
      reply({
        protocolVersion: "2025-06-18",
        capabilities: { tools: {} },
        serverInfo: { name: "cadpole-listener", version: "fake" },
        instructions: "fake",
      });
      break;
    case "tools/list":
      reply({ tools: GRANTED.map((n) => ({ name: n, description: n, inputSchema: { type: "object" } })) });
      break;
    case "tools/call":
      reply(callTool(msg.params?.name, msg.params?.arguments ?? {}));
      break;
    case "notifications/initialized":
      break;
    // The test reads the transcript through this, rather than through a file.
    case "cadpole/fake/transcript":
      reply({ calls, messages: session.messages, cursor });
      break;
    default:
      if (msg.id !== undefined) write({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: "no" } });
  }
});
