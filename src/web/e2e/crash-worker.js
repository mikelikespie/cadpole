// A dedicated worker that fails on request, for crash.spec.ts.
//
// It exists because a worker's failures reach neither `window.onerror` nor
// `unhandledrejection` on the page, so the only way to know the worker path
// works is to run a real worker. It uses the *production* shim
// (`worker-report.js`, built from src/web/src/worker-report.ts) rather than a
// copy of it, so what is under test is the code a real worker imports, not a
// test double of it.
//
// Served only from //src/web:e2e_dist, never from //src/web:dist.

import { installWorkerReporting } from "./worker-report.js";

installWorkerReporting(self, { operation: "crash-test" });

self.addEventListener("message", (ev) => {
  if (ev.data === "throw") {
    throw new Error("boom inside the worker");
  }
  if (ev.data === "reject") {
    void Promise.reject(new Error("nobody caught this one in the worker"));
  }
});

self.postMessage("ready");
