// The viewport's drawing buffer (drawing-buffer.ts): what size it is, and
// that a change of size is never left undrawn.

import assert from "node:assert/strict";
import { test } from "node:test";
import { DrawingBuffer, bufferSizeFor } from "../src/drawing-buffer.js";

/** A canvas that is blank from a resize until the next draw, and a log of both. */
function canvas() {
  const log = [];
  let blank = true;
  let size = null;
  const buffer = new DrawingBuffer({
    resize(s) {
      size = s;
      blank = true;
      log.push(`resize ${s.width}x${s.height}@${s.ratio}`);
    },
    draw() {
      assert.notEqual(size, null, "drawn before it had a size");
      blank = false;
      log.push("draw");
    },
  });
  return { buffer, log, blank: () => blank, size: () => size };
}

test("the buffer is the viewport's box in whole CSS pixels at the screen's ratio, two at most", () => {
  assert.deepEqual(bufferSizeFor(390.6, 844.2, 3, false), { width: 390, height: 844, ratio: 2 });
  assert.deepEqual(bufferSizeFor(1280, 800, 1.5, false), { width: 1280, height: 800, ratio: 1.5 });
  assert.deepEqual(bufferSizeFor(1280, 800, 1, false), { width: 1280, height: 800, ratio: 1 });
  // A ratio that is missing or nonsense is one.
  assert.deepEqual(bufferSizeFor(100, 50, 0, false), { width: 100, height: 50, ratio: 1 });
  assert.deepEqual(bufferSizeFor(100, 50, NaN, false), { width: 100, height: 50, ratio: 1 });
});

test("the lighter picture is one device pixel a CSS pixel, and never more than the screen has", () => {
  assert.deepEqual(bufferSizeFor(390, 844, 3, true), { width: 390, height: 844, ratio: 1 });
  assert.deepEqual(bufferSizeFor(390, 844, 0.75, true), { width: 390, height: 844, ratio: 0.75 });
});

test("a viewport with no area has no buffer", () => {
  assert.equal(bufferSizeFor(0, 844, 2, false), null);
  assert.equal(bufferSizeFor(390, 0, 2, false), null);
  assert.equal(bufferSizeFor(0.4, 0.4, 2, false), null);
  assert.equal(bufferSizeFor(NaN, 100, 2, false), null);
});

test("every change of size is drawn into before sync returns", () => {
  const c = canvas();
  assert.equal(c.buffer.sync(390, 844, 2, false), true);
  assert.equal(c.blank(), false);
  // A window dragged smaller a few pixels at a time, turned on its side, and back.
  for (const [w, h] of [[384, 840], [378, 836], [372, 832], [844, 390], [390, 844]]) {
    assert.equal(c.buffer.sync(w, h, 2, false), true);
    assert.equal(c.blank(), false, `left blank at ${w}x${h}`);
  }
  // Each resize is followed at once by its draw.
  assert.equal(c.log.length, 12);
  for (let i = 0; i < c.log.length; i += 2) {
    assert.match(c.log[i], /^resize /);
    assert.equal(c.log[i + 1], "draw");
  }
});

test("the same size again neither resizes nor draws", () => {
  const c = canvas();
  c.buffer.sync(390, 844, 2, false);
  const before = c.log.length;
  assert.equal(c.buffer.sync(390, 844, 2, false), false);
  // A fraction of a pixel is not a new size, and neither is a ratio past the most drawn.
  assert.equal(c.buffer.sync(390.4, 844.9, 2, false), false);
  assert.equal(c.buffer.sync(390, 844, 3, false), false);
  assert.equal(c.log.length, before);
});

test("a new pixel ratio, and the lighter picture and back, are each a new buffer that is drawn", () => {
  const c = canvas();
  c.buffer.sync(390, 844, 2, false);
  assert.equal(c.buffer.sync(390, 844, 1, false), true);
  assert.deepEqual(c.size(), { width: 390, height: 844, ratio: 1 });
  assert.equal(c.buffer.sync(390, 844, 2, false), true);
  assert.equal(c.buffer.sync(390, 844, 2, true), true);
  assert.deepEqual(c.size(), { width: 390, height: 844, ratio: 1 });
  assert.equal(c.blank(), false);
  assert.equal(c.buffer.sync(390, 844, 2, false), true);
  assert.deepEqual(c.size(), { width: 390, height: 844, ratio: 2 });
  assert.equal(c.blank(), false);
});

test("a viewport that loses its area keeps its buffer, and gets a drawn one when it has area again", () => {
  const c = canvas();
  c.buffer.sync(390, 844, 2, false);
  const before = c.log.length;
  assert.equal(c.buffer.sync(0, 0, 2, false), false);
  assert.equal(c.buffer.sync(390, 0, 2, false), false);
  assert.equal(c.log.length, before);
  assert.deepEqual(c.buffer.size, { width: 390, height: 844, ratio: 2 });
  // Back at the size it had: nothing to do. Back at another: resized and drawn.
  assert.equal(c.buffer.sync(390, 844, 2, false), false);
  assert.equal(c.buffer.sync(390, 600, 2, false), true);
  assert.equal(c.blank(), false);
});

test("after a reset the same size is set and drawn again", () => {
  const c = canvas();
  c.buffer.sync(390, 844, 2, false);
  c.buffer.reset();
  assert.equal(c.buffer.size, null);
  assert.equal(c.buffer.sync(390, 844, 2, false), true);
  assert.deepEqual(c.log, ["resize 390x844@2", "draw", "resize 390x844@2", "draw"]);
});
