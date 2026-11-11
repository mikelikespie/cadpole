// Cadpole service worker.
//
// - Precaches the static bundle (and the wasm engine files when present) so
//   static / playground mode works offline.
// - Static assets: stale-while-revalidate (fast loads, fresh on next visit).
// - /api/*: network-first with a cache fallback for GET; never touches
//   /api/events (Server-Sent Events must stream straight from the network).
//   The UI's own calls are `POST /api/ops/<Rpc>` in protobuf, and
//   a POST is never intercepted or cached -- the Cache API keys on the URL,
//   which for an operation says nothing about what was asked. So session mode
//   has no offline replay: with the server unreachable the page falls back to
//   the static playground and says so. Static mode, which is what the
//   precache is for, is unaffected.

// Failure reporting. A service worker's `error` and
// `unhandledrejection` reach neither the page's handlers nor the server, so a
// caching bug here is invisible to everyone: the page keeps working off the
// network and nobody ever learns why the offline build is broken. The worker
// has no bearer token, so it posts to its clients and the page forwards
// (`adoptServiceWorker` in src/web/src/failure.js). The message shape is
// `WorkerFailureMessage` in src/web/src/failure.ts; it is repeated by hand
// here because a classic service worker cannot import the bundle.
const CADPOLE_FAILURE_MESSAGE = "cadpole:failure";

async function reportFailure(kind, message, stack, location) {
  try {
    const clients = await self.clients.matchAll({ includeUncontrolled: true, type: "window" });
    for (const client of clients) {
      client.postMessage({ __cadpole: CADPOLE_FAILURE_MESSAGE, kind, message, stack, location, operation: "service_worker" });
    }
  } catch {
    // No clients (the worker outlives every tab): the console is all there is.
    console.error("cadpole sw:", message);
  }
}

self.addEventListener("error", (event) => {
  const err = event.error;
  const where = event.filename ? `${event.filename}:${event.lineno ?? 0}:${event.colno ?? 0}` : "";
  void reportFailure(
    "CLIENT_FAILURE_KIND_EXCEPTION",
    err ? `${err.name}: ${err.message}` : String(event.message ?? "unknown service worker error"),
    err && err.stack ? err.stack : "",
    where,
  );
});

self.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  const isErr = reason instanceof Error;
  void reportFailure(
    "CLIENT_FAILURE_KIND_REJECTION",
    isErr ? `${reason.name}: ${reason.message}` : String(reason),
    isErr && reason.stack ? reason.stack : "",
    "",
  );
});

// The activate handler deletes every cache whose
// key is not this one, so changing the name is how a phone holding a bad
// bundle is forced to let go of it. Bump this whenever what is cached changes
// shape; a routine rebuild does not need it, because code is network-first.
const CACHE = "cadpole-static-v2";
// `engine-worker.js` is in here rather than in OPTIONAL because it *is* where
// evaluation happens: a playground that came back from the cache
// without it would fall back to running the engine on the main thread, which
// is the freeze this is all about, and offline is exactly when nobody can be
// told why.
const PRECACHE = ["./", "./index.html", "./app.js", "./engine-worker.js", "./styles.css", "./manifest.webmanifest"];
// Optional: only present when the wasm engine has been built.
const OPTIONAL = ["./cadpole_wasm.js", "./cadpole_wasm_bg.wasm"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE);
      await cache.addAll(PRECACHE.map((u) => new Request(u, { cache: "reload" })));
      await Promise.allSettled(
        OPTIONAL.map(async (u) => {
          const res = await fetch(new Request(u, { cache: "reload" }));
          if (res.ok) await cache.put(u, res);
        }),
      );
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) if (key !== CACHE) await caches.delete(key);
      await self.clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  const url = new URL(req.url);
  if (url.origin !== self.location.origin) return;
  if (url.pathname.endsWith("/api/events")) return; // SSE: never intercept
  if (url.pathname.includes("/api/")) {
    if (req.method !== "GET") return;
    event.respondWith(networkFirst(req));
    return;
  }
  if (req.method !== "GET") return;
  // Code is network-first; everything else is stale-while-revalidate.
  //
  // A broken engine served stale-while-revalidate goes on loading out of this
  // cache after the fix is served, which from the outside looks exactly like
  // the page hanging for ever. Stale-while-revalidate is right for a stylesheet, where a
  // version behind costs nothing, and wrong for the engine, where a version
  // behind is the last bug rather than the current build. Offline still
  // works: network-first falls back to the cache.
  event.respondWith(isCode(url.pathname) ? networkFirst(req) : staleWhileRevalidate(req));
});

