// Builds the Cadpole site into one self-contained static directory.
//
//   node build.mjs --site <dir> --examples <dir> --web-dist <dir> \
//       --embed <site3d.js> --pages <dir of published viewer pages> --out <dir>
//
// Everything on the page is generated from files in this repository: the
// scripts under examples/ and site/features/, the viewer pages `cadpole
// publish` wrote for them, the web app's engine, and the pictures under
// site/static/. The frame sets and the recording named in media.lock.json are
// large and are not in the tree; the page is written for them all the same,
// and tools/assemble.mjs adds them.
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { gzipSync } from "node:zlib";
import { createHash } from "node:crypto";

import { hl, hlLines, E, T } from "./highlight.mjs";
import { CARDS, GALLERY, PLAY, SCENES } from "./models.mjs";
import { slim } from "./slim.mjs";

const args = {};
for (let i = 2; i < process.argv.length; i += 2) args[process.argv[i].replace(/^--/, "")] = process.argv[i + 1];
for (const k of ["site", "examples", "web-dist", "embed", "pages", "out"]) if (!args[k]) throw new Error(`missing --${k}`);
const SITE = args.site, EX = args.examples, WEB = args["web-dist"], OUT = args.out;
const GH = "https://github.com/mikelikespie/cadpole";
/** Where the site is published: link previews need absolute addresses for the page and its picture. */
const SITE_URL = "https://mikelikespie.github.io/cadpole/";

const read = (p) => readFileSync(p, "utf8");
const json = (p) => JSON.parse(read(p));
const size = (p) => statSync(p).size;
const FEAT = json(join(SITE, "content/features.json"));
const DRAW = json(join(SITE, "content/drawings.json"));
const AGENT = json(join(SITE, "content/agent.json"));
// The headline, the sub-line and the text a link preview shows: one place.
const COPY = json(join(SITE, "content/site.json"));
const BLD = json(join(SITE, "content/buildings.json"));
const LOCK = json(join(SITE, "media.lock.json"));
const STATIC = join(SITE, "static");

const LOGO = '<svg viewBox="0 0 128 128" aria-hidden="true"><path d="M72 62C90 62 92 88 106 89c9 1 13-9 19-14-2 11-8 24-20 24-17 0-19-19-37-20Z" fill="#2fbf9f"/><circle cx="50" cy="58" r="30" fill="#2fbf9f"/><circle cx="60" cy="49" r="9.5" fill="#fff"/><circle cx="62.5" cy="49" r="4.8" fill="#0B2E2A"/></svg>';
const ICON_GH = '<svg viewBox="0 0 16 16" aria-hidden="true"><path fill="currentColor" d="M8 0a8 8 0 0 0-2.53 15.59c.4.07.55-.17.55-.38v-1.33c-2.23.48-2.7-1.07-2.7-1.07-.36-.93-.89-1.17-.89-1.17-.73-.5.05-.49.05-.49.8.06 1.23.83 1.23.83.72 1.22 1.88.87 2.33.66.07-.52.28-.87.5-1.07-1.77-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 0 1 4 0c1.53-1.03 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.28.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.19c0 .21.15.46.55.38A8 8 0 0 0 8 0Z"/></svg>';
const ICON_SHARE = '<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M12 15V3m0 0L7.5 7.5M12 3l4.500 4.500M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-7"/></svg>';
const EXT = '<svg class="ext" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M6.5 3.5H3.5v9h9v-3M9 3.5h3.5V7M12.5 3.5 7.5 8.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg><span class="sr"> (opens in a new tab)</span>';
const fullLink = (name, label = "Full viewer", cls = "btn small") => `<a class="${cls}" href="view/${name}/" data-full="${name}" target="_blank" rel="noopener">${label}${EXT}</a>`;
const modelName = (m) => m.split("/")[1].split(".")[0];

// ---- action rows -------------------------------------------------------------
// Every row of downloads and actions on the page is this one component: up to
// two labelled groups ("Download", "Open"), each a grid of equal pills. A
// download pill names the file type with its size in muted figures; an action
// pill is a link or a button. The groups sit side by side when there is room
// and stack when there is not; inside a group the pills fill equal columns
// (styles in site.css, `.acts`).
const dlPill = (href, type, bytes) => `<a class="pill" href="${href}" download><span class="pill-t">${type}</span><span class="pill-s">${kb(bytes)}</span></a>`;
const open3d = (name, title, view) => `<button class="pill act" type="button" data-sheet3d="models/${name}.cpmesh" data-pic="media/${name}-1200.webp" data-title="${E(title)}" data-view="${view}">Open in 3D</button>`;
const fullPill = (name) => fullLink(name, "Full viewer", "pill act");
const scriptPill = (file) => `<a class="pill act" href="scripts/${file}" type="text/plain">Script</a>`;
const actGroup = (label, pills, cls = "") => (pills.length ? `<div class="acts-g" role="group" aria-label="${label}"><p class="acts-h" aria-hidden="true">${label}</p><div class="acts-p${pills.length === 3 ? " three" : ""}${cls}">${pills.join("")}</div></div>` : "");
const actRow = ({ download = [], open = [] }) => `<div class="acts">${actGroup("Download", download, " short")}${actGroup("Open", open)}</div>`;

/** The lines between `# snippet` and `# end snippet` of a script the build runs. */
function snippet(path) {
  const out = [];
  let on = false;
  for (const l of read(path).split("\n")) {
    const s = l.trim();
    if (s === "# snippet") { on = true; continue; }
    if (s === "# end snippet") break;
    if (on) out.push(l.trimEnd());
  }
  if (!out.length) throw new Error(`${path}: no "# snippet" lines`);
  return out;
}
const svgBox = (path) => { const [, , w, h] = /viewBox="([^"]+)"/.exec(read(path).slice(0, 2000))[1].split(/\s+/).map(Number); return [w, h]; };
/** A short digest of a file's bytes, for an address that changes when the file does (a browser keeps no stale copy). */
const fileHash = (p) => createHash("sha256").update(readFileSync(p)).digest("hex").slice(0, 10);
const kb = (n) => (n >= 10000 ? `${(n / 1000).toFixed(0)} KB` : `${(n / 1000).toFixed(1)} KB`);
const draw = (f) => join(STATIC, "draw", f);

