#!/usr/bin/env node
// Dev server for the web UI without the Rust CLI.
//
//   node devserver.mjs <dist-dir> [--port 7331] [--static] [--script file.star]
//   bazel run //src/web:devserver -- --port 7331
//
// Serves the static files from <dist-dir> and an in-memory implementation of
// the `cadpole serve` session API (docs/architecture.md): the operation registry
// at `POST /api/cadpole.v1.CadpoleOps/<Rpc>` -- unary Connect, binary protobuf
// -- with the `/api/ops/<Rpc>` alias beside it, and the SSE
// stream. With --static the /api routes 404 so the UI runs in static mode.
//
// The protobuf types are the generated ones from //src/web:cadpole_pb -- the same
// schema the browser and the real server use -- so this file cannot drift into
// being a second definition of the wire format.
//
// The built-in sample script is written for the mock engine (mock.ts only
// scrapes `param(`, `sketch(` and `extrude(`); the real Starlark API differs).
// `--script <file>` serves a different one -- that is how //src/web:e2e_test
// feeds the real wasm engine a script it can actually evaluate
// (e2e/real-engine.star).

import { createServer } from "node:http";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, appendFileSync, writeFileSync } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { extname, isAbsolute, join, normalize, relative, resolve, sep } from "node:path";
import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import {
  GetSessionRequestSchema,
  PostMessageRequestSchema,
  PostMessageResponseSchema,
  ReplyFinish,
  ReplyRequestSchema,
  ReplyResponseSchema,
  SaveRequestSchema,
  SaveResponseSchema,
  SessionSchema,
  SetParamsRequestSchema,
  SetParamsResponseSchema,
  SetScriptRequestSchema,
  SetScriptResponseSchema,
  SetSelectionRequestSchema,
  SetSelectionResponseSchema,
  StreamReplyRequestSchema,
  StreamReplyResponseSchema,
} from "./gen/cadpole/v1/ops_pb.js";
import { EvaluateRequestSchema, EvaluateResponseSchema } from "./gen/cadpole/v1/engine_pb.js";

const args = process.argv.slice(2);
const flagValue = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
// The flags that take a value: their values are not the dist directory.
const VALUE_FLAGS = new Set(["--port", "--script", "--token"]);
const distArg = args.find((a, i) => !a.startsWith("--") && !VALUE_FLAGS.has(args[i - 1] ?? "")) ?? "dist";
const portArg = flagValue("--port");
const port = portArg !== undefined ? Number(portArg) : Number(process.env.PORT ?? 7331);
const staticOnly = args.includes("--static");
const scriptFile = flagValue("--script");
const sourceDist = resolve(process.cwd(), distArg);
// `--token <t>` makes the dev server behave like `cadpole serve` does about
// authentication: every /api request needs `Authorization: Bearer <t>`, and
// `?token=` is not accepted anywhere, the event stream included. The
// e2e suite uses it to exercise the browser's half of that.
const requiredToken = flagValue("--token");
// `--hot` makes this dev server able to *rebuild*, which is what the hot
// reload suite needs and nothing else does. It serves a writable copy
// of <dist-dir> and exposes `POST /api/hot/<what>`, which edits one file of
// that copy and emits the `build_changed` the real server's watcher would.
// Off by default, because a route that rewrites the bundle is not something a
// dev server should have lying around.
const hotReload = args.includes("--hot");

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".map": "application/json",
  ".json": "application/json",
  ".webmanifest": "application/manifest+json",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
};

const DEFAULT_SCRIPT = `# Plate with a cylindrical boss (dev server sample).
# Parameters show up in the Parameters panel; drag them and the model re-evaluates.

width = param("width", 60, min=20, max=200, step=1, unit="mm", description="Plate width (X)")
depth = param("depth", 40, min=20, max=200, step=1, unit="mm", description="Plate depth (Y)")
height = param("height", 20, min=2, max=100, step=0.5, unit="mm", description="Plate thickness (Z)")
boss_d = param("boss_d", 18, min=2, max=120, step=0.5, unit="mm", description="Boss diameter")
boss_h = param("boss_h", 12, min=0, max=100, step=0.5, unit="mm", description="Boss height")
with_boss = param("with_boss", True, description="Include the boss")
finish = param("finish", "matte", choices=["matte", "satin", "gloss"], description="Surface finish")

base_profile = sketch(plane="XY").rect(width, depth, center=True)
base = extrude(base_profile, distance=height, name="base")

boss_profile = sketch(plane=base.face("top")).circle(d=boss_d)
boss = extrude(boss_profile, distance=boss_h, name="boss")
`;

const scriptPath = scriptFile ? resolve(process.cwd(), scriptFile) : null;
const scriptSource = scriptPath ? readFileSync(scriptPath, "utf8") : DEFAULT_SCRIPT;

// ---------------------------------------------------------------- session

