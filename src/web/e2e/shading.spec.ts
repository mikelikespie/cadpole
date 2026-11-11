// The viewport's lighting, held to numbers. A model once opened nearly black
// here -- metals lit by nothing, since only a reflection lights a metal and
// there was nothing to reflect -- and no test noticed, because every picture
// the suite compared was a picture of the same mistake.
//
// playwright.config.ts publishes e2e/shading.star once per sample (a dark
// metal, a white matte part, a painted one, a dark matte one) and has
// `cadpole render` draw each from the same side on a green key. This reads
// the luma of the model's own pixels in both and asserts three things: every
// surface is readable (the median and the darkest tenth sit inside a band),
// nothing is blown out, and the viewport stays near the renderer's picture.
// The bands are wide on purpose: a GPU and a software rasterizer differ by a
// few levels, a dark model by a hundred.

import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { SHADING_DIR, SHADING_FILM, SHADING_REFERENCE, SHADING_SAMPLES, publishDir } from "./playwright.config.js";

type Sample = (typeof SHADING_SAMPLES)[number];
interface Luma {
  /** How many pixels were the model's. */
  n: number;
  p10: number;
  p50: number;
  p90: number;
  /** The share of them with a channel at the top of its range. */
  clipped: number;
}

/**
 * Luma (Rec. 709 on the encoded values, 0..255) of a picture's model pixels.
 * With `other`, the model is where the two pictures differ; without, it is
 * everything that is not the green key.
 */
async function luma(page: Page, shot: Buffer, other: Buffer | null): Promise<Luma> {
  return page.evaluate(
    async ([a, b]) => {
      const load = async (d: string): Promise<Uint8ClampedArray> => {
        const img = new Image();
        img.src = `data:image/png;base64,${d}`;
        await img.decode();
        const c = new OffscreenCanvas(img.width, img.height);
        const g = c.getContext("2d");
        if (!g) throw new Error("no 2d context");
        g.drawImage(img, 0, 0);
        return g.getImageData(0, 0, img.width, img.height).data;
      };
      const A = await load(a as string);
      const B = b ? await load(b) : null;
      const l: number[] = [];
      let clipped = 0;
      for (let i = 0; i < A.length; i += 4) {
        const [r, g, bl] = [A[i] ?? 0, A[i + 1] ?? 0, A[i + 2] ?? 0];
        if (B) {
          if (Math.abs(r - (B[i] ?? 0)) + Math.abs(g - (B[i + 1] ?? 0)) + Math.abs(bl - (B[i + 2] ?? 0)) < 40) continue;
        } else if (g > 200 && r < 80 && bl < 80) continue;
        l.push(0.2126 * r + 0.7152 * g + 0.0722 * bl);
        if (Math.max(r, g, bl) >= 253) clipped++;
      }
      l.sort((x, y) => x - y);
      const q = (p: number): number => Math.round(l[Math.min(l.length - 1, Math.floor(p * l.length))] ?? -1);
      return { n: l.length, p10: q(0.1), p50: q(0.5), p90: q(0.9), clipped: clipped / Math.max(1, l.length) };
    },
    [shot.toString("base64"), other ? other.toString("base64") : null] as const,
  );
}

/** Wait until the viewer has stopped drawing: rendering is on demand. */
async function settled(page: Page): Promise<void> {
  const frames = (): Promise<number> => page.evaluate(() => Number(document.documentElement.dataset["frames"] ?? 0));
  let last = await frames();
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    await page.waitForTimeout(500);
    const now = await frames();
    if (now === last) return;
    last = now;
  }
}

/** The viewport's picture of a sample in a display mode, from a named view. */
async function picture(page: Page, sample: Sample | typeof SHADING_FILM, mode: string, view = "iso"): Promise<Buffer> {
  await page.goto(`/${SHADING_DIR}/${sample}/index.html?mode=${mode}&view=${view}`);
  await expect(page.locator("html")).toHaveClass(/ready/);
  await expect(page.locator("html")).toHaveAttribute("data-evals", "1");
  await expect(page.locator("html")).toHaveAttribute("data-view-mode", mode);
  await page.mouse.move(2, 2);
  await settled(page);
  return page.locator("canvas.viewport-canvas").screenshot();
}

