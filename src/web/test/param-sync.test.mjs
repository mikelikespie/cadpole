// The page's parameter overrides against the session's, when a write fails.
//
// The page refuses to let a session read overwrite an edit the server has not
// acknowledged yet. If that write is *refused* (a network blip, a restart, a
// non-2xx), the edit is never going to be acknowledged: holding on to the
// "unacknowledged" mark from then on means every later change another writer
// makes (an assistant's `SetParams`, `cadpole op`) is ignored by the page,
// which goes on evaluating its own stale set -- and the next successful edit,
// which replaces, deletes the other writer's values.

import assert from "node:assert/strict";
import { test } from "node:test";
import { ParamSync } from "../src/param-sync.js";

const session = (overrides, revision = 1) => ({
  id: "s",
  revision,
  lastEventSeq: 0,
  webBuild: "",
  document: { id: "d", name: "d", display_unit: "mm", scripts: [], param_overrides: overrides },
  selection: { selected: [], hovered: null, note: "" },
  messages: [],
  clients: 1,
});

/** A page holding `overrides`, and a server holding `server.overrides`. */
function page(initial) {
  const server = { overrides: { ...initial }, revision: 1, refuse: 0 };
  const shown = { overrides: { ...initial } };
  const reported = [];
  const deps = {
    current: () => shown.overrides,
    put: async (o) => {
      if (server.refuse > 0) {
        server.refuse -= 1;
        throw new TypeError("Failed to fetch");
      }
      server.overrides = { ...o };
      server.revision += 1;
      return session(server.overrides, server.revision);
    },
    read: async () => session(server.overrides, server.revision),
    apply: (s) => {
      // What applySession does with the overrides.
      if (!sync.dirty) shown.overrides = s.document.param_overrides;
    },
    report: (err) => reported.push(err),
  };
  const sync = new ParamSync(deps);
  /** A `params_changed` from another writer: the page reads the session. */
  const refresh = async () => deps.apply(await deps.read());
  const edit = (name, value) => {
    sync.edited();
    shown.overrides = { ...shown.overrides, [name]: value };
  };
  return { server, shown, sync, edit, refresh, reported };
}

test("after a refused write the page follows the session again", async () => {
  const p = page({ thickness: 6 });
  p.server.refuse = 1;
  p.edit("width", 70);
  await p.sync.send();
  assert.equal(p.reported.length, 1, "the failure was not reported");

  // An assistant changes a parameter; the page re-reads the session.
  p.server.overrides = { ...p.server.overrides, thickness: 9 };
  p.server.revision += 1;
  await p.refresh();
  assert.equal(p.shown.overrides.thickness, 9, "the page ignored the server's thickness after a failed write");

  // The user's next edit does not delete the assistant's value.
  p.edit("depth", 33);
  await p.sync.send();
  assert.equal(p.server.overrides.thickness, 9, "the next edit deleted the other writer's override");
  assert.equal(p.server.overrides.depth, 33);
});

test("a refused write leaves the page showing what the session holds", async () => {
  const p = page({ thickness: 6 });
  p.server.refuse = 1;
  p.edit("width", 70);
  await p.sync.send();
  assert.equal(p.sync.dirty, false);
  assert.deepEqual(p.shown.overrides, { thickness: 6 }, "the page kept a value the session never took");
});

test("a newer edit made while a write was failing is still the page's", async () => {
  const p = page({ thickness: 6 });
  p.server.refuse = 1;
  p.edit("width", 70);
  const sending = p.sync.send();
  p.edit("width", 71);
  await sending;
  assert.equal(p.sync.dirty, true, "an edit nobody has sent yet lost its protection");
  assert.equal(p.shown.overrides.width, 71);
});

test("a successful write clears the mark and adopts the answer", async () => {
  const p = page({});
  p.edit("width", 70);
  await p.sync.send();
  assert.equal(p.sync.dirty, false);
  assert.deepEqual(p.server.overrides, { width: 70 });
  assert.deepEqual(p.shown.overrides, { width: 70 });
});
