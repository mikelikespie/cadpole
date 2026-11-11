// The viewer's ground grid and bounding box, built by the real Viewer methods
// on a stand-in `this` (no canvas, no renderer): which lines the grid draws,
// and that a rebuild releases what the last build allocated.

import assert from "node:assert/strict";
import { test } from "node:test";
import { Viewer } from "../src/viewer.js";

const buildGrid = Viewer.prototype["buildGrid"];
const rebuildBBox = Viewer.prototype["rebuildBBox"];

function gridHost(unit = "mm") {
  return {
    scene: { add() {}, remove() {} },
    unit,
    grid: null,
    gridHalf: 0,
    gridStrength: 1,
    gridVisible: true,
  };
}

/** The grid's lines parallel to Y, by x, with their colour; the axis line excluded. */
function verticals(grid, half) {
  const pos = grid.geometry.getAttribute("position").array;
  const col = grid.geometry.getAttribute("color").array;
  const out = new Map();
  const segments = pos.length / 6;
  // The last two segments are the X and Y axis lines.
  for (let s = 0; s < segments - 2; s++) {
    const [x0, y0, , x1, y1] = pos.slice(s * 6, s * 6 + 5);
    if (x0 === x1 && y0 === -half && y1 === half) out.set(x0, col.slice(s * 6, s * 6 + 3).join(","));
  }
  return out;
}

test("the grid's lines sit on multiples of the spacing from the origin, with major lines, for every half-width", () => {
  for (const half of [1000, 1050, 1150, 1200, 2350, 7950]) {
    const host = gridHost();
    buildGrid.call(host, half);
    const lines = verticals(host.grid, half);
    const minor = half <= 1000 ? 50 : 100;
    const major = minor * 5;
    for (const x of lines.keys()) assert.equal(Math.abs(x % minor), 0, `half ${half}: a line at x=${x} is off the ${minor} grid`);
    const n = Math.floor(half / minor);
    assert.equal(lines.size, 2 * n, `half ${half}: every multiple of ${minor} but 0 has a line`);
    const majorColour = lines.get(major);
    assert.ok(majorColour !== undefined, `half ${half}: a line at x=${major}`);
    assert.notEqual(majorColour, lines.get(minor), `half ${half}: x=${major} is drawn as a major line`);
    assert.equal(lines.get(-major), majorColour);
  }
});

test("rebuilding the grid disposes the old material as well as the old geometry", () => {
  const host = gridHost();
  buildGrid.call(host, 1000);
  const old = host.grid;
  let geometry = false;
  let material = false;
  old.geometry.addEventListener("dispose", () => (geometry = true));
  old.material.addEventListener("dispose", () => (material = true));
  buildGrid.call(host, 1200);
  assert.ok(geometry, "geometry disposed");
  assert.ok(material, "material disposed");
});

test("rebuilding the bounding box disposes the old material as well as the old geometry", (t) => {
  const saved = globalThis.document;
  globalThis.document = { createElement: () => ({ remove() {}, className: "", textContent: "" }) };
  t.after(() => {
    globalThis.document = saved;
  });
  const host = {
    scene: { add() {}, remove() {} },
    bboxLines: null,
    dimLabels: [],
    bboxVisible: true,
    views: [{}],
    sceneBox: { min: { x: 0, y: 0, z: 0 }, max: { x: 10, y: 20, z: 30 } },
    labelsEl: { appendChild() {} },
    displayUnit: "mm",
  };
  rebuildBBox.call(host);
  const old = host.bboxLines;
  let material = false;
  old.material.addEventListener("dispose", () => (material = true));
  rebuildBBox.call(host);
  assert.ok(material, "material disposed");
  // Hidden: the lines go, and so does their material.
  const last = host.bboxLines;
  let lastMaterial = false;
  last.material.addEventListener("dispose", () => (lastMaterial = true));
  host.bboxVisible = false;
  rebuildBBox.call(host);
  assert.equal(host.bboxLines, null);
  assert.ok(lastMaterial, "material disposed when the box is hidden");
});
