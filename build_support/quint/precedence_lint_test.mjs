// The precedence lint (precedence_lint.mjs) flags each of the bad file's four
// unbracketed assignments and nothing in the good file.
import { readFileSync } from "node:fs";
import { lint } from "./precedence_lint.mjs";

const read = (f) => readFileSync(new URL(`./testdata/${f}`, import.meta.url), "utf8");
let failed = false;

const bad = lint("precedence_bad.qnt", read("precedence_bad.qnt"));
const badLines = bad.map((f) => Number(f.split(":")[1]));
if (JSON.stringify(badLines) !== JSON.stringify([6, 8, 13, 16])) {
  console.error(`FAIL: bad file flagged at lines ${badLines}, want 6,8,13,16:\n${bad.join("\n")}`);
  failed = true;
}
const good = lint("precedence_good.qnt", read("precedence_good.qnt"));
if (good.length) {
  console.error(`FAIL: good file flagged:\n${good.join("\n")}`);
  failed = true;
}
if (failed) process.exit(1);
console.log("ok: 4 flagged in the bad file (lines 6, 8, 13, 16), none in the good one");
