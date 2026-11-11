// Section planes: the spelling, the slice, the loops and the caps.

import assert from "node:assert/strict";
import { test } from "node:test";

import { capTriangles, cutColors, formatSection, joinLoops, keeps, parseSection, signedArea2, sliceMesh } from "../src/section.js";

/** An axis-aligned box as a closed mesh, its eight corners duplicated per face as an engine mesh does. */
function box(x0, y0, z0, x1, y1, z1) {
  const c = [
    [x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0],
    [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1],
  ];
  const faces = [
    [0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7],
  ];
  const pos = [];
  const idx = [];
  for (const f of faces) {
    const base = pos.length / 3;
    for (const k of f) pos.push(...c[k]);
    idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
  }
  return { positions: new Float32Array(pos), indices: new Uint32Array(idx) };
}

/** Two meshes as one (a frame: a box with a box-shaped hole is two shells here). */
function merge(a, b) {
  const n = a.positions.length / 3;
  return {
    positions: new Float32Array([...a.positions, ...b.positions]),
    indices: new Uint32Array([...a.indices, ...[...b.indices].map((i) => i + n)]),
  };
}

function capArea(tris) {
  let a = 0;
  for (let i = 0; i < tris.length; i += 6) {
    const [ax, ay, bx, by, cx, cy] = tris.slice(i, i + 6);
    a += Math.abs((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)) / 2;
  }
  return a;
}

test("the spelling is cadpole render --section's, one horizontal and one vertical plane", () => {
  assert.deepEqual(parseSection(""), { horizontal: null, vertical: null });
  const s = parseSection("z=2400, -y=3000");
  assert.deepEqual(s.horizontal, { axis: 2, at: 2400, removePositive: true });
  assert.deepEqual(s.vertical, { axis: 1, at: 3000, removePositive: false });
  assert.equal(formatSection(s), "z=2400,-y=3000");
  assert.equal(formatSection(parseSection("x=304.80000000000001")), "x=304.8");
  assert.throws(() => parseSection("z=1,z=2"), /one horizontal/);
  assert.throws(() => parseSection("x=1,y=2"), /one vertical/);
  assert.throws(() => parseSection("w=1"), /axis/);
  assert.throws(() => parseSection("z=up"), /not a number/);
  assert.throws(() => parseSection("z"), /expected/);
});

test("z=40 keeps what is below 40, -z=40 what is above, the plane itself kept", () => {
  const [below] = [parseSection("z=40").horizontal];
  assert.equal(keeps(below, 0, 0, 39), true);
  assert.equal(keeps(below, 0, 0, 40), true);
  assert.equal(keeps(below, 0, 0, 41), false);
  const above = parseSection("-z=40").horizontal;
  assert.equal(keeps(above, 0, 0, 39), false);
  assert.equal(keeps(above, 0, 0, 41), true);
});

test("a closed box cuts into one closed loop and fills to its cross-section", () => {
  const b = box(0, 0, 0, 30, 20, 10);
  const segs = sliceMesh(b.positions, b.indices, null, parseSection("z=4").horizontal);
  const { loops, open } = joinLoops(segs);
  assert.equal(loops.length, 1);
  assert.equal(open.length, 0);
  assert.equal(Math.abs(signedArea2(loops[0])) / 2, 600);
  assert.ok(Math.abs(capArea(capTriangles(loops)) - 600) < 1e-6);
  // A vertical cut: the y-z section, 20 x 10.
  const v = joinLoops(sliceMesh(b.positions, b.indices, null, parseSection("x=12").vertical));
  assert.ok(Math.abs(capArea(capTriangles(v.loops)) - 200) < 1e-6);
});

test("a plane that misses the body, or lies along its face, cuts nothing", () => {
  const b = box(0, 0, 0, 30, 20, 10);
  assert.equal(sliceMesh(b.positions, b.indices, null, parseSection("z=11").horizontal).length, 0);
  // Exactly the top face: a vertex on the plane counts as below, so nothing straddles.
  assert.equal(sliceMesh(b.positions, b.indices, null, parseSection("z=10").horizontal).length, 0);
});

test("a hole is a hole: a frame's cap is the ring, not the whole outline", () => {
  // An outer shell and an inner one, as a wall around a room cuts.
  const outer = box(0, 0, 0, 10, 10, 3);
  const inner = box(2, 2, -1, 8, 8, 4);
  // Reverse the inner shell's winding: it is the hole's surface.
  const flipped = new Uint32Array(inner.indices);
  for (let i = 0; i < flipped.length; i += 3) [flipped[i + 1], flipped[i + 2]] = [flipped[i + 2], flipped[i + 1]];
  const m = merge(outer, { positions: inner.positions, indices: flipped });
  const { loops } = joinLoops(sliceMesh(m.positions, m.indices, null, parseSection("z=1").horizontal));
  assert.equal(loops.length, 2);
  assert.ok(Math.abs(capArea(capTriangles(loops)) - (100 - 36)) < 1e-6);
});

test("an open sheet leaves a chain that is outlined, never filled", () => {
  // One quad standing up in the x-z plane: a terrain fin, not a solid.
  const positions = new Float32Array([0, 0, 0, 10, 0, 0, 10, 0, 5, 0, 0, 5]);
  const indices = new Uint32Array([0, 1, 2, 0, 2, 3]);
  const { loops, open } = joinLoops(sliceMesh(positions, indices, null, parseSection("z=2").horizontal));
  assert.equal(loops.length, 0);
  assert.equal(open.length, 1);
  assert.equal(capTriangles(loops).length, 0);
});

test("a body drawn by a transform is cut where it is drawn", () => {
  const b = box(0, 0, 0, 10, 10, 10);
  // Translate by z = +100 (column-major: translation in 12..14).
  const m = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 100, 1];
  assert.equal(sliceMesh(b.positions, b.indices, null, parseSection("z=105").horizontal).length, 0);
  const { loops } = joinLoops(sliceMesh(b.positions, b.indices, m, parseSection("z=105").horizontal));
  assert.equal(loops.length, 1);
});

test("a cut face is its body's colour moved toward a pale grey, and its outline a shade of that", () => {
  const luma = (c) => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  const colours = { red: [0.78, 0.25, 0.18], steel: [0.42, 0.39, 0.44], white: [0.95, 0.95, 0.94], soot: [0.16, 0.17, 0.18], black: [0, 0, 0] };
  for (const [name, rgb] of Object.entries(colours)) {
    const { fill, line } = cutColors(rgb);
    for (const c of [...fill, ...line]) assert.ok(c >= 0 && c <= 1, name);
    // Lighter than the body, except where the body is already paler than the grey it moves toward.
    if (luma(rgb) < 0.88) assert.ok(luma(fill) > luma(rgb), name);
    else assert.ok(luma(fill) < luma(rgb), name);
    // Close to the body's colour: a quarter of the way and no further.
    rgb.forEach((c, k) => assert.ok(Math.abs(fill[k] - c) <= 0.22 + 1e-9, name));
    // The outline shows against the face.
    assert.ok(Math.abs(luma(line) - luma(fill)) > 0.1, `${name}: line ${line} on fill ${fill}`);
  }
  // Darker on a light face, lighter on a dark one; never the same for two different bodies' hues.
  assert.ok(luma(cutColors(colours.white).line) < luma(cutColors(colours.white).fill));
  assert.ok(luma(cutColors(colours.black).line) > luma(cutColors(colours.black).fill));
  assert.deepEqual(cutColors([0.4, 0.4, 0.4]).fill.map((c) => +c.toFixed(4)), [0.52, 0.52, 0.52]);
});
