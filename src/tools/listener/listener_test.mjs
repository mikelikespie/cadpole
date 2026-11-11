// The listener, end to end up to the model seam: the real MCP stdio client,
// the real loop, the real policy, against `fake_cadpole_mcp.mjs` and a
// scripted driver standing in for the model.
//
//   bazel test //src/tools/listener:listener_test
//
// The seam itself, the process the model runs in, is `driver_test.mjs`.

import { strict as assert } from "node:assert";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

import { McpStdioClient } from "./mcp_client.mjs";
import { Listener, isTurnWorthy, promptFor } from "./listener.mjs";
import { ReplyStream } from "./reply_stream.mjs";
import { scriptedDriver } from "./driver.mjs";
import { EXPECTED_DENIED, EXPECTED_TOOLS, checkTools } from "./policy.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const fake = resolve(here, "fake_cadpole_mcp.mjs");

/**
 * @param {{events?: any[], listening?: boolean, env?: Record<string, string>}} opts
 */
async function connect(opts = {}) {
  const client = new McpStdioClient({
    command: process.execPath,
    args: [fake],
    env: {
      CADPOLE_FAKE_EVENTS: JSON.stringify(opts.events ?? []),
      CADPOLE_FAKE_LISTENING: opts.listening === false ? "0" : "1",
      ...(opts.env ?? {}),
    },
  });
  await client.start();
  return client;
}

const SELECTION_WITH_NOTE = {
  seq: 8,
  at: "t",
  kind: "selection_changed",
  by: "web",
  data: { selected: [{ feature_id: "blk", kind: "face", topo_name: "blk/top" }], note: "this wall is too thin" },
};
const BARE_SELECTION = { seq: 9, at: "t", kind: "selection_changed", by: "web", data: { selected: [], note: "" } };
const USER_MESSAGE = { seq: 10, at: "t", kind: "message", by: "web", data: { from: "user", text: "make it 3mm" } };
const OWN_REPLY = { seq: 11, at: "t", kind: "message", by: "listener", data: { from: "assistant", text: "done" } };

test("only a note or a message is worth a turn", () => {
  assert.equal(isTurnWorthy(SELECTION_WITH_NOTE), true);
  assert.equal(isTurnWorthy(USER_MESSAGE), true);
  // A bare click is not a request. Acting on one would start a turn every time
  // the user looked at something.
  assert.equal(isTurnWorthy(BARE_SELECTION), false);
  assert.equal(isTurnWorthy(OWN_REPLY), false, "the assistant's own messages are not requests");
  assert.equal(isTurnWorthy({ kind: "params_changed", data: {} }), false);
  assert.equal(isTurnWorthy({ kind: "eval_finished", data: {} }), false);
  assert.equal(isTurnWorthy(null), false);
});

test("the prompt labels what is a request and what is data", () => {
  const p = promptFor(SELECTION_WITH_NOTE);
  assert.match(p, /kind="selection_changed"/);
  assert.match(p, /content-is="data-about-what-the-user-is-looking-at"/);
  assert.match(p, /this wall is too thin/);
  // A message is the other thing, and says so.
  assert.match(promptFor(USER_MESSAGE), /content-is="request-from-the-user"/);
});

test("the published tool list is the granted capability, and nothing else", async (t) => {
  const client = await connect();
  t.after(() => client.stop());
  const tools = await client.toolNames();
  assert.deepEqual([...tools].sort(), [...EXPECTED_TOOLS].sort());
  const check = checkTools(tools);
  assert.equal(check.ok, true, JSON.stringify(check));
  for (const denied of EXPECTED_DENIED) assert.equal(tools.includes(denied), false, `${denied} was offered`);
});