// Kept in the shape of `cadpole.v1.Session` (camelCase, one `ParamValue` oneof
// per override) so it encodes with `create(SessionSchema, ...)` and no
// translation table sits in the middle. 64-bit fields become BigInt on the way
// out, not here.
const session = {
  id: "dev-session",
  revision: 1,
  document: {
    id: "dev-doc",
    name: "devserver",
    displayUnit: "mm",
    scripts: [{ path: "main.star", source: scriptSource }],
    paramOverrides: {},
  },
  selection: { selected: [], hovered: undefined, note: "" },
  messages: [],
  clients: 0,
};
let seq = 0;
const events = [];
const sseClients = new Set();

function emit(kind, data) {
  const e = { seq: ++seq, at: new Date().toISOString(), kind, data };
  events.push(e);
  if (events.length > 500) events.shift();
  for (const res of sseClients) res.write(`id: ${e.seq}\nevent: ${kind}\ndata: ${JSON.stringify(e)}\n\n`);
}

// --------------------------------------------------------------- hot reload
//
// Rebuild notifications, for `//src/web:e2e_test` and for anyone driving
// the UI by hand. `cadpole serve` fingerprints the built bundle file by file and
// tells open pages when it changes; the page then does the least that
// suffices -- swap a stylesheet, swap the engine worker, or (only for the UI
// bundle itself) offer a reload. Exercising that needs a server that can
// actually change one file of the bundle and leave the rest alone, which is
// exactly what a read-only `bazel-bin/src/web/dist` cannot do.
//
// So with `--hot` this serves a writable *copy* and offers one route:
//
//   POST /api/hot/styles   append a rule to styles.css
//   POST /api/hot/engine   append a custom section to cadpole_wasm_bg.wasm
//   POST /api/hot/glue     append a comment to cadpole_wasm.js
//   POST /api/hot/worker   append a comment to engine-worker.js
//   POST /api/hot/app      append a comment to app.js
//
// Each answers `{ build }` and emits the same `build_changed` the real
// watcher emits.
//
// `engine-worker.js` is fingerprinted here and not (yet) by
// `WEB_BUILD_FILES` in src/rust/cadpole-cli/src/server.rs. Nothing compares one
// server's ids against the other's -- a page only ever diffs two ids from the
// same server -- so the two lists may differ, and this one is ahead because
// the worker bundle is where evaluation happens and a page can
// replace it without reloading.
const WEB_BUILD_FILES = ["index.html", "app.js", "styles.css", "cadpole_wasm_bg.wasm", "cadpole_wasm.js", "engine-worker.js"];

const dist = hotReload ? mkdtempSync(join(tmpdir(), "cadpole-hot-dist-")) : sourceDist;
if (hotReload) {
  // File by file, following links: under Bazel the files of dist are
  // symlinks into the build output, and `cpSync` keeps nested links even
  // with `dereference`. Copied as links, every rewrite below (and the chmod)
  // would land in that shared output, so one run's edits would show up in
  // the next run, or in a concurrent one, before it had rebuilt anything.
  const copyTree = (from, to) => {
    for (const name of readdirSync(from)) {
      const src = join(from, name);
      if (statSync(src).isDirectory()) {
        mkdirSync(join(to, name));
        copyTree(src, join(to, name));
      } else {
        copyFileSync(src, join(to, name));
      }
    }
  };
  copyTree(sourceDist, dist);
  // Bazel outputs are read-only; the whole point here is to rewrite them.
  for (const name of readdirSync(dist)) {
    try {
      chmodSync(join(dist, name), 0o644);
    } catch {
      /* directories and anything already writable */
    }
  }
  process.on("exit", () => {
    try {
      rmSync(dist, { recursive: true, force: true });
    } catch {
      /* best effort */
    }
  });
}

/**
 * The bundle fingerprint, in the shape `cadpole serve` produces: one
 * `name:size:hash` field per file, joined with `|`, and `name:-` for a file
 * that is not there.
 *
 * The *shape* is the contract -- `src/web/src/hot-reload.ts` splits on it and
 * compares fields -- and the hash function is not, so this uses sha256 where
 * the Rust uses a much faster FNV. Nothing compares one server's ids against
 * the other's; a page only ever compares two ids from the same server.
 */
function computeBuildId() {
  return WEB_BUILD_FILES.map((name) => {
    const p = join(dist, name);
    try {
      const bytes = readFileSync(p);
      return `${name}:${bytes.length}:${createHash("sha256").update(bytes).digest("hex").slice(0, 16)}`;
    } catch {
      return `${name}:-`;
    }
  }).join("|");
}

let webBuild = hotReload ? computeBuildId() : "";
let hotCounter = 0;

