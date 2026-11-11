// Screenshots queued for a message (the strip above the message box).
//
// Sending posts what is queued and takes it off the queue. The post is a
// network request carrying PNGs, and the toolbar's Snapshot button stays live
// while it is in flight, so the queue can grow during the await -- and a
// snapshot added then was neither sent nor kept: the reset after the post
// emptied the whole queue.

import assert from "node:assert/strict";
import { test } from "node:test";
import { Store, postWithQueued } from "../src/state.js";

const shot = (name) => ({ name, mime: "image/png", data: name });

test("a snapshot queued while the message is being posted stays queued", async () => {
  const store = new Store({ pendingAttachments: [shot("one")] });
  let release;
  const sent = [];
  const posting = postWithQueued(store, (atts) => {
    sent.push(...(atts ?? []));
    return new Promise((r) => (release = r));
  });
  // The user takes another snapshot before the POST returns.
  store.set({ pendingAttachments: [...store.get().pendingAttachments, shot("two")] });
  release("ok");
  assert.equal(await posting, "ok");
  assert.deepEqual(sent.map((a) => a.name), ["one"]);
  assert.deepEqual(store.get().pendingAttachments.map((a) => a.name), ["two"], "the second snapshot was lost");
});

test("a failed post keeps the queue", async () => {
  const store = new Store({ pendingAttachments: [shot("one")] });
  await assert.rejects(postWithQueued(store, () => Promise.reject(new Error("offline"))));
  assert.deepEqual(store.get().pendingAttachments.map((a) => a.name), ["one"]);
});

test("a message with nothing queued posts no attachments", async () => {
  const store = new Store({ pendingAttachments: [] });
  let got = "unset";
  await postWithQueued(store, async (atts) => (got = atts));
  assert.equal(got, undefined);
});
