// A static file server on loopback, for looking at a built site and for the
// checks: byte ranges (a video seeks), the right type for wasm, no caching.
//
//   bazel run //site:serve -- [--port 8766] [--root DIR]
import { createReadStream, statSync } from "node:fs";
import http from "node:http";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const TYPES = {
  ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".json": "application/json",
  ".svg": "image/svg+xml", ".png": "image/png", ".webp": "image/webp", ".avif": "image/avif", ".wasm": "application/wasm", ".mp4": "video/mp4",
  ".webm": "video/webm", ".pdf": "application/pdf", ".dxf": "application/dxf", ".star": "text/plain; charset=utf-8", ".webmanifest": "application/manifest+json",
};

/** Serve `root` on 127.0.0.1:`port` (0 picks a free one). Resolves to the server once it listens. */
export function serve(root, port = 0) {
  root = resolve(root);
  const server = http.createServer((req, res) => {
    let p = normalize(decodeURIComponent(new URL(req.url, "http://localhost").pathname));
    if (p.endsWith("/")) p += "index.html";
    const f = join(root, p);
    let st;
    try {
      if (!f.startsWith(root)) throw new Error("outside the site");
      st = statSync(f);
    } catch {
      res.writeHead(404);
      return res.end("not found");
    }
    if (st.isDirectory()) {
      res.writeHead(301, { location: p + "/" });
      return res.end();
    }
    const h = { "content-type": TYPES[extname(f)] || "application/octet-stream", "accept-ranges": "bytes", "cache-control": "no-cache" };
    const m = /bytes=(\d*)-(\d*)/.exec(req.headers.range || "");
    if (m) {
      const a = m[1] ? +m[1] : 0, b = m[2] ? +m[2] : st.size - 1;
      res.writeHead(206, { ...h, "content-range": `bytes ${a}-${b}/${st.size}`, "content-length": b - a + 1 });
      return createReadStream(f, { start: a, end: b }).pipe(res);
    }
    res.writeHead(200, { ...h, "content-length": st.size });
    createReadStream(f).pipe(res);
  });
  return new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(port, "127.0.0.1", () => done(server));
  });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const opt = { port: "8766", root: "." };
  for (let i = 2; i < process.argv.length; i += 2) opt[process.argv[i].replace(/^--/, "")] = process.argv[i + 1];
  // A directory given on the command line is the caller's, not the runfiles'.
  const root = resolve(process.env.BUILD_WORKING_DIRECTORY && process.argv.lastIndexOf("--root") > 3 ? process.env.BUILD_WORKING_DIRECTORY : ".", opt.root);
  const server = await serve(root, +opt.port);
  console.log(`serving ${root} at http://127.0.0.1:${server.address().port}/`);
}
