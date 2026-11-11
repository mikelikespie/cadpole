// The Connect protocol, unary, in the browser.
//
// This is the client half of what `src/rust/cadpole-cli/src/connect.rs` serves:
// `POST <base>/<package>.<Service>/<Method>` with the message as the body,
// `application/proto` in and out, and an error as JSON `{code, message}` with
// the HTTP status Connect assigns to the code.
//
// **There is no method table here, and no generated client file either.**
// `protoc-gen-es` already emits one: `//src/web:cadpole_pb` generates
// `CadpoleOps`, a `GenService` whose `method` record is every rpc of
// `service CadpoleOps` with its request and response schemas, from the same
// descriptor set the Rust registry, the Swift types and the CLI are built
// from. `createClient` turns that descriptor into a typed client at run time,
// so adding an rpc to `ops.proto` adds a method here with no edit to this file
// and nothing to keep in step — the same property `cadpole_ops::registry()` gives
// the CLI, the MCP tools and the routes.
//
// What is deliberately *not* here: a protobuf codec. Encoding stays with
// `@bufbuild/protobuf`, where varints, packed fields, 64-bit values, unknown
// fields and proto3 presence are already right. A transport that is wrong
// fails loudly; a codec that is wrong corrupts quietly.

import { create, fromBinary, toBinary, type DescMessage, type MessageInitShape, type MessageShape } from "@bufbuild/protobuf";
import type { GenService, GenServiceMethods } from "@bufbuild/protobuf/codegenv2";
import type { DescMethod, DescService } from "@bufbuild/protobuf";

/** The Connect codes, as they are spelled in an error envelope. */
export type ConnectCode =
  | "canceled"
  | "unknown"
  | "invalid_argument"
  | "deadline_exceeded"
  | "not_found"
  | "already_exists"
  | "permission_denied"
  | "resource_exhausted"
  | "failed_precondition"
  | "aborted"
  | "out_of_range"
  | "unimplemented"
  | "internal"
  | "unavailable"
  | "data_loss"
  | "unauthenticated";

/**
 * A failed call, with the HTTP status it arrived as.
 *
 * `status` is what the UI has always switched on (401 means the server
 * restarted and this token is stale); `code` is what the protocol says, and is
 * the thing to write new logic against, because it survives a proxy rewriting
 * a status and it is the same word the Rust side used.
 */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly code: ConnectCode = "unknown",
  ) {
    super(message);
    this.name = "ApiError";
  }
}

/** The status Connect assigns to each code, inverted: a fallback when a proxy ate the body. */
function codeOfStatus(status: number): ConnectCode {
  switch (status) {
    case 400:
      return "invalid_argument";
    case 401:
      return "unauthenticated";
    case 403:
      return "permission_denied";
    case 404:
      return "unimplemented";
    case 409:
      return "aborted";
    case 412:
      return "failed_precondition";
    case 413:
    case 429:
      return "resource_exhausted";
    case 499:
      return "canceled";
    case 501:
      return "unimplemented";
    case 503:
      return "unavailable";
    case 504:
      return "deadline_exceeded";
    default:
      return status >= 500 ? "internal" : "unknown";
  }
}

/** The one content type this client speaks, in both directions. */
export const PROTO_MEDIA_TYPE = "application/proto";

/**
 * Per-call extras. A bare `AbortSignal` is still accepted, and is the common
 * case.
 *
 * `keepalive` is for a write issued while the page is going away -- a
 * `pagehide`, or the moment before a reload. Without it the browser cancels
 * the request with the document and the write is simply lost; with it the
 * request outlives the page. It is limited to small bodies (64 KB across all
 * in-flight keepalive requests), which is why it is opt-in per call rather
 * than the default: the writes that need it are a parameter map and a
 * selection.
 */
export interface CallOptions {
  signal?: AbortSignal;
  keepalive?: boolean;
}