test("a denied operation is refused at the far end, not by asking nicely", async (t) => {
  const client = await connect();
  t.after(() => client.stop());
  // Calling it by name anyway -- which is what a confused or hostile agent
  // would do -- gets an error, not a silent success.
  await assert.rejects(() => client.callTool("set_script", { path: "main.star", source: "b = 1\n" }), /refused by the server/);
  await assert.rejects(() => client.callTool("set_selection", { selected: [] }), /refused by the server/);
  await assert.rejects(() => client.callTool("add_asset", { name: "x.svg" }), /refused by the server/);
  // ...while the one it is supposed to use works.
  const r = await client.callTool("edit_script", { path: "main.star", base_revision: 3, edits: [{ old_str: "40", new_str: "60" }] });
  assert.equal(r.applied, 1);
});

const message = (seq, text) => ({ seq, at: "t", kind: "message", by: "web", data: { from: "user", text } });

test("a failed turn ends its reply as failed with one line, and the next message is still handled", async (t) => {
  const client = await connect({ events: [message(8, "a"), message(9, "b"), message(10, "c")] });
  t.after(() => client.stop());
  let n = 0;
  const listener = new Listener({
    cadpole: client,
    client: "listener:test",
    pollMs: 10,
    flushMs: 5,
    driver: {
      async ask(_prompt, sink) {
        n += 1;
        if (n === 1) throw new Error("the turn failed (success): Not logged in\nand a second line");
        if (n === 2) {
          sink.text("Making the plate");
          throw new Error("Claude Code exited (code 7) in the middle of the turn");
        }
        sink.text("fine");
        return ["fine"];
      },
      stop: async () => {},
    },
  });
  await listener.resume();
  const acted = await listener.step();
  assert.equal(acted.length, 3, "handled in order, one at a time");
  const transcript = await client.request("cadpole/fake/transcript", {});
  assert.deepEqual(
    transcript.messages.map((m) => [m.state, m.text]),
    [
      // Before the model did anything: a line of its own.
      [undefined, "The listener could not finish this: the turn failed (success): Not logged in"],
      // After it had written: the reply keeps what was written and ends with the line.
      ["failed", "Making the plate\n\nThe listener could not finish this: Claude Code exited (code 7) in the middle of the turn"],
      ["done", "fine"],
    ],
    "the failure is one line, and nothing is left in progress",
  );
  assert.ok(transcript.messages.every((m) => m.from === "assistant" && !m.activity));
});

/** The `stream_reply` calls a transcript holds, as [append, activity, finish]. */
const streamCalls = (transcript) =>
  transcript.calls.filter((c) => c.name === "stream_reply").map((c) => [c.args.append, c.args.activity, c.args.finish ?? ""]);

test("a reply is started at once, grows a few times a second and settles", async (t) => {
  const client = await connect();
  t.after(() => client.stop());
  const reply = new ReplyStream({ cadpole: client, flushMs: 20 });
  await reply.begin();
  let transcript = await client.request("cadpole/fake/transcript", {});
  assert.deepEqual(streamCalls(transcript), [["", "thinking…", ""]], "the message exists before the model has said anything");
  assert.equal(transcript.messages[0].state, "streaming");

  // Many small pieces inside one interval are one call.
  for (const piece of ["Mak", "ing ", "the ", "plate ", "thicker"]) reply.sink.text(piece);
  await new Promise((r) => setTimeout(r, 60));
  reply.sink.activity("editing the script…");
  await new Promise((r) => setTimeout(r, 60));
  reply.sink.activity("");
  reply.sink.text(".");
  await reply.end();

  transcript = await client.request("cadpole/fake/transcript", {});
  assert.deepEqual(streamCalls(transcript), [
    ["", "thinking…", ""],
    ["Making the plate thicker", "thinking…", ""],
    ["", "editing the script…", ""],
    [".", "", "REPLY_FINISH_DONE"],
  ]);
  assert.deepEqual(transcript.messages.map((m) => [m.state, m.text, m.activity]), [["done", "Making the plate thicker.", ""]]);
  assert.equal(transcript.calls.filter((c) => c.name === "reply").length, 0, "one message, not two");
});

