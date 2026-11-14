// An illustrated step-by-step guide from a model: one picture per step,
// rendered by `cadpole render`, annotated through the camera that drew it
// (annotate.mjs), and laid out as a page for a phone and a page for paper.
//
// The guide is a JSON file beside the model (docs/illustrations.md has the
// format). Rendering is the slow part and its results are kept: a shot is
// rendered again only when what it asks of the renderer changed.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { cameraOf, composeSvg, overlay, validateAnnotations } from "./annotate.mjs";

/** The look of a guide picture unless the guide says otherwise. */
export const DEFAULT_RENDER = [
  "--backend", "path", "--samples", "96", "--denoise", "--tonemap", "neutral",
  "--no-grid", "--no-edges", "--lighting", "product", "--backdrop", "white", "--floor", "matte",
];

const NAME = /^[a-z0-9][a-z0-9_-]*$/;

function fail(where, message) {
  throw new Error(`${where}: ${message}`);
}

function checkShot(s, where) {
  if (!s || typeof s !== "object") fail(where, "is an object");
  if (typeof s.name !== "string" || !NAME.test(s.name)) fail(where, `"name" is lower-case letters, digits, - and _, got ${JSON.stringify(s.name)}`);
  if (s.params !== undefined && (typeof s.params !== "object" || Array.isArray(s.params))) fail(where, '"params" is an object of parameter overrides');
  if (s.layers !== undefined && (typeof s.layers !== "object" || Array.isArray(s.layers))) fail(where, '"layers" is an object: a layer name to "on", "off" or an opacity');
  for (const [k, v] of Object.entries(s.layers || {})) {
    if (!(v === "on" || v === "off" || (typeof v === "number" && v >= 0 && v <= 1))) fail(where, `layer "${k}" is "on", "off" or an opacity from 0 to 1, got ${JSON.stringify(v)}`);
  }
  if (s.size !== undefined && !(Array.isArray(s.size) && s.size.length === 2 && s.size.every((v) => Number.isInteger(v) && v >= 16))) fail(where, '"size" is [width, height] in pixels');
  const c = s.camera;
  if (c !== undefined) {
    if (!c || typeof c !== "object") fail(where, '"camera" is an object');
    const forms = [c.view !== undefined, c.eye !== undefined, c.direction !== undefined].filter(Boolean).length;
    if (forms !== 1) fail(where, '"camera" has exactly one of "view" (a preset or "az=..,el=.."), "eye" (with "target") or "direction" (with "target" and "distance")');
    const vec = (k) => {
      if (!Array.isArray(c[k]) || c[k].length !== 3 || c[k].some((v) => typeof v !== "number")) fail(where, `camera "${k}" is [x, y, z]`);
    };
    if (c.eye !== undefined) {
      vec("eye");
      vec("target");
    }
    if (c.direction !== undefined) {
      vec("direction");
      vec("target");
      if (typeof c.distance !== "number" || !(c.distance > 0)) fail(where, 'camera "distance" is a positive number of millimetres');
    }
    if (c.up !== undefined) vec("up");
  }
  try {
    validateAnnotations(s.annotations || []);
  } catch (e) {
    fail(where, e.message);
  }
}

/**
 * Check a guide and return it. Throws an Error naming what is wrong.
 * @param {object} guide the parsed JSON
 */
export function validateGuide(guide) {
  if (!guide || typeof guide !== "object") fail("guide", "is an object");
  if (typeof guide.title !== "string" || !guide.title.trim()) fail("guide", '"title" is missing');
  if (typeof guide.model !== "string" || !guide.model) fail("guide", '"model" is the path of the script, relative to the guide');
  if (guide.units !== undefined && !["mm", "in"].includes(guide.units)) fail("guide", '"units" is "mm" or "in"');
  if (guide.render !== undefined && !(Array.isArray(guide.render) && guide.render.every((a) => typeof a === "string"))) fail("guide", '"render" is a list of `cadpole render` flags');
  if (!Array.isArray(guide.steps) || guide.steps.length === 0) fail("guide", '"steps" is a list with at least one step');
  const seen = new Set();
  const name = (s, where) => {
    checkShot(s, where);
    if (seen.has(s.name)) fail(where, `the name "${s.name}" is used twice`);
    seen.add(s.name);
  };
  (guide.overview || []).forEach((s, i) => name(s, `overview[${i}]`));
  (guide.parts || []).forEach((p, i) => {
    if (typeof p.label !== "string" || !p.label) fail(`parts[${i}]`, '"label" is what the part is called');
    name(p, `parts[${i}]`);
  });
  guide.steps.forEach((s, i) => {
    if (typeof s.title !== "string" || !s.title.trim()) fail(`steps[${i}]`, '"title" is missing');
    name(s, `steps[${i}]`);
  });
  (guide.sections || []).forEach((s, i) => {
    if (typeof s.title !== "string" || !Array.isArray(s.text)) fail(`sections[${i}]`, 'has a "title" and "text", a list of paragraphs');
  });
  return guide;
}

