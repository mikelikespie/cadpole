// The marks a user sees before the app has loaded: the splash, the tab's
// favicon and the installed (PWA) icons. They show the product's mark, a drawn
// shape, and none of them is a letter standing in for one.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const html = readFileSync("index.html", "utf8");
const manifest = JSON.parse(readFileSync("manifest.webmanifest", "utf8"));
const title = html.match(/<title>([^<]*)<\/title>/)?.[1] ?? "";

/** The SVG path data of a `data:image/svg+xml,...` URL. */
const paths = (dataUrl) => [...decodeURIComponent(dataUrl).matchAll(/ d='([^']*)'/g)].map((m) => m[1]);

/** A letter drawn as three strokes: a stem and two diagonals from its midpoint. */
const isStrokedLetter = (d) => /^M\s*[\d.]+\s+[\d.]+\s*v[\d.]+\s*M[\d.\s]+l[-\d.\s]+M[\d.\s]+l[-\d.\s]+$/.test(d.trim());

test("the splash shows the product's mark, not a letter", () => {
  assert.equal(title, "Cadpole");
  const splash = html.match(/<div class="splash">([\s\S]*?)<\/div>\s*<\/div>|<div class="splash">([\s\S]*?)<\/div>/)?.[0] ?? "";
  assert.ok(/class="splash-mark"/.test(splash), `no splash mark: ${splash}`);
  assert.ok(!/>\s*[A-Za-z]\s*</.test(splash), `the splash shows a letter: ${splash}`);
});

test("the favicon and the installed icons are not a stroked letter", () => {
  const icons = [html.match(/<link rel="icon" href="([^"]*)"/)?.[1], ...manifest.icons.map((i) => i.src)];
  assert.equal(icons.length, 3);
  for (const icon of icons) {
    assert.ok(icon, "an icon is missing");
    const ds = paths(icon);
    assert.ok(ds.length > 0, `no glyph in ${icon}`);
    for (const d of ds) assert.ok(!isStrokedLetter(d), `an icon draws a stroked letter: ${d}`);
  }
});
