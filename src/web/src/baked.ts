// A published page: a document evaluated ahead of time by `cadpole bake`
// or `cadpole publish`, opened read-only with no server and no Starlark.
//
// The artifact is a `cadpole.v1.BakedModel`, zlib-deflated: the same
// `cadpole.v1.Model` `RemoteEngine` decodes from `cadpole serve`, plus a name and a
// hash of the sources. So nothing here reads geometry -- `wire.modelFrom`
// does, exactly as it does for a remote evaluation -- and the rest of the app
// sees an engine (`BakedEngine`) that answers every evaluation with the one
// model it holds, and a session (`PublishedApi`) that holds the one document
// and refuses to change it.
//
// Where the page finds it, first match wins:
//
//   ?baked=<url>                     any bake, relative to the page or absolute
//   <meta name="cadpole-baked" ...>     what `cadpole publish` writes into index.html
//
// A published page loads no wasm, starts no worker, probes no `/api` and
// registers no service worker: `cadpole publish` ships none of them.

import { fromBinary } from "@bufbuild/protobuf";

import { BakedModelSchema, type BakedModel } from "../gen/cadpole/v1/eval_pb.js";
import type { SelectionPut, SessionApi } from "./api.js";
import type { EngineLike, EvaluateRequest, Evaluated, MeshData } from "./engine.js";
import type { ConnectionStatus } from "./state.js";
import type { ApiDoc, CadpoleEvent, Message, Model, Session, Vec3 } from "./types.js";
import * as wire from "./wire.js";

/** The newest `BakedModel.schema_version` this page reads. */
export const BAKED_SCHEMA_VERSION = 1;
/** The `<meta>` `cadpole publish` writes (src/rust/cadpole-cli/src/publish.rs). */
export const BAKED_META = "cadpole-baked";
/**
 * The largest a bake may inflate to: the CLI's own cap. A model of ~1,500 bodies
 * is 17 MB; a decompression bomb stops here, not at the tab's memory.
 */
export const MAX_INFLATED = 512 << 20;

/** Where this page's bake is, or null for an ordinary page. */
export function bakedSource(params: URLSearchParams, doc: Document = document): string | null {
  const q = params.get("baked");
  if (q) return new URL(q, location.href).href;
  const meta = doc.querySelector<HTMLMetaElement>(`meta[name="${BAKED_META}"]`);
  const c = meta?.content.trim();
  return c ? new URL(c, location.href).href : null;
}

/**
 * `cadpole.v1.PublishedView`: how the publisher asked the page to
 * open. Lengths are millimetres, like the model's.
 */
export interface PublishedView {
  /** A view preset ("home", "iso", "top", ...), or "" for home. */
  camera: string;
  eye: Vec3 | null;
  target: Vec3 | null;
  /** "", "on", "dim" or "off". */
  grid: string;
  /** A section in `cadpole render --section`'s spelling, or "". */
  section: string;
  /** The layers Home, Iso and Fit frame; empty for every drawn body. */
  frameLayers: string[];
  /** The display mode to open in ("xray", ...), or "" for the viewer's own. */
  mode: string;
  /** Where the page's back link goes, or "" for none. Unchecked: see `safeHomeUrl`. */
  homeUrl: string;
  /** What that link is called, or "". */
  homeLabel: string;
}

/** A bake, decoded: everything the page shows comes from here. */
export interface Baked {
  name: string;
  model: Model;
  meshes: MeshData[];
  /** The tessellation tolerance, mm. */
  tolerance: number;
  generator: string;
  sourceSha256: string;
  /** How the publisher asked the page to open; null for as any page. */
  view: PublishedView | null;
  /** The script's own `render_defaults(camera=)`, mm, if it declares one. */
  scriptCamera: { eye: Vec3; target: Vec3 | null } | null;
  /** The scripts, when the publisher included them; otherwise empty. */
  scripts: { path: string; source: string }[];
}

/** Inflate a zlib stream, refusing to grow past `limit` bytes. */
export async function inflate(bytes: Uint8Array, limit = MAX_INFLATED): Promise<Uint8Array> {
  const stream = new Blob([bytes as BlobPart]).stream().pipeThrough(new DecompressionStream("deflate"));
  const reader = stream.getReader();
  const parts: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > limit) {
      await reader.cancel();
      throw new Error(`this bake inflates past ${limit >> 20} MB`);
    }
    parts.push(value);
  }
  const out = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.byteLength;
  }
  return out;
}

/** A baked model's bytes as the page's model. */
export async function decodeBaked(bytes: Uint8Array): Promise<Baked> {
  let msg: BakedModel;
  try {
    msg = fromBinary(BakedModelSchema, await inflate(bytes));
  } catch (err) {
    throw new Error(`not a baked model: ${err instanceof Error ? err.message : String(err)}`);
  }
  if (msg.schemaVersion < 1 || msg.schemaVersion > BAKED_SCHEMA_VERSION) {
    throw new Error(`this bake is schema ${msg.schemaVersion}; this page reads 1..${BAKED_SCHEMA_VERSION} -- publish it again with this build`);
  }
  if (!msg.model) throw new Error("this bake holds no model");
  const { model, meshes } = wire.modelFrom(msg.model);
  return {
    name: msg.name || "model",
    model,
    meshes,
    tolerance: msg.tolerance,
    generator: msg.generator,
    sourceSha256: msg.sourceSha256,
    view: msg.view
      ? {
          camera: msg.view.camera,
          eye: msg.view.eye ? { x: msg.view.eye.x, y: msg.view.eye.y, z: msg.view.eye.z } : null,
          target: msg.view.target ? { x: msg.view.target.x, y: msg.view.target.y, z: msg.view.target.z } : null,
          grid: msg.view.grid,
          section: msg.view.section,
          frameLayers: [...msg.view.frameLayers],
          mode: msg.view.mode,
          homeUrl: msg.view.homeUrl,
          homeLabel: msg.view.homeLabel,
        }
      : null,
    scriptCamera: msg.model.renderDefaults?.eye
      ? {
          eye: { x: msg.model.renderDefaults.eye.x, y: msg.model.renderDefaults.eye.y, z: msg.model.renderDefaults.eye.z },
          target: msg.model.renderDefaults.target
            ? { x: msg.model.renderDefaults.target.x, y: msg.model.renderDefaults.target.y, z: msg.model.renderDefaults.target.z }
            : null,
        }
      : null,
    scripts: (msg.document?.scripts ?? []).map((s) => ({ path: s.path || "main.star", source: s.source })),
  };
}

