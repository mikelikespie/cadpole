// Defensive coercion of engine / server JSON into the strict shapes in types.ts.
//
// The proto -> JSON mapping is snake_case, but a few encodings are ambiguous
// (enums as names or numbers, Vec3 as {x,y,z} or [x,y,z], ParamValue as a
// plain value or a `{number: 1}` oneof wrapper). Everything here accepts all
// of those so the UI never sees `undefined` where a value is expected.

import type { ReplyFrame } from "./reply-stream.js";
import type {
  Attachment,
  Body,
  Diagnostic,
  DisplayUnits,
  EntityKind,
  EntityRef,
  Feature,
  CadpoleDocument,
  CadpoleEvent,
  Layer,
  LengthUnit,
  Message,
  Model,
  ParamInfo,
  ParamValue,
  Plate,
  PlateBody,
  Selection,
  Session,
  Severity,
  SourceSpan,
  TopoEdge,
  TopoFace,
  TopoVertex,
  Vec3,
} from "./types.js";

type Raw = Record<string, unknown>;

function obj(v: unknown): Raw {
  return v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Raw) : {};
}

function arr(v: unknown): unknown[] {
  return Array.isArray(v) ? v : [];
}

function str(v: unknown, fallback = ""): string {
  return typeof v === "string" ? v : v === null || v === undefined ? fallback : String(v);
}

function num(v: unknown, fallback = 0): number {
  if (typeof v === "number") return Number.isFinite(v) ? v : fallback;
  if (typeof v === "string" && v.trim() !== "") {
    const n = Number(v);
    return Number.isFinite(n) ? n : fallback;
  }
  return fallback;
}

function optNum(v: unknown): number | null {
  return v === null || v === undefined ? null : num(v);
}

function bool(v: unknown): boolean {
  return v === true || v === "true" || v === 1;
}

function strList(v: unknown): string[] {
  return arr(v).map((x) => str(x));
}

function strMap(v: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, val] of Object.entries(obj(v))) out[k] = str(val);
  return out;
}

export function vec3(v: unknown): Vec3 {
  if (Array.isArray(v)) return { x: num(v[0]), y: num(v[1]), z: num(v[2]) };
  const o = obj(v);
  return { x: num(o["x"]), y: num(o["y"]), z: num(o["z"]) };
}

export function isVec3(v: unknown): v is Vec3 {
  const o = obj(v);
  return typeof o["x"] === "number" && typeof o["y"] === "number" && typeof o["z"] === "number";
}

/** Accepts a plain value or a oneof wrapper such as `{number: 3}` / `{vector: {...}}`. */
export function paramValue(v: unknown): ParamValue {
  if (typeof v === "number" || typeof v === "boolean" || typeof v === "string") return v;
  if (Array.isArray(v)) return vec3(v);
  const o = obj(v);
  if ("number" in o) return num(o["number"]);
  if ("boolean" in o) return bool(o["boolean"]);
  if ("text" in o) return str(o["text"]);
  if ("vector" in o) return vec3(o["vector"]);
  if (isVec3(o)) return { x: o["x"] as number, y: o["y"] as number, z: o["z"] as number };
  return 0;
}

export function paramValueMap(v: unknown): Record<string, ParamValue> {
  const out: Record<string, ParamValue> = {};
  for (const [k, val] of Object.entries(obj(v))) out[k] = paramValue(val);
  return out;
}

export function severity(v: unknown): Severity {
  if (typeof v === "number") return v >= 3 ? "error" : v === 2 ? "warning" : "info";
  const s = str(v).toLowerCase();
  if (s.includes("err")) return "error";
  if (s.includes("warn")) return "warning";
  return "info";
}

export function lengthUnit(v: unknown): LengthUnit {
  if (typeof v === "number") return (["mm", "mm", "cm", "m", "in", "ft"] as const)[v] ?? "mm";
  const s = str(v).toLowerCase().replace("length_unit_", "");
  if (s === "ft-in" || s === "ft_in" || s === "ftin") return "ft-in";
  if (s === "inch") return "in";
  return s === "cm" || s === "m" || s === "in" || s === "ft" ? s : "mm";
}

/** The model's `display_units`, or `undefined` for a script that declared none. */
export function displayUnits(v: unknown): DisplayUnits | undefined {
  if (v === null || v === undefined || typeof v !== "object") return undefined;
  const o = obj(v);
  if (!str(o["unit"])) return undefined;
  return { unit: lengthUnit(o["unit"]), fraction: num(o["fraction"], 0) };
}

