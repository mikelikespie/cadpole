// The guide builder against a stand-in renderer: what it asks of the
// renderer, what it keeps between runs, and what it writes.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { buildGuide, pageHtml, renderArgs, validateGuide } from "./guide.mjs";

const FAKE = path.join(path.dirname(fileURLToPath(import.meta.url)), "fake_cadpole_render.mjs");

function scratch(guide) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "illustrate-"));
  fs.writeFileSync(path.join(dir, "toy.star"), "box(1, 1, 1)\n");
  const guidePath = path.join(dir, "toy.guide.json");
  fs.writeFileSync(guidePath, JSON.stringify(guide));
  const log = path.join(dir, "calls.log");
  process.env.FAKE_CADPOLE_LOG = log;
  const calls = () => (fs.existsSync(log) ? fs.readFileSync(log, "utf8").trim().split("\n").filter(Boolean).map((l) => JSON.parse(l)) : []);
  const opts = { guidePath, outDir: path.join(dir, "out"), cadpole: process.execPath, extraArgs: [FAKE] };
  return { dir, opts, calls };
}

const GUIDE = {
  title: "A toy: putting it together",
  model: "toy.star",
  params: { size: "2" },
  intro: ["Two parts."],
  parts: [{ name: "part_lid", key: "A", label: "Lid", count: "1", layers: { base: "off" } }],
  steps: [
    {
      name: "lid_on",
      title: "Put the lid on",
      text: "It drops straight down.",
      params: { explode: "20" },
      layers: { lid: 0.4 },
      camera: { target: [0, 0, 0], direction: [0, -1, 0], distance: 10, fov: 30 },
      annotations: [{ type: "arrow", from: [0, 0, 3], to: [0, 0, 1], text: "down" }],
    },
    { name: "done", title: "Done", camera: { view: "az=30,el=20" } },
  ],
  sections: [{ title: "Care", text: ["Keep it dry."] }],
};

test("a guide is checked before anything is rendered", () => {
  assert.throws(() => validateGuide({ title: "x", model: "m.star", steps: [] }), /at least one step/);
  assert.throws(() => validateGuide({ ...GUIDE, steps: [{ name: "Bad Name", title: "t" }] }), /lower-case/);
  assert.throws(() => validateGuide({ ...GUIDE, steps: [GUIDE.steps[0], { ...GUIDE.steps[0] }] }), /used twice/);
  assert.throws(() => validateGuide({ ...GUIDE, steps: [{ name: "a", title: "t", camera: { view: "top", eye: [0, 0, 1], target: [0, 0, 0] } }] }), /exactly one of/);
  assert.throws(() => validateGuide({ ...GUIDE, steps: [{ name: "a", title: "t", layers: { lid: 2 } }] }), /opacity from 0 to 1/);
  assert.throws(() => validateGuide({ ...GUIDE, steps: [{ name: "a", title: "t", annotations: [{ type: "arrow" }] }] }), /steps\[0\]: annotation 0/);
});

test("a shot's request carries its parameters, layers and camera", () => {
  const args = renderArgs(GUIDE, GUIDE.steps[0], { model: "/m/toy.star", png: "/o/a.png", camera: "/o/a.json" });
  const s = args.join(" ");
  assert.match(s, /^render \/m\/toy\.star -p size=2 -p explode=20 --layer lid=0\.4 --size 1500x1000 --eye=0,-10,0 --target=0,0,0 --fov 30 /);
  assert.match(s, /-o \/o\/a\.png --camera-json \/o\/a\.json$/);
  const part = renderArgs(GUIDE, GUIDE.parts[0], { model: "m", png: "p", camera: "c" }).join(" ");
  assert.match(part, /--layer base=off --size 640x440 --view az=215,el=38/);
});

test("a guide builds: pictures, a page, a manifest", () => {
  const { opts, calls } = scratch(GUIDE);
  const r = buildGuide(GUIDE, opts);
  assert.equal(r.ok, true);
  assert.deepEqual(r.renders.map((x) => x.status), ["rendered", "rendered", "rendered"]);
  assert.equal(calls().length, 3);
  const svg = fs.readFileSync(path.join(opts.outDir, "img", "lid_on.svg"), "utf8");
  assert.match(svg, /<image href="data:image\/png;base64,/);
  assert.match(svg, />down</);
  const page = fs.readFileSync(path.join(opts.outDir, "index.html"), "utf8");
  assert.match(page, /<span class="num">1<\/span>Put the lid on/);
  assert.match(page, /img\/lid_on\.svg/);
  assert.match(page, /A&nbsp; Lid/);
  assert.match(page, /Keep it dry\./);
  const manifest = JSON.parse(fs.readFileSync(path.join(opts.outDir, "guide-manifest.json"), "utf8"));
  assert.deepEqual(manifest.steps.map((s) => [s.number, s.name, s.image]), [[1, "lid_on", "img/lid_on.svg"], [2, "done", "img/done.svg"]]);
  assert.equal(manifest.pdf, null);
});

test("an unchanged shot is not rendered again; a changed one is", () => {
  const { opts, calls } = scratch(GUIDE);
  buildGuide(GUIDE, opts);
  const again = buildGuide(GUIDE, opts);
  assert.deepEqual(again.renders.map((x) => x.status), ["kept", "kept", "kept"]);
  assert.equal(calls().length, 3);
  // Moving a label renders nothing; moving the camera renders that shot.
  const moved = structuredClone(GUIDE);
  moved.steps[0].annotations[0].text = "straight down";
  assert.deepEqual(buildGuide(moved, opts).renders.map((x) => x.status), ["kept", "kept", "kept"]);
  assert.match(fs.readFileSync(path.join(opts.outDir, "img", "lid_on.svg"), "utf8"), />straight down</);
  moved.steps[1].camera = { view: "az=60,el=20" };
  assert.deepEqual(buildGuide(moved, opts).renders.map((x) => x.status), ["kept", "kept", "rendered"]);
  // A change to the model renders everything.
  fs.appendFileSync(path.join(path.dirname(opts.guidePath), "toy.star"), "# changed\n");
  assert.deepEqual(buildGuide(moved, opts).renders.map((x) => x.status), ["rendered", "rendered", "rendered"]);
});

test("a shot that fails is reported and the guide is not called whole", () => {
  const broken = structuredClone(GUIDE);
  broken.steps[1].params = { fail: "1" };
  const { opts } = scratch(broken);
  const r = buildGuide(broken, opts);
  assert.equal(r.ok, false);
  const bad = r.renders.find((x) => x.status === "failed");
  assert.equal(bad.name, "done");
  assert.match(bad.detail, /no such body/);
  assert.deepEqual(r.missing, ["done"]);
});

test("the page escapes what the guide says", () => {
  const g = { ...GUIDE, title: "Nuts & <bolts>" };
  assert.match(pageHtml(g, (n) => `img/${n}.svg`), /<h1>Nuts &amp; &lt;bolts&gt;<\/h1>/);
});