/**
 * Fetch and decode the bake at `url`.
 *
 * `onProgress` hears how much has arrived, as a fraction when the server said
 * how much there is: a bake of a large model is megabytes, and on a phone's
 * connection that is long enough to want a number.
 */
export async function fetchBaked(url: string, onProgress?: (loaded: number, fraction: number | null) => void): Promise<Baked> {
  const res = await fetch(url, { cache: "no-cache" });
  if (!res.ok) throw new Error(`could not fetch the published model ${url}: HTTP ${res.status}`);
  if (!onProgress || !res.body) return decodeBaked(new Uint8Array(await res.arrayBuffer()));
  // The length is only the bake's when nothing re-encoded it on the way.
  const length = res.headers.get("content-encoding") ? 0 : Number(res.headers.get("content-length") ?? 0);
  const reader = res.body.getReader();
  const parts: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    total += value.byteLength;
    onProgress(total, length > 0 ? Math.min(1, total / length) : null);
  }
  const bytes = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    bytes.set(p, at);
    at += p.byteLength;
  }
  return decodeBaked(bytes);
}

/**
 * The engine of a published page: every evaluation is the bake.
 *
 * It ignores the scripts and the overrides it is handed, which is the honest
 * answer -- there is nothing here that could evaluate them -- and the page
 * never offers a control that would send different ones (the Parameters
 * panel is read-only in published mode).
 */
export class BakedEngine implements EngineLike {
  readonly kind = "baked" as const;
  readonly buildId: string;

  constructor(private readonly baked: Baked) {
    this.buildId = `baked by ${baked.generator || "cadpole"}`;
  }

  evaluate(_req: EvaluateRequest, signal?: AbortSignal): Promise<Evaluated> {
    if (signal?.aborted) {
      const e = new Error("aborted");
      e.name = "AbortError";
      return Promise.reject(e);
    }
    return Promise.resolve({ model: this.baked.model, meshes: this.baked.meshes, tolerance: this.baked.tolerance, kind: this.kind, elapsedMs: 0 });
  }

  apiDoc(): Promise<ApiDoc> {
    return Promise.resolve({ functions: [], types: [] });
  }

  dispose(): void {
    /* holds nothing but the bake */
  }
}

/**
 * The session of a published page: one document, read-only, in memory.
 *
 * Selection lives here like anywhere else (the inspector and the highlight
 * read it), and so do a visitor's own notes; nothing is written to storage,
 * so a published page on the same origin as the playground cannot clobber
 * the playground's document.
 */
export class PublishedApi implements SessionApi {
  readonly mode = "published" as const;
  private session: Session;

  constructor(baked: Baked) {
    this.session = {
      id: `baked:${baked.sourceSha256.slice(0, 16) || baked.name}`,
      revision: 1,
      lastEventSeq: 0,
      webBuild: "",
      document: {
        id: `baked:${baked.sourceSha256.slice(0, 16) || baked.name}`,
        name: baked.name,
        display_unit: baked.model.display_units?.unit ?? "mm",
        // The evaluator wants a script to "evaluate"; the bake engine
        // ignores it. A placeholder when the sources were not published.
        scripts: baked.scripts.length > 0 ? baked.scripts : [{ path: "main.star", source: "" }],
        param_overrides: {},
      },
      selection: { selected: [], hovered: null, note: "" },
      messages: [],
      clients: 1,
    };
  }

  getSession(): Promise<Session> {
    return Promise.resolve(this.session);
  }
  putScript(): Promise<Session> {
    return Promise.reject(new Error("a published model is read-only"));
  }
  putParams(): Promise<Session> {
    // The panel is read-only; an override that reached here anyway changes
    // nothing, and saying so as a rejection would only surface as an error.
    return Promise.resolve(this.session);
  }
  putSelection(sel: SelectionPut): Promise<Session> {
    this.session = {
      ...this.session,
      selection: { selected: sel.selected.map((r) => ({ ...r })), hovered: sel.hovered ? { ...sel.hovered } : null, note: sel.note ?? "" },
    };
    return Promise.resolve(this.session);
  }
  postMessage(from: "user" | "assistant", text: string): Promise<Message> {
    const m: Message = { seq: this.session.messages.length + 1, from, text, at: new Date().toISOString() };
    this.session = { ...this.session, messages: [...this.session.messages, m] };
    return Promise.resolve(m);
  }
  save(): Promise<string | null> {
    return Promise.resolve(null);
  }
  subscribe(_after: number, _onEvent: (e: CadpoleEvent) => void, onStatus: (s: ConnectionStatus) => void): () => void {
    onStatus("static");
    return () => undefined;
  }
}