export function entityKind(v: unknown): EntityKind {
  if (typeof v === "number") {
    return (
      (
        [
          "feature",
          "feature",
          "body",
          "face",
          "edge",
          "vertex",
          "sketch",
          "sketch_entity",
          "param",
          "asset",
        ] as const
      )[v] ?? "feature"
    );
  }
  const s = str(v).toLowerCase().replace("entity_kind_", "");
  switch (s) {
    // `feature` is listed even though it is also the fallback below: this
    // switch is the vocabulary, and a kind that is only covered by `default`
    // cannot be told from one nobody has heard of. `//spec:names_test` diffs
    // these arms against `cadpole.v1.EntityKind`.
    case "feature":
    case "body":
    case "face":
    case "edge":
    case "vertex":
    case "sketch":
    case "sketch_entity":
    case "param":
    case "asset":
      return s;
    default:
      return "feature";
  }
}

export function span(v: unknown): SourceSpan | null {
  if (v === null || v === undefined) return null;
  const o = obj(v);
  const s: SourceSpan = {
    path: str(o["path"]),
    start_line: num(o["start_line"]),
    start_col: num(o["start_col"]),
    end_line: num(o["end_line"]),
    end_col: num(o["end_col"]),
  };
  if (s.start_line === 0 && s.end_line === 0 && s.path === "") return null;
  if (s.end_line < s.start_line) s.end_line = s.start_line;
  return s;
}

export function diagnostic(v: unknown): Diagnostic {
  const o = obj(v);
  return {
    severity: severity(o["severity"]),
    message: str(o["message"]),
    span: span(o["span"]),
    feature_id: str(o["feature_id"]),
  };
}

export function paramInfo(v: unknown): ParamInfo {
  const o = obj(v);
  return {
    name: str(o["name"]),
    default_value: paramValue(o["default_value"]),
    value: paramValue(o["value"]),
    overridden: bool(o["overridden"]),
    description: str(o["description"]),
    unit: str(o["unit"]),
    min: optNum(o["min"]),
    max: optNum(o["max"]),
    step: optNum(o["step"]),
    choices: arr(o["choices"]).map((c) => str(c)),
    span: span(o["span"]),
  };
}

export function feature(v: unknown): Feature {
  const o = obj(v);
  return {
    id: str(o["id"]),
    kind: str(o["kind"]),
    name: str(o["name"]),
    inputs: strList(o["inputs"]),
    outputs: strList(o["outputs"]),
    args: strMap(o["args"]),
    span: span(o["span"]),
    suppressed: bool(o["suppressed"]),
    diagnostics: arr(o["diagnostics"]).map(diagnostic),
  };
}

function topoFace(v: unknown): TopoFace {
  const o = obj(v);
  return {
    name: str(o["name"]),
    surface_kind: str(o["surface_kind"]),
    centroid: vec3(o["centroid"]),
    normal: vec3(o["normal"]),
    area: num(o["area"]),
    edges: strList(o["edges"]),
    tri_start: num(o["tri_start"]),
    tri_count: num(o["tri_count"]),
  };
}

function topoEdge(v: unknown): TopoEdge {
  const o = obj(v);
  return {
    name: str(o["name"]),
    curve_kind: str(o["curve_kind"]),
    start: vec3(o["start"]),
    end: vec3(o["end"]),
    length: num(o["length"]),
    faces: strList(o["faces"]),
    polyline: arr(o["polyline"]).map(vec3),
  };
}

function topoVertex(v: unknown): TopoVertex {
  const o = obj(v);
  return { name: str(o["name"]), position: vec3(o["position"]) };
}

/** A linear RGB triple from either `[r, g, b]` or `{r, g, b}`; null if absent. */
function colorTriple(v: unknown): [number, number, number] | null {
  if (v === undefined || v === null) return null;
  if (Array.isArray(v)) {
    if (v.length < 3) return null;
    return [num(v[0]), num(v[1]), num(v[2])];
  }
  if (typeof v === "object") {
    const k = obj(v);
    if (!("r" in k) && !("g" in k) && !("b" in k)) return null;
    return [num(k["r"]), num(k["g"]), num(k["b"])];
  }
  return null;
}

