// Two session writes in flight at once (HttpApi).
//
// The operation registry answers a write with what the write did, not with a
// whole session, so `HttpApi` applies each answer to its cached copy of the
// session and hands the page that. Writes overlap in ordinary use: after a
// "Reset all" that grows the feature tree under a parked pointer, the
// parameters' 300 ms debounce and the selection's 100 ms one expire together,
// and a `SetSelection` is in flight while the reset's `SetParams` is. If the
// selection's answer were built on the copy taken *before* either was sent,
// it would carry the pre-reset overrides back to the page -- which had just
// been told its own write landed -- and the page would evaluate parameters the
// server no longer held.

import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { HttpApi } from "../src/api.js";
import {
  SessionSchema,
  SetParamsRequestSchema,
  SetParamsResponseSchema,
  SetScriptResponseSchema,
  SetSelectionResponseSchema,
} from "../gen/cadpole/v1/ops_pb.js";

const number = (value) => ({ value: { case: "number", value } });

/**
 * A session server whose answers wait until the test releases them, in the
 * order it chooses: `fetch` is the Connect transport's only way out.
 */
function fakeServer() {
  const state = {
    revision: 1,
    overrides: { count: number(4), one: number(2), every: number(2) },
    scripts: [{ path: "main.star", source: "count = param(\"count\", 40)\n" }],
  };
  const calls = [];
  globalThis.fetch = (url, init) =>
    new Promise((resolve) => {
      calls.push({ rpc: String(url).split("/").pop(), body: new Uint8Array(init.body), resolve });
    });
  const reply = (call, schema, message) =>
    call.resolve(new Response(toBinary(schema, create(schema, message)), { status: 200, headers: { "content-type": "application/proto" } }));
  return {
    state,
    /** Wait until `n` calls have been sent. */
    async sent(n) {
      for (let i = 0; i < 100 && calls.length < n; i++) await new Promise((r) => setTimeout(r, 0));
      assert.equal(calls.length, n, `expected ${n} calls, saw ${calls.map((c) => c.rpc).join(", ")}`);
    },
    /** Answer the oldest unanswered call to `rpc`, the way the server would. */
    answer(rpc) {
      const i = calls.findIndex((c) => c.rpc === rpc && !c.answered);
      assert.ok(i >= 0, `no ${rpc} waiting`);
      const call = calls[i];
      call.answered = true;
      switch (rpc) {
        case "GetSession":
          return reply(call, SessionSchema, {
            id: "s",
            revision: BigInt(state.revision),
            document: { id: "d", name: "d", displayUnit: "mm", scripts: state.scripts, paramOverrides: state.overrides },
            selection: { selected: [], note: "" },
          });
        case "SetParams": {
          const req = fromBinary(SetParamsRequestSchema, call.body);
          const next = { ...state.overrides, ...req.overrides };
          for (const k of req.clear) delete next[k];
          state.overrides = next;
          state.revision += 1;
          return reply(call, SetParamsResponseSchema, { revision: BigInt(state.revision), paramOverrides: state.overrides });
        }
        case "SetSelection":
          return reply(call, SetSelectionResponseSchema, { revision: BigInt(state.revision), selected: 0 });
        case "SetScript":
          state.revision += 1;
          return reply(call, SetScriptResponseSchema, { revision: BigInt(state.revision) });
        default:
          throw new Error(`the fake server does not answer ${rpc}`);
      }
    },
  };
}

afterEach(() => {
  delete globalThis.fetch;
});

async function loaded(server) {
  const api = new HttpApi();
  const first = api.getSession();
  await server.sent(1);
  server.answer("GetSession");
  await first;
  return api;
}

test("a selection answered after a parameter write does not bring the old parameters back", async () => {
  const server = fakeServer();
  const api = await loaded(server);

  // "Reset all", and a hover pushed in the same task.
  const params = api.putParams({});
  const selection = api.putSelection({ selected: [], hovered: { feature_id: "pin_4", body_id: "pin_4#4", kind: "body" } });
  await server.sent(3);

  // The server takes them in the order they were sent.
  server.answer("SetParams");
  assert.deepEqual((await params).document.param_overrides, {});
  server.answer("SetSelection");
  const after = await selection;
  assert.deepEqual(after.document.param_overrides, {}, "the selection's answer reinstated the overrides the reset cleared");
  assert.equal(after.selection.hovered?.body_id, "pin_4#4");
});

test("a parameter write answered after a selection keeps the selection", async () => {
  const server = fakeServer();
  const api = await loaded(server);

  const selection = api.putSelection({ selected: [{ feature_id: "pin_0", body_id: "pin_0#0", kind: "body" }] });
  const params = api.putParams({ count: 8 });
  await server.sent(3);
  server.answer("SetSelection");
  await selection;
  server.answer("SetParams");
  const after = await params;
  assert.equal(after.selection.selected.length, 1, "the parameters' answer dropped the selection");
  assert.deepEqual(after.document.param_overrides, { count: 8 });
});

test("a script write answered after a parameter write keeps the parameters", async () => {
  const server = fakeServer();
  const api = await loaded(server);

  const params = api.putParams({});
  const script = api.putScript({ path: "main.star", source: "count = param(\"count\", 8)\n" });
  await server.sent(3);
  server.answer("SetParams");
  await params;
  server.answer("SetScript");
  const after = await script;
  assert.deepEqual(after.document.param_overrides, {});
  assert.equal(after.document.scripts[0]?.source, "count = param(\"count\", 8)\n");
});