test("a throttled piece is sent again with the next one, in order", async (t) => {
  const client = await connect({ env: { CADPOLE_FAKE_THROTTLE: "2" } });
  t.after(() => client.stop());
  const reply = new ReplyStream({ cadpole: client, flushMs: 10 });
  await reply.begin();
  for (const piece of ["one ", "two ", "three ", "four ", "five"]) {
    reply.sink.text(piece);
    await new Promise((r) => setTimeout(r, 25));
  }
  await reply.end();
  const transcript = await client.request("cadpole/fake/transcript", {});
  assert.equal(transcript.messages[0].text, "one two three four five", "nothing lost and nothing doubled");
  assert.equal(transcript.messages[0].state, "done");
});

test("while nothing arrives, the reply in progress is kept alive", async (t) => {
  const client = await connect();
  t.after(() => client.stop());
  const reply = new ReplyStream({ cadpole: client, flushMs: 10, keepAliveMs: 40 });
  await reply.begin();
  reply.sink.activity("rendering…");
  await new Promise((r) => setTimeout(r, 200));
  await reply.end("it broke");
  const calls = streamCalls(await client.request("cadpole/fake/transcript", {}));
  const keepAlives = calls.filter(([append, activity, finish]) => append === "" && activity === "rendering…" && finish === "");
  // One call for the activity line, then one per keep-alive period.
  assert.ok(keepAlives.length >= 3 && keepAlives.length <= 7, JSON.stringify(calls));
  assert.deepEqual(calls.at(-1), ["it broke", "", "REPLY_FINISH_FAILED"]);
});

test("a reply that reaches the size limit is cut and says so", async (t) => {
  const client = await connect();
  t.after(() => client.stop());
  const reply = new ReplyStream({ cadpole: client, flushMs: 10, maxBytes: 12 });
  await reply.begin();
  reply.sink.text("0123456789");
  await new Promise((r) => setTimeout(r, 30));
  reply.sink.text("abcdé and much more");
  await reply.end();
  const transcript = await client.request("cadpole/fake/transcript", {});
  assert.equal(transcript.messages[0].text, "0123456789ab\n\n(The reply was cut short: it reached the size limit for one message.)");
});

test("a server without stream_reply gets the reply whole", async (t) => {
  const client = await connect({ env: { CADPOLE_FAKE_NO_STREAM: "1" } });
  t.after(() => client.stop());
  /** @type {string[]} */
  const logged = [];
  const reply = new ReplyStream({ cadpole: client, flushMs: 10, log: (l) => logged.push(l) });
  await reply.begin();
  reply.sink.text("All of it");
  reply.sink.text(" at once.");
  await reply.end();
  const transcript = await client.request("cadpole/fake/transcript", {});
  assert.deepEqual(transcript.messages.map((m) => m.text), ["All of it at once."]);
  assert.ok(logged.some((l) => /posted whole/.test(l)), logged.join("\n"));
});

test("the message /new starts a new conversation without a model turn", async (t) => {
  const client = await connect({ events: [message(8, " /new "), message(9, "after")] });
  t.after(() => client.stop());
  /** @type {string[]} */
  const did = [];
  const listener = new Listener({
    cadpole: client,
    client: "listener:test",
    pollMs: 10,
    driver: {
      ask: async () => (did.push("ask"), []),
      reset: async () => void did.push("reset"),
      stop: async () => {},
    },
  });
  await listener.resume();
  await listener.step();
  assert.deepEqual(did, ["reset", "ask"]);
  const transcript = await client.request("cadpole/fake/transcript", {});
  assert.match(transcript.messages[0].text, /Started a new conversation/);
});

