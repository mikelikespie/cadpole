// Native versus wasm: the same examples through the same engine on two
// targets, diffed to the bit.
//
//   bazel test //src/rust/cadpole-wasm:native_vs_wasm_test --test_output=all
//
// For every runnable `examples/*.star`, and every `stdlib/*_test.star` (the
// documents that exercise the embedded `@cadpole/` library), this builds
// one probe input (the script, its sibling scripts, the example assets as
// hex, the default tolerances), runs it through `native_probe` (the engine
// compiled for this
// machine) and through `Engine.probe` in the wasm engine under Node, and
// compares the two answers category by category:
//
//   names        every face name of every body in topology order (the order
//                `select` resolves in, which is canonical), and the sets
//                of edge and vertex names
//   order        the edge and vertex lists in topology order -- the `edge:N`
//                / `vertex:N` addresses, which the tessellation decides and
//                which are not names
//   counts       faces, edges, vertices, triangles, mesh vertices
//   volume       Mesh::volume, as f64 bits
//   bbox         the mesh bounding box, as f64 bits
//   topology     a digest of every face centroid/normal/area, edge
//                endpoints/length and vertex position, f64 bits
//   mesh         a digest of the f32 positions and u32 indices
//   inspect      the `Inspect` report (solid verdict, folds, surface classes)
//   keys         each body's kernel-op memo key: the digest of every
//                input that made it, which must be the same bytes on both
//                targets or a session could never share a memo across them
//   diagnostics  every diagnostic, kernel notes included (the ladder rung)
//   features     feature ids, kinds, outputs and recorded args
//   tests        for a stdlib test document, every unswept `test(...)`:
//                pass/fail, message, and what it returned as f64 bits
//   ok           whether the evaluation succeeded at all
//
// `expected_divergence.tsv` lists the (example, category) pairs still allowed
// to differ. The test fails in both directions: a pair off the list that
// differs is a regression; a pair on the list that now matches must be pruned,
// so the list only ever shrinks. The per-example table is printed at the end
// and written to the undeclared outputs as `native_vs_wasm.md`.

import { strict as assert } from "node:assert";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const CATEGORIES = ["ok", "features", "diagnostics", "tests", "names", "order", "counts", "volume", "bbox", "topology", "mesh", "inspect", "keys"];

/** A path under this test's runfiles, or an error naming what was looked for. */
function runfile(...parts) {
  const runfiles = process.env.JS_BINARY__RUNFILES ?? process.env.RUNFILES_DIR ?? "";
  for (const root of [join(runfiles, "_main"), join(runfiles, "cadpole"), resolve(".")]) {
    const p = join(root, ...parts);
    if (existsSync(p)) return p;
  }
  throw new Error(`cannot find ${parts.join("/")} (runfiles: ${runfiles || "unset"})`);
}

/** The runnable examples, sorted: not `*_future.star`, not marked planned-api. */
function examples(dir) {
  const out = [];
  for (const name of readdirSync(dir).sort()) {
    if (!name.endsWith(".star") || name.endsWith("_future.star")) continue;
    const source = readFileSync(join(dir, name), "utf8");
    if (source.split("\n", 1)[0].includes("cadpole: planned-api")) continue;
    out.push({ name, source });
  }
  return out;
}

/** The standard library's test documents: `stdlib/*_test.star`, which load `@cadpole/...` from the engine itself. */
function stdlibTests(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .sort()
    .filter((name) => name.endsWith("_test.star"))
    .map((name) => ({ name: `stdlib/${name}`, source: readFileSync(join(dir, name), "utf8") }));
}

/** The probe input for one example: the entry as main.star, every sibling under its own name. */
function probeInput(dir, all, entry, assets, runTests) {
  const scripts = [{ path: "main.star", source: entry.source }];
  for (const s of all) if (s.name !== entry.name) scripts.push({ path: s.name, source: s.source });
  return JSON.stringify({ scripts, overrides: {}, tolerance: 0, boolean_tolerance: 0, assets, run_tests: runTests });
}