function callOptions(opts: AbortSignal | CallOptions | undefined): CallOptions {
  if (!opts) return {};
  return opts instanceof AbortSignal ? { signal: opts } : opts;
}

/** Per-request extras the app supplies: the bearer token, the client id. */
export type HeaderSource = () => Record<string, string>;

/** What to do with a 401, which always means "this token is not this server's". */
export type Unauthorized = () => void;

export interface TransportOptions {
  /** The Connect base URL. Cadpole serves the service under `/api`. */
  baseUrl: string;
  headers?: HeaderSource;
  onUnauthorized?: Unauthorized;
}

/**
 * One unary call, encoded, sent and decoded.
 *
 * Errors: a non-2xx response carries a Connect envelope, and the envelope is
 * what the message comes from. A response that is not one — a proxy's HTML
 * error page, a truncated body — still produces an `ApiError` with the code
 * the status implies, because a client that throws `SyntaxError` at the sight
 * of an error page has lost the error.
 */
export class ConnectTransport {
  constructor(private readonly options: TransportOptions) {}

  async unary<I extends DescMessage, O extends DescMessage>(
    service: DescService,
    method: { name: string; input: I; output: O },
    input: MessageInitShape<I>,
    opts?: AbortSignal | CallOptions,
  ): Promise<MessageShape<O>> {
    const { signal, keepalive } = callOptions(opts);
    const url = `${this.options.baseUrl}/${service.typeName}/${method.name}`;
    const body = toBinary(method.input, create(method.input, input));
    const init: RequestInit = {
      method: "POST",
      headers: {
        "content-type": PROTO_MEDIA_TYPE,
        accept: PROTO_MEDIA_TYPE,
        // Not required by this server, and sent anyway: it is what tells a
        // Connect server that the caller means the Connect protocol rather
        // than a bare POST that happens to look like one.
        "connect-protocol-version": "1",
        ...(this.options.headers?.() ?? {}),
      },
      cache: "no-store",
      body: body as BodyInit,
    };
    if (signal) init.signal = signal;
    if (keepalive) init.keepalive = true;
    const res = await fetch(url, init);
    if (!res.ok) throw await this.failure(res, `${service.typeName}/${method.name}`);
    return fromBinary(method.output, new Uint8Array(await res.arrayBuffer()));
  }

  private async failure(res: Response, what: string): Promise<ApiError> {
    // A 401 means this token is not the one the running server minted --
    // almost always because the server restarted. Drop it rather than
    // retrying with it forever; the UI asks for the new URL.
    if (res.status === 401) this.options.onUnauthorized?.();
    let code = codeOfStatus(res.status);
    let message = "";
    try {
      const envelope = (await res.json()) as { code?: string; message?: string };
      if (typeof envelope.code === "string") code = envelope.code as ConnectCode;
      if (typeof envelope.message === "string") message = envelope.message;
    } catch {
      /* not an envelope; the status is all there is */
    }
    return new ApiError(res.status, `${what}: ${code}${message ? `: ${message.slice(0, 300)}` : ""}`, code);
  }
}

/**
 * A typed client for a generated service descriptor.
 *
 * `S["method"]` is the record `protoc-gen-es` generated from the schema, so
 * the methods, their names and their message types all come from `ops.proto`
 * and none of them is written down here.
 */
export type Client<S extends GenService<GenServiceMethods>> = {
  [K in keyof S["method"]]: (
    input: MessageInitShape<S["method"][K]["input"]>,
    opts?: AbortSignal | CallOptions,
  ) => Promise<MessageShape<S["method"][K]["output"]>>;
};

export function createClient<S extends GenService<GenServiceMethods>>(service: S, transport: ConnectTransport): Client<S> {
  const client: Record<string, unknown> = {};
  for (const method of (service as DescService).methods) {
    client[method.localName] = (input: never, opts?: AbortSignal | CallOptions) =>
      transport.unary(service as DescService, method as DescMethod & { input: DescMessage; output: DescMessage }, input, opts);
  }
  return client as Client<S>;
}