// ---- pinned scroll scenes ----------------------------------------------------
const HERO_BRAND = `<a class="brand hero-brand" href="#top" aria-label="Cadpole">${LOGO}<span>Cadpole</span></a>`;
const HERO = `<p class="eyebrow">${COPY.hero.eyebrow}</p>
<h1>${COPY.hero.headline[0]} <span>${COPY.hero.headline[1]}</span></h1>
<p class="lede">${COPY.hero.sub}</p>
<div class="hero-cta"><a class="btn primary" href="#start">Get started</a><a class="btn" href="${GH}">${ICON_GH}GitHub</a></div>
<ul class="facts">${COPY.hero.facts.map((f) => `<li>${f}</li>`).join("")}</ul>`;

function scene(s, first) {
  const set = (v) => LOCK.sets[`seq/${s.seq}/${v}`]?.count ?? 0;
  const n = set("d"), nm = set("m");
  if (!n || !nm) throw new Error(`media.lock.json has no frame set for ${s.seq}`);
  const w = 720, h = Math.round(720 / s.arv);
  const steps = s.steps.map(([a, b, view, model, title, body, alt], i) => {
    const dm = model ? ` data-model="${model}"` : "";
    const still = Math.min(1, i ? (a + Math.min(b, 1)) / 2 : 0);
    const hero = title === "HERO";
    const inner = hero ? HERO : `<p class="eyebrow">${s.eyebrow}</p><h2>${title}</h2><p>${body}</p>`;
    const cls = hero ? "step hero on" : i === 0 ? "step on" : "step";
    return `<div class="${cls}" data-from="${a}" data-to="${b}" data-view="${view}" data-still="${still.toFixed(3)}" data-alt="${E(alt)}"${dm}>${inner}</div>`;
  });
  const r = s.read;
  const firstRead = `${r.keys[0][1].toFixed(r.digits)}${r.unit}`;
  const spec = E(JSON.stringify({ keys: r.keys, digits: r.digits, unit: r.unit }));
  const lazy = first ? 'fetchpriority="high"' : 'loading="lazy"';
  const name = modelName(s.model);
  return `
<section class="scene${first ? " first" : ""}" id="${s.id}" style="--len:${s.len};--ard:${s.ar};--arp:${s.arp ?? s.ar}" data-seq="${s.seq}"${set("x") ? ' data-xl="1"' : ""} data-n="${n}" data-nm="${nm}" aria-label="${E(s.eyebrow)}">
<div class="pin">${first ? HERO_BRAND : ""}
<div class="vis">
<picture><source media="(min-width: 700px) and (min-height: 700px)" srcset="seq/${s.seq}/d/000.avif" type="image/avif"><img class="poster" src="seq/${s.seq}/m/000.webp" width="${w}" height="${h}" alt="${E(s.steps[0][6])}" ${lazy} decoding="async"></picture>
<canvas class="seq" aria-hidden="true"></canvas>
<div class="live"></div>
<p class="hint3d when-live">Drag to orbit · two fingers or scroll to move · pinch to zoom · double-tap to centre</p>
</div>
<div class="caps">
<div class="steps">${steps.join("")}</div>
<div class="vis-actions">${fullLink(name)}<button class="btn small when-poster" type="button" data-open3d="${s.model}">Explore in 3D</button><button class="btn small primary when-live" type="button" data-close3d>Done</button></div>
<div class="meter" aria-hidden="true"><span class="read">${r.label} <b data-read="${spec}">${firstRead}</b></span><span class="track"><i></i></span></div>
</div>
</div>
</section>`;
}