/** A valid, ignorable WebAssembly custom section: id 0, LEB length, name, payload. */
function wasmCustomSection(name, payload) {
  const leb = (n) => {
    const out = [];
    do {
      let b = n & 0x7f;
      n >>>= 7;
      if (n) b |= 0x80;
      out.push(b);
    } while (n);
    return Buffer.from(out);
  };
  const nameBytes = Buffer.from(name, "utf8");
  const body = Buffer.concat([leb(nameBytes.length), nameBytes, Buffer.from(payload, "utf8")]);
  return Buffer.concat([Buffer.from([0x00]), leb(body.length), body]);
}

/**
 * Change one file of the served bundle the way a rebuild would, and tell
 * everyone.
 *
 * Every mutation is *semantically inert* on purpose. A stylesheet gains a
 * custom property nothing reads unless a test asks for it; the wasm gains a
 * custom section, which is the one thing you can append to a module and have
 * it still validate and behave identically; the glue and the app bundle gain
 * a comment. What is under test is the mechanism, not a behaviour change, and
 * an engine that computed something different after a swap would make "did
 * the swap happen" and "is the new engine right" the same assertion.
 */
function rebuildWebFile(what) {
  const n = ++hotCounter;
  if (what === "styles") {
    appendFileSync(join(dist, "styles.css"), `\n:root { --cadpole-hot: "${n}"; }\n`);
  } else if (what === "engine") {
    const p = join(dist, "cadpole_wasm_bg.wasm");
    writeFileSync(p, Buffer.concat([readFileSync(p), wasmCustomSection("cadpole.hot", String(n))]));
  } else if (what === "glue") {
    appendFileSync(join(dist, "cadpole_wasm.js"), `\n// cadpole hot ${n}\n`);
  } else if (what === "worker") {
    appendFileSync(join(dist, "engine-worker.js"), `\n// cadpole hot ${n}\n`);
  } else if (what === "app") {
    appendFileSync(join(dist, "app.js"), `\n// cadpole hot ${n}\n`);
  } else {
    return null;
  }
  webBuild = computeBuildId();
  emit("build_changed", { build: webBuild });
  console.log(`  hot: rebuilt ${what}; told ${sseClients.size} open page(s)`);
  return webBuild;
}

function json(res, status, body) {
  const s = JSON.stringify(body);
  res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store", "content-length": Buffer.byteLength(s) });
  res.end(s);
}

/** One protobuf message as the response body, the way `cadpole serve` sends it. */
function proto(res, schema, message) {
  const bytes = Buffer.from(toBinary(schema, create(schema, message)));
  res.writeHead(200, { "content-type": "application/proto", "cache-control": "no-store", "content-length": bytes.length });
  res.end(bytes);
}

async function readBytes(req) {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  return new Uint8Array(Buffer.concat(chunks));
}

/** A `cadpole.v1.ParamValue` as the plain value the event stream carries. */
function paramValueJson(v) {
  return v?.value?.value ?? null;
}

function overridesJson() {
  return Object.fromEntries(Object.entries(session.document.paramOverrides).map(([k, v]) => [k, paramValueJson(v)]));
}

/** The session, as `cadpole.v1.Session` wants it (64-bit fields as BigInt). */
function sessionMessage() {
  return {
    ...session,
    revision: BigInt(session.revision),
    lastEventSeq: BigInt(seq),
    // What `cadpole serve` reports, so the page can tell on load whether it is
    // running the bundle being served. Empty without `--hot`, which
    // is the same thing the real server says when it serves no web directory.
    webBuild,
    messages: session.messages.map((m) => ({
      seq: BigInt(m.seq),
      from: m.from,
      text: m.text,
      at: m.at,
      attachments: m.images?.length ?? 0,
      state: m.state ?? "",
      activity: m.activity ?? "",
    })),
    assistantListening: listening,
  };
}

/** The JSON an event carries for a message: no image bytes, like the real server. */
function messageJson(m) {
  return {
    seq: m.seq,
    from: m.from,
    text: m.text,
    at: m.at,
    attachments: (m.images ?? []).map(() => ({ kind: "screenshot", png_base64: "" })),
    ...(m.state ? { state: m.state } : {}),
  };
}

// A reply written a piece at a time, as `cadpole serve` handles `StreamReply`:
// the first call starts a message, later ones append to it, the last ends it.
// What it gains is sent as a `reply_stream` frame, with no id, to the pages
// that asked with `?live=1`; the one event is the finished message. The
// limits and the author check are the real server's business and are not
// repeated here.
const liveClients = new Set();
/** The Listen switch (`PUT /api/session/assistant`). */
let listening = false;

