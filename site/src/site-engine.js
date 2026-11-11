// The site's engine worker. It downloads the wasm itself so the page can show
// progress, compiles it while it streams (no 32 MB copy held in memory), and
// then hands over to the web app's own engine worker, which finds the engine
// already initialised and serves evaluations as it does in the app.
//
// What happens here is reported on a BroadcastChannel; the page shows it with
// ?debug=1.
const ch = typeof BroadcastChannel === "function" ? new BroadcastChannel("cadpole-site-engine") : null;
const say = (m) => { try { ch?.postMessage(m); } catch {} };
let wasm = null;
if (ch) ch.onmessage = (e) => { if (e.data?.ask === "memory") say({ memory: wasm ? wasm.memory.buffer.byteLength : 0, tag: e.data.tag }); };
const describe = (e) => (e instanceof Error ? `${e.name}: ${e.message}` : String(e));
try {
  say({ stage: "worker running" });
  const glue = await import("./cadpole_wasm.js");
  say({ stage: "glue loaded" });
  const expected = Number(new URL(import.meta.url).searchParams.get("n")) || 0;
  const url = new URL("./cadpole_wasm_bg.wasm", import.meta.url);
  const fetched = async () => {
    const r = await fetch(url);
    if (!r.ok) throw new Error(`engine download failed: HTTP ${r.status}`);
    say({ stage: `download started (type ${r.headers.get("content-type")}, encoding ${r.headers.get("content-encoding") || "none"}, length ${r.headers.get("content-length") || "unknown"})` });
    return r;
  };
  let got = 0;
  const counted = (r) => {
    if (!r.body || typeof TransformStream !== "function") return r;
    let last = 0;
    got = 0;
    const count = new TransformStream({
      transform(chunk, c) {
        got += chunk.byteLength;
        const now = performance.now();
        if (now - last > 80) { last = now; say({ loaded: got, total: Math.max(expected, got) }); }
        c.enqueue(chunk);
      },
      flush() { say({ loaded: got, total: got }); },
    });
    return new Response(r.body.pipeThrough(count), { status: 200, headers: { "content-type": "application/wasm" } });
  };
  try {
    wasm = await glue.default({ module_or_path: counted(await fetched()) });
  } catch (e) {
    // A browser that will not compile from a stream gets the plain path once,
    // unless the stream never was the problem.
    const text = describe(e);
    if (/Content Security Policy|unsafe-eval|out of memory/i.test(text)) throw e;
    say({ stage: `streaming compile failed (${text}); trying again from a buffer` });
    const bytes = await (await fetched()).arrayBuffer();
    say({ loaded: bytes.byteLength, total: bytes.byteLength });
    wasm = await glue.default({ module_or_path: bytes });
  }
  say({ stage: "compiled", memory: wasm.memory.buffer.byteLength });
  await import("./engine-worker.js");
  say({ stage: "engine worker loaded" });
} catch (e) {
  say({ stage: `failed: ${describe(e)}` });
  self.postMessage({ __cadpole: "cadpole:engine", id: 0, ok: false, op: "init", message: describe(e), stack: e instanceof Error ? (e.stack ?? "") : "" });
}