/** Every picture a guide asks for, in page order. */
export function shotsOf(guide) {
  return [...(guide.overview || []), ...(guide.parts || []), ...guide.steps];
}

function sizeOf(guide, shot, isPart) {
  return shot.size || (isPart ? [640, 440] : guide.size || [1500, 1000]);
}

/**
 * The `cadpole render` arguments of one shot (without the binary).
 * @param {object} guide
 * @param {object} shot
 * @param {{model: string, png: string, camera: string}} files absolute paths
 */
export function renderArgs(guide, shot, files) {
  const isPart = (guide.parts || []).includes(shot);
  const args = ["render", files.model];
  const params = { ...(guide.params || {}), ...(shot.params || {}) };
  for (const [k, v] of Object.entries(params)) args.push("-p", `${k}=${v}`);
  for (const [k, v] of Object.entries(shot.layers || {})) args.push("--layer", `${k}=${v}`);
  const [w, h] = sizeOf(guide, shot, isPart);
  args.push("--size", `${w}x${h}`);
  const c = shot.camera || { view: isPart ? "az=215,el=38" : "product" };
  if (c.view !== undefined) {
    args.push("--view", String(c.view));
  } else {
    let eye = c.eye;
    if (!eye) {
      const l = Math.hypot(...c.direction);
      eye = c.target.map((t, k) => t + (c.direction[k] / l) * c.distance);
    }
    const f = (v) => v.map((x) => Number(x.toFixed(3))).join(",");
    args.push(`--eye=${f(eye)}`, `--target=${f(c.target)}`);
    if (c.up) args.push(`--up=${f(c.up)}`);
  }
  if (c.fov !== undefined) args.push("--fov", String(c.fov));
  args.push(...(shot.render || guide.render || DEFAULT_RENDER));
  args.push("-o", files.png, "--camera-json", files.camera);
  return args;
}

function digest(args, modelBytes) {
  const h = createHash("sha256");
  h.update(JSON.stringify(args.filter((a) => !path.isAbsolute(a))));
  h.update(modelBytes);
  return h.digest("hex");
}

/**
 * Render the guide's pictures into `outDir/raw`, skipping every shot whose
 * request and model are unchanged since the picture there was made.
 * @returns {Array<{name: string, status: "rendered"|"kept"|"failed", detail?: string}>}
 */
export function renderShots(guide, { guidePath, outDir, cadpole, only = null, force = false, extraArgs = [], log = () => {} }) {
  const model = path.resolve(path.dirname(guidePath), guide.model);
  const modelBytes = fs.readFileSync(model);
  const raw = path.join(outDir, "raw");
  fs.mkdirSync(raw, { recursive: true });
  const results = [];
  for (const shot of shotsOf(guide)) {
    if (only && !only.includes(shot.name)) continue;
    const files = { model, png: path.join(raw, `${shot.name}.png`), camera: path.join(raw, `${shot.name}.json`) };
    const args = renderArgs(guide, shot, files);
    const key = digest(args, modelBytes);
    const keyFile = path.join(raw, `${shot.name}.key`);
    if (!force && fs.existsSync(files.png) && fs.existsSync(files.camera) && fs.existsSync(keyFile) && fs.readFileSync(keyFile, "utf8") === key) {
      results.push({ name: shot.name, status: "kept" });
      continue;
    }
    log(`rendering ${shot.name}`);
    const r = spawnSync(cadpole, [...extraArgs, ...args], { encoding: "utf8", maxBuffer: 64 << 20 });
    const errors = `${r.stdout || ""}${r.stderr || ""}`.split("\n").filter((l) => l.startsWith("error"));
    if (r.error || r.status !== 0 || !fs.existsSync(files.png) || !fs.existsSync(files.camera)) {
      results.push({ name: shot.name, status: "failed", detail: r.error ? String(r.error.message) : errors.join(" | ").slice(0, 600) || `cadpole render exited ${r.status}` });
      continue;
    }
    fs.writeFileSync(keyFile, key);
    results.push({ name: shot.name, status: "rendered" });
  }
  return results;
}

