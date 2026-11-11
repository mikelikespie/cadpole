// Entry point of every `quint_test` target (build_support/quint/defs.bzl).
//
// First the precedence lint (precedence_lint.mjs): `x' = a and b` parses as
// `(x' = a) and b`, a guard that can silently stop every step, so it is refused
// before anything runs. Then three checks over each model:
//
//   1. `quint typecheck`  -- the model is well typed;
//   2. `quint run`        -- random simulation against every named invariant;
//   3. `quint test`       -- every `run` definition in the file, as a scenario;
//   4. `quint run --init` -- every mutant listed for the model: the model
//                            started from a deliberately broken variant, which
//                            must violate the invariant named for it. An
//                            invariant no mutant can break is not known to
//                            check anything.
//
// The `run` names are read out of the model rather than listed in the BUILD
// file, because `quint test` with no `--match` silently selects only names
// ending in "Test" -- none of ours do -- and then reports "passed" having run
// nothing.
//
// Environment, all set by the macro:
//
//   QUINT_PLAN           JSON: {samples, steps, models: [{file, invariants,
//                        mutants: [[init, invariant]]}]}, where `file` is an
//                        rlocationpath.
//   QUINT_HOME_MARKER    rlocationpath of a file at the root of the
//                        Bazel-fetched evaluator directory; its dirname
//                        becomes QUINT_HOME, which is where
//                        `dist/src/rust/binaryManager.js` looks for
//                        `quint_evaluator` before it would download one.
//   QUINT_EVALUATOR_DIR  the subdirectory of QUINT_HOME the evaluator must be
//                        in, e.g. `rust-evaluator-v0.6.0`.
//   QUINT_CLI            path (relative to the chdir'd cwd) of the quint CLI.
//
// Overrides, for a deeper run by hand: `--samples=N`, `--steps=N`, `--seed=0x…`
// on the command line (`bazel run //spec:quint_check -- --samples=20000`), or
// SAMPLES / STEPS / SEED in the environment (`--test_env=SAMPLES=20000`).

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { lint } from "./precedence_lint.mjs";

function die(message) {
  console.error(`quint_test: ${message}`);
  process.exit(1);
}

const runfiles = process.env["JS_BINARY__RUNFILES"] ?? process.env["RUNFILES_DIR"];
if (!runfiles) die("neither JS_BINARY__RUNFILES nor RUNFILES_DIR is set");

/** Resolve a Bazel rlocationpath ("<repo>/some/file") against the runfiles tree. */
function rlocation(p, what) {
  if (!p) die(`${what} is not set`);
  const abs = isAbsolute(p) ? p : join(runfiles, p);
  if (!existsSync(abs)) die(`${what} does not exist in runfiles: ${abs}`);
  return abs;
}

// ----------------------------------------------------------------- inputs

const plan = JSON.parse(process.env["QUINT_PLAN"] ?? die("QUINT_PLAN is not set"));

function knob(name, fallback) {
  const flag = process.argv.slice(2).find((a) => a.startsWith(`--${name.toLowerCase()}=`));
  if (flag) return flag.slice(flag.indexOf("=") + 1);
  return process.env[name] ?? fallback;
}

const samples = String(knob("SAMPLES", plan.samples));
const steps = String(knob("STEPS", plan.steps));
const seed = knob("SEED", "");

// ------------------------------------------------------- the pinned quint

const cli = resolve(process.env["QUINT_CLI"] ?? "node_modules/@informalsystems/quint/dist/src/cli.js");
if (!existsSync(cli)) die(`the quint CLI is not at ${cli} (cwd ${process.cwd()})`);

// QUINT_HOME is where quint looks for the Rust evaluator, and where it would
// otherwise download one from GitHub at test time. Point it at the pinned copy
// in runfiles and check the binary really is there, so "the evaluator moved"
// fails here with a sentence rather than as a network error inside quint.
const quintHome = dirname(rlocation(process.env["QUINT_HOME_MARKER"], "QUINT_HOME_MARKER"));
const evaluator = join(quintHome, process.env["QUINT_EVALUATOR_DIR"] ?? "", "quint_evaluator");
if (!existsSync(evaluator)) {
  die(
    `no quint_evaluator at ${evaluator}.\n` +
      "  The pinned evaluator version and the one this quint asks for have diverged;\n" +
      "  see QUINT_EVALUATOR_VERSION in build_support/quint/evaluator.bzl.",
  );
}
process.env["QUINT_HOME"] = quintHome;

