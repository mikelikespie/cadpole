// The wire format: how the browser talks to `cadpole serve`.
//
// Every call is one unary Connect rpc — `POST /api/cadpole.v1.CadpoleOps/<Rpc>`
// carrying a protobuf-encoded request and answered with a protobuf-encoded
// response, both `application/proto`. There is no JSON shape of these
// messages anywhere in this code base: the types come from
// `proto/cadpole/v1/*.proto` through protoc-gen-es (//src/web:cadpole_pb), the same
// descriptor the CLI, MCP and the server's own registry are generated from,
// and so does `ops` below — the client is the generated `CadpoleOps` service
// descriptor, not a list of methods anybody typed out.
//
// This module is the whole of that: the client (`ops`) and the conversions
// between `cadpole.v1.*` messages and the plain shapes in `types.ts` that the UI
// is written against. `api.ts` stays the UI's interface and contains no
// encoding of any kind; `connect.ts` is the protocol and knows nothing about
// Cadpole.

import type { MessageInitShape } from "@bufbuild/protobuf";
import { base64Decode } from "@bufbuild/protobuf/wire";

import type { ParamValue as PbParamValue } from "../gen/cadpole/v1/document_pb.js";
import type {
  Body as PbBody,
  Diagnostic as PbDiagnostic,
  Feature as PbFeature,
  Layer as PbLayer,
  Model as PbModel,
  ParamInfo as PbParamInfo,
  Plate as PbPlate,
  SourceSpan as PbSourceSpan,
  TopoEdge as PbTopoEdge,
  TopoFace as PbTopoFace,
  TopoVertex as PbTopoVertex,
} from "../gen/cadpole/v1/eval_pb.js";
import type { Vec3 as PbVec3 } from "../gen/cadpole/v1/geometry_pb.js";
import type { Session as PbSession, SessionMessage as PbSessionMessage, SessionRef as PbSessionRef } from "../gen/cadpole/v1/ops_pb.js";
import { ParamValueSchema } from "../gen/cadpole/v1/document_pb.js";
import { CadpoleOps, MessageAttachmentSchema, SessionRefSchema } from "../gen/cadpole/v1/ops_pb.js";
import { ConnectTransport, createClient } from "./connect.js";
import type { MeshData } from "./engine.js";
import * as norm from "./normalize.js";
import { authHeaders, clearToken } from "./token.js";
import type {
  Attachment,
  Body,
  Diagnostic,
  EntityRef,
  Feature,
  CadpoleDocument,
  Layer,
  Message,
  Model,
  ParamInfo,
  ParamValue,
  Plate,
  Selection,
  Session,
  SourceSpan,
  TopoEdge,
  TopoFace,
  TopoVertex,
  Transform,
  Vec3,
} from "./types.js";

export { ApiError, PROTO_MEDIA_TYPE } from "./connect.js";

/**
 * The Cadpole operation registry as a typed client.
 *
 * `CadpoleOps` is generated: `ops.evaluate`, `ops.getSession`, `ops.editScript`
 * and the rest exist because `service CadpoleOps` declares them, with the request
 * and response types the schema gives them. Nothing in the browser lists the
 * operations, so an rpc added to `ops.proto` is callable here on the next
 * build and one that is removed stops compiling.
 *
 * The token goes in a header on every request, reads included: the server
 * requires it everywhere and accepts it nowhere else (docs/security.md).
 */
export const ops = createClient(
  CadpoleOps,
  new ConnectTransport({
    // The Connect base URL. `/api` and not `/`, because everything under
    // `/api/` is what the server authenticates.
    baseUrl: "/api",
    headers: authHeaders,
    onUnauthorized: clearToken,
  }),
);

// ---------------------------------------------------------------------------
// cadpole.v1 messages -> the shapes in types.ts
//
// 64-bit fields arrive as `bigint` (revision, seq); the UI has always treated
// them as numbers, and a session revision does not reach 2^53.

const n = (v: bigint): number => Number(v);

