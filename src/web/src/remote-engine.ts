// `RemoteEngine`: evaluation on `cadpole serve`, over the wire format.
//
// One `POST /api/cadpole.v1.CadpoleOps/Evaluate` carrying the document, answered
// with a `cadpole.v1.Model` whose bodies already have their meshes. The client
// chooses local or remote per request, tries it, and falls back to local with
// a visible note.
//
// **It sends the document rather than letting the server use its session's.**
// `fill_document` in cadpole-ops fills the request from the host only when the
// caller sent none, and the browser's parameters lead the session's: the UI
// evaluates on every slider move and pushes `SetParams` on a 300 ms debounce.
// Evaluating the session's copy would render a model the user is not looking
// at for as long as that debounce lasts. Sending the document also makes this
// engine work against a stateless render host, which has no
// session at all.
//
// It sends exactly what the local engines are given -- scripts and parameter
// overrides -- so the two are the same function of the same inputs. Assets are
// the one thing missing, and they are missing from `SessionDocument` too, so
// the wasm engine cannot see them either; this is not a capability the remote
// path loses.

import { DEFAULT_TOLERANCE, throwIfAborted, type EngineLike, type EvaluateRequest, type Evaluated } from "./engine.js";
import type { ApiDoc } from "./types.js";
import * as wire from "./wire.js";
import { ops } from "./wire.js";

/** Thrown when the server could not be reached or refused us. */
export class RemoteUnavailable extends Error {
  constructor(
    message: string,
    /** HTTP status when there was one; 0 for a network failure or a timeout. */
    readonly status: number,
    readonly cause_: unknown = null,
  ) {
    super(message);
    this.name = "RemoteUnavailable";
  }
}

export class RemoteEngine implements EngineLike {
  readonly kind = "remote" as const;
  /**
   * Which build is answering. Filled in from the server's own identity if it
   * ever offers one; until then it is honest about being the far end, so a
   * crash report cannot claim the local wasm produced a remote model.
   */
  readonly buildId = "cadpole serve (remote)";

  constructor(
    /**
     * How long to wait before giving up, in ms. Also sent as
     * `EvaluateRequest.deadline_ms`, so the *server* stops as well rather than
     * finishing an evaluation nobody is waiting for -- the Starlark
     * interpreter and every builtin check it.
     */
    private readonly timeoutMs = 60_000,
  ) {}

  async evaluate(req: EvaluateRequest, signal?: AbortSignal): Promise<Evaluated> {
    throwIfAborted(signal);
    const t0 = performance.now();
    const tolerance = req.tolerance ?? DEFAULT_TOLERANCE;
    const deadlineMs = req.deadlineMs && req.deadlineMs > 0 ? req.deadlineMs : this.timeoutMs;
    const ctl = new AbortController();
    const onAbort = (): void => ctl.abort();
    signal?.addEventListener("abort", onAbort, { once: true });
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      ctl.abort();
    }, deadlineMs);
    try {
      const res = await ops.evaluate(
        {
          document: {
            scripts: req.scripts.map((s) => ({ path: s.path, source: s.source })),
            paramOverrides: wire.paramValuesTo(req.overrides),
          },
          tolerance,
          deadlineMs: BigInt(Math.round(deadlineMs)),
          preview: req.preview === true,
        },
        ctl.signal,
      );
      if (!res.model) throw new RemoteUnavailable("the server answered Evaluate with no model", 0);
      const { model, meshes } = wire.modelFrom(res.model);
      return { model, meshes, tolerance, kind: this.kind, elapsedMs: performance.now() - t0 };
    } catch (err) {
      if (timedOut) {
        throw new RemoteUnavailable(`the server did not answer within ${Math.round(deadlineMs / 1000)}s`, 0, err);
      }
      // The caller's own cancellation is not a remote failure and must not
      // demote the document: it means the user moved on.
      if (signal?.aborted) throw err;
      throw RemoteEngine.classify(err);
    } finally {
      clearTimeout(timer);
      signal?.removeEventListener("abort", onAbort);
    }
  }

  /**
   * Turn whatever `fetch` threw into something the policy can act on.
   *
   * The distinction that matters is "there is no server here" versus "there is
   * a server and it will not talk to us": both fall back to local, but only
   * the second is worth telling the user how to fix.
   */
  private static classify(err: unknown): RemoteUnavailable {
    if (err instanceof RemoteUnavailable) return err;
    if (err instanceof wire.ApiError) {
      if (err.status === 401 || err.status === 403) {
        return new RemoteUnavailable(
          "the server refused this token -- it has been restarted; open the URL `cadpole serve` printed",
          err.status,
          err,
        );
      }
      return new RemoteUnavailable(err.message, err.status, err);
    }
    const why = err instanceof Error ? err.message : String(err);
    return new RemoteUnavailable(`the server could not be reached (${why})`, 0, err);
  }

  async apiDoc(signal?: AbortSignal): Promise<ApiDoc> {
    return apiDocFromResponse(await ops.apiDoc({}, signal));
  }

  dispose(): void {
    /* nothing is held: every call owns its own AbortController */
  }
}

type ApiDocParamMessage = { name: string; type: string; doc: string; required: boolean; defaultValue: string };
type ApiDocFunctionMessage = { name: string; doc: string; params: ApiDocParamMessage[]; returns: string; onType: string; constant: boolean };

/** An `ApiDocResponse` from the server as the app's `ApiDoc`, the shape the wasm engine returns. */
export function apiDocFromResponse(res: {
  functions: ApiDocFunctionMessage[];
  types: { name: string; doc: string; methods: ApiDocFunctionMessage[]; attrs: ApiDocParamMessage[] }[];
}): ApiDoc {
  const param = (p: ApiDocParamMessage) => ({ name: p.name, type: p.type, doc: p.doc, required: p.required, default_value: p.defaultValue });
  const fn = (f: ApiDocFunctionMessage) => ({
    name: f.name,
    doc: f.doc,
    params: f.params.map(param),
    returns: f.returns,
    on_type: f.onType,
    constant: f.constant,
  });
  return {
    functions: res.functions.map(fn),
    types: res.types.map((t) => ({ name: t.name, doc: t.doc, methods: t.methods.map(fn), attrs: t.attrs.map(param) })),
  };
}
