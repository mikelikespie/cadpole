// The transcendental lint: no `f64` math method on the geometry path
// outside `cadpole_core::fmath`.
//
//   bazel test //src/rust/cadpole-wasm:transcendental_lint_test
//
// `f64::sin` and its siblings are glibc natively and the bundled libm in
// wasm, and the two differ in the last bit; `cadpole_core::fmath` is the one
// implementation both targets share. This reads the sources of the four
// geometry crates (cadpole-core, cadpole-script, cadpole-field, cadpole-sketch-solve) and
// fails on any call that would route around it:
//
//   x.sin()  x.atan2(y)  ...        the inherent methods, by name
//   f64::sin(x)  Rad::sin_cos(a)     the same functions by path
//   Matrix4::from_axis_angle(..)     cgmath's rotations, which are Rad::sin_cos
//   v.angle(w)                       cgmath's Vector::angle, which is atan2
//   x.mul_add(y, z)  x.powi(n)       not transcendental, but not IEEE-pinned
//                                    either: `powi` is an LLVM intrinsic that
//                                    lowers differently per target, and a
//                                    fused multiply-add is the one thing the
//                                    no-contraction policy is about
//
// What is exempt, and why: `fmath.rs` itself; `cadpole-core/src/render*`, whose
// output is an image compared under a pixel tolerance, not a name;
// `libm::` calls, which are the pinned implementation by its own name
// (cadpole-sketch-solve sits below cadpole-core and calls it directly); Starlark
// snippets inside test strings (`math.sin(...)` is the script's namespace);
// and any line marked `// not f64:` with the type whose own method it is
// (`Interval::sin` in cadpole-field).

import { strict as assert } from "node:assert";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const FUNCS = "sin|cos|sin_cos|tan|asin|acos|atan|atan2|sinh|cosh|tanh|exp|exp2|ln|log2|log10|powf|powi|hypot|cbrt|mul_add";
const METHOD = new RegExp(`([A-Za-z0-9_\\)\\]\\?]|\\bmath)\\s*\\.(${FUNCS})\\(`, "g");
const PATH = new RegExp(`\\b(f64|f32|S|T|Rad|Deg|Float|num_traits::Float)::(${FUNCS})\\(`, "g");
const CGMATH = /\bfrom_axis_angle\(|\bfrom_angle_[xyz]\(|[A-Za-z0-9_\)\]]\.angle\(/g;

function runfilesRoot() {
  const runfiles = process.env.JS_BINARY__RUNFILES ?? process.env.RUNFILES_DIR ?? "";
  for (const root of [join(runfiles, "_main"), join(runfiles, "cadpole"), resolve(".")]) {
    if (existsSync(join(root, "src", "rust", "cadpole-core", "src", "fmath.rs"))) return root;
  }
  throw new Error(`cannot find the cadpole-core sources (runfiles: ${runfiles || "unset"})`);
}

function* rustFiles(dir) {
  for (const name of readdirSync(dir).sort()) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) yield* rustFiles(p);
    else if (name.endsWith(".rs")) yield p;
  }
}

const CRATES = ["cadpole-core", "cadpole-script", "cadpole-field", "cadpole-sketch-solve", "cadpole-import"];

function exempt(rel) {
  return rel === "src/rust/cadpole-core/src/fmath.rs" || rel.startsWith("src/rust/cadpole-core/src/render");
}

function main() {
  const root = runfilesRoot();
  const offences = [];
  let files = 0;
  for (const crate of CRATES) {
    for (const file of rustFiles(join(root, "src", "rust", crate, "src"))) {
      const rel = relative(root, file);
      if (exempt(rel)) continue;
      files++;
      const lines = readFileSync(file, "utf8").split("\n");
      lines.forEach((line, i) => {
        const code = line.replace(/\/\/.*$/, "");
        if (line.includes("// not f64:")) return;
        for (const m of code.matchAll(METHOD)) {
          // `math.sin(...)` is a Starlark snippet inside a Rust string.
          if (m[1] === "math") continue;
          offences.push(`${rel}:${i + 1}: .${m[2]}( -- use cadpole_core::fmath::${m[2]}\n    ${line.trim()}`);
        }
        for (const m of code.matchAll(PATH)) {
          offences.push(`${rel}:${i + 1}: ${m[1]}::${m[2]}( -- use cadpole_core::fmath::${m[2]}\n    ${line.trim()}`);
        }
        for (const m of code.matchAll(CGMATH)) {
          offences.push(`${rel}:${i + 1}: ${m[0]} -- cgmath's angles are f64::sin_cos/atan2; build the matrix from fmath (geom::axis_angle)\n    ${line.trim()}`);
        }
      });
    }
  }
  assert.ok(files > 50, `only ${files} Rust files found under ${root}; the sources are not in the runfiles`);
  console.log(`${files} files across ${CRATES.join(", ")}: ${offences.length} transcendental(s) outside fmath`);
  assert.ok(
    offences.length === 0,
    `${offences.length} transcendental call(s) on the geometry path route around cadpole_core::fmath:\n\n${offences.join("\n")}`,
  );
}

main();