/** `body.material(...)` as the JSON view reports it. */
export function bodyMaterial(v: unknown): Body["material"] {
  if (v === undefined || v === null) return null;
  const o = obj(v);
  const c = o["base_color"];
  const rough = num(o["roughness"]);
  const metal = num(o["metallic"]);
  const op = o["opacity"];
  return {
    // The JSON view writes a colour as `[r, g, b]`; the proto writes a message
    // with named fields, and some older payloads used the object shape here
    // too. Accept both. Reading the wrong one silently yields (0, 0, 0), which
    // is not an error anywhere -- it is simply black, and every body in the
    // viewport went black before this distinction was made.
    color: colorTriple(c),
    // Zero is proto3's "unset" here, and zero roughness is a mirror.
    roughness: rough > 0 ? rough : null,
    metallic: metal > 0 ? metal : null,
    opacity: op === undefined || op === null ? null : num(op),
  };
}

export function body(v: unknown, index: number): Body {
  const o = obj(v);
  const topo = obj(o["topology"]);
  const bbox = obj(o["bbox"]);
  return {
    id: str(o["id"], `body_${index}`),
    feature_id: str(o["feature_id"]),
    name: str(o["name"]),
    bbox: { min: vec3(bbox["min"]), max: vec3(bbox["max"]) },
    topology: {
      faces: arr(topo["faces"]).map(topoFace),
      edges: arr(topo["edges"]).map(topoEdge),
      vertices: arr(topo["vertices"]).map(topoVertex),
    },
    mesh_index: num(o["mesh_index"], index),
    visible: o["visible"] === undefined ? true : bool(o["visible"]),
    material: bodyMaterial(o["material"]),
    part_body: str(o["part_body"]),
    transform: transform(o["transform"]),
    instance: str(o["instance"]),
    layer: str(o["layer"]),
  };
}

/** `cadpole.v1.Layer` as the JSON view spells it. */
export function layer(v: unknown, index: number): Layer {
  const o = obj(v);
  const op = num(o["opacity"], 1);
  return {
    name: str(o["name"]),
    doc: str(o["doc"]),
    visible: o["visible"] === undefined ? true : bool(o["visible"]),
    opacity: Math.max(0, Math.min(1, Number.isFinite(op) ? op : 1)),
    color: colorTriple(o["color"]),
    order: Math.max(0, Math.round(num(o["order"], index))),
  };
}

/** `cadpole.v1.Transform` as the JSON view spells it: `translation` and `rotation` as number arrays. */
function transform(v: unknown): Body["transform"] {
  if (v === null || v === undefined) return null;
  const o = obj(v);
  const t = arr(o["translation"]);
  const r = arr(o["rotation"]);
  if (t.length < 3 || r.length < 4) return null;
  return {
    translation: { x: num(t[0]), y: num(t[1]), z: num(t[2]) },
    rotation: { x: num(r[0]), y: num(r[1]), z: num(r[2]), w: num(r[3], 1) },
  };
}

/** `cadpole.v1.PlateBody` as the JSON view spells it. */
function plateBody(v: unknown): PlateBody {
  const o = obj(v);
  return {
    body_id: str(o["body_id"]),
    object: str(o["object"]),
    // A placement is always sent; a missing one is the identity, which draws
    // the body where it is rather than nowhere.
    transform: transform(o["transform"]) ?? { translation: { x: 0, y: 0, z: 0 }, rotation: { x: 0, y: 0, z: 0, w: 1 } },
    slot: Math.max(1, Math.round(num(o["slot"], 1))),
    color: colorTriple(o["color"]) ?? [0.75, 0.75, 0.75],
    filament: str(o["filament"]),
  };
}

/** `cadpole.v1.Plate` as the JSON view spells it. */
export function plate(v: unknown, index: number): Plate {
  const o = obj(v);
  return {
    index: Math.max(1, Math.round(num(o["index"], index + 1))),
    name: str(o["name"]),
    path: str(o["path"]),
    bed: { x: num(o["bed_x"], 256), y: num(o["bed_y"], 256) },
    origin: vec3(o["origin"]),
    bodies: arr(o["bodies"]).map(plateBody),
  };
}

