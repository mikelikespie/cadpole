// The Parameters panel's pure parts: which choice lists become
// segments, the slider's step, and what one tap on the stepper does.

import assert from "node:assert/strict";
import { test } from "node:test";
import { isSegmented, niceStep, stepped } from "../src/ui/params.js";

test("few short choices are segments; many, or long, are a picker", () => {
  assert.equal(isSegmented(["D", "B"]), true);
  assert.equal(isSegmented(["off", "east", "west"]), true);
  assert.equal(isSegmented(["body", "lid", "off"]), true);
  assert.equal(isSegmented(["auto", "brep", "mesh"]), true);
  assert.equal(isSegmented(["light", "skeletonized", "full"]), true, "twelve characters still fit a segment");
  assert.equal(isSegmented(["none", "corner_magnets_only"]), false, "one label too long for a segment");
  assert.equal(isSegmented(["bambu_a1", "bambu_a1_mini", "bambu_p1s", "bambu_x1c", "prusa_mk4", "ender3"]), false);
  assert.equal(isSegmented(["a", "b", "c", "d", "e"]), false, "five is one too many");
  assert.equal(isSegmented([]), false);
});

test("niceStep is the script's step, else a round hundredth of the range", () => {
  const p = (min, max, step) => ({ min, max, step });
  assert.equal(niceStep(p(0, 100, 2.5)), 2.5);
  assert.equal(niceStep(p(0, 100, null)), 1);
  assert.equal(niceStep(p(0, 10, null)), 0.1);
  assert.equal(niceStep(p(20, 200, null)), 1);
  assert.equal(niceStep(p(0, 250, null)), 2);
  assert.equal(niceStep(p(0, 700, null)), 5);
  assert.equal(niceStep(p(null, null, null)), 1);
});

test("a stepper tap moves to the neighbouring grid point and stays in range", () => {
  const p = { min: 20, max: 200 };
  assert.equal(stepped(60, 1, p, 1), 61);
  assert.equal(stepped(60, -1, p, 1), 59);
  assert.equal(stepped(60.7, 1, p, 1), 61, "a typed value goes to the next point, not over it");
  assert.equal(stepped(60.3, -1, p, 1), 60);
  assert.equal(stepped(200, 1, p, 1), 200, "clamped at max");
  assert.equal(stepped(20, -1, p, 1), 20, "clamped at min");
  assert.equal(stepped(0.3, 1, { min: 0, max: 8 }, 0.1), 0.4, "no float noise");
  assert.equal(stepped(2, 1, { min: 0.5, max: 10 }, 0.5), 2.5, "the grid is anchored at min");
  assert.equal(stepped(5, -1, { min: null, max: null }, 1), 4, "no range at all");
});
