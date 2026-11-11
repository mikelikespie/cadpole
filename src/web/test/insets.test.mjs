// What a panel lying over the viewport covers of its edges (ui/insets.ts).

import assert from "node:assert/strict";
import { test } from "node:test";
import { coveredInsets, sameInsets, NO_INSETS } from "../src/ui/insets.js";

const view = { left: 0, top: 0, right: 390, bottom: 788 };

test("a sheet across the bottom covers the bottom edge as far up as it reaches", () => {
  assert.deepEqual(coveredInsets(view, [{ left: 0, top: 434, right: 390, bottom: 788 }]), { top: 0, right: 0, bottom: 354, left: 0 });
  // Reaching below the viewport (over a tab bar, a keyboard) changes nothing.
  assert.deepEqual(coveredInsets(view, [{ left: 0, top: 434, right: 390, bottom: 900 }]), { top: 0, right: 0, bottom: 354, left: 0 });
});

test("a bar across the top covers the top edge", () => {
  assert.deepEqual(coveredInsets(view, [{ left: 0, top: 0, right: 390, bottom: 60 }]), { top: 60, right: 0, bottom: 0, left: 0 });
});

test("a panel down a side covers that side", () => {
  const wide = { left: 0, top: 0, right: 1000, bottom: 600 };
  assert.deepEqual(coveredInsets(wide, [{ left: 0, top: 0, right: 300, bottom: 600 }]), { top: 0, right: 0, bottom: 0, left: 300 });
  assert.deepEqual(coveredInsets(wide, [{ left: 700, top: 50, right: 1000, bottom: 600 }]), { top: 0, right: 300, bottom: 0, left: 0 });
});

test("a panel beside the viewport, and a card over one corner, cover no edge", () => {
  assert.ok(sameInsets(coveredInsets(view, [{ left: 390, top: 0, right: 700, bottom: 788 }]), NO_INSETS));
  const wide = { left: 0, top: 0, right: 800, bottom: 1000 };
  assert.ok(sameInsets(coveredInsets(wide, [{ left: 12, top: 580, right: 400, bottom: 1000 }]), NO_INSETS));
});

test("several covers add up edge by edge, the deepest winning on each", () => {
  const got = coveredInsets(view, [
    { left: 0, top: 0, right: 390, bottom: 58 },
    { left: 0, top: 560, right: 390, bottom: 788 },
    { left: 0, top: 434, right: 390, bottom: 788 },
  ]);
  assert.deepEqual(got, { top: 58, right: 0, bottom: 354, left: 0 });
});

test("a sheet lifted by a keyboard: the bottom is covered from the sheet's top, whatever shows between them", () => {
  const keyboard = { left: 0, top: 508, right: 390, bottom: 844 };
  const sheet = { left: 0, top: 225, right: 390, bottom: 452 };
  assert.deepEqual(coveredInsets(view, [sheet, keyboard]), { top: 0, right: 0, bottom: 563, left: 0 });
  assert.deepEqual(coveredInsets(view, [keyboard, sheet]), { top: 0, right: 0, bottom: 563, left: 0 });
  // The largest clear stretch is what is kept, wherever it lies.
  assert.deepEqual(coveredInsets(view, [{ left: 0, top: 100, right: 390, bottom: 160 }, keyboard]), { top: 160, right: 0, bottom: 280, left: 0 });
});

test("a viewport with no area has no insets", () => {
  assert.ok(sameInsets(coveredInsets({ left: 0, top: 0, right: 0, bottom: 0 }, [view]), NO_INSETS));
});
