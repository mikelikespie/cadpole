// The site as it is published: the build's output plus the large media the
// tree does not hold, written to a directory a static host can serve as it is
// (a `gh-pages` checkout, say).
//
//   bazel run //site:assemble -- --media DIR --out DIR
//
// --media is a directory with the frame sets and the recording at the paths
// media.lock.json names (seq/<set>/<variant>/NNN.avif|webp, agent/*.mp4|webm):
// what `bazel run //site:render -- encode` writes, or any earlier copy of the
// published site. Every file is checked against the lock before anything is
// written, and --out is replaced, not merged, so nothing stale survives.
import { createHash } from "node:crypto";
import { chmodSync, cpSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, rmSync, statSync } from "node:fs";
import { basename, join, resolve } from "node:path";

const opt = {};
for (let i = 2; i < process.argv.length; i += 2) opt[process.argv[i].replace(/^--/, "")] = process.argv[i + 1];
for (const k of ["dist", "lock", "media", "out"]) if (!opt[k]) { console.error(`usage: assemble --media DIR --out DIR (missing --${k})`); process.exit(2); }
const cwd = process.env.BUILD_WORKING_DIRECTORY ?? ".";
const media = resolve(cwd, opt.media), out = resolve(cwd, opt.out);
const lock = JSON.parse(readFileSync(opt.lock, "utf8"));

const sha = (p) => createHash("sha256").update(readFileSync(p)).digest();
const problems = [];
for (const [dir, want] of Object.entries(lock.sets)) {
  const d = join(media, dir);
  if (!existsSync(d)) { problems.push(`${dir}: missing`); continue; }
  const files = readdirSync(d).sort();
  const h = createHash("sha256");
  for (const f of files) h.update(Buffer.concat([Buffer.from(f), Buffer.from([0]), sha(join(d, f))]));
  if (files.length !== want.count) problems.push(`${dir}: ${files.length} files, the lock says ${want.count}`);
  else if (h.digest("hex") !== want.sha256) problems.push(`${dir}: not the frames the lock names (re-rendered? run: bazel run //site:render -- lock ${opt.media})`);
}
for (const [file, want] of Object.entries(lock.files)) {
  const p = join(media, file);
  if (!existsSync(p)) problems.push(`${file}: missing`);
  else if (sha(p).toString("hex") !== want.sha256) problems.push(`${file}: not the file the lock names`);
}
if (problems.length) {
  console.error(`the media under ${media} does not match media.lock.json:\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
if (existsSync(out) && readdirSync(out).some((f) => f !== ".git" && f !== "CNAME") && !existsSync(join(out, "site.webmanifest"))) {
  console.error(`${out} holds something other than a copy of this site; give an empty directory`);
  process.exit(1);
}
mkdirSync(out, { recursive: true });
let n = 0, bytes = 0;
// The build's files are read-only; a published copy is an ordinary directory.
const walk = (d) => { for (const f of readdirSync(d)) { if (f === ".git") continue; const p = join(d, f), s = statSync(p); if (s.isDirectory()) { chmodSync(p, 0o755); walk(p); } else { chmodSync(p, 0o644); n++; bytes += s.size; } } };
// A git checkout keeps its .git and a custom domain its CNAME; the rest is replaced.
walk(out);
for (const f of readdirSync(out)) if (f !== ".git" && f !== "CNAME") rmSync(join(out, f), { recursive: true, force: true });
cpSync(opt.dist, out, { recursive: true, dereference: true });
// The build's own tree links some files to where they were built; a published copy holds the files.
const solid = (d) => { for (const f of readdirSync(d)) { const p = join(d, f), s = lstatSync(p); if (s.isSymbolicLink()) { const to = realpathSync(p); rmSync(p); cpSync(to, p, { recursive: true, dereference: true }); } if (statSync(p).isDirectory() && f !== ".git") solid(p); } };
solid(out);
walk(out);
for (const dir of Object.keys(lock.sets)) cpSync(join(media, dir), join(out, dir), { recursive: true });
for (const file of Object.keys(lock.files)) cpSync(join(media, file), join(out, file));
n = 0; bytes = 0;
walk(out);
console.log(`${basename(out)}: ${n} files, ${(bytes / 1e6).toFixed(1)} MB, written to ${out}`);