/**
 * Draw every shot's annotations over its render: `outDir/img/<name>.svg`,
 * a standalone file with the picture inside it.
 * @returns {{warnings: string[], images: Record<string, {svg: string, width: number, height: number}>}}
 */
export function composeShots(guide, { outDir, only = null }) {
  const raw = path.join(outDir, "raw");
  const img = path.join(outDir, "img");
  fs.mkdirSync(img, { recursive: true });
  const warnings = [];
  const images = {};
  for (const shot of shotsOf(guide)) {
    if (only && !only.includes(shot.name)) continue;
    const png = path.join(raw, `${shot.name}.png`);
    const cam = path.join(raw, `${shot.name}.json`);
    if (!fs.existsSync(png) || !fs.existsSync(cam)) {
      warnings.push(`${shot.name}: not rendered yet`);
      continue;
    }
    const camera = cameraOf(JSON.parse(fs.readFileSync(cam, "utf8")));
    const o = overlay(camera, shot.annotations || []);
    for (const w of o.warnings) warnings.push(`${shot.name}: ${w}`);
    const href = `data:image/png;base64,${fs.readFileSync(png).toString("base64")}`;
    const svg = path.join(img, `${shot.name}.svg`);
    fs.writeFileSync(svg, composeSvg(camera, href, o.svg));
    images[shot.name] = { svg, width: camera.width, height: camera.height };
  }
  return { warnings, images };
}

/**
 * Flatten composed pictures to PNG with a headless Chromium, and print a
 * page to PDF with it. Returns null when `chrome` is not given: the SVGs
 * and the HTML page are the guide then.
 */
