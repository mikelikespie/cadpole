// Traces from a Quint model, for a replay against the implementation.
//
// `quint run --out-itf` writes the traces its random simulation walks, one
// file each, in the Informal Trace Format: a list of states, each a value of
// every variable of the model. The replays (listener_replay.mjs,
// wakeups_replay.mjs) make the implementation take the same steps and compare
// what it shows with each state.
//
// The pinned quint and its evaluator are found the way
// build_support/quint/runner.mjs finds them, through the environment
// `quint_replay` in build_support/quint/defs.bzl sets.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync } from "node:fs";
import { dirname, isAbsolute, join } from "node:path";

const runfiles = process.env["JS_BINARY__RUNFILES"] ?? process.env["RUNFILES_DIR"] ?? "";

/** A Bazel rlocationpath, resolved against the runfiles tree. */
export function rlocation(p, what) {
  if (!p) throw new Error(`${what} is not set`);
  const abs = isAbsolute(p) ? p : join(runfiles, p);
  if (!existsSync(abs)) throw new Error(`${what} does not exist in runfiles: ${abs}`);
  return abs;
}

/** A file of this repository in the runfiles tree, by its path from the root. */
export function inTree(path) {
  return rlocation(join("_main", path), path);
}

/** `--name=value` from the command line, else the environment, else the fallback. */
export function knob(name, fallback) {
  const flag = process.argv.slice(2).find((a) => a.startsWith(`--${name.toLowerCase()}=`));
  if (flag) return flag.slice(flag.indexOf("=") + 1);
  return process.env[`REPLAY_${name}`] ?? fallback;
}

/**
 * An ITF value as plain JavaScript: integers as numbers, sets as arrays,
 * maps as `Map`, a sum type's value as `{tag, value}`.
 */
export function plain(v) {
  if (Array.isArray(v)) return v.map(plain);
  if (v === null || typeof v !== "object") return v;
  if ("#bigint" in v) return Number(v["#bigint"]);
  if ("#set" in v) return v["#set"].map(plain);
  if ("#tup" in v) return v["#tup"].map(plain);
  if ("#map" in v) return new Map(v["#map"].map(([k, x]) => [plain(k), plain(x)]));
  const out = {};
  for (const [k, x] of Object.entries(v)) {
    if (k !== "#meta") out[k] = plain(x);
  }
  return out;
}

/**
 * Simulate `model` and return the traces it walked, each a list of states.
 *
 * The seed makes the set of traces a function of the model alone, so a
 * failure can be run again.
 *
 * @param {{model: string, init?: string, step?: string, traces: number, steps: number, seed: string, dir: string}} opts
 * @returns {any[][]}
 */
export function generate(opts) {
  const cli = join(rlocation(process.env["QUINT_PACKAGE"], "QUINT_PACKAGE"), "dist", "src", "cli.js");
  const home = dirname(rlocation(process.env["QUINT_HOME_MARKER"], "QUINT_HOME_MARKER"));
  const out = join(opts.dir, `${opts.init ?? "init"}-${opts.seed}`);
  mkdirSync(out, { recursive: true });
  const r = spawnSync(
    process.execPath,
    [
      cli,
      "run",
      opts.model,
      `--init=${opts.init ?? "init"}`,
      `--step=${opts.step ?? "step"}`,
      `--max-samples=${opts.traces}`,
      `--n-traces=${opts.traces}`,
      `--max-steps=${opts.steps}`,
      // One thread: the traces are then the same for the same seed.
      "--n-threads=1",
      `--seed=${opts.seed}`,
      "--backend=rust",
      `--out-itf=${join(out, "t{seq}.itf.json")}`,
    ],
    { encoding: "utf8", env: { ...process.env, QUINT_HOME: home, HOME: process.env["TEST_TMPDIR"] ?? process.env["HOME"] ?? "" } },
  );
  if (r.status !== 0) throw new Error(`quint run ${opts.model} failed:\n${r.stdout}\n${r.stderr}`);
  const files = readdirSync(out)
    .filter((f) => f.endsWith(".itf.json"))
    .sort((a, b) => Number(/\d+/.exec(a)?.[0] ?? 0) - Number(/\d+/.exec(b)?.[0] ?? 0));
  if (!files.length) throw new Error(`quint run ${opts.model} wrote no trace`);
  return files.map((f) => JSON.parse(readFileSync(join(out, f), "utf8")).states.map(plain));
}

/**
 * Run `one(trace, index)` over the traces, a few at a time.
 *
 * @param {any[][]} traces @param {number} width @param {(trace: any[], index: number) => Promise<void>} one
 */
export async function inParallel(traces, width, one) {
  let next = 0;
  const worker = async () => {
    while (next < traces.length) {
      const i = next++;
      await one(traces[i], i);
    }
  };
  await Promise.all(Array.from({ length: Math.min(width, traces.length) }, worker));
}

/** Fail with the two values when they differ. */
export function same(actual, expected, what) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${what}: the implementation shows ${a}, the model says ${e}`);
}
