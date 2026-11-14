#!/usr/bin/env node
// Build an illustrated guide from a model (docs/illustrations.md).
//
//   node src/tools/illustrate/main.mjs GUIDE.json --out-dir DIR [options]
//
// Everything this prints goes to stderr except the last line on success,
// the path of the page.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { buildGuide, validateGuide } from "./guide.mjs";

const USAGE = `usage: illustrate GUIDE.json --out-dir DIR [options]

Renders one picture per step of the guide with \`cadpole render\`, draws the
guide's arrows and labels on each through the camera that rendered it, and
writes a page of them: DIR/index.html, DIR/img/, and with --chrome flattened
PNGs and DIR/guide.pdf.

  --out-dir DIR      where to write (kept between runs: an unchanged shot is not rendered again)
  --cadpole BIN      the cadpole binary (default: $CADPOLE, else \`cadpole\` on PATH)
  --chrome BIN       a Chromium or Chrome binary, to flatten the pictures to PNG and print the PDF
                     (default: $CADPOLE_CHROME; without one the pictures stay SVG)
  --only NAME        only this shot; repeatable
  --compose-only     draw the annotations again over the renders already there
  --force            render every shot again
  --cadpole-arg ARG  an argument for every cadpole call, before the subcommand; repeatable
                     (--cadpole-arg=--eval-budget-secs --cadpole-arg=900)
  --check            validate the guide and stop
`;

function parse(argv) {
  const o = { only: [], extraArgs: [], cadpole: process.env.CADPOLE || "cadpole", chrome: process.env.CADPOLE_CHROME || null };
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const val = () => {
      const eq = a.indexOf("=");
      if (eq > 0) return a.slice(eq + 1);
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    const flag = a.split("=")[0];
    if (flag === "--out-dir") o.outDir = val();
    else if (flag === "--cadpole") o.cadpole = val();
    else if (flag === "--chrome") o.chrome = val();
    else if (flag === "--only") o.only.push(val());
    else if (flag === "--cadpole-arg") o.extraArgs.push(val());
    else if (a === "--compose-only") o.composeOnly = true;
    else if (a === "--force") o.force = true;
    else if (a === "--check") o.check = true;
    else if (a === "-h" || a === "--help") o.help = true;
    else if (a.startsWith("-")) throw new Error(`unknown option ${a}`);
    else rest.push(a);
  }
  if (rest.length > 1) throw new Error("one guide at a time");
  o.guidePath = rest[0];
  return o;
}

export function main(argv) {
  let o;
  try {
    o = parse(argv);
  } catch (e) {
    process.stderr.write(`illustrate: ${e.message}\n${USAGE}`);
    return 2;
  }
  if (o.help) {
    process.stderr.write(USAGE);
    return 0;
  }
  if (!o.guidePath || (!o.outDir && !o.check)) {
    process.stderr.write(USAGE);
    return 2;
  }
  let guide;
  try {
    guide = validateGuide(JSON.parse(fs.readFileSync(o.guidePath, "utf8")));
  } catch (e) {
    process.stderr.write(`illustrate: ${o.guidePath}: ${e.message}\n`);
    return 1;
  }
  if (o.check) {
    process.stderr.write(`${o.guidePath}: ok, ${guide.steps.length} steps\n`);
    return 0;
  }
  let result;
  try {
    result = buildGuide(guide, {
      guidePath: path.resolve(o.guidePath),
      outDir: path.resolve(o.outDir),
      cadpole: o.cadpole,
      chrome: o.chrome,
      only: o.only.length ? o.only : null,
      composeOnly: !!o.composeOnly,
      force: !!o.force,
      extraArgs: o.extraArgs,
      log: (m) => process.stderr.write(`${m}\n`),
    });
  } catch (e) {
    process.stderr.write(`illustrate: ${e.message}\n`);
    return 1;
  }
  for (const r of result.renders) {
    if (r.status === "failed") process.stderr.write(`FAILED ${r.name}: ${r.detail}\n`);
  }
  for (const w of result.warnings) process.stderr.write(`warning: ${w}\n`);
  if (!result.ok) {
    process.stderr.write(`illustrate: the guide is incomplete${result.missing.length ? `: no picture for ${result.missing.join(", ")}` : ""}\n`);
    return 1;
  }
  process.stdout.write(`${result.files.page}\n`);
  return 0;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  process.exit(main(process.argv.slice(2)));
}