export function rasterizer(chrome) {
  if (!chrome) return null;
  const base = ["--headless=new", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1", "--no-first-run"];
  const run = (args) => {
    const r = spawnSync(chrome, [...base, ...args], { encoding: "utf8" });
    if (r.error || r.status !== 0) throw new Error(`the browser failed: ${r.error ? r.error.message : (r.stderr || "").slice(-400)}`);
  };
  return {
    png(svgPath, pngPath, width, height) {
      run([`--window-size=${width},${height}`, `--screenshot=${pngPath}`, pathToFileURL(svgPath).href]);
      if (!fs.existsSync(pngPath)) throw new Error(`the browser wrote no ${pngPath}`);
    },
    pdf(htmlPath, pdfPath) {
      run(["--no-pdf-header-footer", `--print-to-pdf=${pdfPath}`, pathToFileURL(htmlPath).href]);
      if (!fs.existsSync(pdfPath)) throw new Error(`the browser wrote no ${pdfPath}`);
    },
  };
}

const CSS = `
*{box-sizing:border-box}
body{margin:0;font-family:-apple-system,BlinkMacSystemFont,"Helvetica Neue",Helvetica,Arial,sans-serif;color:#1d2733;background:#fff;line-height:1.45;font-size:17px}
main{max-width:860px;margin:0 auto;padding:18px 16px 60px}
h1{font-size:26px;margin:8px 0 2px;font-weight:650}
h2{font-size:20px;margin:34px 0 6px;font-weight:650;border-top:1px solid #e3e6ea;padding-top:18px}
p{margin:6px 0 10px}
.sub{color:#5b6673;margin:0 0 12px}
figure{margin:10px 0 18px;break-inside:avoid}
figure img{width:100%;height:auto;display:block;border:1px solid #e3e6ea;border-radius:8px}
.step{break-inside:avoid;margin:22px 0}
.step h3{font-size:18px;margin:0 0 4px;font-weight:650}
.num{display:inline-block;width:30px;height:30px;line-height:30px;border-radius:50%;background:#F0531C;color:#fff;text-align:center;font-weight:700;margin-right:8px;font-size:16px}
.parts{display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:10px;margin:10px 0}
.part{border:1px solid #e3e6ea;border-radius:8px;padding:6px 8px 8px;break-inside:avoid}
.part img{width:100%;height:auto;display:block}
.part b{font-size:16px}.part span{color:#5b6673;font-size:14px;display:block}
@media print{body{font-size:11.5pt}main{max-width:none;padding:0}h2{break-after:avoid}.step h3{break-after:avoid}
 .parts{grid-template-columns:repeat(4,1fr)}figure img{max-height:6.1in;width:auto;max-width:100%;margin:0 auto}.step figure img{max-height:3.6in}}
@page{size:letter;margin:0.55in}
`;

function esc(t) {
  return String(t).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/**
 * The guide as one HTML page. `src(name)` gives the address of a shot's
 * picture, relative to the page.
 */
export function pageHtml(guide, src) {
  const fig = (shot, alt) => `<figure><img src="${esc(src(shot.name))}" alt="${esc(alt)}"></figure>`;
  const paras = (list) => (list || []).map((p) => `<p>${esc(p)}</p>`).join("\n");
  const h = [];
  h.push(`<h1>${esc(guide.title)}</h1>`);
  if (guide.subtitle) h.push(`<p class="sub">${esc(guide.subtitle)}</p>`);
  h.push(paras(guide.intro));
  for (const s of guide.overview || []) h.push(fig(s, s.title || guide.title));
  if ((guide.parts || []).length) {
    h.push("<h2>Parts</h2>", '<div class="parts">');
    for (const p of guide.parts) {
      h.push(`<div class="part"><img src="${esc(src(p.name))}" alt="${esc(p.label)}"><b>${p.key ? `${esc(p.key)}&nbsp; ` : ""}${esc(p.label)}</b>${p.count ? `<span>${esc(p.count)}</span>` : ""}</div>`);
    }
    h.push("</div>");
  }
  h.push(`<h2>${esc(guide.steps_title || "Steps")}</h2>`);
  guide.steps.forEach((s, i) => {
    h.push(`<div class="step"><h3><span class="num">${i + 1}</span>${esc(s.title)}</h3>${s.text ? `<p>${esc(s.text)}</p>` : ""}${fig(s, s.title)}</div>`);
  });
  for (const s of guide.sections || []) h.push(`<h2>${esc(s.title)}</h2>`, paras(s.text));
  return (
    `<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">` +
    `<title>${esc(guide.title)}</title><style>${CSS}</style></head>\n<body><main>\n${h.filter(Boolean).join("\n")}\n</main></body></html>\n`
  );
}

/**
 * Build a guide: render, annotate, lay out. Writes under `outDir`:
 *   raw/<name>.png, .json   the renders and their cameras (kept between runs)
 *   img/<name>.svg          each picture with its annotations (standalone)
 *   img/<name>.png          the same flattened, with a browser
 *   index.html              the guide as a page
 *   guide.pdf               the page on paper, with a browser
 *   guide-manifest.json     what was made
 * @returns {{ok: boolean, renders: object[], warnings: string[], files: object}}
 */
export function buildGuide(guide, opts) {
  validateGuide(guide);
  const { outDir, chrome = null, composeOnly = false, log = () => {} } = opts;
  fs.mkdirSync(outDir, { recursive: true });
  const renders = composeOnly ? [] : renderShots(guide, opts);
  const failed = renders.filter((r) => r.status === "failed");
  const { warnings, images } = composeShots(guide, opts);
  const raster = rasterizer(chrome);
  const ext = raster ? "png" : "svg";
  const files = { page: path.join(outDir, "index.html"), pdf: null, images: {} };
  for (const [name, im] of Object.entries(images)) {
    let file = im.svg;
    if (raster) {
      file = im.svg.replace(/\.svg$/, ".png");
      log(`flattening ${name}`);
      raster.png(im.svg, file, im.width, im.height);
    }
    files.images[name] = path.relative(outDir, file);
  }
  fs.writeFileSync(files.page, pageHtml(guide, (name) => `img/${name}.${ext}`));
  if (raster) {
    files.pdf = path.join(outDir, "guide.pdf");
    raster.pdf(files.page, files.pdf);
  }
  const missing = shotsOf(guide).filter((s) => !images[s.name]).map((s) => s.name);
  const manifest = {
    title: guide.title,
    steps: guide.steps.map((s, i) => ({ number: i + 1, name: s.name, title: s.title, text: s.text || "", image: files.images[s.name] || null })),
    overview: (guide.overview || []).map((s) => ({ name: s.name, image: files.images[s.name] || null })),
    parts: (guide.parts || []).map((p) => ({ key: p.key || "", label: p.label, count: p.count || "", image: files.images[p.name] || null })),
    page: "index.html",
    pdf: files.pdf ? "guide.pdf" : null,
    renders,
    warnings,
    missing,
  };
  fs.writeFileSync(path.join(outDir, "guide-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return { ok: failed.length === 0 && (opts.only ? true : missing.length === 0), renders, warnings, files, missing };
}