function streamReply(r) {
  const finish = r.finish === ReplyFinish.DONE ? "done" : r.finish === ReplyFinish.FAILED ? "failed" : "";
  let m = session.messages.find((x) => x.seq === Number(r.seq));
  if (Number(r.seq) === 0) {
    m = { seq: (session.messages.at(-1)?.seq ?? 0) + 1, from: "assistant", text: "", at: new Date().toISOString(), images: [], state: "streaming", activity: "" };
    session.messages.push(m);
  } else if (!m || m.state !== "streaming") {
    throw new Error(`message ${r.seq} is not in progress`);
  }
  const offset = Buffer.byteLength(m.text);
  m.text += r.append;
  m.activity = finish ? "" : r.activity;
  if (finish) {
    m.state = finish;
    emit("message", messageJson(m));
  } else {
    const frame = { seq: 0, kind: "reply_stream", data: { seq: m.seq, from: m.from, at: m.at, state: m.state, offset, length: Buffer.byteLength(m.text), append: r.append, activity: m.activity } };
    for (const res of liveClients) res.write(`event: reply_stream\ndata: ${JSON.stringify(frame)}\n\n`);
  }
  return m;
}

/** The dev server's own assistant: its answer, written the way a model's arrives. */
async function answerInPieces(text) {
  const pause = (ms) => new Promise((r) => setTimeout(r, ms));
  await pause(300);
  const m = streamReply({ seq: 0, append: "", activity: "thinking…", finish: 0 });
  await pause(200);
  const pieces = text.match(/(?:\S+\s*){1,3}/g) ?? [text];
  for (const [i, piece] of pieces.entries()) {
    streamReply({ seq: m.seq, append: piece, activity: "", finish: i === pieces.length - 1 ? ReplyFinish.DONE : 0 });
    await pause(40);
  }
}

function appendMessage(from, text, images) {
  const m = { seq: (session.messages.at(-1)?.seq ?? 0) + 1, from, text, at: new Date().toISOString(), images: images ?? [] };
  session.messages.push(m);
  emit("message", messageJson(m));
  return m;
}

// ---------------------------------------------------------------------------
// Evaluate: the server side of the remote engine.
//
// This dev server cannot run the real engine -- it is Node, and `cadpole_wasm.js`
// is the web target -- so it builds a box from the document's parameters and
// returns it as a real `cadpole.v1.Model`, meshes and all. That is enough for the
// thing under test, which is not the geometry: it is that the browser encodes
// an `EvaluateRequest`, decodes an `EvaluateResponse`, turns
// `Body.mesh.positions` into triangles the viewer draws, and reports "server"
// as the engine that ran. A mistake anywhere in that shows up as the wrong box.
//
// Two behaviours are driven from the script text rather than from a command
// line flag, so one dev server exercises every case and the test drives it by
// editing the document the way a user would:
//
//   # devserver: fail-eval        Evaluate answers 500, so the browser has to
//                                 fall back to its local engine and say so.
//   # devserver: slow-eval 2000   Evaluate takes that long, so a burst has
//                                 something to be in flight during and the
//                                 Cancel button has something to cancel.

/**
 * The `param(...)` calls in a script, as `cadpole.v1.ParamInfo`.
 *
 * Scraped with a regular expression, the way `mock.ts` does it, and for the
 * same reason: this is not an interpreter. It matters more than it looks.
 * The Parameters panel is built from whatever model the *last* evaluation
 * produced, so if this stand-in reported a different set of parameters from
 * the ones the real engine reports, the panel would change every time the user
 * switched engines -- and every control they were holding would vanish. In
 * production both engines run the same script through the same code and the
 * question does not arise; here it has to be arranged.
 */