/** The viewport's luma for a sample: its shaded picture, where that differs from the per-face colours. */
async function viewportLuma(page: Page, sample: Sample, view = "iso"): Promise<Luma> {
  const shaded = await picture(page, sample, "shaded", view);
  const faces = await picture(page, sample, "faces", view);
  return luma(page, shaded, faces);
}

/**
 * What each sample must look like: the median and the darkest tenth of its
 * pixels. The floors are what "readable" means -- under them a surface is a
 * silhouette -- and the ceilings are what keeps a dark part dark and a white
 * one short of paper.
 */
const BANDS: Record<Sample, { p50: [number, number]; p10: [number, number] }> = {
  dark_metal: { p50: [45, 150], p10: [18, 110] },
  white_matte: { p50: [170, 250], p10: [110, 235] },
  painted: { p50: [70, 175], p10: [35, 140] },
  dark_matte: { p50: [32, 110], p10: [9, 80] },
};

test.describe("the viewport's lighting", () => {
  test.describe.configure({ timeout: 120_000 });

  for (const sample of SHADING_SAMPLES) {
    test(`${sample}: readable, not clipped, and near the renderer's picture`, async ({ page }) => {
      const got = await viewportLuma(page, sample);
      const want = await luma(page, readFileSync(join(publishDir, SHADING_DIR, sample, SHADING_REFERENCE)), null);
      const said = `viewport ${JSON.stringify(got)}, renderer ${JSON.stringify(want)}`;
      // In the log on every run: the next person to move a light wants the numbers.
      console.log(`shading ${sample}: ${said}`);
      expect(got.n, `the model is on screen: ${said}`).toBeGreaterThan(5000);
      expect(want.n, `the renderer drew it: ${said}`).toBeGreaterThan(5000);
      const band = BANDS[sample];
      expect(got.p50, `median luma: ${said}`).toBeGreaterThanOrEqual(band.p50[0]);
      expect(got.p50, `median luma: ${said}`).toBeLessThanOrEqual(band.p50[1]);
      expect(got.p10, `the darkest tenth: ${said}`).toBeGreaterThanOrEqual(band.p10[0]);
      expect(got.p10, `the darkest tenth: ${said}`).toBeLessThanOrEqual(band.p10[1]);
      expect(got.clipped, `pixels at the top of the range: ${said}`).toBeLessThan(0.05);
      // Form reads: the lit side and the shaded side are different greys.
      expect(got.p90 - got.p10, `spread between light and shade: ${said}`).toBeGreaterThanOrEqual(12);
      // And the renderer's picture of the same body is the same kind of grey.
      expect(Math.abs(got.p50 - want.p50), `median against the renderer: ${said}`).toBeLessThanOrEqual(55);
      expect(Math.abs(got.p10 - want.p10), `darkest tenth against the renderer: ${said}`).toBeLessThanOrEqual(60);
    });
  }

  test("a sleeve a hair off a faceted post is drawn whole from across the room", async ({ page }) => {
    // The sleeve is the only light thing in the picture, so its pixels bound
    // a box, and inside that box (seen square on, a rectangle) everything is
    // sleeve: none of the dark post under it shows through. Faces carry no
    // slope-scaled depth offset for this reason; the edges are pulled
    // forward instead. On the software rasteriser the suite runs on, this
    // sample is drawn whole either way, so this guards the picture, not that
    // choice: the embossed mug that shows it needs wrapped artwork, which is too
    // slow to build here.
    const shot = await picture(page, SHADING_FILM, "shaded", "front");
    const band = await page.evaluate(async (d) => {
      const img = new Image();
      img.src = `data:image/png;base64,${d}`;
      await img.decode();
      const c = new OffscreenCanvas(img.width, img.height);
      const g = c.getContext("2d");
      if (!g) throw new Error("no 2d context");
      g.drawImage(img, 0, 0);
      const [w, h] = [img.width, img.height];
      const px = g.getImageData(0, 0, w, h).data;
      const light = (x: number, y: number): boolean => {
        const i = (y * w + x) * 4;
        return 0.2126 * (px[i] ?? 0) + 0.7152 * (px[i + 1] ?? 0) + 0.0722 * (px[i + 2] ?? 0) > 120;
      };
      let [x0, x1, y0, y1] = [w, 0, h, 0];
      // The middle of the picture only: the axis gizmo in the corner is light too.
      for (let y = Math.round(h * 0.2); y < h * 0.8; y++) {
        for (let x = Math.round(w * 0.25); x < w * 0.75; x++) {
          if (!light(x, y)) continue;
          [x0, x1, y0, y1] = [Math.min(x0, x), Math.max(x1, x), Math.min(y0, y), Math.max(y1, y)];
        }
      }
      let [n, lit] = [0, 0];
      for (let y = y0 + 2; y <= y1 - 2; y++) {
        for (let x = x0 + 2; x <= x1 - 2; x++) {
          n++;
          if (light(x, y)) lit++;
        }
      }
      return { box: [x0, x1, y0, y1], pixels: n, lit };
    }, shot.toString("base64"));
    expect(band.pixels, JSON.stringify(band)).toBeGreaterThan(1000);
    expect(band.lit / band.pixels, JSON.stringify(band)).toBeGreaterThan(0.995);
  });

  test("the lights follow the camera: the back of a model is lit like its front", async ({ page }) => {
    const front = await viewportLuma(page, "dark_metal", "front");
    const back = await viewportLuma(page, "dark_metal", "back");
    const said = `front ${JSON.stringify(front)}, back ${JSON.stringify(back)}`;
    expect(Math.abs(front.p50 - back.p50), said).toBeLessThanOrEqual(25);
    expect(back.p10, said).toBeGreaterThanOrEqual(BANDS.dark_metal.p10[0]);
  });
});

