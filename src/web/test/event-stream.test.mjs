// The event stream across a `cadpole serve` restart (HttpApi.subscribe).
//
// A restarted server is a new session with a new event log that starts again
// at sequence 1, and with a pinned token (or --no-auth) the tab reconnects to
// it without a 401. The stream's cursor (`after=`) is the previous run's, and
// the real server answers `after=N` with nothing until its own log passes N:
// the page reads "live" while every script, parameter and message event of the
// new run is dropped. The session's `id` is regenerated on every start, so
// the client can tell, and must resume from the new run's own watermark.

import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { create, toBinary } from "@bufbuild/protobuf";
import { HttpApi } from "../src/api.js";
import { SessionSchema } from "../gen/cadpole/v1/ops_pb.js";

globalThis.window ??= globalThis;

const tick = () => new Promise((r) => setTimeout(r, 0));
async function until(cond, what, ms = 5_000) {
  const t0 = Date.now();
  while (!cond()) {
    if (Date.now() - t0 > ms) assert.fail(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 10));
  }
}

/**
 * One run of `cadpole serve`: a random session id, a log from seq 1, and the
 * same filters as `get_events` (backlog `seq > after`, live tail likewise).
 */
function serverRun(id) {
  const log = [];
  const streams = new Set();
  const enc = new TextEncoder();
  const frame = (e) => enc.encode(`id: ${e.seq}\nevent: ${e.kind}\ndata: ${JSON.stringify(e)}\n\n`);
  return {
    id,
    log,
    emit(kind, data) {
      const e = { seq: log.length + 1, at: "", kind, data };
      log.push(e);
      for (const s of streams) if (e.seq > s.after) s.ctl.enqueue(frame(e));
    },
    stop() {
      for (const s of streams) s.ctl.close();
      streams.clear();
    },
    events(after) {
      let s;
      const body = new ReadableStream({
        start(ctl) {
          s = { after, ctl };
          streams.add(s);
          for (const e of log) if (e.seq > after) ctl.enqueue(frame(e));
        },
        cancel() {
          streams.delete(s);
        },
      });
      return new Response(body, { status: 200, headers: { "content-type": "text/event-stream" } });
    },
    session() {
      const msg = create(SessionSchema, {
        id,
        revision: 1n,
        lastEventSeq: BigInt(log.length),
        document: { id: "d", name: "d", displayUnit: "mm", scripts: [], paramOverrides: {} },
        selection: { selected: [], note: "" },
      });
      return new Response(toBinary(SessionSchema, msg), { status: 200, headers: { "content-type": "application/proto" } });
    },
  };
}

/** The port: whichever run is up, or connection refused while none is. */
function port() {
  const p = { run: null, eventsAfter: [] };
  globalThis.fetch = async (url) => {
    const u = String(url);
    if (!p.run) throw new TypeError("fetch failed");
    if (u.startsWith("/api/events")) {
      const after = Number(new URL(u, "http://x").searchParams.get("after"));
      p.eventsAfter.push(after);
      return p.run.events(after);
    }
    if (u.endsWith("/GetSession")) return p.run.session();
    throw new Error(`unexpected request ${u}`);
  };
  return p;
}

let unsubscribe = null;
afterEach(() => {
  unsubscribe?.();
  unsubscribe = null;
  delete globalThis.fetch;
});

test("after a server restart the stream follows the new run's log", async () => {
  const p = port();
  p.run = serverRun("run-1");
  const api = new HttpApi();
  await api.getSession();

  const got = [];
  const statuses = [];
  const restarts = [];
  unsubscribe = api.subscribe(
    0,
    (e) => got.push(e.data?.text ?? e.kind),
    (s) => statuses.push(s),
    undefined,
    (sess) => restarts.push(sess),
  );
  for (let i = 1; i <= 6; i++) p.run.emit("message", { text: `before-${i}` });
  await until(() => got.length === 6, "the first run's events");

  // Restart on the same port: same token, new session, log from 1.
  p.run.stop();
  p.run = serverRun("run-2");
  p.run.emit("client_joined", {});
  await until(() => p.eventsAfter.length === 2 && statuses.at(-1) === "live", "the reconnection");
  p.run.emit("message", { text: "after-restart-1" });
  p.run.emit("message", { text: "after-restart-2" });
  await until(() => got.includes("after-restart-2"), "the new run's events");

  assert.deepEqual(got.slice(6), ["after-restart-1", "after-restart-2"]);
  assert.equal(restarts.length, 1, "the page was not told the server restarted");
  assert.equal(restarts[0].id, "run-2");
  assert.equal(p.eventsAfter[1], 1, "the reconnection resumed from the old run's cursor");
});

test("a reconnection to the same run resumes where it left off", async () => {
  const p = port();
  const run = serverRun("run-1");
  p.run = run;
  const api = new HttpApi();
  await api.getSession();

  const got = [];
  const restarts = [];
  const statuses = [];
  unsubscribe = api.subscribe(0, (e) => got.push(e.seq), (s) => statuses.push(s), undefined, (s) => restarts.push(s));
  run.emit("params_changed", {});
  run.emit("params_changed", {});
  await until(() => got.length === 2, "the first events");

  // The stream drops; the server does not restart.
  run.stop();
  run.emit("params_changed", {});
  await until(() => p.eventsAfter.length === 2 && statuses.at(-1) === "live", "the reconnection");
  await until(() => got.length === 3, "the missed event");
  for (let i = 0; i < 5; i++) await tick();

  assert.deepEqual(got, [1, 2, 3]);
  assert.equal(p.eventsAfter[1], 2);
  assert.equal(restarts.length, 0);
});