function scrapeParams(source, overrides) {
  const out = [];
  const re = /\bparam\(\s*"([A-Za-z_][A-Za-z0-9_]*)"\s*,\s*([^,)]+)([^)]*)\)/g;
  for (let m = re.exec(source); m !== null; m = re.exec(source)) {
    const [, name, rawDefault, rest] = m;
    const lit = rawDefault.trim();
    let def;
    if (lit === "True" || lit === "False") def = { case: "boolean", value: lit === "True" };
    else if (/^["']/.test(lit)) def = { case: "text", value: lit.slice(1, -1) };
    else if (Number.isFinite(Number(lit))) def = { case: "number", value: Number(lit) };
    else continue;
    const kw = (k) => {
      const hit = new RegExp(`\\b${k}\\s*=\\s*(-?[0-9.]+)`).exec(rest ?? "");
      return hit ? Number(hit[1]) : undefined;
    };
    const doc = /\b(?:doc|description)\s*=\s*"([^"]*)"/.exec(rest ?? "");
    const unit = /\bunit\s*=\s*"([^"]*)"/.exec(rest ?? "");
    // `choices=["a", "b"]`: the panel builds a segmented control or a picker
    // from these, so they have to travel from here as they do from
    // the real engine, or the row would change shape when the engine does.
    const list = /\bchoices\s*=\s*\[([^\]]*)\]/.exec(rest ?? "");
    const choices = list
      ? list[1]
          .split(",")
          .map((c) => c.trim().replace(/^["']|["']$/g, ""))
          .filter((c) => c.length > 0)
      : [];
    const override = overrides[name];
    const value = override?.value ?? def;
    out.push({
      name,
      defaultValue: { value: def },
      value: { value },
      overridden: override !== undefined,
      description: doc ? doc[1] : "",
      unit: unit ? unit[1] : "",
      min: kw("min"),
      max: kw("max"),
      step: kw("step"),
      choices,
    });
  }
  return out;
}

/**
 * The plates the real engine would lay out, scraped from the
 * script's `export(..., "x.3mf", flavor=flavor.bambu, plate=..., ...)` calls
 * the way `mock.ts` scrapes them, so the plates view can be exercised
 * against this server too. The stand-in model has one body, `blk`, and it
 * stands on every plate the script exports anything to: what is under test
 * is that `Model.plates` travels and is drawn, not which body is where.
 */
function scrapePlates(source, body) {
  const re = /\bexport\(\s*(?:\[[^\]]*\]|[A-Za-z_][A-Za-z0-9_]*)\s*,\s*"([^"]+)"([^)]*)\)/g;
  const names = new Map();
  const calls = [];
  for (let m = re.exec(source); m !== null; m = re.exec(source)) {
    const [, path, rest] = m;
    if (!/\bflavor\s*=\s*flavor\.bambu\b/.test(rest)) continue;
    const list = names.get(path) ?? [];
    names.set(path, list);
    const num = /\bplate\s*=\s*(\d+)/.exec(rest);
    const named = /\bplate\s*=\s*"([^"]*)"/.exec(rest);
    let plate = 1;
    if (num) plate = Math.max(1, Number(num[1]));
    else if (named) {
      const at = list.indexOf(named[1]);
      plate = at >= 0 ? at + 1 : list.push(named[1]);
    }
    while (list.length < plate) list.push("");
    const pin = /\bextruder\s*=\s*\[?\s*(\d+)/.exec(rest);
    const object = /\bname\s*=\s*"([^"]*)"/.exec(rest);
    calls.push({ path, plate, slot: pin ? Number(pin[1]) : 1, object: object ? object[1] : body.name });
  }
  const bed = 256;
  const colors = [[0.91, 0.72, 0.29], [0.18, 0.56, 0.56], [0.85, 0.29, 0.24], [0.29, 0.5, 0.84]];
  const out = [];
  for (const [path, list] of names) {
    list.forEach((name, i) => {
      const call = calls.find((c) => c.path === path && c.plate === i + 1);
      // Bambu's grid: ceil(sqrt(N)) plates to a row.
      const cols = Math.max(1, Math.ceil(Math.sqrt(list.length)));
      const origin = { x: (i % cols) * 1.2 * bed, y: -Math.floor(i / cols) * 1.2 * bed, z: 0 };
      const { min, max } = body.bbox;
      const shift = call ? [bed / 2 - (min.x + max.x) / 2, bed / 2 - (min.y + max.y) / 2] : [0, 0];
      const c = colors[((call?.slot ?? 1) - 1) % colors.length];
      out.push({
        index: i + 1,
        name,
        path,
        bedX: bed,
        bedY: bed,
        origin,
        bodies: call
          ? [{
              bodyId: body.id,
              object: call.object,
              transform: { translation: { x: origin.x + shift[0], y: origin.y + shift[1], z: 0 }, rotation: { x: 0, y: 0, z: 0, w: 1 } },
              slot: call.slot,
              color: { r: c[0], g: c[1], b: c[2] },
              filament: "",
            }]
          : [],
      });
    });
  }
  return out;
}

