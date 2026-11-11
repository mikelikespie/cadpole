// The input router's two pure rules: which wheel events are a notched mouse
// wheel, and when a two-finger twist outweighs the pinch it rides on.

import assert from "node:assert/strict";
import { test } from "node:test";

import { notched, twistEngages } from "../src/input/router.js";

const px = (deltaX, deltaY, extra = {}) => ({ deltaMode: 0, deltaX, deltaY, ...extra });

test("a notched wheel: line mode, whole multiples of 40 px, or 100 px and more, straight down or up", () => {
  assert.equal(notched({ deltaMode: 1, deltaX: 0, deltaY: 3 }), true);
  assert.equal(notched(px(0, 40)), true);
  assert.equal(notched(px(0, -120)), true);
  assert.equal(notched(px(0, 100)), true);
  assert.equal(notched(px(0, 53)), false);
});

test("a trackpad or a finger: fractions, sideways parts, small steps, momentum", () => {
  assert.equal(notched(px(0, 2.5)), false);
  assert.equal(notched(px(-44.9, -55)), false);
  assert.equal(notched(px(0, 12)), false);
  assert.equal(notched(px(-40, 0)), false);
  assert.equal(notched(px(0, 0)), false);
  assert.equal(notched(px(0, 120, { momentum: true })), false);
});

test("a twist engages past 10 degrees, and only while it outweighs the pinch", () => {
  assert.equal(twistEngages(9, 1), false);
  assert.equal(twistEngages(12, 1), true);
  assert.equal(twistEngages(-30, 1), true);
  // A pinch that wobbles: 9 degrees at 1.8x.
  assert.equal(twistEngages(9, 1.8), false);
  // 20 degrees while doubling: ln 2 = 0.69 outweighs 20 / 45 = 0.44.
  assert.equal(twistEngages(20, 2), false);
  assert.equal(twistEngages(40, 1.2), true);
});
