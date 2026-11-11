// The worker half of failure reporting.
//
// This is its own bundle (`worker-report.js` in dist), not part of `app.js`,
// because a worker that imported `app.js` would run the whole UI. Any worker
// Cadpole owns imports this and calls `installWorkerReporting(self)` as its first
// statement.
//
// Why a worker needs its own: `window.onerror` and `unhandledrejection` are
// page-scoped. A dedicated worker that panics fires neither, so without this
// its failures are invisible to the page and therefore to the server.
//
// A worker has no bearer token and no session, so it does not talk to the
// server. It posts to the page, which does. The message shape is
// `WorkerFailureMessage` in failure.ts; sw.js repeats it by hand because a
// classic service worker cannot import a module.

const WORKER_FAILURE_MESSAGE = "cadpole:failure";

export interface WorkerReportingOptions {
  /** What this worker is for, e.g. "evaluate". Goes in the report. */
  operation?: string;
}

interface MinimalWorkerScope {
  addEventListener(type: string, listener: (ev: never) => void): void;
  postMessage(message: unknown): void;
}

/**
 * Catch this worker's uncaught errors and unhandled rejections and post them
 * to the page. Returns nothing: there is nothing to undo, and a worker that
 * has crashed is in no position to clean up.
 */
export function installWorkerReporting(scope: MinimalWorkerScope, opts: WorkerReportingOptions = {}): void {
  const post = (kind: string, message: string, stack: string, location: string): void => {
    try {
      scope.postMessage({
        __cadpole: WORKER_FAILURE_MESSAGE,
        kind,
        message,
        stack,
        location,
        operation: opts.operation ?? "",
      });
    } catch {
      // A structured-clone failure here would be a second crash on the path
      // out of the first one. The console is the fallback of last resort.
      console.error("cadpole: could not post the worker failure to the page", message);
    }
  };

  scope.addEventListener("error", ((ev: ErrorEvent) => {
    const err = ev.error as Error | undefined;
    const where = ev.filename ? `${ev.filename}${ev.lineno ? `:${ev.lineno}` : ""}${ev.colno ? `:${ev.colno}` : ""}` : "";
    post("CLIENT_FAILURE_KIND_EXCEPTION", err ? `${err.name}: ${err.message}` : ev.message, err?.stack ?? "", where);
    // Handled here, with the worker's own stack. Letting it propagate to the
    // page as well would report the same failure twice, the second time with
    // less in it (the page sees a bare "Uncaught Error", no stack).
    ev.preventDefault?.();
  }) as (ev: never) => void);

  scope.addEventListener("unhandledrejection", ((ev: PromiseRejectionEvent) => {
    const reason = ev.reason as unknown;
    const err = reason instanceof Error ? reason : undefined;
    post(
      "CLIENT_FAILURE_KIND_REJECTION",
      err ? `${err.name}: ${err.message}` : String(reason),
      err?.stack ?? "",
      "",
    );
  }) as (ev: never) => void);
}

/**
 * Report a wasm panic from inside a worker: the Rust message and location out
 * of `take_panic()`, which is the whole reason a panic report is worth more
 * than `RuntimeError: unreachable`.
 */
export function reportWorkerPanic(
  scope: MinimalWorkerScope,
  panic: { message: string; location?: string; stack?: string },
  operation = "",
): void {
  scope.postMessage({
    __cadpole: WORKER_FAILURE_MESSAGE,
    kind: "CLIENT_FAILURE_KIND_WASM_PANIC",
    message: panic.message,
    location: panic.location ?? "",
    stack: panic.stack ?? "",
    operation,
  });
}