function loadAssets(dir) {
  const assets = {};
  if (!existsSync(dir)) return assets;
  for (const name of readdirSync(dir).sort()) {
    if (name.startsWith(".") || name === "BUILD.bazel" || name === "BUILD") continue;
    assets[name] = readFileSync(join(dir, name)).toString("hex");
  }
  return assets;
}

/** Load the wasm-bindgen module the way the page does, from a copy Node will treat as ESM. */
async function loadWasm(distDir) {
  const tmp = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "cadpole-wasm-"));
  cpSync(distDir, tmp, { recursive: true, dereference: true });
  writeFileSync(join(tmp, "package.json"), '{"type":"module"}');
  const mod = await import(pathToFileURL(join(tmp, "cadpole_wasm.js")).href);
  await mod.default({ module_or_path: readFileSync(join(tmp, "cadpole_wasm_bg.wasm")) });
  return mod;
}

/** The comparable projection of a probe answer for one category. */
function project(out, category) {
  const bodies = out.bodies ?? [];
  switch (category) {
    case "ok":
      return JSON.stringify(out.ok);
    case "features":
      return JSON.stringify(out.features);
    case "diagnostics":
      return JSON.stringify(out.diagnostics);
    case "tests":
      return JSON.stringify(out.tests ?? []);
    case "names":
      return JSON.stringify(bodies.map((b) => [b.id, b.faces, [...b.edges].sort(), [...b.vertices].sort()]));
    case "order":
      return JSON.stringify(bodies.map((b) => [b.id, b.edges, b.vertices]));
    case "counts":
      return JSON.stringify(bodies.map((b) => [b.id, b.counts]));
    case "volume":
      return JSON.stringify(bodies.map((b) => [b.id, b.volume]));
    case "bbox":
      return JSON.stringify(bodies.map((b) => [b.id, b.bbox]));
    case "topology":
      return JSON.stringify(bodies.map((b) => [b.id, b.topology]));
    case "mesh":
      return JSON.stringify(bodies.map((b) => [b.id, b.mesh]));
    case "inspect":
      return JSON.stringify(bodies.map((b) => [b.id, b.inspect]));
    case "keys":
      return JSON.stringify(bodies.map((b) => [b.id, b.key]));
    default:
      throw new Error(`unknown category ${category}`);
  }
}

/** Where two lists of strings first differ, for the report. */
function firstDifference(a, b) {
  const n = Math.max(a.length, b.length);
  for (let i = 0; i < n; i++) if (a[i] !== b[i]) return { index: i, native: a[i], wasm: b[i] };
  return null;
}

