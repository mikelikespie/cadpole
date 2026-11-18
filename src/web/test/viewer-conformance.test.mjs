// The web viewer against the reference every orbiting 3D view keeps
// (src/apple/ViewerCore/viewer_conformance.json, which the Swift views test
// against too): the elevation limits and a flick easing onto them, the
// bed's fade under the horizon, and the wheel rule.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  bedFade,
  bedFadeFromHeight,
  clampElevation,
  coastElevation,
  MAX_ELEVATION,
  MIN_ELEVATION,
  wheelAction,
} from "../src/orbit.js";

const ref = JSON.parse(readFileSync(new URL("./viewer_conformance.json", import.meta.url), "utf8"));
const near = (a, b, tol, what) => assert.ok(Math.abs(a - b) <= tol, `${what}: ${a}, want ${b}`);

test("the elevation limits: 85 degrees under the bed to 89 over it", () => {
  assert.equal(MIN_ELEVATION, ref.elevationLimits.min);
  assert.equal(MAX_ELEVATION, ref.elevationLimits.max);
  for (const [e, want] of ref.clamp) near(clampElevation(e), want, 1e-9, `clamp(${e})`);
});

test("a coasting step eases onto a limit", () => {
  for (const c of ref.coastSteps) {
    const [e, v] = coastElevation(c.elevation, c.velocity, c.dt);
    near(e, c.toElevation, 1e-5, `coast(${c.elevation}, ${c.velocity}) elevation`);
    near(v, c.toVelocity, 1e-5, `coast(${c.elevation}, ${c.velocity}) speed`);
  }
});

test("a flick into a limit never passes it, comes to rest on it, and slows as it nears it", () => {
  for (const c of ref.coastRuns) {
    let e = c.elevation;
    let v = c.velocity;
    const path = [e];
    for (let t = 0; t < c.seconds && v !== 0; t += c.dt) {
      [e, v] = coastElevation(e, v, c.dt);
      v *= Math.exp(-c.dt / c.decaySeconds);
      if (Math.abs(v) < 2) v = 0;
      path.push(e);
    }
    for (const p of path) assert.ok(c.endsAt < 0 ? p >= c.endsAt : p <= c.endsAt, `passed the limit: ${p}`);
    near(path[path.length - 1], c.endsAt, c.within, `run from ${c.elevation} ends`);
    const steps = path.slice(1).map((p, i) => Math.abs(p - path[i]));
    for (const s of steps.slice(-3)) assert.ok(s < c.lastStepUnder, `last steps ${steps.slice(-3)}`);
  }
});

test("the bed fades under the horizon, smoothly", () => {
  for (const c of ref.bedFade) {
    const f = bedFade(c.elevation);
    for (const k of ["above", "fill", "lines", "shadow", "underLight"]) near(f[k], c[k], 1e-5, `${k} at ${c.elevation}`);
  }
  for (const c of ref.bedFadeFromHeight) near(bedFadeFromHeight(c.height, c.distance).above, c.above, 1e-5, `height ${c.height}`);
});

test("the wheel rule", () => {
  for (const c of ref.wheel) {
    assert.equal(wheelAction(c.input, c.option, c.shift, c.inPage), c.action, `${c.input} option ${c.option} shift ${c.shift} page ${c.inPage}`);
  }
});
