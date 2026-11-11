// Writes traces of a model's `step` as one JSON file, for a test that
// replays them against the implementation (README.md, "Replayed traces").
//
//   traces.mjs --out FILE [--traces N] [--steps N] [--seed 0x...] [--drop var,var]
//
// Runs the pinned quint with its Bazel-fetched evaluator, as runner.mjs does:
// `quint run` with `--out-itf`, a fixed seed so the file is the same on every
// build, and no invariant, so the traces are plain samples of `step`. The
// output is `{model, seed, steps, traces}`, each trace the list of its states
// in the Informal Trace Format, without the variables named by `--drop`
// (bookkeeping the replay does not read).
//
// Environment, set by the BUILD file:
//
//   QUINT_MODEL          rlocationpath of the model
//   QUINT_HOME_MARKER    rlocationpath of a file at the root of the evaluator
//   QUINT_EVALUATOR_DIR  the evaluator's subdirectory of QUINT_HOME
//   QUINT_CLI            rlocationpath of the quint CLI

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, join } from "node:path";

function die(message) {
  console.error(`traces: ${message}`);
  process.exit(1);
}

const runfiles = process.env["JS_BINARY__RUNFILES"] ?? process.env["RUNFILES_DIR"];
if (!runfiles) die("neither JS_BINARY__RUNFILES nor RUNFILES_DIR is set");

function rlocation(name) {
  const p = process.env[name];
  if (!p) die(`${name} is not set`);
  const abs = isAbsolute(p) ? p : join(runfiles, p);
  if (!existsSync(abs)) die(`${name} does not exist in runfiles: ${abs}`);
  return abs;
}

const argv = process.argv.slice(2);
const arg = (name, fallback) => {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 ? argv[i + 1] : fallback;
};
const out = arg("out") ?? die("--out FILE is required");
const traces = Number(arg("traces", "300"));
const steps = Number(arg("steps", "30"));
const seed = arg("seed", "0x5eed");
const drop = new Set((arg("drop", "") ?? "").split(",").filter(Boolean));

const model = rlocation("QUINT_MODEL");
const cli = rlocation("QUINT_CLI");
const quintHome = dirname(rlocation("QUINT_HOME_MARKER"));
if (!existsSync(join(quintHome, process.env["QUINT_EVALUATOR_DIR"] ?? "", "quint_evaluator"))) {
  die(`no quint_evaluator under ${quintHome}; see build_support/quint/evaluator.bzl`);
}

const work = mkdtempSync(join(tmpdir(), "quint-traces-"));
const run = spawnSync(
  process.execPath,
  [
    cli,
    "run",
    model,
    `--max-samples=${traces}`,
    `--n-traces=${traces}`,
    `--max-steps=${steps}`,
    "--backend=rust",
    `--seed=${seed}`,
    "--verbosity=0",
    `--out-itf=${join(work, "t_{seq}.itf.json")}`,
  ],
  { stdio: ["ignore", "inherit", "inherit"], env: { ...process.env, QUINT_HOME: quintHome, HOME: work } },
);
if (run.error || run.status !== 0) die(`quint run failed (${run.error?.message ?? `exit ${run.status}`})`);

const files = readdirSync(work)
  .filter((f) => f.endsWith(".itf.json"))
  .sort((a, b) => Number(/\d+/.exec(a)?.[0]) - Number(/\d+/.exec(b)?.[0]));
if (files.length !== traces) die(`asked quint for ${traces} traces and got ${files.length}`);
const bundle = {
  model: basename(model),
  seed,
  steps,
  traces: files.map((f) =>
    JSON.parse(readFileSync(join(work, f), "utf8")).states.map((state) =>
      Object.fromEntries(Object.entries(state).filter(([name]) => name !== "#meta" && !drop.has(name))),
    ),
  ),
};
writeFileSync(out, JSON.stringify(bundle));
rmSync(work, { recursive: true, force: true });