/** A one-line account of what differs in a category, for the log. */
function describe(category, native, wasm) {
  const nb = native.bodies ?? [];
  const wb = wasm.bodies ?? [];
  if (nb.length !== wb.length) return `${nb.length} bodies natively, ${wb.length} in wasm`;
  for (let i = 0; i < nb.length; i++) {
    const n = nb[i];
    const w = wb[i];
    switch (category) {
      case "names": {
        const d = firstDifference(n.faces, w.faces);
        if (d) return `${n.id} faces[${d.index}]: ${JSON.stringify(d.native)} vs ${JSON.stringify(d.wasm)}`;
        for (const kind of ["edges", "vertices"]) {
          const only = (a, b) => a.filter((x) => !b.includes(x));
          const nOnly = only(n[kind], w[kind]);
          const wOnly = only(w[kind], n[kind]);
          if (nOnly.length || wOnly.length) return `${n.id} ${kind}: native only ${JSON.stringify(nOnly.slice(0, 2))}, wasm only ${JSON.stringify(wOnly.slice(0, 2))}`;
        }
        break;
      }
      case "order": {
        for (const kind of ["edges", "vertices"]) {
          const d = firstDifference(n[kind], w[kind]);
          if (d) return `${n.id} ${kind}[${d.index}]: ${JSON.stringify(d.native)} vs ${JSON.stringify(d.wasm)}`;
        }
        break;
      }
      case "counts":
        if (JSON.stringify(n.counts) !== JSON.stringify(w.counts)) return `${n.id}: ${JSON.stringify(n.counts)} vs ${JSON.stringify(w.counts)}`;
        break;
      case "volume":
        if (n.volume !== w.volume) return `${n.id}: ${n.volume} vs ${w.volume}`;
        break;
      case "bbox":
        if (JSON.stringify(n.bbox) !== JSON.stringify(w.bbox)) return `${n.id}: bbox bits differ`;
        break;
      case "topology":
        if (n.topology !== w.topology) return `${n.id}: ${n.topology} vs ${w.topology}`;
        break;
      case "mesh":
        if (n.mesh !== w.mesh) return `${n.id}: ${n.mesh} vs ${w.mesh}`;
        break;
      case "keys":
        if (n.key !== w.key) return `${n.id}: ${n.key} vs ${w.key}`;
        break;
      case "inspect":
        if (n.inspect !== w.inspect) {
          let k = 0;
          while (k < n.inspect.length && n.inspect[k] === w.inspect[k]) k++;
          const lo = Math.max(0, k - 40);
          return `${n.id} at char ${k}: …${n.inspect.slice(lo, k + 60)} vs …${w.inspect.slice(lo, k + 60)}`;
        }
        break;
      default:
        break;
    }
  }
  if (category === "diagnostics") {
    const d = firstDifference(
      native.diagnostics.map((x) => JSON.stringify(x)),
      wasm.diagnostics.map((x) => JSON.stringify(x)),
    );
    if (d) return `diagnostic[${d.index}]: ${d.native ?? "(none)"} vs ${d.wasm ?? "(none)"}`;
  }
  if (category === "tests") {
    const d = firstDifference(
      (native.tests ?? []).map((x) => JSON.stringify(x)),
      (wasm.tests ?? []).map((x) => JSON.stringify(x)),
    );
    if (d) return `test[${d.index}]: ${d.native ?? "(none)"} vs ${d.wasm ?? "(none)"}`;
  }
  if (category === "features") {
    const d = firstDifference(
      native.features.map((x) => JSON.stringify(x)),
      wasm.features.map((x) => JSON.stringify(x)),
    );
    if (d) return `feature[${d.index}]: ${d.native ?? "(none)"} vs ${d.wasm ?? "(none)"}`;
  }
  return "differs";
}

function readExpected(path) {
  const expected = new Map();
  for (const line of readFileSync(path, "utf8").split("\n")) {
    if (!line.trim() || line.startsWith("#")) continue;
    const [example, category, why] = line.split("\t");
    assert.ok(CATEGORIES.includes(category), `${path}: unknown category ${JSON.stringify(category)} on line ${JSON.stringify(line)}`);
    expected.set(`${example}\t${category}`, why ?? "");
  }
  return expected;
}

function pad(s, n) {
  return String(s).padEnd(n);
}