export function model(v: unknown): Model {
  const o = obj(v);
  const m: Model = {
    params: arr(o["params"]).map(paramInfo),
    features: arr(o["features"]).map(feature),
    bodies: arr(o["bodies"]).map(body),
    diagnostics: arr(o["diagnostics"]).map(diagnostic),
    eval_millis: num(o["eval_millis"]),
    plates: arr(o["plates"]).map(plate),
    layers: arr(o["layers"]).map(layer),
  };
  const units = displayUnits(o["display_units"] ?? o["displayUnits"]);
  if (units) m.display_units = units;
  return m;
}

export function entityRef(v: unknown): EntityRef {
  const o = obj(v);
  const ref: EntityRef = { feature_id: str(o["feature_id"]), kind: entityKind(o["kind"]) };
  const bodyId = str(o["body_id"]);
  const topo = str(o["topo_name"]);
  if (bodyId) ref.body_id = bodyId;
  if (topo) ref.topo_name = topo;
  return ref;
}

export function selection(v: unknown): Selection {
  const o = obj(v);
  const hovered = o["hovered"];
  return {
    selected: arr(o["selected"]).map(entityRef),
    hovered: hovered && Object.keys(obj(hovered)).length > 0 ? entityRef(hovered) : null,
    note: str(o["note"]),
  };
}

export function message(v: unknown): Message {
  const o = obj(v);
  const from = str(o["from"]).toLowerCase();
  const m: Message = {
    seq: num(o["seq"]),
    from: from === "assistant" ? "assistant" : "user",
    text: str(o["text"]),
    at: str(o["at"]),
  };
  const state = str(o["state"]);
  if (state === "streaming" || state === "done" || state === "failed") m.state = state;
  if (m.state === "streaming" && str(o["activity"])) m.activity = str(o["activity"]);
  const atts = arr(o["attachments"])
    .map((a) => {
      const ao = obj(a);
      // An attachment with no bytes is a *mention* of an image, not an image:
      // the event stream and the session read both say how many screenshots a
      // message has without carrying them (they come from GetAttachment). Drop
      // those rather than rendering a broken <img>.
      if (typeof ao["png_base64"] !== "string" || ao["png_base64"] === "") return null;
      const cam = obj(ao["camera"]);
      const att: Attachment = {
        kind: "screenshot",
        png_base64: ao["png_base64"],
        camera: {
          eye: vec3(cam["eye"]),
          target: vec3(cam["target"]),
          up: cam["up"] === undefined ? { x: 0, y: 0, z: 1 } : vec3(cam["up"]),
          orthographic: bool(cam["orthographic"]),
          fov_degrees: num(cam["fov_degrees"], 40),
        },
      };
      if (ao["annotated"] !== undefined) att.annotated = bool(ao["annotated"]);
      if (typeof ao["width"] === "number") att.width = ao["width"];
      if (typeof ao["height"] === "number") att.height = ao["height"];
      return att;
    })
    .filter((a): a is Attachment => a !== null);
  if (atts.length) m.attachments = atts;
  return m;
}

export function document(v: unknown): CadpoleDocument {
  const o = obj(v);
  return {
    id: str(o["id"]),
    name: str(o["name"], "untitled"),
    display_unit: lengthUnit(o["display_unit"]),
    scripts: arr(o["scripts"]).map((s) => {
      const so = obj(s);
      return { path: str(so["path"], "main.star"), source: str(so["source"]) };
    }),
    param_overrides: paramValueMap(o["param_overrides"]),
  };
}

export function session(v: unknown): Session {
  const o = obj(v);
  return {
    id: str(o["id"]),
    revision: num(o["revision"]),
    lastEventSeq: num(o["last_event_seq"]),
    webBuild: str(o["web_build"]),
    document: document(o["document"]),
    selection: selection(o["selection"]),
    messages: arr(o["messages"]).map(message),
    clients: num(o["clients"]),
  };
}

export function event(v: unknown): CadpoleEvent {
  const o = obj(v);
  return { seq: num(o["seq"]), at: str(o["at"]), kind: str(o["kind"]), data: o["data"] };
}

/** The payload of a `reply_stream` frame (reply-stream.ts). */
export function replyFrame(v: unknown): ReplyFrame {
  const o = obj(v);
  return { seq: num(o["seq"]), at: str(o["at"]), offset: num(o["offset"]), append: str(o["append"]), activity: str(o["activity"]) };
}

export function isMessageLike(v: unknown): boolean {
  const o = obj(v);
  return typeof o["text"] === "string" && typeof o["from"] === "string";
}