/** A box as a cadpole.v1.Model: 8 vertices, 12 edges, 6 named faces, 12 triangles. */
function boxModel({ width, depth, height, tolerance, params, source, colors = false }) {
  const x0 = -width / 2, x1 = width / 2, y0 = -depth / 2, y1 = depth / 2, z0 = 0, z1 = height;
  const corners = [
    [x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0],
    [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1],
  ];
  const faceDefs = [
    { name: "blk/bottom", quad: [0, 3, 2, 1], normal: [0, 0, -1] },
    { name: "blk/top", quad: [4, 5, 6, 7], normal: [0, 0, 1] },
    { name: "blk/front", quad: [0, 1, 5, 4], normal: [0, -1, 0] },
    { name: "blk/right", quad: [1, 2, 6, 5], normal: [1, 0, 0] },
    { name: "blk/back", quad: [2, 3, 7, 6], normal: [0, 1, 0] },
    { name: "blk/left", quad: [3, 0, 4, 7], normal: [-1, 0, 0] },
  ];
  const positions = [], normals = [], indices = [], faceIds = [], faces = [];
  faceDefs.forEach((f, fi) => {
    const base = positions.length / 3;
    for (const c of f.quad) {
      positions.push(corners[c][0], corners[c][1], corners[c][2]);
      normals.push(f.normal[0], f.normal[1], f.normal[2]);
    }
    indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
    faceIds.push(fi, fi);
    const mid = (i) => f.quad.reduce((a, c) => a + corners[c][i], 0) / 4;
    faces.push({
      name: f.name,
      surfaceKind: "plane",
      centroid: { x: mid(0), y: mid(1), z: mid(2) },
      normal: { x: f.normal[0], y: f.normal[1], z: f.normal[2] },
      area: 0,
      edges: [],
      triStart: fi * 2,
      triCount: 2,
      index: fi,
      aliases: [],
    });
  });
  const edgePairs = [[0,1],[1,2],[2,3],[3,0],[4,5],[5,6],[6,7],[7,4],[0,4],[1,5],[2,6],[3,7]];
  const edges = edgePairs.map(([a, b], i) => {
    const p = corners[a], q = corners[b];
    const v = (c) => ({ x: c[0], y: c[1], z: c[2] });
    return {
      name: `blk/e${i}`,
      curveKind: "line",
      start: v(p),
      end: v(q),
      length: Math.hypot(q[0] - p[0], q[1] - p[1], q[2] - p[2]),
      faces: [],
      polyline: [v(p), v(q)],
      dihedralDegrees: 90,
      convex: true,
      index: i,
    };
  });
  const vertices = corners.map((c, i) => ({ name: `blk/v${i}`, position: { x: c[0], y: c[1], z: c[2] }, index: i, faces: [] }));
  const body = {
    id: "extrude_1#0",
    featureId: "extrude_1",
    name: "blk",
    bbox: { min: { x: x0, y: y0, z: z0 }, max: { x: x1, y: y1, z: z1 } },
    topology: { faces, edges, vertices },
    // `server_colors`: the stand-in box carries a colour per vertex,
    // as an imported scan's mesh does -- red below, amber above -- so the
    // test can see the viewer draw `Mesh.colors` and not its body colour.
    mesh: colors
      ? {
          positions,
          normals,
          indices,
          faceIds,
          colors: Uint8Array.from({ length: positions.length }, (_, i) => {
            const z = positions[i - (i % 3) + 2];
            return z > (z0 + z1) / 2 ? [230, 150, 20][i % 3] : [220, 30, 20][i % 3];
          }),
        }
      : { positions, normals, indices, faceIds },
    visible: true,
  };
  return {
    params,
    features: [{
      id: "extrude_1",
      kind: "extrude",
      name: "blk",
      inputs: [],
      outputs: ["extrude_1#0"],
      args: { tolerance: String(tolerance) },
      suppressed: false,
      diagnostics: [],
    }],
    bodies: [body],
    diagnostics: [],
    evalMillis: 0,
    plates: scrapePlates(source ?? "", body),
  };
}

/** `# devserver: <directive>` lines in the document's scripts. */
/**
 * How the test asked this server to behave, from the document's parameters.
 *
 * Not from the script text, and not from a command line flag. The UI does not
 * expose script editing in session mode, so a test cannot change the source;
 * it *can* move a slider, and moving a slider is the gesture the whole
 * in-flight design is about. `e2e/engine.star` declares `server_fail` and
 * `server_slow_ms` for exactly this, and the real engine ignores both.
 */
function evalDirectives(doc) {
  const ov = doc.paramOverrides ?? {};
  const val = (name) => ov[name]?.value;
  const slow = val("server_slow_ms");
  const fail = val("server_fail");
  const colors = val("server_colors");
  return {
    fail: fail?.case === "boolean" ? fail.value : false,
    slowMs: slow?.case === "number" ? slow.value : 0,
    colors: colors?.case === "boolean" ? colors.value : false,
  };
}