async function main() {
  const examplesDir = runfile("examples");
  const probeBin = runfile("src", "rust", "cadpole-wasm", "native_probe");
  const wasmDir = runfile("src", "rust", "cadpole-wasm", "cadpole_wasm");
  const expectedPath = runfile("src", "rust", "cadpole-wasm", "determinism", "expected_divergence.tsv");
  const expected = readExpected(expectedPath);

  const all = examples(examplesDir);
  assert.ok(all.length > 0, `no examples under ${examplesDir}`);
  const documents = [...all, ...stdlibTests(join(examplesDir, "..", "stdlib"))];
  const assets = loadAssets(join(examplesDir, "assets"));
  const wasm = await loadWasm(wasmDir);
  const engine = new wasm.Engine();

  const rows = [];
  const differing = new Set();
  const notes = [];
  let nativeTotal = 0;
  let wasmTotal = 0;
  for (const entry of documents) {
    // A stdlib test document stands alone -- its `load("@cadpole/...")` is
    // answered by the engine, not by a sibling file -- and its geometry is
    // built inside its tests, so those run.
    const stdlib = entry.name.startsWith("stdlib/");
    const input = probeInput(examplesDir, stdlib ? [] : all, entry, assets, stdlib);

    let t0 = performance.now();
    const proc = spawnSync(probeBin, [], { input, encoding: "utf8", maxBuffer: 1 << 30 });
    const nativeMs = performance.now() - t0;
    assert.equal(proc.status, 0, `${entry.name}: native_probe failed: ${proc.stderr}`);
    const native = JSON.parse(proc.stdout);

    t0 = performance.now();
    let wasmOut;
    try {
      wasmOut = engine.probe(input);
    } catch (e) {
      const panic = wasm.take_panic?.();
      throw new Error(`${entry.name}: wasm probe threw: ${e}${panic ? `\n  panic: ${panic}` : ""}`);
    }
    const wasmMs = performance.now() - t0;
    const w = JSON.parse(wasmOut);

    nativeTotal += nativeMs;
    wasmTotal += wasmMs;
    const row = { example: entry.name, bodies: native.bodies.length, nativeMs, wasmMs, cells: {} };
    for (const category of CATEGORIES) {
      const same = project(native, category) === project(w, category);
      row.cells[category] = same;
      if (!same) {
        differing.add(`${entry.name}\t${category}`);
        notes.push(`${entry.name} / ${category}: ${describe(category, native, w)}`);
      }
    }
    rows.push(row);
  }

  // The table.
  const head = ["example", "bodies", "native ms", "wasm ms", ...CATEGORIES];
  const widths = head.map((h) => h.length);
  const lines = rows.map((r) => [
    r.example,
    String(r.bodies),
    r.nativeMs.toFixed(0),
    r.wasmMs.toFixed(0),
    ...CATEGORIES.map((c) => (r.cells[c] ? "same" : "DIFF")),
  ]);
  for (const l of lines) l.forEach((cell, i) => (widths[i] = Math.max(widths[i], cell.length)));
  const fmt = (cells) => `| ${cells.map((c, i) => pad(c, widths[i])).join(" | ")} |`;
  const table = [fmt(head), `|${widths.map((w) => "-".repeat(w + 2)).join("|")}|`, ...lines.map(fmt)];
  const summary = [
    `${rows.length} examples; ${rows.filter((r) => CATEGORIES.every((c) => r.cells[c])).length} identical in every category;`,
    `${differing.size} (example, category) pairs differ; ${expected.size} allowed by expected_divergence.tsv.`,
    `native ${(nativeTotal / 1000).toFixed(1)} s, wasm ${(wasmTotal / 1000).toFixed(1)} s.`,
  ].join(" ");
  const report = [...table, "", summary, ...(notes.length ? ["", ...notes] : [])].join("\n");
  console.log(report);
  if (process.env.TEST_UNDECLARED_OUTPUTS_DIR) {
    writeFileSync(join(process.env.TEST_UNDECLARED_OUTPUTS_DIR, "native_vs_wasm.md"), report + "\n");
  }

  // The ratchet, in both directions.
  const unexpected = [...differing].filter((k) => !expected.has(k));
  const stale = [...expected.keys()].filter((k) => !differing.has(k));
  const problems = [];
  if (unexpected.length) {
    problems.push(
      `${unexpected.length} pair(s) differ between native and wasm that expected_divergence.tsv does not allow:\n` +
        unexpected.map((k) => `  ${k.replace("\t", " / ")}`).join("\n"),
    );
  }
  if (stale.length) {
    problems.push(
      `${stale.length} pair(s) in expected_divergence.tsv now match — prune them, the list only shrinks:\n` +
        stale.map((k) => `  ${k.replace("\t", " / ")}`).join("\n"),
    );
  }
  assert.ok(problems.length === 0, problems.join("\n\n"));
}

await main();
