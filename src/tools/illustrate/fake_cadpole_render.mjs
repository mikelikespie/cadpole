// A stand-in for `cadpole render` in guide_test.mjs: writes a one-pixel PNG
// to -o and a camera sidecar to --camera-json, and appends its arguments to
// the file named by FAKE_CADPOLE_LOG. `-p fail=1` makes it fail the way the
// CLI does.
import fs from "node:fs";

const args = process.argv.slice(2);
if (process.env.FAKE_CADPOLE_LOG) fs.appendFileSync(process.env.FAKE_CADPOLE_LOG, `${JSON.stringify(args)}\n`);
if (args.includes("fail=1")) {
  process.stderr.write("error: main.star:3:1: no such body\n");
  process.exit(1);
}
const value = (flag) => args[args.indexOf(flag) + 1];
const [w, h] = value("--size").split("x").map(Number);
const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";
fs.writeFileSync(value("-o"), Buffer.from(PNG, "base64"));
fs.writeFileSync(
  value("--camera-json"),
  JSON.stringify({
    width: w,
    height: h,
    units: "mm",
    frames: [{ index: 0, eye: [0, -10, 0], target: [0, 0, 0], view: [1, 0, 0, 0, 0, 0, 1, 0, 0, -1, 0, -10, 0, 0, 0, 1], projection: [h / w, 0, 0, 0, 0, 1, 0, 0, 0, 0, -1.002, -0.2, 0, 0, -1, 0] }],
  }),
);