// The operations the UI calls, by rpc name. Each takes the decoded request and
// returns [schema, message] for the response.
const OPS = {
  GetSession: {
    request: GetSessionRequestSchema,
    run: () => [SessionSchema, sessionMessage()],
  },
  SetScript: {
    request: SetScriptRequestSchema,
    run: (r) => {
      const path = r.path || "main.star";
      if (r.baseRevision !== 0n && r.baseRevision !== BigInt(session.revision)) {
        const current = session.document.scripts.find((s) => s.path === path)?.source ?? "";
        return [SetScriptResponseSchema, { conflict: true, currentRevision: BigInt(session.revision), currentSource: current }];
      }
      const file = { path, source: r.source };
      const i = session.document.scripts.findIndex((s) => s.path === path);
      if (i >= 0) session.document.scripts[i] = file;
      else session.document.scripts.push(file);
      session.revision++;
      emit("script_changed", { path, revision: session.revision });
      return [SetScriptResponseSchema, { revision: BigInt(session.revision) }];
    },
  },
  SetParams: {
    request: SetParamsRequestSchema,
    run: (r) => {
      for (const [k, v] of Object.entries(r.overrides)) session.document.paramOverrides[k] = v;
      for (const k of r.clear) delete session.document.paramOverrides[k];
      session.revision++;
      emit("params_changed", { overrides: overridesJson(), revision: session.revision });
      return [SetParamsResponseSchema, { revision: BigInt(session.revision), paramOverrides: session.document.paramOverrides }];
    },
  },
  SetSelection: {
    request: SetSelectionRequestSchema,
    run: (r) => {
      session.selection = { selected: r.selected, hovered: r.hovered, note: r.note };
      emit("selection_changed", {
        selected: r.selected.map((s) => ({ feature_id: s.featureId, body_id: s.bodyId, kind: s.kind, topo_name: s.topoName })),
        note: r.note,
      });
      return [SetSelectionResponseSchema, { revision: BigInt(session.revision), selected: r.selected.length }];
    },
  },
  PostMessage: {
    request: PostMessageRequestSchema,
    run: (r) => {
      if (r.attachments.length) {
        console.log(`  message: ${r.attachments.length} screenshot(s), ${r.attachments[0].png.length} bytes`);
      }
      const m = appendMessage("user", r.text, r.attachments);
      // A tiny fake assistant so the chat is exercisable end-to-end. A
      // message that asks for silence gets none, for a test that plays the
      // assistant itself.
      if (!m.text.includes("[no reply]")) {
        const sel = session.selection.selected.map((s) => `${s.kind} ${s.topoName || s.bodyId || s.featureId}`).join(", ");
        void answerInPieces(`(dev server) I received \u201c${m.text}\u201d. ${sel ? `You have selected: ${sel}.` : "Nothing is selected."}`);
      }
      return [
        PostMessageResponseSchema,
        { message: { seq: BigInt(m.seq), from: m.from, text: m.text, at: m.at, attachments: m.images.length } },
      ];
    },
  },
  Reply: {
    request: ReplyRequestSchema,
    run: (r) => [ReplyResponseSchema, { seq: BigInt(appendMessage("assistant", r.text, []).seq) }],
  },
  StreamReply: {
    request: StreamReplyRequestSchema,
    run: (r) => {
      const m = streamReply(r);
      return [StreamReplyResponseSchema, { seq: BigInt(m.seq), state: m.state, length: BigInt(Buffer.byteLength(m.text)), throttled: false }];
    },
  },
  Save: {
    request: SaveRequestSchema,
    run: () => [SaveResponseSchema, { path: "/tmp/devserver-document" }],
  },
  Evaluate: {
    request: EvaluateRequestSchema,
    run: async (r) => {
      // `fill_document` on the real server fills an absent document from the
      // session; the browser always sends one (remote-engine.ts says why), and
      // this mirrors both.
      const doc = r.document ?? session.document;
      const { fail, slowMs, colors } = evalDirectives(doc);
      if (fail) throw new Error("the dev server was told to fail this evaluation");
      if (slowMs > 0) await new Promise((res) => setTimeout(res, slowMs));
      const pick = (name, def) => {
        const v = (doc.paramOverrides ?? {})[name];
        return v && v.value?.case === "number" ? v.value.value : def;
      };
      const t0 = Date.now();
      const source = (doc.scripts ?? []).map((s) => s.source ?? "").join("\n");
      const model = boxModel({
        // `width`/`depth`/`height` for the built-in sample; `size`/`thickness`
        // for e2e/engine.star, so a slider drag in either document changes the
        // shape this server answers with and a stale mesh would be visible.
        width: pick("width", pick("size", 60)),
        depth: pick("depth", pick("size", 40)),
        height: pick("height", pick("thickness", 20)),
        tolerance: r.tolerance || 0.05,
        params: scrapeParams(source, doc.paramOverrides ?? {}),
        source,
        colors,
      });
      model.evalMillis = Date.now() - t0;
      return [EvaluateResponseSchema, { model }];
    },
  },
};

