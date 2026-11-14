// The projection and the overlay, on a camera small enough to do by hand.
import assert from "node:assert/strict";
import { test } from "node:test";

import { cameraOf, composeSvg, formatLength, overlay, project, validateAnnotations } from "./annotate.mjs";

// An eye at (0, -10, 0) looking at the origin, z up, a 90 degree lens, a
// 1000 x 1000 picture: the view's rows are right, up and minus forward.
const SIDECAR = {
  width: 1000,
  height: 1000,
  units: "mm",
  frames: [
    {
      eye: [0, -10, 0],
      target: [0, 0, 0],
      view: [1, 0, 0, 0, 0, 0, 1, 0, 0, -1, 0, -10, 0, 0, 0, 1],
      projection: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, -1.002, -0.2, 0, 0, -1, 0],
    },
  ],
};

test("a model point lands where the camera puts it", () => {
  const cam = cameraOf(SIDECAR);
  const centre = project(cam, [0, 0, 0]);
  assert.deepEqual([centre.x, centre.y, centre.behind], [500, 500, false]);
  // One millimetre right and up, ten away: a tenth of the half-frame.
  const p = project(cam, [1, 0, 1]);
  assert.ok(Math.abs(p.x - 550) < 1e-9 && Math.abs(p.y - 450) < 1e-9, JSON.stringify(p));
  // Nearer the eye the same offset is larger on the picture.
  const near = project(cam, [1, -5, 0]);
  assert.ok(Math.abs(near.x - 600) < 1e-9);
});

test("a point behind the camera says so", () => {
  assert.equal(project(cameraOf(SIDECAR), [0, -20, 0]).behind, true);
});

test("a sidecar without matrices is refused", () => {
  assert.throws(() => cameraOf({ width: 1, height: 1, frames: [{}] }), /view and projection/);
  assert.throws(() => cameraOf({ width: 1, height: 1, frames: [] }, 0), /no frame 0/);
});

test("an arrow is drawn from its start to its tip", () => {
  const { svg, warnings } = overlay(cameraOf(SIDECAR), [{ type: "arrow", from: [-2, 0, 0], to: [2, 0, 0], text: "push" }]);
  assert.deepEqual(warnings, []);
  assert.match(svg, /<line x1="400" y1="500"/);
  assert.match(svg, /<polygon points="600,500 /);
  assert.match(svg, />push</);
});

test("a label keeps its text on the picture", () => {
  const { svg } = overlay(cameraOf(SIDECAR), [{ type: "label", at: [9, 0, 0], text: "a long label near the right edge", offset: [200, 0], anchor: "start" }]);
  const x = Number(/<text x="([0-9.]+)"/.exec(svg)[1]);
  assert.ok(x + 0.6 * 20 * "a long label near the right edge".length <= 1000, `text starts at ${x}`);
});

test("a point off the picture is a warning, not an error", () => {
  const { warnings } = overlay(cameraOf(SIDECAR), [{ type: "dot", at: [50, 0, 0], text: "1" }]);
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /off the picture/);
});

test("every kind draws, and text is escaped", () => {
  const { svg } = overlay(cameraOf(SIDECAR), [
    { type: "dim", from: [-3, 0, -3], to: [3, 0, -3], text: "6 mm" },
    { type: "note", text: "a < b & c" },
    { type: "outline", points: [[0, 0, 0], [1, 0, 0], [1, 0, 1]] },
    { type: "dot", at: [0, 0, 2], text: "2" },
    { type: "label", at: [0, 0, 0], text: "centre", out: 100 },
  ]);
  assert.match(svg, /a &lt; b &amp; c/);
  assert.match(svg, /fill-opacity="0.45"/);
  assert.match(svg, />6 mm</);
});

test("a malformed annotation names itself", () => {
  assert.throws(() => validateAnnotations([{ type: "arrow", from: [0, 0, 0] }]), /annotation 0 \(arrow\): "to" is missing/);
  assert.throws(() => validateAnnotations([{ type: "bubble" }]), /"type" is one of/);
  assert.throws(() => validateAnnotations([{ type: "label", at: [0, 0], text: "x" }]), /model point \[x, y, z\]/);
});

test("the composed file holds the picture under the overlay", () => {
  const svg = composeSvg({ width: 40, height: 30 }, "data:image/png;base64,AAAA", "<circle/>");
  assert.match(svg, /^<svg [^>]*viewBox="0 0 40 30"/);
  assert.ok(svg.indexOf("<image") < svg.indexOf("<circle/>"));
});

test("lengths read the way a ruler does", () => {
  assert.equal(formatLength(25.4, "in"), "1 in");
  assert.equal(formatLength(19.05, "in"), "3/4 in");
  assert.equal(formatLength(171.45, "in", 8), "6 3/4 in");
  assert.equal(formatLength(12.34, "mm"), "12.3 mm");
});