export function paramValueFrom(v: PbParamValue): ParamValue {
  switch (v.value.case) {
    case "number":
      return v.value.value;
    case "boolean":
      return v.value.value;
    case "text":
      return v.value.value;
    case "vector":
      return { x: v.value.value.x, y: v.value.value.y, z: v.value.value.z };
    default:
      return 0;
  }
}

export function paramValueTo(v: ParamValue): MessageInitShape<typeof ParamValueSchema> {
  if (typeof v === "number") return { value: { case: "number", value: v } };
  if (typeof v === "boolean") return { value: { case: "boolean", value: v } };
  if (typeof v === "string") return { value: { case: "text", value: v } };
  return { value: { case: "vector", value: { x: v.x, y: v.y, z: v.z } } };
}

export function paramValuesTo(overrides: Record<string, ParamValue>): Record<string, MessageInitShape<typeof ParamValueSchema>> {
  const out: Record<string, MessageInitShape<typeof ParamValueSchema>> = {};
  for (const [k, v] of Object.entries(overrides)) out[k] = paramValueTo(v);
  return out;
}

export function paramValuesFrom(overrides: { [k: string]: PbParamValue }): Record<string, ParamValue> {
  const out: Record<string, ParamValue> = {};
  for (const [k, v] of Object.entries(overrides)) out[k] = paramValueFrom(v);
  return out;
}

export function refFrom(r: PbSessionRef): EntityRef {
  const ref: EntityRef = { feature_id: r.featureId, kind: norm.entityKind(r.kind) };
  if (r.bodyId) ref.body_id = r.bodyId;
  if (r.topoName) ref.topo_name = r.topoName;
  return ref;
}

export function refTo(r: EntityRef): MessageInitShape<typeof SessionRefSchema> {
  // `kind` is the lowercase string the session API uses, which is what the UI
  // already carries; SessionRef exists precisely so it does not have to become
  // an enum on the way through.
  return { featureId: r.feature_id, bodyId: r.body_id ?? "", kind: r.kind, topoName: r.topo_name ?? "" };
}

export function messageFrom(m: PbSessionMessage): Message {
  return {
    seq: n(m.seq),
    from: m.from === "assistant" ? "assistant" : "user",
    text: m.text,
    at: m.at,
    // `SessionMessage.attachments` is a count, not the images: the bytes never
    // travel in a session read or an event. The UI keeps the ones it just
    // sent and fetches older ones through GetAttachment.
    //
    // `state` and `activity` are set for a reply written a piece at a time.
    ...(m.state === "streaming" || m.state === "done" || m.state === "failed" ? { state: m.state } : {}),
    ...(m.state === "streaming" && m.activity ? { activity: m.activity } : {}),
  };
}

export function attachmentTo(a: Attachment): MessageInitShape<typeof MessageAttachmentSchema> {
  return {
    kind: a.kind,
    png: base64Decode(a.png_base64),
    camera: {
      eye: { ...a.camera.eye },
      target: { ...a.camera.target },
      up: { ...a.camera.up },
      orthographic: a.camera.orthographic,
      fov_degrees: a.camera.fov_degrees,
    },
    annotated: a.annotated ?? false,
  };
}

export function documentFrom(d: PbSession["document"]): CadpoleDocument {
  return {
    id: d?.id ?? "",
    name: d?.name || "untitled",
    display_unit: norm.lengthUnit(d?.displayUnit),
    scripts: (d?.scripts ?? []).map((s) => ({ path: s.path || "main.star", source: s.source })),
    param_overrides: paramValuesFrom(d?.paramOverrides ?? {}),
    asset_names: d?.assetNames ?? [],
  };
}

export function selectionFrom(s: PbSession["selection"]): Selection {
  return {
    selected: (s?.selected ?? []).map(refFrom),
    hovered: s?.hovered ? refFrom(s.hovered) : null,
    note: s?.note ?? "",
  };
}

export function sessionFrom(s: PbSession): Session {
  return {
    id: s.id,
    revision: n(s.revision),
    lastEventSeq: n(s.lastEventSeq),
    webBuild: s.webBuild,
    document: documentFrom(s.document),
    selection: selectionFrom(s.selection),
    messages: s.messages.map(messageFrom),
    clients: s.clients,
    assistantListening: s.assistantListening,
  };
}