async function handleApi(req, res, url) {
  const path = url.pathname;
  const hot = req.method === "POST" ? path.match(/^\/api\/hot\/([a-z]+)$/)?.[1] : undefined;
  if (hot) {
    if (!hotReload) return json(res, 404, { error: "the dev server was not started with --hot" });
    const build = rebuildWebFile(hot);
    if (build === null) return json(res, 400, { error: `unknown target "${hot}" (styles, engine, glue, worker, app)` });
    return json(res, 200, { build });
  }
  // A test plays the assistant through this: one `StreamReply` call as JSON,
  // `{seq, append, activity, finish: "" | "done" | "failed"}`.
  if (req.method === "POST" && path === "/api/dev/stream-reply") {
    const body = JSON.parse(Buffer.from(await readBytes(req)).toString("utf8") || "{}");
    const finish = body.finish === "done" ? ReplyFinish.DONE : body.finish === "failed" ? ReplyFinish.FAILED : 0;
    try {
      const m = streamReply({ seq: Number(body.seq ?? 0), append: String(body.append ?? ""), activity: String(body.activity ?? ""), finish });
      return json(res, 200, { seq: m.seq, state: m.state });
    } catch (err) {
      return json(res, 400, { error: String(err?.message ?? err) });
    }
  }
  if (req.method === "PUT" && path === "/api/session/assistant") {
    listening = Boolean(JSON.parse(Buffer.from(await readBytes(req)).toString("utf8") || "{}").listening);
    emit("assistant_listening", { listening });
    return json(res, 200, {});
  }
  if (req.method === "GET" && path === "/api/events") {
    res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-store", connection: "keep-alive" });
    res.write(": connected\n\n");
    const after = Number(url.searchParams.get("after") ?? 0);
    for (const e of events) if (e.seq > after) res.write(`id: ${e.seq}\nevent: ${e.kind}\ndata: ${JSON.stringify(e)}\n\n`);
    sseClients.add(res);
    if (!["", "0", "false"].includes(url.searchParams.get("live") ?? "")) liveClients.add(res);
    session.clients = sseClients.size;
    emit("client_joined", { clients: session.clients });
    const ping = setInterval(() => res.write(": ping\n\n"), 15000);
    req.on("close", () => {
      clearInterval(ping);
      sseClients.delete(res);
      liveClients.delete(res);
      session.clients = sseClients.size;
      emit("client_left", { clients: session.clients });
    });
    return;
  }
  // The Connect route the UI uses, and the `/api/ops/<Rpc>` alias the
  // e2e helpers still post JSON to. One dispatch, as on the real server; only
  // the shape of a refusal differs, because a Connect client reads an envelope
  // and the alias's callers read `{error}`.
  const connect = req.method === "POST" ? path.match(/^\/api\/cadpole\.v1\.CadpoleOps\/([A-Za-z0-9_]+)$/)?.[1] : undefined;
  const op = connect ?? (req.method === "POST" ? path.match(/^\/api\/ops\/([A-Za-z0-9_]+)$/)?.[1] : undefined);
  if (op) {
    const fail = (status, code, message) =>
      connect ? json(res, status, { code, message }) : json(res, status, { error: message, op });
    const impl = OPS[op];
    if (!impl) {
      return fail(connect ? 501 : 404, "unimplemented", `unknown operation "${op}" (this dev server implements the ones the UI calls)`);
    }
    const ct = String(req.headers["content-type"] ?? "");
    const proto_ct = ct.startsWith("application/proto") || ct.startsWith("application/x-protobuf") || ct.startsWith("application/protobuf");
    if (!proto_ct) {
      return fail(415, "unimplemented", "the dev server speaks the wire format only: send application/proto");
    }
    const request = fromBinary(impl.request, await readBytes(req));
    let schema, message;
    try {
      [schema, message] = await impl.run(request);
    } catch (err) {
      // What `cadpole serve` does with a failing operation: a code and a message,
      // never a protobuf body.
      return fail(500, "internal", String(err?.message ?? err));
    }
    return proto(res, schema, message);
  }
  if (path === "/api/model" || path === "/api/render") return json(res, 501, { error: "not implemented by the dev server" });
  json(res, 404, { error: "not found" });
}

async function serveStatic(req, res, url) {
  let p = decodeURIComponent(url.pathname);
  if (p.endsWith("/")) p += "index.html";
  const file = normalize(join(dist, p));
  // Inside `dist`, not merely spelled like it: a bare prefix test would let
  // `/..%2fdist-old/x` through to a sibling called `dist-old`.
  const rel = relative(dist, file);
  if (rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) {
    res.writeHead(403);
    return res.end();
  }
  try {
    const st = await stat(file);
    if (!st.isFile()) throw new Error("not a file");
    const data = await readFile(file);
    res.writeHead(200, {
      "content-type": MIME[extname(file)] ?? "application/octet-stream",
      "content-length": data.length,
      "cache-control": "no-cache",
    });
    res.end(data);
  } catch {
    res.writeHead(404, { "content-type": "text/plain" });
    res.end("not found");
  }
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", `http://${req.headers.host ?? "localhost"}`);
  try {
    if (url.pathname.startsWith("/api/")) {
      if (staticOnly) return json(res, 404, { error: "static mode" });
      if (requiredToken !== undefined) {
        const presented = (req.headers.authorization ?? "").replace(/^Bearer /, "");
        if (presented !== requiredToken) return json(res, 401, { error: "missing or wrong bearer token" });
      }
      return await handleApi(req, res, url);
    }
    await serveStatic(req, res, url);
  } catch (err) {
    console.error(err);
    if (!res.headersSent) json(res, 500, { error: String(err) });
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(
    `cadpole devserver: http://127.0.0.1:${port}/  (dist: ${dist}${staticOnly ? ", static mode" : ""}${scriptPath ? `, script: ${scriptPath}` : ""})`,
  );
  console.log("  add ?mock=1 to use the mock engine, ?static=1 to force static mode");
  if (hotReload) console.log(`  hot reload: serving a writable copy at ${dist}; POST /api/hot/{styles,engine,glue,worker,app}`);
});
