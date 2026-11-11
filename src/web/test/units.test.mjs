// The display convention in the browser: `fmtLen` writes the same
// string `cadpole inspect` and the drawings write for the same millimetres, so a
// size reads the same in the viewport, the inspector and the sheet.

import assert from "node:assert/strict";
import { test } from "node:test";
import { fmtArea, fmtFtIn, fmtLen, fmtParamLength, isLengthUnit, parseLen } from "../src/util.js";
import { displayUnits, lengthUnit, model } from "../src/normalize.js";

test("feet and inches read as an architect writes them", () => {
  assert.equal(fmtFtIn(3810), `12'-6"`);
  assert.equal(fmtFtIn(45 * 304.8 + 6 * 25.4), `45'-6"`);
  assert.equal(fmtFtIn(152.4), `6"`);
  assert.equal(fmtFtIn(3810 + 12.7), `12'-6 1/2"`);
  // 7 3/16" at sixteenths, 7 1/4" at quarters, 7" at halves.
  const odd = 25.4 * (7 + 3 / 16);
  assert.equal(fmtFtIn(odd, 16), `7 3/16"`);
  assert.equal(fmtFtIn(odd, 4), `7 1/4"`);
  assert.equal(fmtFtIn(odd, 2), `7"`);
  assert.equal(fmtFtIn(odd, 0), `7.19"`);
  // Rounding up carries into the next foot, and a sign survives.
  assert.equal(fmtFtIn(25.4 * (11 + 63 / 64)), `1'-0"`);
  assert.equal(fmtFtIn(-3810), `-12'-6"`);
  assert.equal(fmtFtIn(0), `0"`);
});

test("fmtLen routes every unit, ft-in included", () => {
  assert.equal(fmtLen(3810, "mm"), "3810 mm");
  assert.equal(fmtLen(3810, "m"), "3.81 m");
  assert.equal(fmtLen(3810, "ft-in"), `12'-6"`);
  assert.equal(fmtLen(3810, "ft-in", 8), `12'-6"`);
  assert.equal(fmtArea(304.8 * 304.8 * 1240, "ft-in"), "1240 sq ft");
  assert.equal(fmtArea(1e4, "mm"), "10000 mm²");
  assert.ok(isLengthUnit("ft-in"));
  assert.ok(isLengthUnit("mm"));
  assert.ok(!isLengthUnit("deg"));
});

test("a length parameter reads in the document's units, in its own otherwise", () => {
  // The value is in the unit the param declared: ten feet, sixty-six inches.
  const ftin = { unit: "ft-in", fraction: 16 };
  assert.equal(fmtParamLength(10, "ft", ftin), `10'-0"`);
  assert.equal(fmtParamLength(45.5, "ft", ftin), `45'-6"`);
  assert.equal(fmtParamLength(66, "in", ftin), `5'-6"`);
  assert.equal(fmtParamLength(152.4, "mm", ftin), `6"`);
  assert.equal(fmtParamLength(45.5, "ft-in", ftin), `45'-6"`);
  assert.equal(fmtParamLength(3810, "mm", { unit: "m", fraction: 0 }), "3.81 m");
  assert.equal(fmtParamLength(10, "ft", { unit: "mm", fraction: 0 }), "3048 mm");
  // No declaration: the number and its unit, as before.
  assert.equal(fmtParamLength(10, "ft", undefined), "10 ft");
  assert.equal(fmtParamLength(6, "in", undefined), "6 in");
  assert.equal(fmtParamLength(4, "", undefined), "4");
  // Not a length: never converted.
  assert.equal(fmtParamLength(30, "deg", ftin), "30 deg");
  assert.equal(fmtParamLength(50, "%", ftin), "50 %");
});

test("the model carries the script's units, or nothing", () => {
  assert.equal(lengthUnit("ft-in"), "ft-in");
  assert.equal(lengthUnit("ft_in"), "ft-in");
  assert.equal(lengthUnit("inch"), "in");
  assert.equal(lengthUnit("LENGTH_UNIT_FT"), "ft");
  assert.deepEqual(displayUnits({ unit: "ft-in", fraction: 16 }), { unit: "ft-in", fraction: 16 });
  assert.equal(displayUnits(undefined), undefined);
  assert.equal(displayUnits({}), undefined);
  const withUnits = model({ params: [], features: [], bodies: [], diagnostics: [], eval_millis: 1, display_units: { unit: "ft-in", fraction: 8 } });
  assert.deepEqual(withUnits.display_units, { unit: "ft-in", fraction: 8 });
  const without = model({ params: [], features: [], bodies: [], diagnostics: [], eval_millis: 1 });
  assert.equal(without.display_units, undefined);
});

test("a typed length reads back as the readout wrote it", () => {
  const close = (a, b, tol = 1e-6) => assert.ok(a !== null && Math.abs(a - b) < tol, `${a} vs ${b}`);
  close(parseLen(`12'-6"`, "ft-in"), 3810);
  close(parseLen(`12' 6"`, "ft-in"), 3810);
  close(parseLen(`12'6 1/2"`, "ft-in"), 3810 + 12.7);
  close(parseLen(`4'`, "mm"), 1219.2);
  close(parseLen(`6"`, "mm"), 152.4);
  close(parseLen(`-2'-3"`, "ft-in"), -(2 * 304.8 + 3 * 25.4));
  close(parseLen("4.5", "ft-in"), 4.5 * 304.8);
  close(parseLen("1200", "mm"), 1200);
  close(parseLen("1.2 m", "ft-in"), 1200);
  close(parseLen("54in", "mm"), 54 * 25.4);
  // What fmtLen writes, parseLen reads, to the sixteenth it was rounded to.
  close(parseLen(fmtLen(3000, "ft-in"), "ft-in"), 3000, 25.4 / 32);
  close(parseLen(fmtLen(1234, "m"), "m"), 1234);
  assert.equal(parseLen("tall", "mm"), null);
  assert.equal(parseLen("", "mm"), null);
});