// A cut face is the body's own colour, flat: moved a quarter of the way
// toward a pale grey and drawn unlit, so its pixels are one known colour
// each. Its outline is a hairline in a darker shade of that colour, which
// is what parts two bodies of one colour where the cut crosses both.
// (`cutColors` in src/section.ts; the numbers are written out again here so
// that a change to the style has to be made twice.)

/**
 * The colour a body's cut face has on screen, 0..255 a channel: the body's
 * colour a quarter of the way toward a pale grey, then what the viewport
 * does to every pixel -- its exposure and the neutral tone curve.
 */
function cutFill(hex: string): [number, number, number] {
  const fill = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => v + (0.88 - v) * 0.25);
  let c = fill.map((v) => (v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)) * 1.3);
  const low = Math.min(...c);
  const offset = low < 0.08 ? low - 6.25 * low * low : 0.04;
  c = c.map((v) => v - offset);
  const peak = Math.max(...c);
  if (peak >= 0.76) {
    const d = 1 - 0.76;
    const newPeak = 1 - (d * d) / (peak + d - 0.76);
    const g = 1 - 1 / (0.4 * (peak - newPeak) + 1);
    c = c.map((v) => (v * newPeak) / peak).map((v) => v + (newPeak - v) * g);
  }
  return c.map((v) => Math.round((v <= 0.0031308 ? v * 12.92 : 1.055 * Math.pow(v, 1 / 2.4) - 0.055) * 255)) as [number, number, number];
}

/** A sample cut through its axis and seen square on, as pixels. */
async function cutPicture(page: Page, sample: Sample, mode: string): Promise<{ w: number; h: number; px: number[] }> {
  await page.goto(`/${SHADING_DIR}/${sample}/index.html?mode=${mode}&view=front&section=-y=0`);
  await expect(page.locator("html")).toHaveClass(/ready/);
  await expect(page.locator("html")).toHaveAttribute("data-section", "-y=0");
  await expect.poll(async () => Number(await page.locator("html").getAttribute("data-section-tris"))).toBeGreaterThan(0);
  await page.mouse.move(2, 2);
  await settled(page);
  const shot = await page.locator("canvas.viewport-canvas").screenshot();
  return page.evaluate(async (d) => {
    const img = new Image();
    img.src = `data:image/png;base64,${d}`;
    await img.decode();
    const c = new OffscreenCanvas(img.width, img.height);
    const g = c.getContext("2d");
    if (!g) throw new Error("no 2d context");
    g.drawImage(img, 0, 0);
    return { w: img.width, h: img.height, px: Array.from(g.getImageData(0, 0, img.width, img.height).data) };
  }, shot.toString("base64"));
}