// ---------------------------------------------------------------------------
// cadpole.v1.Model -> the UI's Model plus its meshes
//
// The remote engine's whole job is this conversion. `cadpole.v1.Body` carries its
// `Mesh` inline with packed `positions` / `normals` / `indices` / `face_ids`,
// which is exactly what the viewer wants, so a remote evaluation comes back
// with the triangles already computed and there is nothing left to do but
// rename the fields.
//
// `mesh_index` does not exist in the proto: a `Body` *has* its mesh. The UI's
// shape keeps the index because the wasm engine hands meshes over separately,
// so here the index is simply the body's position.

function vecFrom(v: PbVec3 | undefined): Vec3 {
  return { x: v?.x ?? 0, y: v?.y ?? 0, z: v?.z ?? 0 };
}

function spanFrom(s: PbSourceSpan | undefined): SourceSpan | null {
  if (!s) return null;
  if (!s.path && s.startLine === 0 && s.endLine === 0) return null;
  return {
    path: s.path,
    start_line: s.startLine,
    start_col: s.startCol,
    end_line: Math.max(s.endLine, s.startLine),
    end_col: s.endCol,
  };
}

function diagnosticFrom(d: PbDiagnostic): Diagnostic {
  return {
    // cadpole.v1.Severity: 1 info, 2 warning, 3 error.
    severity: d.severity >= 3 ? "error" : d.severity === 2 ? "warning" : "info",
    message: d.message,
    span: spanFrom(d.span),
    feature_id: d.featureId,
  };
}

function paramInfoFrom(p: PbParamInfo): ParamInfo {
  return {
    name: p.name,
    default_value: p.defaultValue ? paramValueFrom(p.defaultValue) : 0,
    value: p.value ? paramValueFrom(p.value) : 0,
    overridden: p.overridden,
    description: p.description,
    unit: p.unit,
    // proto3 `optional` arrives as `undefined`, which is exactly "the script
    // did not say"; `null` is how the UI spells that.
    min: p.min ?? null,
    max: p.max ?? null,
    step: p.step ?? null,
    choices: p.choices,
    span: spanFrom(p.span),
  };
}

function featureFrom(f: PbFeature): Feature {
  return {
    id: f.id,
    kind: f.kind,
    name: f.name,
    inputs: f.inputs,
    outputs: f.outputs,
    args: { ...f.args },
    span: spanFrom(f.span),
    suppressed: f.suppressed,
    diagnostics: f.diagnostics.map(diagnosticFrom),
  };
}

function faceFrom(f: PbTopoFace): TopoFace {
  return {
    name: f.name,
    surface_kind: f.surfaceKind,
    centroid: vecFrom(f.centroid),
    normal: vecFrom(f.normal),
    area: f.area,
    edges: f.edges,
    tri_start: f.triStart,
    tri_count: f.triCount,
  };
}

function edgeFrom(e: PbTopoEdge): TopoEdge {
  return {
    name: e.name,
    curve_kind: e.curveKind,
    start: vecFrom(e.start),
    end: vecFrom(e.end),
    length: e.length,
    faces: e.faces,
    polyline: e.polyline.map(vecFrom),
  };
}

function vertexFrom(v: PbTopoVertex): TopoVertex {
  return { name: v.name, position: vecFrom(v.position) };
}

function transformFrom(t: PbBody["transform"]): Transform | null {
  if (!t) return null;
  return {
    translation: vecFrom(t.translation),
    rotation: { x: t.rotation?.x ?? 0, y: t.rotation?.y ?? 0, z: t.rotation?.z ?? 0, w: t.rotation?.w ?? 1 },
  };
}

function bodyFrom(b: PbBody, index: number, meshIndex: number): Body {
  return {
    id: b.id || `body_${index}`,
    feature_id: b.featureId,
    name: b.name,
    bbox: { min: vecFrom(b.bbox?.min), max: vecFrom(b.bbox?.max) },
    topology: {
      faces: (b.topology?.faces ?? []).map(faceFrom),
      edges: (b.topology?.edges ?? []).map(edgeFrom),
      vertices: (b.topology?.vertices ?? []).map(vertexFrom),
    },
    mesh_index: meshIndex,
    visible: b.visible,
    material: materialFrom(b.material),
    part_body: b.partBody,
    transform: transformFrom(b.transform),
    instance: b.instance,
    layer: b.layer,
  };
}