test("a selection with a note produces a turn and a reply in the session", async (t) => {
  const client = await connect({ events: [SELECTION_WITH_NOTE, BARE_SELECTION] });
  t.after(() => client.stop());
  /** @type {string[]} */
  const prompts = [];
  const listener = new Listener({
    cadpole: client,
    client: "listener:test",
    pollMs: 10,
    // Stands in for the model: read the selection, edit the script, reply.
    // Exactly the shape the system prompt asks for.
    driver: scriptedDriver({
      call: (n, a) => client.callTool(n, a),
      async turn(prompt, cadpole, sink) {
        prompts.push(prompt);
        const sel = await cadpole.call("get_selection", {});
        const session = await cadpole.call("get_session", {});
        await cadpole.call("edit_script", {
          path: "main.star",
          base_revision: session.revision,
          edits: [{ old_str: 'param("w", 40)', new_str: 'param("w", 60)' }],
        });
        const text = `Widened ${sel.selected[0].topo_name} to 60 mm.`;
        sink.text(text);
        return [text];
      },
    }),
  });

  await listener.resume();
  const acted = await listener.step();

  assert.equal(acted.length, 1, "only the noted selection was acted on");
  assert.equal(acted[0].seq, SELECTION_WITH_NOTE.seq);
  assert.equal(prompts.length, 1);
  assert.match(prompts[0], /this wall is too thin/);

  const transcript = await client.request("cadpole/fake/transcript", {});
  assert.equal(transcript.messages.length, 1, "the reply reached the session");
  assert.match(transcript.messages[0].text, /blk\/top/);
  assert.equal(transcript.messages[0].from, "assistant");
  assert.equal(transcript.messages[0].state, "done");
  // The cursor moved past both events, server side, so a restart does not
  // replay them.
  assert.equal(transcript.cursor, BARE_SELECTION.seq);
  assert.equal(listener.cursor, BARE_SELECTION.seq);
});

test("with Listen off, nothing is acted on", async (t) => {
  const client = await connect({ events: [USER_MESSAGE], listening: false });
  t.after(() => client.stop());
  let turns = 0;
  const listener = new Listener({
    cadpole: client,
    client: "listener:test",
    pollMs: 10,
    driver: scriptedDriver({
      call: (n, a) => client.callTool(n, a),
      async turn() {
        turns += 1;
        return [];
      },
    }),
  });
  await listener.resume();
  assert.equal(listener.listening, false);
  const acted = await listener.step();
  assert.equal(acted.length, 0);
  assert.equal(turns, 0, "the UI's Listen toggle is the human's switch, and it is off");
  // The cursor still advances, so turning Listen on later does not replay
  // everything that happened while it was off.
  assert.equal(listener.cursor, USER_MESSAGE.seq);
});

test("the UI turning Listen on mid-stream is obeyed from that event onwards", async (t) => {
  const client = await connect({
    events: [
      { seq: 12, kind: "message", data: { from: "user", text: "before" } },
      { seq: 13, kind: "assistant_listening", data: { listening: true } },
      { seq: 14, kind: "message", data: { from: "user", text: "after" } },
    ],
    listening: false,
  });
  t.after(() => client.stop());
  /** @type {string[]} */
  const seen = [];
  const listener = new Listener({
    cadpole: client,
    client: "listener:test",
    pollMs: 10,
    driver: scriptedDriver({
      call: (n, a) => client.callTool(n, a),
      async turn(prompt) {
        seen.push(/"text": "([^"]+)"/.exec(prompt)?.[1] ?? "?");
        return [];
      },
    }),
  });
  await listener.resume();
  await listener.step();
  assert.deepEqual(seen, ["after"], "the message before the toggle is not retroactively acted on");
});

test("a turn that throws does not stop the listener", async (t) => {
  const client = await connect({ events: [USER_MESSAGE] });
  t.after(() => client.stop());
  /** @type {string[]} */
  const logged = [];
  const listener = new Listener({
    cadpole: client,
    client: "listener:test",
    pollMs: 10,
    log: (s) => logged.push(s),
    driver: scriptedDriver({
      call: (n, a) => client.callTool(n, a),
      async turn() {
        throw new Error("the model fell over");
      },
    }),
  });
  await listener.resume();
  const acted = await listener.step();
  assert.equal(acted.length, 1);
  assert.equal(listener.stopped, false);
  assert.ok(logged.some((l) => l.includes("the model fell over")), logged.join("\n"));
});

