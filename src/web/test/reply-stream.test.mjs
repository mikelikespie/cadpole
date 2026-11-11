// A reply in progress, as the page folds it into the message list
// (src/reply-stream.ts): frames, the finished message, and session reads,
// in whatever order they arrive.

import assert from "node:assert/strict";
import { test } from "node:test";
import { applyFrame, mergeSnapshot, upsertMessage } from "../src/reply-stream.js";

const frame = (offset, append, activity = "") => ({ seq: 5, at: "t", offset, append, activity });
const user = { seq: 4, from: "user", text: "make it thicker", at: "t0" };

test("frames start a message and append to it", () => {
  let { messages, resync } = applyFrame([user], frame(0, "", "thinking…"));
  assert.equal(resync, false);
  assert.deepEqual(messages[1], { seq: 5, from: "assistant", text: "", at: "t", state: "streaming", activity: "thinking…" });
  ({ messages, resync } = applyFrame(messages, frame(0, "Makíng ")));
  ({ messages, resync } = applyFrame(messages, frame(8, "it thicker", "rendering…")));
  assert.equal(resync, false);
  assert.equal(messages[1].text, "Makíng it thicker", "offsets are bytes of UTF-8, and í is two");
  assert.equal(messages[1].activity, "rendering…");
  ({ messages } = applyFrame(messages, frame(18, "")));
  assert.equal("activity" in messages[1], false, "an empty activity clears the line");
  assert.equal(messages[0], user, "other messages are untouched");
});

test("a frame that does not continue what is held asks for a session read", () => {
  // The first frame the page sees is not the first of the message.
  let r = applyFrame([user], frame(7, "thicker"));
  assert.deepEqual([r.resync, r.messages.length], [true, 1]);
  // A frame was missed in the middle.
  const held = applyFrame([user], frame(0, "Making")).messages;
  r = applyFrame(held, frame(9, "thicker"));
  assert.equal(r.resync, true);
  assert.equal(r.messages, held, "and nothing is changed on a guess");
});

test("a frame the page is already past changes only the activity", () => {
  const held = [{ seq: 5, from: "assistant", text: "Making it thicker", at: "t", state: "streaming" }];
  let r = applyFrame(held, frame(7, "it thicker"));
  assert.equal(r.messages, held, "the same text is not appended twice");
  r = applyFrame(held, frame(7, "it thicker", "rendering…"));
  assert.equal(r.messages[0].text, "Making it thicker");
  assert.equal(r.messages[0].activity, "rendering…");
});

test("a message that has ended is not reopened", () => {
  const done = { seq: 5, from: "assistant", text: "Making it thicker.", at: "t", state: "done" };
  assert.equal(applyFrame([done], frame(18, " And more")).messages[0], done, "a late frame");
  assert.deepEqual(upsertMessage([done], { ...done, text: "Making", state: "streaming" }), [done], "a stale copy");
  assert.deepEqual(mergeSnapshot([done], [{ ...done, text: "Making", state: "streaming" }]), [done], "a stale session read");
  const whole = { seq: 6, from: "assistant", text: "posted whole", at: "t" };
  assert.equal(applyFrame([whole], { ...frame(12, "!"), seq: 6 }).messages[0], whole, "nor is a message that was never streamed");
});

test("the finished message replaces the copy in progress", () => {
  const streaming = { seq: 5, from: "assistant", text: "Making it", at: "t", state: "streaming", activity: "rendering…" };
  const done = { seq: 5, from: "assistant", text: "Making it thicker.", at: "t", state: "done" };
  assert.deepEqual(upsertMessage([user, streaming], done), [user, done]);
  assert.deepEqual(upsertMessage([user], done), [user, done], "also when no frame was ever seen");
  const again = [user, done];
  assert.equal(upsertMessage(again, { ...done }), again, "the same message twice is one message");
});

test("a posted message keeps whichever copy has the pictures", () => {
  const withPicture = { ...user, attachments: [{ kind: "screenshot", png_base64: "AAAA" }] };
  assert.deepEqual(upsertMessage([withPicture], user), [withPicture]);
  assert.deepEqual(upsertMessage([user], withPicture), [withPicture]);
  // The same number with other text is another message (a restarted server).
  assert.equal(upsertMessage([user], { ...user, text: "something else" }).length, 2);
});

test("a session read never takes a reply in progress backwards", () => {
  const ahead = { seq: 5, from: "assistant", text: "Making it thicker", at: "t", state: "streaming" };
  const behind = { seq: 5, from: "assistant", text: "Making", at: "t", state: "streaming" };
  assert.deepEqual(mergeSnapshot([user, ahead], [user, behind]), [user, ahead]);
  assert.deepEqual(mergeSnapshot([user, behind], [user, ahead]), [user, ahead], "and a newer read moves it on");
  const done = { ...ahead, text: "Making it thicker.", state: "done" };
  assert.deepEqual(mergeSnapshot([user, ahead], [user, done]), [user, done]);
  const withPicture = { ...user, attachments: [{ kind: "screenshot", png_base64: "AAAA" }] };
  assert.deepEqual(mergeSnapshot([withPicture], [user]), [withPicture], "local screenshots are kept");
});