const near = (px: number[], i: number, c: readonly number[], tol: number): boolean =>
  Math.abs((px[i] ?? 0) - (c[0] as number)) <= tol && Math.abs((px[i + 1] ?? 0) - (c[1] as number)) <= tol && Math.abs((px[i + 2] ?? 0) - (c[2] as number)) <= tol;

test.describe("a section's cut faces", () => {
  test.describe.configure({ timeout: 120_000 });

  for (const mode of ["shaded", "xray"]) {
    test(`painted, ${mode}: each cut face is its body's colour, lightened and flat`, async ({ page }) => {
      const { w, h, px } = await cutPicture(page, "painted", mode);
      const drum = cutFill("#c8402f");
      const boss = cutFill("#2f6fc8");
      let [drums, bosses] = [0, 0];
      for (let i = 0; i < px.length; i += 4) {
        if (near(px, i, drum, 6)) drums++;
        else if (near(px, i, boss, 6)) bosses++;
      }
      const said = `drum ${drum.join(",")}: ${drums} px, boss ${boss.join(",")}: ${bosses} px of ${w}x${h}`;
      // Flat: thousands of pixels of exactly the one colour, which no lit surface gives.
      expect(drums, said).toBeGreaterThan(5000);
      expect(bosses, said).toBeGreaterThan(800);
      // The middle of the picture is the drum's core, cut: its colour and not a dark fill.
      const mid = (Math.round(h / 2) * w + Math.round(w / 2) + 12) * 4;
      expect(near(px, mid, drum, 6), `the middle pixel is ${px.slice(mid, mid + 3).join(",")}; ${said}`).toBe(true);
    });
  }

  test("white on white: a thin darker outline parts two bodies of one colour", async ({ page }) => {
    const { w, h, px } = await cutPicture(page, "white_matte", "shaded");
    const fill = cutFill("#f2f2f0");
    const luma = (i: number): number => 0.2126 * (px[i] ?? 0) + 0.7152 * (px[i + 1] ?? 0) + 0.0722 * (px[i + 2] ?? 0);
    const fillLuma = 0.2126 * fill[0] + 0.7152 * fill[1] + 0.0722 * fill[2];
    // A column a little off the axis: down it the boss's cut face, the
    // outlines where the boss and the drum overlap, and the drum's.
    const x = Math.round(w / 2) + 12;
    const column = Array.from({ length: h }, (_, y) => (y * w + x) * 4);
    const isFill = column.map((i) => near(px, i, fill, 6));
    const first = isFill.indexOf(true);
    const last = isFill.lastIndexOf(true);
    expect(last - first, "the cut face spans the column").toBeGreaterThan(100);
    // The runs of other pixels inside the face: each an outline.
    const runs: { at: number; length: number; darkest: number }[] = [];
    for (let y = first; y <= last; y++) {
      if (isFill[y]) continue;
      const run = { at: y, length: 0, darkest: 255 };
      for (; y <= last && !isFill[y]; y++) {
        run.length++;
        run.darkest = Math.min(run.darkest, luma(column[y] as number));
      }
      runs.push(run);
    }
    const said = `fill ${fill.join(",")}, outlines ${JSON.stringify(runs)}`;
    expect(runs.length, said).toBeGreaterThanOrEqual(2);
    for (const run of runs) {
      // A hairline, at any device pixel ratio the suite runs at ...
      expect(run.length, said).toBeLessThanOrEqual(3);
      // ... that shows against the face, in a shade of it rather than black.
      // (A hairline between two pixel rows is shared by them, each part face.)
      expect(run.darkest, said).toBeLessThan(fillLuma - 15);
      expect(run.darkest, said).toBeGreaterThan(fillLuma * 0.5 - 30);
    }
  });
});