// The toggle an event is judged by is the one it was sent under, whenever
// the listener happened to attach (spec/wakeups.qnt,
// `everyHandOverWasSentWhileListening` and `nothingActionableIsSkipped`).
test("a backlog is judged by the toggle each event was sent under, not by the toggle now", async (t) => {
  const backlog = async (/** @type {boolean} */ now) => {
    const client = await connect({
      events: [
        { seq: 12, kind: "message", data: { from: "user", text: "before the change" } },
        // As the server writes it: the new value, and the one before.
        { seq: 13, kind: "assistant_listening", data: { listening: now, was: !now } },
        { seq: 14, kind: "message", data: { from: "user", text: "after the change" } },
      ],
      listening: now,
    });
    t.after(() => client.stop());
    /** @type {string[]} */
    const seen = [];
    const listener = new Listener({
      cadpole: client,
      client: "listener:test",
      pollMs: 10,
      driver: scriptedDriver({
        call: (n, a) => client.callTool(n, a),
        async turn(prompt) {
          seen.push(/"text": "([^"]+)"/.exec(prompt)?.[1] ?? "?");
          return [];
        },
      }),
    });
    await listener.resume();
    assert.equal(listener.listening, now, "what the session says now");
    await listener.step();
    return seen;
  };
  assert.deepEqual(await backlog(true), ["after the change"], "typed while Listen was off: turning it on later does not hand it over");
  assert.deepEqual(await backlog(false), ["before the change"], "typed while Listen was on: turning it off later does not lose it");
});

// Stopped in the middle of a turn, with a second message already in the
// batch: no turn is started for the second (carrying on with the batch would
// start one in a new process while the listener is shutting down), and
// neither is recorded as read, so the next run answers both (spec/listener.qnt, `nothingStartsAfterAStop`
// and `nothingIsDroppedSilently`).
test("stopping in the middle of a turn starts nothing more and leaves the batch for the next run", async (t) => {
  const client = await connect({ events: [message(8, "a"), message(9, "b")] });
  t.after(() => client.stop());
  /** @type {string[]} */
  const asked = [];
  /** @type {(e: Error) => void} */
  let interrupt = () => {};
  /** @type {() => void} */
  let started = () => {};
  const inFlight = new Promise((r) => (started = () => r(undefined)));
  const listener = new Listener({
    cadpole: client,
    client: "listener:test",
    pollMs: 10,
    driver: {
      ask: (prompt) =>
        new Promise((_, reject) => {
          asked.push(/"text": "([^"]+)"/.exec(prompt)?.[1] ?? "?");
          interrupt = reject;
          started();
        }),
      // What the real driver does to the turn in flight when it is stopped.
      stop: async () => interrupt(new Error("the listener is stopping")),
    },
  });
  await listener.resume();
  const step = listener.step();
  await inFlight;
  await listener.stop();
  const acted = await step;
  assert.deepEqual(asked, ["a"], "no turn is started once the listener is stopping");
  assert.equal(acted.length, 0);
  const transcript = await client.request("cadpole/fake/transcript", {});
  assert.deepEqual(transcript.messages, [], "and the interruption is not reported as a failed turn");
  assert.equal(transcript.cursor, 7, "the server's cursor is still in front of both messages");
});

// The position is recorded after every event acted on, so a listener that
// dies during the second turn of a batch repeats that turn and not the first
// (spec/wakeups.qnt, `handedOverAtMostOnceUnlessInterrupted`).
test("the cursor is recorded after each event acted on, not once per batch", async (t) => {
  const client = await connect({ events: [message(8, "a"), message(9, "b")] });
  t.after(() => client.stop());
  /** @type {() => void} */
  let second = () => {};
  const reachedSecond = new Promise((r) => (second = () => r(undefined)));
  let n = 0;
  const listener = new Listener({
    cadpole: client,
    client: "listener:test",
    pollMs: 10,
    driver: {
      ask: () => {
        n += 1;
        if (n === 1) return Promise.resolve(["done"]);
        second();
        // The process dies here: this turn never ends.
        return new Promise(() => {});
      },
      stop: async () => {},
    },
  });
  await listener.resume();
  void listener.step();
  await reachedSecond;
  const transcript = await client.request("cadpole/fake/transcript", {});
  assert.equal(transcript.cursor, 8, "the first message is behind the cursor while the second is being answered");
});
