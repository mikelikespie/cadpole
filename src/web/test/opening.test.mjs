// How a page opens: the address over the publisher's choices.

import assert from "node:assert/strict";
import { test } from "node:test";

import { formatPoint, openingFrom, parseMode, parsePoint, safeHomeUrl, viewLink } from "../src/opening.js";

const q = (s) => new URLSearchParams(s);
const published = (over = {}) => ({ camera: "", eye: null, target: null, section: "", mode: "", ...over });

test("a display mode is spelled as the viewer spells it, hyphens forgiven", () => {
  assert.equal(parseMode("xray"), "xray");
  assert.equal(parseMode("X-Ray"), "xray");
  assert.equal(parseMode("shaded-edges"), "shaded_edges");
  assert.equal(parseMode("ghost"), null);
  assert.equal(parseMode(""), null);
});

test("a point is three finite numbers", () => {
  assert.deepEqual(parsePoint("1, -2.5,3e2"), { x: 1, y: -2.5, z: 300 });
  assert.equal(parsePoint("1,2"), null);
  assert.equal(parsePoint("1,2,"), null);
  assert.equal(parsePoint("1,2,x"), null);
  assert.equal(parsePoint("1,2,Infinity"), null);
  assert.equal(formatPoint({ x: 1.04, y: -0.01, z: 2.96 }), "1,0,3");
});

test("a page with nothing asked opens as it always did", () => {
  const o = openingFrom(q(""), null);
  assert.deepEqual(
    { section: o.section, mode: o.mode, preset: o.preset, eye: o.eye, cameraFromAddress: o.cameraFromAddress, modeFromAddress: o.modeFromAddress },
    { section: "", mode: null, preset: null, eye: null, cameraFromAddress: false, modeFromAddress: false },
  );
  assert.deepEqual(o.ignored, []);
});

test("the publisher's choices open the page", () => {
  const o = openingFrom(q(""), published({ camera: "top", section: "z=40", mode: "xray" }));
  assert.equal(o.section, "z=40");
  assert.equal(o.mode, "xray");
  assert.equal(o.preset, "top");
  assert.equal(o.cameraFromAddress, false);
  assert.equal(o.modeFromAddress, false);
  const e = openingFrom(q(""), published({ camera: "top", eye: { x: 1, y: 2, z: 3 }, target: { x: 0, y: 0, z: 0 } }));
  assert.deepEqual(e.eye, { x: 1, y: 2, z: 3 });
  assert.deepEqual(e.target, { x: 0, y: 0, z: 0 });
  assert.equal(e.preset, null, "an eye beats a preset");
});

test("the address overrides each choice on its own", () => {
  const view = published({ camera: "top", section: "z=40", mode: "xray" });
  const m = openingFrom(q("mode=shaded"), view);
  assert.deepEqual([m.mode, m.modeFromAddress, m.section, m.preset], ["shaded", true, "z=40", "top"]);
  const s = openingFrom(q("section=-y=0"), view);
  assert.deepEqual([s.section, s.mode, s.preset], ["-y=0", "xray", "top"]);
  // An empty section in the address is a way to say "uncut".
  assert.equal(openingFrom(q("section="), view).section, "");
  const c = openingFrom(q("eye=10,20,30&target=1,2,3"), view);
  assert.deepEqual([c.eye, c.target, c.preset, c.cameraFromAddress], [{ x: 10, y: 20, z: 30 }, { x: 1, y: 2, z: 3 }, null, true]);
});

test("the address's camera replaces the publisher's whole", () => {
  const view = published({ eye: { x: 1, y: 2, z: 3 }, target: { x: 9, y: 9, z: 9 } });
  const p = openingFrom(q("view=front"), view);
  assert.deepEqual([p.preset, p.eye, p.target], ["front", null, null]);
  const e = openingFrom(q("eye=5,5,5"), view);
  assert.deepEqual([e.eye, e.target], [{ x: 5, y: 5, z: 5 }, null], "not aimed at the publisher's target");
});

test("what does not parse is ignored and named", () => {
  const o = openingFrom(q("mode=ghost&view=sideways&eye=1,2&target=1,2,3"), published({ mode: "xray", camera: "top" }));
  assert.deepEqual([o.mode, o.preset, o.eye, o.target], ["xray", "top", null, null]);
  assert.deepEqual(o.ignored, ["mode=ghost", "view=sideways", "eye=1,2", "target without eye"]);
});

test("a link to a view round-trips through the address", () => {
  const v = { eye: { x: 420.04, y: -380, z: 260 }, target: { x: 0, y: 0, z: 40 }, mode: "xray", section: "-y=0,z=12.5" };
  const href = viewLink("https://example.org/m/?baked=a.bake&view=top&mode=faces#frag", v);
  assert.equal(href, "https://example.org/m/?baked=a.bake&section=-y=0,z=12.5&mode=xray&eye=420,-380,260&target=0,0,40#frag");
  const o = openingFrom(new URL(href).searchParams, null);
  assert.deepEqual([o.section, o.mode, o.eye, o.target], ["-y=0,z=12.5", "xray", { x: 420, y: -380, z: 260 }, { x: 0, y: 0, z: 40 }]);
  const plain = viewLink("https://example.org/m/?mode=xray&section=z=1", { ...v, mode: null, section: "" });
  assert.equal(plain, "https://example.org/m/?eye=420,-380,260&target=0,0,40");
});

test("a home link is http(s) once resolved, or nothing", () => {
  const base = "https://example.org/models/turbojet/";
  assert.equal(safeHomeUrl("../../#turbojet", base), "https://example.org/#turbojet");
  assert.equal(safeHomeUrl("https://cadpole.example/", base), "https://cadpole.example/");
  assert.equal(safeHomeUrl("javascript:alert(1)", base), null);
  assert.equal(safeHomeUrl(" JavaScript:alert(1)", base), null);
  assert.equal(safeHomeUrl("data:text/html,x", base), null);
  assert.equal(safeHomeUrl("", base), null);
});