/// Whether a request carries the `?b=` token the page adds when it knows the
/// build moved. `hot-reload.ts` puts it on the stylesheet and on the engine
/// worker; it means "not whatever you have cached".
function isCacheBusted(req) {
  return new URL(req.url).searchParams.has("b");
}

/// The files that are the application itself, as opposed to its assets.
function isCode(pathname) {
  // `engine-worker.js` is code by the same argument as the wasm it loads: it
  // *is* where evaluation happens, and served stale it would pair a
  // new app.js with an old worker on the first load after a rebuild. `sw.js`
  // too, so a caching bug here cannot outlive its own fix.
  return (
    /(^|\/)(index\.html|app\.js|cadpole_wasm\.js|cadpole_wasm_bg\.wasm|worker-report\.js|engine-worker\.js|sw\.js)$/.test(pathname) ||
    pathname === "/" ||
    pathname.endsWith("/")
  );
}

// The page asks for this when the server says the build changed, so a reload
// cannot be served the copy that is about to be replaced. Without it a live
// reload would put a phone straight back on the cached old engine.
self.addEventListener("message", (event) => {
  if (!event.data || event.data.type !== "cadpole:drop-caches") return;
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) await caches.delete(key);
      event.source?.postMessage({ type: "cadpole:caches-dropped" });
    })(),
  );
});

async function networkFirst(req) {
  const cache = await caches.open(CACHE);
  try {
    const res = await fetch(req);
    if (res.ok) cache.put(req, res.clone());
    return res;
  } catch (err) {
    const cached = await cache.match(req);
    if (cached) return cached;
    // A navigation to a URL with a query string -- `/?mock=1`, `/?static=1`,
    // any bookmark or installed shortcut -- is not in the precache, which
    // holds `./` and `./index.html` only. Without this, bare `/` came up
    // offline and every one of those failed outright with ERR_FAILED, which
    // is a strange way for an installed app to behave.
    // `staleWhileRevalidate` has had this fallback all along.
    if (req.mode === "navigate") {
      const shell = await cache.match("./index.html");
      if (shell) return shell;
    }
    throw err;
  }
}

async function staleWhileRevalidate(req) {
  const cache = await caches.open(CACHE);
  // `ignoreSearch` so `styles.css?v=3` is answered by the precached
  // `styles.css` -- except when the query is a *cache buster*, which is the
  // one case where the caller has said in as many words that the copy in
  // here is the wrong one. A hot stylesheet swap fetches
  // `styles.css?b=<token>` precisely because the build changed; answering it
  // from the precache would leave the page on the old sheet while it printed
  // "styles updated without reloading", and a following plain reload would be
  // stale too, because the background refresh only lands on the load after.
  const cached = await cache.match(req, { ignoreSearch: !isCacheBusted(req) });
  const network = fetch(req)
    .then((res) => {
      if (res.ok) cache.put(req, res.clone());
      return res;
    })
    .catch(() => null);
  if (cached) {
    // Refresh in the background.
    network.catch(() => {});
    return cached;
  }
  const res = await network;
  if (res) return res;
  // Navigation fallback so the app shell loads offline at any path.
  if (req.mode === "navigate") {
    const shell = await cache.match("./index.html");
    if (shell) return shell;
  }
  return new Response("offline", { status: 503, headers: { "content-type": "text/plain" } });
}