/** part.star as the page shows it: the model, without its heading and the tests below it. */
const partModel = () => read(join(EX, "part.star")).split("\n# Tests:")[0].replace(/^(#[^\n]*\n)+\n/, "").trimEnd() + "\n";

// ---- playground --------------------------------------------------------------
function play() {
  const tabs = [], srcs = [];
  PLAY.forEach(([key, fn, label, view], i) => {
    const src = key === "part" ? partModel() : read(join(EX, fn));
    if (src.includes("</script")) throw new Error(`${fn} cannot be set inline`);
    tabs.push(`<button class="tab" type="button" aria-pressed="${i === 0}" data-src="src-${key}" data-file="${fn}" data-poster="media/${key}-720.webp" data-full="${key}" data-view="${view}">${label}</button>`);
    srcs.push(`<script type="text/plain" id="src-${key}">${src}</script>`);
  });
  const first = partModel();
  return `
<section class="block wrap" id="play">
<div class="head"><p class="eyebrow">Parts and assemblies · try it here</p><h2>Change the script. The part follows.</h2>
<p class="lede">This is the real engine, compiled to WebAssembly and running in your browser. Move a slider or edit a line and the model is rebuilt from the script.</p></div>
<div class="tabs" role="group" aria-label="Example scripts">${tabs.join("")}</div>
<div class="play">
<div class="play-right">
<div class="play-view"><img src="media/part-720.webp" srcset="media/part-720.webp 720w, media/part-1200.webp 1200w" sizes="(min-width:1000px) 600px, 100vw" width="720" height="720" alt="The part the script describes" loading="lazy" decoding="async"><div class="live"></div>
<div class="play-over"><button class="btn primary" type="button" data-start>Run it live</button><small>Loads the engine once, about <span data-engine-mb>${ENGINE_MB}</span> MB</small></div><div class="load" hidden><i></i></div></div>
<div class="play-acts acts-p"><button class="pill act" type="button" data-take aria-pressed="false">Zoom and pan</button><a class="pill act" href="view/part/" data-full="part" title="Opens this script with its default values in the Cadpole viewer" target="_blank" rel="noopener">Full viewer${EXT}</a></div>
<p class="status" role="status"><i></i><span>Rendered ahead of time. Start the engine to change it.</span></p>
<div class="sliders"></div>
</div>
<div class="play-left">
<div class="code"><div class="code-head"><span data-file>${PLAY[0][1]}</span><button class="copy" type="button" data-copy=".editor">Copy</button></div>
<textarea class="editor" spellcheck="false" autocapitalize="off" autocomplete="off" autocorrect="off" aria-label="Script source">${T(first)}</textarea><button class="code-more" type="button" data-more aria-expanded="false">Show the full script</button></div>
</div>
</div>
${srcs.join("")}
</section>`;
}

// ---- cards -------------------------------------------------------------------
function card({ img, alt, title, body, tags = [], paper = false, btn = "", model = null, view = "-30,24,24,1", name = null }) {
  name = name ?? title.replace(/<[^>]+>/g, "");
  const m = model ? ` data-sheet3d="models/${model}.cpmesh" data-view="${view}"` : "";
  const what = model ? `Open ${E(name)} in 3D` : `Enlarge the picture: ${E(name)}`;
  const fig = paper
    ? `<figure class="paper"><button class="pic" type="button" data-pic="${img}" data-paper data-title="${E(name)}" aria-label="${what}"><img src="${img}" alt="${E(alt)}" loading="lazy" decoding="async" width="720" height="720"></button></figure>`
    : `<figure><button class="pic" type="button" data-pic="media/${img}-1200.webp"${m} data-title="${E(name)}" aria-label="${what}"><img src="media/${img}-720.webp" srcset="media/${img}-720.webp 720w, media/${img}-1200.webp 1200w" sizes="(min-width:1000px) 380px, 84vw" width="720" height="720" alt="${E(alt)}" loading="lazy" decoding="async"></button></figure>`;
  const t = tags.length ? `<ul class="tags">${tags.map((x) => `<li>${x}</li>`).join("")}</ul>` : "";
  return `<article class="card">${fig}<div class="txt"><h3>${title}</h3><p>${body}</p>${t}${btn}</div></article>`;
}

function outputs() {
  return `
<section class="block wrap" id="out">
<div class="head"><p class="eyebrow">Shared by both · what comes out</p><h2>Solids, drawings and print files.</h2>
<p class="lede">The script is the source. Everything else is generated from it, so it never goes stale.</p></div>
<div class="rail" style="--cols:4">${CARDS.map(card).join("")}</div><div class="dots" aria-hidden="true"></div>
</section>`;
}

function gallery() {
  const cards = GALLERY.map(([k, t, b, v, script]) => card({
    img: k, alt: t, title: t, body: b, model: k, view: v,
    btn: actRow({ open: [open3d(k, t, v), fullPill(k), scriptPill(script)] }),
  }));
  return `
<section class="block wrap" id="gallery">
<div class="head"><p class="eyebrow">Gallery</p><h2>Each of these is one script.</h2>
<p class="lede">The scripts are published with this site. Open one and turn it round.</p></div>
<div class="rail" style="--cols:3">${cards.join("")}</div><div class="dots" aria-hidden="true"></div>
</section>`;
}

function tools() {
  const items = [
    ["Coding agents", "<p>An MCP server gives an agent every operation as a typed tool: evaluate a script, read the diagnostics, look at the render, read the measurements, run the tests, then try again.</p>"],
    ["Command line", "<p><code>render</code>, <code>sweep</code>, <code>animate</code>, <code>export</code>, <code>test</code> and <code>serve</code> in one binary. It runs the same on Linux and on a Mac.</p>"],
    ["Web editor", "<p><code>cadpole serve</code> opens an editor and a 3D view in the browser. It is the same engine as the command line, compiled to WebAssembly.</p>"],
    ["Tests", "<p>A script can measure itself: clearances, volumes, wall thickness. Every example in the repository runs under test.</p>"],
  ];
  const acc = items.map(([t, b], i) => `<details${i === 0 ? " open" : ""}><summary>${t}</summary><div class="body">${b}</div></details>`).join("");
  const code = partModel();
  return `
<section class="block wrap" id="tools">
<div class="two">
<div><div class="head"><p class="eyebrow">Shared by both · how it is driven</p><h2>One engine, four ways in.</h2>
<p class="lede">A model is a short script in Starlark, a small deterministic Python dialect: the same script gives the same model every time, wherever it runs. An agent writes it; you can too.</p></div>
<div class="code"><div class="code-head"><span>part.star</span><button class="copy" type="button" data-copy="#code-part">Copy</button></div><pre id="code-part">${hlLines(code.trimEnd())}</pre></div>
</div>
<div class="acc">${acc}</div>
</div>
</section>`;
}

// ---- what it can do ----------------------------------------------------------
const dwg = (src, path, label, cls = "", extra = "") => {
  const [w, h] = svgBox(path);
  return `<div class="dwg ${cls}" data-svg="${src}" style="aspect-ratio:${w.toFixed(2)}/${h.toFixed(2)}" role="img" aria-label="${E(label)}"${extra}></div>`;
};
const drawBtn = (src, path, label, title, note) => `<button class="dwg-open" type="button" data-draw="${src}" data-title="${E(title)}" data-note="${E(note)}"><span class="sr">Enlarge the drawing: ${E(title)}</span>${dwg(src, path, label).replace('role="img"', 'aria-hidden="true"')}</button>`;
const VEC = Object.fromEntries(DRAW.drawings.map((v) => [v.id, v]));
const PLAN_VIEW = "-35,55,24,1";
const planLines = () => snippet(join(EX, "cabin_plan.star"));

// The gallery: one grid of small tiles, a picture and the operation's name. A
// tile opens the lightbox with what the card is about: the model in 3D for a
// model, the drawing itself (pan and zoom) for a drawing, and under it the
// one-line description, the script lines and the downloads. A card's `kind`
// says which; its tile picture is of the same kind (a drawing's tile is the SVG).
function features() {
  const tiles = [];
  const snip = (code, fname) => `<div class="code"><div class="code-head"><span>${E(fname)}</span><button class="copy" type="button" data-copy aria-label="Copy the script lines">Copy</button></div><pre aria-label="${E(fname)}">${hl(code)}</pre></div>`;
  const ids = new Set();
  const tile = ({ id, title, kind, thumb, open, info }) => {
    if (ids.has(id)) throw new Error(`two gallery cards are called ${id}: the address #f-${id} must name one`);
    ids.add(id);
    tiles.push(`<li><button class="tile" type="button" id="f-${id}" data-card="${id}" data-kind="${kind}" ${open} aria-haspopup="dialog"><span class="tile-pic">${thumb}</span><span class="tile-name">${E(title)}</span></button><template data-card-info="${id}">${info}</template></li>`);
  };
  // The part of a sheet a tile shows (fractions of its width and height): the views, without the title block.
  const CROP = { "sheet-flanged-boss": [0.16, 0.19, 0.75, 0.8], "sheet-pillow-block": [0.11, 0.18, 0.75, 0.81], "section-pillow-block": [0.07, 0.08, 0.61, 0.88] };
  const drawThumb = (stem, label) => {
    const [w, h] = svgBox(draw(`${stem}.svg`)), c = CROP[stem] ?? [0, 0, 1, 1];
    return `<span class="dwg tile-dwg" data-svg="draw/${stem}.svg"${CROP[stem] ? ` data-crop="${c.join(",")}"` : ""} style="aspect-ratio:${(w * (c[2] - c[0])).toFixed(2)}/${(h * (c[3] - c[1])).toFixed(2)}" aria-hidden="true" data-label="${E(label)}"></span>`;
  };
  const drawOpen = (stem, title) => `data-draw="draw/${stem}.svg" data-title="${E(title)}" data-note="${E(`Vector drawing · ${kb(size(draw(`${stem}.raw.svg`)))} as SVG`)}"`;
  for (const m of FEAT) {
    const i = m.id, code = snippet(join(SITE, "features", `${i}.star`)).join("\n");
    if (m.kind === "drawing") {
      tile({ id: i, title: m.title, kind: "drawing", thumb: drawThumb(m.drawing, m.alt), open: drawOpen(m.drawing, m.title),
        info: `<p class="card-blurb">${E(m.blurb)}</p>${snip(code, `${i}.star`)}${actRow({ download: fileSet(m.drawing), open: [`<button class="pill act" type="button" data-sheet3d="models/feat-${i}.cpmesh" data-title="${E(m.title)}" data-view="${m.view}">View the model in 3D</button>`] })}` });
    } else {
      tile({ id: i, title: m.title, kind: "model",
        thumb: `<img src="media/feat-${i}-360.webp" srcset="media/feat-${i}-360.webp 360w, media/feat-${i}-720.webp 720w" sizes="(min-width:1400px) 200px, (min-width:700px) 22vw, 46vw" width="360" height="360" alt="${E(m.alt)}" loading="lazy" decoding="async">`,
        open: `data-pic="media/feat-${i}-1200.webp" data-sheet3d="models/feat-${i}.cpmesh" data-view="${m.view}" data-title="${E(m.title)}"`,
        info: `<p class="card-blurb">${E(m.blurb)}</p>${snip(code, `${i}.star`)}` });
    }
  }
  for (const m of DRAW.cards) {
    const v = VEC[m.drawing];
    tile({ id: m.id, title: m.title, kind: "drawing", thumb: drawThumb(v.id, m.alt), open: drawOpen(v.id, m.title),
      info: `<p class="card-blurb">${E(m.blurb)}</p>${snip(snippet(join(SITE, "features", m.script)).join("\n"), m.script)}${actRow({ download: fileSet(v.id) })}` });
  }
  return `
<section class="block wrap" id="can">
<div class="head"><p class="eyebrow">Shared by both · modelling operations</p><h2>Small scripts, real geometry.</h2>
<p class="lede">The operations every model is built from, a bracket and a house alike. Each picture was made by a few lines of script: open one to read them, turn the model round or look over the drawing.</p></div>
<ul class="tiles">${tiles.join("")}</ul>
</section>`;
}

// ---- vector drawings ---------------------------------------------------------
const cmdBlock = (lines) => `<div class="cmd multi"><code>${E(lines.join("\n"))}</code><button class="copy" type="button" data-copy aria-label="Copy command">Copy</button></div>`;
// A drawing's files, the ones its recipe writes: always the SVG, a PDF and a DXF where there are.
const fileSet = (stem) => [[`${stem}.raw.svg`, "SVG"], [`${stem}.pdf`, "PDF"], [`${stem}.dxf`, "DXF"]].filter(([f]) => existsSync(draw(f))).map(([f, t]) => dlPill(`draw/${f}`, t, size(draw(f))));
const downloads = (v) => actRow({ download: fileSet(v.id) });
// The drawings section: a path-traced picture and the line drawing of the same model from the same
// parallel camera, one over the other, zoomed and panned together (src/site.js). The SVG is drawn
// again at every magnification; the picture is a fixed number of pixels.
function drawings() {
  const v = VEC["pair-thumbscrew"], pic = `draw/${v.id}-${v.px}.webp`;
  const [x, y, k] = v.start;
  return `
<section class="block wrap" id="draw">
<div class="head"><p class="eyebrow">Shared by both · vector drawings</p><h2>Lines, not pixels.</h2>
<p class="lede">The model that renders also draws: hidden-line views, dimensioned sheets and hatched sections, written as SVG, PDF and DXF. A line is stored as its ends, so it stays sharp at any size.</p></div>
<figure class="vfig vzoom">
<div class="zf-tags" aria-hidden="true"><span>Picture <small>WebP, ${kb(size(join(STATIC, pic)))}</small></span><span>Lines <small>SVG, ${kb(size(draw(`${v.id}.raw.svg`)))}</small></span></div>
<div class="zf" data-zoom data-start="${x},${y},${k}" data-picture="${pic}" tabindex="0" aria-label="${E(v.alt)}. Zoomed in on the thread where it enters the nut. Pinch or ${"\u2318"}/Ctrl and scroll to zoom, drag or scroll to move. Plus and minus keys zoom, arrow keys move, zero shows the whole model.">
<div class="dwg zf-svg" data-svg="draw/${v.id}.svg"></div>
<div class="zf-left"><canvas class="zf-png" hidden></canvas></div>
<div class="zf-split" role="slider" tabindex="0" aria-label="The line between the picture and the drawing" aria-valuemin="0" aria-valuemax="100" aria-valuenow="50" aria-valuetext="50% SVG"><i></i></div>
<p class="zf-note" aria-live="polite"></p>
</div>
<div class="zbar"><div class="zgrp"><button class="btn small zb" type="button" data-z="out" aria-label="Zoom out">\u2212</button><span class="zk" aria-label="Magnification">${k}\u00d7</span><button class="btn small zb" type="button" data-z="in" aria-label="Zoom in">+</button></div><button class="btn small" type="button" data-z="reset">Whole model</button></div>
<figcaption><h3>${E(v.title)}</h3><p>${E(v.blurb)}</p>${cmdBlock(v.command)}${downloads(v)}</figcaption></figure>
</section>`;
}

// ---- floor plan --------------------------------------------------------------
const ftin = (ft) => { const whole = Math.trunc(ft), inch = Math.round((ft - whole) * 12); return `${whole} ft` + (inch ? ` ${inch} in` : ""); };
/** The widths the plan was drawn at, plan-s0.svg onward (rendering/render.py). */
const PLAN_WIDTHS = [14, 14.5, 15, 15.5, 16];

function floorplan() {
  const urls = PLAN_WIDTHS.map((_, i) => `draw/plan-s${i}.svg`);
  const vals = PLAN_WIDTHS.map(ftin);
  return `
<section class="block wrap" id="plan">
<div class="head"><p class="eyebrow">Buildings and plans · floor plans</p><h2>A plan from a list of rooms.</h2>
<p class="lede">An invented two-bedroom cabin, 30 ft by 26 ft. Rooms are rectangles in feet; the walls, door swings, dimension strings, room areas, section and 3D model are all generated from them. The kitchen, the bath and both bedrooms are furnished by the layout solver, which keeps every clearance and the kitchen's work triangle.</p></div>
<div class="plan">
<div class="plan-main">
<button class="dwg-open" type="button" data-draw="${urls[0]}" data-title="Cabin floor plan" data-note="Vector drawing"><span class="sr">Enlarge the drawing: Cabin floor plan</span>${dwg(urls[0], draw("plan-s0.svg"), "Floor plan of a furnished two-bedroom cabin with room names, areas and dimension strings", "plan-live", ` data-states="${E(JSON.stringify(urls))}"`).replace('role="img"', 'aria-hidden="true"')}</button>
<div class="slider plan-slider"><label for="plan-w">living_w</label><output for="plan-w">${vals[0]}</output><input id="plan-w" type="range" min="0" max="${PLAN_WIDTHS.length - 1}" step="1" value="0" data-vals="${E(JSON.stringify(vals))}" aria-label="Living room width"></div>
<p class="cmd-note">One parameter. Move it and the plan is redrawn: the living room, kitchen and deck widen, their areas and the dimension strings follow. Kitchen, bath and bedrooms placed by the layout solver; the living room by hand, held to the same rules by the script's tests.</p>
</div>
<div class="plan-side">
<figure class="plan-3d"><button class="pic" type="button" data-pic="media/plan-cabin-1200.webp" data-sheet3d="models/plan-cabin.cpmesh" data-view="${PLAN_VIEW}" data-title="Cabin" aria-label="Open the cabin in 3D"><img src="media/plan-cabin-720.webp" srcset="media/plan-cabin-720.webp 720w, media/plan-cabin-1200.webp 1200w" sizes="(min-width:1000px) 420px, 100vw" width="720" height="720" alt="The furnished cabin as a 3D model cut at 4 ft like a plan, roof off: kitchen, bath, two bedrooms and the living room" loading="lazy" decoding="async"></button>
<figcaption>${actRow({ open: [open3d("plan-cabin", "Cabin", PLAN_VIEW), fullPill("plan-cabin"), scriptPill("cabin_plan.star")] })}</figcaption></figure>
<div class="plan-two">
<figure>${drawBtn("draw/section.svg", draw("section.svg"), "A section through the cabin's living room and kitchen with ceiling height", "Section through living room and kitchen", "Vector drawing")}<figcaption>Section</figcaption></figure>
<figure>${drawBtn("draw/elevation.svg", draw("elevation.svg"), "The south elevation of the cabin with overall width and ridge height", "South elevation", "Vector drawing")}<figcaption>South elevation</figcaption></figure>
</div>
</div>
</div>
<div class="plan-code"><div class="code"><div class="code-head"><span>cabin_plan.star</span><button class="copy" type="button" data-copy="#code-cabin">Copy</button></div><pre id="code-cabin">${hlLines(planLines().join("\n"))}</pre></div>
${actRow({ download: fileSet("plan") })}</div>
</section>`;
}

// ---- the two kinds of work ---------------------------------------------------
const still = (name, alt, sizes = "(min-width:1000px) 560px, 100vw", w = 720, h = 720) => `<img src="media/${name}-720.webp" srcset="media/${name}-720.webp 720w, media/${name}-1200.webp 1200w" sizes="${sizes}" width="${w}" height="${h}" alt="${E(alt)}" loading="lazy" decoding="async">`;
const docLink = (file, label) => `<a href="${GH}/blob/main/docs/${file}">${label}</a>`;

function modes() {
  const col = (m) => `<article class="mode"><a class="mode-pic" href="#${m.to}" aria-label="${E(m.title)}: go to the section">${still(m.img, m.alt, "(min-width:1000px) 560px, 100vw")}</a>
<div class="txt"><h3>${m.title}</h3><p>${m.body}</p><ul class="tags">${m.tags.map((t) => `<li>${t}</li>`).join("")}</ul>
${actRow({ open: [`<a class="pill act" href="#${m.to}">${m.go}</a>`] })}</div></article>`;
  return `
<section class="block wrap" id="modes">
<div class="head"><p class="eyebrow">Two kinds of work</p><h2>Parts and buildings, one engine.</h2>
<p class="lede">${BLD.modes.lede}</p></div>
<div class="modes">${BLD.modes.cols.map(col).join("")}</div>
<p class="cmd-note modes-shared">${BLD.modes.shared}</p>
</section>`;
}

/** The heading that opens a group of sections. */
const group = (id, g) => `
<section class="group wrap" id="${id}">
<p class="eyebrow">${g.eyebrow}</p><h2>${g.title}</h2>
<p class="lede">${g.lede}</p>${g.doc ? `<p class="cmd-note">${g.note} ${docLink(g.doc, g.docLabel)}.</p>` : ""}
</section>`;

// ---- buildings: the lot, the sun, the solver, the set ------------------------
const facts = (rows) => `<table class="facts-t"><tbody>${rows.map(([k, v]) => `<tr><th scope="row">${k}</th><td>${v}</td></tr>`).join("")}</tbody></table>`;
const acts = (name, title, view, script) => actRow({ open: [open3d(name, title, view), fullPill(name), scriptPill(script)] });

/** A length in feet as the page writes one: whole inches, or a half. */
const ftHalf = (ft) => { const half = Math.round(ft * 24), whole = Math.floor(half / 24), r = half - whole * 24; return `${whole} ft` + (r ? ` ${Math.floor(r / 2) || ""}${r % 2 ? "½" : ""} in` : ""); };
const LAYOUT_SCRIPT = "room_layout_opt.star";
/** What the solver chose (content/layout.json, written by rendering/render.py from the script's own output). */
function layoutFacts() {
  const L = json(join(SITE, "content/layout.json"));
  const name = (n) => ({ living: "Living room", kitchen: "Kitchen", bedroom: "Bedroom", bath: "Bath" })[n] ?? n;
  // Each room's swatch is the colour its floor has in the model: the script's COLORS.
  const colors = JSON.parse(read(join(EX, LAYOUT_SCRIPT)).match(/^COLORS = (\{[^}]*\})/m)[1]);
  const sw = (n) => colors[n] ? `<i class="sw" style="background:${colors[n]}" aria-hidden="true"></i>` : "";
  const width = Math.max(...L.rooms.map((r) => r.x + r.w));
  return [
    ...L.rooms.map((r) => [sw(r.name) + name(r.name), `${ftHalf(r.w)} × ${ftHalf(r.d)}`]),
    ["Floor plate", `${ftHalf(width)} × ${ftHalf(L.depth)}`],
    ["Floor-area cap, two floors", `${L.cap.toLocaleString("en-US")} sq ft, ${Math.round(L.left)} left`],
    ["The problem", `${L.variables} variables, ${L.constraints} rules`],
    ["Result", L.status],
  ];
}

function building(b) {
  const [w, h] = b.size;
  if (b.rules) { const src = read(join(EX, b.script)); for (const l of b.rules) if (!src.includes(l)) throw new Error(`${b.script} has no line "${l}"`); }
  if (b.id === "layout") b = { ...b, facts: layoutFacts() };
  const pic = `<button class="pic" type="button" data-pic="media/${b.img}-1200.webp"${b.model ? ` data-sheet3d="models/${b.model}.cpmesh" data-view="${b.view}"` : ""} data-title="${E(b.name)}" aria-label="${b.model ? `Open ${E(b.name)} in 3D` : `Enlarge the picture: ${E(b.name)}`}">${still(b.img, b.alt, "(min-width:1000px) 640px, 100vw", w, h)}</button>`;
  return `
<section class="block wrap" id="${b.id}">
<div class="head"><p class="eyebrow">Buildings and plans · ${b.eyebrow}</p><h2>${b.title}</h2>
<p class="lede">${b.lede}</p></div>
<figure class="vfig bld"><div class="bld-pic" style="aspect-ratio:${w}/${h}">${pic}</div>
<figcaption><h3>${b.sub}</h3><p>${b.body}</p>${facts(b.facts)}${b.rules ? `<div class="code"><div class="code-head"><span>${b.script}</span></div><pre tabindex="0" aria-label="${E(b.script)}">${hl(b.rules.join("\n"))}</pre></div>` : ""}
${cmdBlock(b.command)}${b.model ? acts(b.model, b.name, b.view, b.script) : actRow({ open: [scriptPill(b.script)] })}</figcaption></figure>
</section>`;
}

function sheets() {
  const b = BLD.set;
  const svg = `draw/${b.id}.svg`;
  return `
<section class="block wrap" id="set">
<div class="head"><p class="eyebrow">Buildings and plans · drawing sets</p><h2>${b.title}</h2>
<p class="lede">${b.lede}</p></div>
<figure class="vfig vsheet">${drawBtn(svg, draw(`${b.id}.svg`), b.alt, b.name, `Vector drawing · ${kb(size(draw(`${b.id}.raw.svg`)))} as SVG`)}
<figcaption><h3>${b.sub}</h3><p>${b.body}</p>${facts(b.facts)}${cmdBlock(b.command)}
${actRow({ download: [dlPill(`draw/${b.id}.raw.svg`, "Sheet SVG", size(draw(`${b.id}.raw.svg`))), dlPill("draw/house_set.pdf", "Set PDF", size(draw("house_set.pdf")))], open: [scriptPill("house_set.star")] })}</figcaption></figure>
</section>`;
}

// ---- work with an agent ------------------------------------------------------
function agent() {
  const caps = (steps) => steps.map((s) => `<li data-t="${s.t}">${E(s.text)}</li>`).join("");
  const code = (c, cls = "") => {
    const isJson = c.startsWith("{");
    if (isJson) c = JSON.stringify(JSON.parse(c), null, 2);
    return `<div class="cmd multi${isJson ? " json" : ""}${cls}"><code>${E(c)}</code><button class="copy" type="button" data-copy aria-label="Copy">Copy</button></div>`;
  };
  // One numbered list, top to bottom: the sentence, then what to run or type under it
  // (beside it on a wide screen, on the same row).
  const how = AGENT.setup.map((s) => {
    const right = [s.command ? code(s.command) : "", s.prompt ? `<p class="how-say">Then ask it:</p>${code(s.prompt, " prompt")}` : "", s.alt ? `<details class="how-alt"><summary>${E(s.alt.summary)}</summary>${code(s.alt.command)}</details>` : ""].join("");
    return `<li><div class="how-t"><p>${E(s.text)}</p>${s.note ? `<p class="cmd-note">${s.note}</p>` : ""}</div>${right ? `<div class="how-c">${right}</div>` : ""}</li>`;
  });
  return `
<section class="block wrap" id="agent">
<div class="head"><p class="eyebrow">Work with an agent</p><h2>Ask for a change. Watch the part follow.</h2>
<p class="lede">Cadpole has an MCP server, so a coding agent can build and edit the model, check the result and see what you have selected in the 3D view. It works with your own Claude Code install. Cadpole ships no agent of its own.</p></div>
<ul class="agent-can">${AGENT.can.map(([t, b]) => `<li><b>${t}</b><span>${b}</span></li>`).join("")}</ul>
<div class="agent">
<div class="agent-main">
<div class="agent-vid" data-video="agent/agent" data-video-phone="agent/agent-phone" data-v="${LOCK.files["agent/agent.mp4"].sha256.slice(0, 10)}" data-v-phone="${LOCK.files["agent/agent-phone.mp4"].sha256.slice(0, 10)}">
<picture><source media="(max-width: 699.9px) and (orientation: portrait)" srcset="agent/agent-phone-poster.webp?v=${fileHash(join(STATIC, "agent", "agent-phone-poster.webp"))}" width="390" height="844"><img src="agent/agent-poster.webp?v=${fileHash(join(STATIC, "agent", "agent-poster.webp"))}" width="1920" height="1200" alt="The Cadpole web app with a bracket model and a Messages panel where a request to an agent is typed" loading="lazy" decoding="async"></picture>
</div>
<div class="agent-ctl"><button class="btn small primary" type="button" data-vplay aria-pressed="false">Play</button><p class="agent-cap" aria-live="off"></p></div>
</div>
<div class="agent-side">
<ol class="agent-steps wide" aria-label="What happens in the recording">${caps(AGENT.steps)}</ol>
<ol class="agent-steps narrow" aria-label="What happens in the recording">${caps(AGENT.steps_phone)}</ol>
<p class="cmd-note">Real session, sped up. No sound.</p>
</div>
</div>
<h3 class="sub">Set it up</h3>
<p class="cmd-note how-lead">Let your agent do it: paste this, and it goes through the steps below with you.</p>
<div class="cmds">${code(AGENT.setup_agent, " prompt")}</div>
<p class="cmd-note how-lead">Or step by step:</p>
<ol class="agent-how">${how.join("")}</ol>
<p class="cmd-note how-foot">Any MCP client can connect the same way; only Claude Code has been tested. The feature is documented as <a href="${GH}/blob/main/docs/assistant-integration.md">assistant integration</a>.</p>
<p class="cmd-note how-foot">Cadpole is an independent project, not affiliated with or endorsed by the maker of Claude Code.</p>
</section>`;
}

// ---- get started -------------------------------------------------------------
const CMDS = [
  ["cadpole render examples/bracket.star -o bracket.png", "Render a picture: <code>bracket.png</code> shows a mounting bracket."],
  ["cadpole sweep examples/planter.star --param top_radius=40,55,70 --param wall=1.6,3 -o sheet.png", "Draw a sheet of variants."],
  ["cadpole animate examples/gear_toy.star -o gear_toy.gif", "Animate a mechanism. This one takes a few minutes."],
  ["cadpole serve examples/bracket.star", "Open the web editor. It prints an <code>open:</code> address; open that in a browser."],
];
const one = (c, note) => `<div>${note ? `<p class="cmd-note">${note}</p>` : ""}<div class="cmd"><code>${E(c)}</code><button class="copy" type="button" data-copy aria-label="Copy command">Copy</button></div></div>`;

/** What a visitor pastes into their own coding agent to set Cadpole up. */
const AGENT_SETUP = `Set up Cadpole from ${GH.replace(/^https:\/\//, "")}: follow its getting-started guide (docs/getting-started.md), build the CLI, and render examples/bracket.star.`;

function start() {
  return `
<section class="block wrap" id="start">
<div class="head"><p class="eyebrow">Get started</p><h2>Point your agent at it, or follow the steps.</h2>
<p class="lede">Cadpole builds on Linux (x86-64 or arm64) and macOS (Apple silicon), from source.</p></div>
<h3 class="sub"><span class="stepn" aria-hidden="true">A</span>Ask your coding agent</h3>
<p class="cmd-note">Paste this into the agent you already use. The <a href="${GH}/blob/main/docs/getting-started.md">getting-started guide</a> is written to be followed end to end: exact commands, what each system needs, and a check after every step.</p>
<div class="cmds"><div class="cmd multi prompt"><code id="agent-setup">${E(AGENT_SETUP)}</code><button class="copy" type="button" data-copy="#agent-setup" aria-label="Copy the request">Copy</button></div></div>
<h3 class="sub"><span class="stepn" aria-hidden="true">B</span>Or do it by hand</h3>
<ol class="start-steps">
<li><h4>Install Bazelisk as <code>bazel</code></h4>
<p class="cmd-note">Follow the <a href="https://github.com/bazelbuild/bazelisk#installation">official Bazelisk installation instructions</a> for your system. It fetches the Bazel version the repository pins, and Bazel fetches the Rust, C++ and Node toolchains: you never run <code>cargo</code> or <code>npm</code>. Linux (x86-64 or arm64) also needs <code>g++</code>; macOS (Apple silicon) needs Xcode 27. The next step checks all of this and prints the command for anything missing.</p></li>
<li><h4>Clone and install</h4>
<div class="cmds">${one(`git clone ${GH}.git && cd cadpole`)}${one("./bin/install", "Checks your machine, builds, and puts <code>cadpole</code> on your <code>PATH</code>. The first build compiles everything and takes a while (about 12 minutes on 8 cores, 25 GB of disk); later ones take seconds. Without installing, <code>./bin/cadpole …</code> works from the checkout.")}</div></li>
<li><h4>Run a first example</h4>
<div class="cmds">${CMDS.map(([c, n]) => one(c, n)).join("")}</div>
<p class="cmd-note">To have your coding agent change the model while you watch, register Cadpole's MCP server: the steps are under <a href="#agent">Work with an agent</a>.</p></li>
</ol>
<p class="cmd-note">Apache-2.0. <a href="${GH}">Source</a>, <a href="${GH}/blob/main/docs/getting-started.md">getting started</a>, the guides to <a href="${GH}/blob/main/docs/parts.md">parts and assemblies</a> and to <a href="${GH}/blob/main/docs/buildings.md">buildings and plans</a>, and the <a href="${GH}/blob/main/docs/starlark-api.md">scripting reference</a> they share are on GitHub. Windows is not supported.</p>
</section>`;
}

// ---- the page ----------------------------------------------------------------
const ENGINE_BYTES = size(join(WEB, "cadpole_wasm_bg.wasm"));
/** What a visitor downloads: hosts serve wasm compressed. */
const ENGINE_MB = Math.round(gzipSync(readFileSync(join(WEB, "cadpole_wasm_bg.wasm")), { level: 6 }).length / 1e6);

function page(full) {
  const desc = COPY.description;
  return `<!doctype html>
<html lang="en" data-engine-bytes="${ENGINE_BYTES}" data-engine-mb="${ENGINE_MB}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>${E(COPY.title)}</title>
<meta name="description" content="${E(desc)}">
<meta name="theme-color" content="#060a09">
<meta name="color-scheme" content="dark">
<link rel="icon" href="icon.svg" type="image/svg+xml">
<link rel="icon" href="icon-32.png" sizes="32x32" type="image/png">
<link rel="apple-touch-icon" href="apple-touch-icon.png">
<link rel="manifest" href="site.webmanifest">
<meta property="og:type" content="website">
<meta property="og:title" content="${E(COPY.title)}">
<meta property="og:description" content="${E(desc)}">
<meta property="og:url" content="${SITE_URL}">
<meta property="og:image" content="${SITE_URL}og.png">
<meta property="og:image:width" content="1200"><meta property="og:image:height" content="630">
<meta name="twitter:card" content="summary_large_image">
<link rel="preload" as="image" href="seq/tj/d/000.avif" type="image/avif" media="(min-width: 700px) and (min-height: 700px)" fetchpriority="high">
<link rel="preload" as="image" href="seq/tj/m/000.webp" type="image/webp" media="not ((min-width: 700px) and (min-height: 700px))" fetchpriority="high">
<link rel="stylesheet" href="site.css">
<script type="module" src="site.js"></script>
</head>
<body>
<a class="skip" href="#main">Skip to content</a>
<header class="top">
<a class="brand" href="#top" aria-label="Cadpole, top of page">${LOGO}<span>Cadpole</span></a>
<nav aria-label="Sections"><a href="#agent">Agents</a><a href="#parts">Parts</a><a href="#buildings">Buildings</a><a href="#play">Try it</a><a href="#can">Features</a><a href="#draw">Drawings</a><a href="#gallery">Gallery</a><a href="#start">Get started</a><a href="${GH}">GitHub</a></nav>
</header>
<main id="main">
<span id="top"></span>
${scene(SCENES[0], true)}
${agent()}
${modes()}
${group("parts", BLD.groups.parts)}
${SCENES.slice(1).map((s) => scene(s, false)).join("")}
${play()}
${group("buildings", BLD.groups.buildings)}
${floorplan()}
${BLD.sections.map(building).join("")}
${sheets()}
${group("shared", BLD.groups.shared)}
${features()}
${drawings()}
${outputs()}
${tools()}
${gallery()}
${start()}
</main>
<footer class="wrap"><span>Cadpole is open source under Apache-2.0.</span><a href="${GH}">github.com/mikelikespie/cadpole</a></footer>
<nav class="bar" aria-label="Quick actions"><a class="btn primary" href="#start">Get started</a><a class="btn" href="${GH}">${ICON_GH}GitHub</a><button class="btn icon" type="button" data-share hidden aria-label="Share this page">${ICON_SHARE}</button></nav>
<dialog class="sheet" aria-labelledby="sheet-title"><div class="sheet-in">
<div class="sheet-head"><h3 id="sheet-title" data-sheet-title>Model</h3><div class="sheet-acts">${fullLink("mascot")}<button class="btn small primary" type="button" data-sheet-close>Close</button></div></div>
<div class="sheet-view"><img class="sheet-pic" alt="" decoding="async"><div class="live"></div><div class="sheet-draw" hidden></div></div>
<div class="sheet-info" hidden></div>
<p class="sheet-foot" role="status"></p>
</div></dialog>
<p class="sr" role="status" id="live-status"></p>
<script type="application/json" id="full-viewers">${JSON.stringify(full)}</script>
</body>
</html>
`;
}

// ---- full viewer: the Cadpole web app, one page per model ---------------------
// `cadpole publish` wrote each model's viewer into <pages>/<name>/. The app
// script and stylesheet are the same for every model, so the site keeps one
// copy; a page's camera, cut and mode come from its link, and its way back
// from --home-url. Each published model also becomes models/<name>.cpmesh,
// the mesh-only copy the page's own 3D draws.
async function views() {
  mkdirSync(join(OUT, "view"));
  mkdirSync(join(OUT, "models"));
  const full = {}, shared = new Set();
  for (const name of readdirSync(args.pages).sort()) {
    const src = join(args.pages, name);
    const meta = json(join(src, "cadpole-publish.json"));
    const dst = join(OUT, "view", name);
    mkdirSync(dst);
    let doc = read(join(src, "index.html"));
    for (const f of meta.files) {
      if (/^(app|styles)\./.test(f)) {
        if (!shared.has(f)) { cpSync(join(src, f), join(OUT, "view", f)); shared.add(f); }
        if (!doc.includes(`"./${f}"`)) throw new Error(`${name}: the page does not load ./${f}`);
        doc = doc.replaceAll(`"./${f}"`, `"../${f}"`);
      } else if (f === meta.model) cpSync(join(src, f), join(dst, f));
    }
    writeFileSync(join(dst, "index.html"), doc);
    full[name] = meta.source_sha256.slice(0, 16);
    writeFileSync(join(OUT, "models", `${name}.cpmesh`), await slim(readFileSync(join(src, meta.model))));
  }
  if (shared.size !== 2) throw new Error(`the viewer pages disagree about the app they load: ${[...shared].join(", ")}`);
  return full;
}

/** Every local address the page names has a file behind it, or is one the lock says assembly brings. */
function checkLinks(doc) {
  const late = (u) => Object.keys(LOCK.sets).some((d) => u.startsWith(d + "/")) || u in LOCK.files;
  const missing = [];
  for (const m of doc.matchAll(/\s(?:href|src|data-pic|data-svg|data-draw|data-poster|data-sheet3d|data-open3d|data-model)="([^"#?]+)[^"]*"/g)) {
    const u = m[1];
    if (/^(https?:|mailto:|data:)/.test(u)) continue;
    const p = join(OUT, u);
    if (!(existsSync(p) && (statSync(p).isFile() || existsSync(join(p, "index.html")))) && !late(u)) missing.push(u);
  }
  if (missing.length) throw new Error(`the page names files the site does not have: ${[...new Set(missing)].join(", ")}`);
}

/** Words the page never uses, and the one place a product name may appear. */
function checkWords(doc) {
  const a = doc.indexOf('<section class="block wrap" id="agent">'), b = doc.indexOf("</section>", a);
  const agentPart = doc.slice(a, b), rest = doc.slice(0, a) + doc.slice(b);
  const bad = [];
  if (/(made|built|endorsed|sponsored|powered|official)[^.]{0,40}Claude/i.test(agentPart.replace(/not affiliated with or endorsed by the maker of Claude Code/, ""))) bad.push("a claim about Claude");
  if ((agentPart.match(/Claude/g) ?? []).length > 6) bad.push("Claude named more than the section needs");
  for (const w of ["Claude", "/home/", "/Users/", "formal", "Quint", "TLA+", "invariant", "model check", "verified", "supply chain", "supply-chain", "cargo-deny", "licence check", "license check"]) if (rest.includes(w)) bad.push(w);
  if (bad.length) throw new Error(`the page says what it must not: ${bad.join("; ")}`);
}

rmSync(OUT, { recursive: true, force: true });
mkdirSync(OUT, { recursive: true });
cpSync(STATIC, OUT, { recursive: true });
mkdirSync(join(OUT, "engine"));
cpSync(join(SITE, "src/site-engine.js"), join(OUT, "engine/site-engine.js"));
for (const f of ["engine-worker.js", "cadpole_wasm.js", "cadpole_wasm_bg.wasm"]) cpSync(join(WEB, f), join(OUT, "engine", f));
cpSync(args.embed, join(OUT, "site3d.js"));
cpSync(join(SITE, "src/site.css"), join(OUT, "site.css"));
cpSync(join(SITE, "src/site.js"), join(OUT, "site.js"));
mkdirSync(join(OUT, "scripts"));
for (const fn of new Set([...PLAY.map((p) => p[1]), ...GALLERY.map((g) => g[4]), "cabin_plan.star", "house_set.star", ...BLD.sections.map((b) => b.script)])) cpSync(join(EX, fn), join(OUT, "scripts", fn));
writeFileSync(join(OUT, "site.webmanifest"), JSON.stringify({
  name: "Cadpole", short_name: "Cadpole", start_url: "./", display: "browser", background_color: "#060a09", theme_color: "#060a09",
  icons: [{ src: "icon-192.png", sizes: "192x192", type: "image/png" }, { src: "icon-512.png", sizes: "512x512", type: "image/png" }],
}, null, 1));
writeFileSync(join(OUT, ".nojekyll"), "");
const full = await views();
const doc = page(full);
checkWords(doc);
checkLinks(doc);
writeFileSync(join(OUT, "index.html"), doc);
console.log(`site: ${Object.keys(full).length} viewer pages, index.html ${doc.length} bytes, engine ${ENGINE_BYTES} bytes (${ENGINE_MB} MB compressed), ${basename(OUT)}`);