// Bazel gives a test TEST_TMPDIR and, under --incompatible_strict_action_env,
// a HOME that may not be writable. Neither quint nor the evaluator should want
// either, but a library that reaches for os.homedir() should land somewhere
// harmless rather than in the developer's real home.
const tmp = process.env["TEST_TMPDIR"];
if (tmp) {
  process.env["TMPDIR"] = process.env["TMPDIR"] ?? tmp;
  process.env["HOME"] = tmp;
}

// ------------------------------------------------------------------- run

/** `run someScenario = …` definitions, in file order. */
function runNames(source) {
  return [...source.matchAll(/^[ \t]*run[ \t]+([A-Za-z_][A-Za-z0-9_]*)/gm)].map((m) => m[1]);
}

let failures = 0;

function quint(label, argv) {
  const started = Date.now();
  const result = spawnSync(process.execPath, [cli, ...argv], { stdio: "inherit", env: process.env });
  const took = ((Date.now() - started) / 1000).toFixed(1);
  if (result.error) die(`could not start quint: ${result.error.message}`);
  const ok = result.status === 0;
  console.log(`${ok ? "[ok]" : "[FAIL]"} ${label} (${took}s)`);
  if (!ok) failures += 1;
  return ok;
}

for (const model of plan.models) {
  const file = rlocation(model.file, `model ${model.file}`);
  const name = model.file.split("/").pop();
  const source = readFileSync(file, "utf8");

  // `x' = a and b` parses as `(x' = a) and b`, a guard that can silently stop
  // every step (precedence_lint.mjs). Refuse it before checking anything.
  console.log(`\n== ${name}: precedence lint`);
  const findings = lint(name, source);
  if (findings.length) {
    for (const f of findings) console.log(f);
    console.log(`[FAIL] ${name} precedence lint (bracket the value or the conjunction)`);
    failures += 1;
    continue;
  }
  console.log(`[ok] ${name} precedence lint`);

  console.log(`\n== ${name}: typecheck`);
  quint(`${name} typecheck`, ["typecheck", file]);

  if (model.invariants && model.invariants.length) {
    console.log(`\n== ${name}: simulate ${samples} samples x ${steps} steps`);
    // The spec path goes BEFORE --invariants: that option takes a list and
    // would otherwise swallow the file name ("Not enough non-option
    // arguments").
    const argv = [
      "run",
      file,
      `--max-samples=${samples}`,
      `--max-steps=${steps}`,
      "--backend=rust",
      ...(seed ? [`--seed=${seed}`] : []),
      "--invariants",
      ...model.invariants,
    ];
    quint(`${name} invariants: ${model.invariants.join(" ")}`, argv);
  }

  for (const [init, invariant] of model.mutants ?? []) {
    console.log(`\n== ${name}: mutant ${init} must violate ${invariant}`);
    const started = Date.now();
    // Its own trace is the counterexample; only the verdict is printed.
    const result = spawnSync(
      process.execPath,
      [cli, "run", file, `--init=${init}`, `--max-samples=${samples}`, `--max-steps=${steps}`, "--backend=rust", ...(seed ? [`--seed=${seed}`] : []), `--invariant=${invariant}`],
      { encoding: "utf8", env: process.env },
    );
    const took = ((Date.now() - started) / 1000).toFixed(1);
    if (result.error) die(`could not start quint: ${result.error.message}`);
    const output = `${result.stdout}${result.stderr}`;
    const caught = result.status !== 0 && output.includes("[violation]");
    if (caught) {
      console.log(`[ok] ${name} mutant ${init}: ${invariant} is violated (${took}s)`);
    } else {
      console.log(output.split("\n").slice(-12).join("\n"));
      console.log(`[FAIL] ${name} mutant ${init}: ${invariant} still holds, so it does not catch what the mutant does wrong (${took}s)`);
      failures += 1;
    }
  }

  const runs = runNames(source);
  if (runs.length) {
    console.log(`\n== ${name}: ${runs.length} scenario run(s)`);
    quint(`${name} scenarios`, ["test", file, "--backend=rust", `--match=^(${runs.join("|")})$`]);
  } else {
    console.log(`\n== ${name}: no \`run\` definitions`);
  }
}

if (failures) {
  console.error(`\nquint_test: ${failures} check(s) failed`);
  process.exit(1);
}
console.log("\nquint_test: every model typechecks, every invariant holds, every scenario passes");