/** `cadpole.v1.Layer` -> the UI's `Layer`. */
function layerFrom(l: PbLayer): Layer {
  return {
    name: l.name,
    doc: l.doc,
    visible: l.visible,
    opacity: Math.max(0, Math.min(1, l.opacity)),
    color: l.color ? [l.color.r, l.color.g, l.color.b] : null,
    order: l.order,
  };
}

/** `cadpole.v1.Plate` -> the UI's `Plate`. */
function plateFrom(p: PbPlate): Plate {
  return {
    index: p.index,
    name: p.name,
    path: p.path,
    bed: { x: p.bedX, y: p.bedY },
    origin: vecFrom(p.origin),
    bodies: p.bodies.map((b) => ({
      body_id: b.bodyId,
      object: b.object,
      transform: transformFrom(b.transform) ?? { translation: { x: 0, y: 0, z: 0 }, rotation: { x: 0, y: 0, z: 0, w: 1 } },
      slot: Math.max(1, b.slot),
      color: b.color ? [b.color.r, b.color.g, b.color.b] : [0.75, 0.75, 0.75],
      filament: b.filament,
    })),
  };
}

function materialFrom(m: PbBody["material"]): Body["material"] {
  if (!m) return null;
  const c = m.baseColor;
  return {
    color: c ? [c.r, c.g, c.b] : null,
    // proto3 scalars default to zero, and zero roughness is a mirror rather
    // than "unset", so a body that never named one must not become chrome.
    roughness: m.roughness > 0 ? m.roughness : null,
    metallic: m.metallic > 0 ? m.metallic : null,
    opacity: m.opacity === undefined ? null : m.opacity,
  };
}

/**
 * A `cadpole.v1.Model` as the UI's `Model` and the meshes that go with it.
 *
 * protobuf-es decodes `repeated float [packed]` to a `number[]`, not to a
 * `Float32Array`, so the arrays are copied once here into the typed arrays the
 * viewer uploads to the GPU. That copy is the price of the generated decoder
 * and it is paid once per evaluation, against an evaluation measured in
 * seconds.
 */
export function modelFrom(m: PbModel): { model: Model; meshes: MeshData[] } {
  const meshes: MeshData[] = [];
  // An instance body carries no mesh: it names the part body whose mesh it
  // shares, and that body precedes it in the list, so its index is
  // known by the time the instance needs it.
  const indexOf = new Map<string, number>();
  const bodies = m.bodies.map((b, i) => {
    let meshIndex = i;
    if (b.partBody && indexOf.has(b.partBody)) {
      meshIndex = indexOf.get(b.partBody) as number;
    } else {
      meshes[i] = {
        positions: new Float32Array(b.mesh?.positions ?? []),
        normals: new Float32Array(b.mesh?.normals ?? []),
        indices: new Uint32Array(b.mesh?.indices ?? []),
        faceIds: new Uint32Array(b.mesh?.faceIds ?? []),
        // `bytes` decodes to a view into the response; a copy lets the
        // response go. Three bytes a vertex, or nothing.
        ...(b.mesh && b.mesh.colors.length > 0 ? { colors: b.mesh.colors.slice() } : {}),
      };
      indexOf.set(b.id, i);
    }
    return bodyFrom(b, i, meshIndex);
  });
  return {
    model: {
      params: m.params.map(paramInfoFrom),
      features: m.features.map(featureFrom),
      bodies,
      diagnostics: m.diagnostics.map(diagnosticFrom),
      eval_millis: m.evalMillis,
      plates: m.plates.map(plateFrom),
      layers: m.layers.map(layerFrom),
      // The script's `units(...)`, or a bake's `--units`:
      // a house reads in feet and inches from a server or a bake alike.
      ...(m.displayUnits?.unit ? { display_units: norm.displayUnits({ unit: m.displayUnits.unit, fraction: m.displayUnits.fraction }) } : {}),
    },
    meshes,
  };
}
