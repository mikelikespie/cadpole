// MockEngine: a hardcoded parametric "plate with a boss" so the whole UI can
// be exercised without the wasm build. It produces a proper cadpole.v1.Model:
// features with source spans, bodies with named faces/edges/vertices,
// per-triangle face ids, diagnostics and eval timing.
//
// It is *not* a Starlark interpreter. It only scrapes `param("name", default,
// ...)` calls (for defaults, ranges and spans), the positions of `sketch(` /
// `extrude(` calls (for feature spans) and a `units(...)` declaration (the
// document's display convention) from the script text.

import { DEFAULT_TOLERANCE, throwIfAborted, type EngineLike, type EvaluateRequest, type Evaluated } from "./engine.js";
import { lengthUnit } from "./normalize.js";
import type {
  ApiDoc,
  Body,
  Diagnostic,
  DisplayUnits,
  Feature,
  Layer,
  Model,
  ParamInfo,
  ParamValue,
  Plate,
  SourceSpan,
  TopoEdge,
  TopoFace,
  TopoVertex,
  Vec3,
} from "./types.js";

interface MeshData {
  positions: Float32Array;
  normals: Float32Array;
  indices: Uint32Array;
  faceIds: Uint32Array;
}

interface BuiltBody {
  mesh: MeshData;
  faces: TopoFace[];
  edges: TopoEdge[];
  vertices: TopoVertex[];
  bbox: { min: Vec3; max: Vec3 };
}

interface ParamSpec {
  name: string;
  /** What the default's literal was: a number, `True`/`False`, or a string. */
  kind: "number" | "bool" | "text";
  def: number | boolean | string;
  min: number;
  max: number;
  step: number;
  unit: string;
  description: string;
  /** `choices=[...]` of a text parameter, else empty. */
  choices: string[];
  span: SourceSpan | null;
}

const numberSpec = (name: string, def: number, min: number, max: number, step: number, unit: string, description: string): ParamSpec => ({
  name,
  kind: "number",
  def,
  min,
  max,
  step,
  unit,
  description,
  choices: [],
  span: null,
});

const DEFAULT_PARAMS: ParamSpec[] = [
  numberSpec("width", 60, 20, 200, 1, "mm", "Plate width (X)"),
  numberSpec("depth", 40, 20, 200, 1, "mm", "Plate depth (Y)"),
  numberSpec("height", 20, 2, 100, 0.5, "mm", "Plate thickness (Z)"),
  numberSpec("boss_d", 18, 2, 120, 0.5, "mm", "Boss diameter"),
  numberSpec("boss_h", 12, 0, 100, 0.5, "mm", "Boss height"),
];

const v3 = (x: number, y: number, z: number): Vec3 => ({ x, y, z });

class MeshBuilder {
  positions: number[] = [];
  normals: number[] = [];
  indices: number[] = [];
  faceIds: number[] = [];

  vertex(p: Vec3, n: Vec3): number {
    this.positions.push(p.x, p.y, p.z);
    this.normals.push(n.x, n.y, n.z);
    return this.positions.length / 3 - 1;
  }

  tri(a: number, b: number, c: number, faceId: number): void {
    this.indices.push(a, b, c);
    this.faceIds.push(faceId);
  }

  triCount(): number {
    return this.faceIds.length;
  }

  build(): MeshData {
    return {
      positions: Float32Array.from(this.positions),
      normals: Float32Array.from(this.normals),
      indices: Uint32Array.from(this.indices),
      faceIds: Uint32Array.from(this.faceIds),
    };
  }
}

function dist(a: Vec3, b: Vec3): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
}

/** Axis-aligned box: x in [-w/2, w/2], y in [-d/2, d/2], z in [z0, z0+h]. */
function buildBox(w: number, d: number, h: number, z0: number): BuiltBody {
  const x0 = -w / 2;
  const x1 = w / 2;
  const y0 = -d / 2;
  const y1 = d / 2;
  const z1 = z0 + h;
  // Vertex corners, named by their (x,y,z) sign combination.
  const corner = (x: 0 | 1, y: 0 | 1, z: 0 | 1): Vec3 => v3(x ? x1 : x0, y ? y1 : y0, z ? z1 : z0);
  const cname = (x: 0 | 1, y: 0 | 1, z: 0 | 1): string =>
    `vertex:${x ? "right" : "left"}/${y ? "back" : "front"}/${z ? "top" : "bottom"}`;

  const mb = new MeshBuilder();
  const faces: TopoFace[] = [];
  // Each face: name, normal, its four corners in CCW order (seen from outside).
  const faceDefs: { name: string; normal: Vec3; corners: [0 | 1, 0 | 1, 0 | 1][] }[] = [
    { name: "face:bottom", normal: v3(0, 0, -1), corners: [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]] },
    { name: "face:top", normal: v3(0, 0, 1), corners: [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]] },
    { name: "face:front", normal: v3(0, -1, 0), corners: [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]] },
    { name: "face:back", normal: v3(0, 1, 0), corners: [[1, 1, 0], [0, 1, 0], [0, 1, 1], [1, 1, 1]] },
    { name: "face:left", normal: v3(-1, 0, 0), corners: [[0, 1, 0], [0, 0, 0], [0, 0, 1], [0, 1, 1]] },
    { name: "face:right", normal: v3(1, 0, 0), corners: [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]] },
  ];
  const faceArea: Record<string, number> = {
    "face:bottom": w * d,
    "face:top": w * d,
    "face:front": w * h,
    "face:back": w * h,
    "face:left": d * h,
    "face:right": d * h,
  };
  faceDefs.forEach((fd, fi) => {
    const triStart = mb.triCount();
    const ids = fd.corners.map(([x, y, z]) => mb.vertex(corner(x, y, z), fd.normal));
    const [a, b, c, e] = ids as [number, number, number, number];
    mb.tri(a, b, c, fi);
    mb.tri(a, c, e, fi);
    const cs = fd.corners.map(([x, y, z]) => corner(x, y, z));
    const centroid = v3(
      cs.reduce((s, p) => s + p.x, 0) / 4,
      cs.reduce((s, p) => s + p.y, 0) / 4,
      cs.reduce((s, p) => s + p.z, 0) / 4,
    );
    faces.push({
      name: fd.name,
      surface_kind: "plane",
      centroid,
      normal: fd.normal,
      area: faceArea[fd.name] ?? 0,
      edges: [],
      tri_start: triStart,
      tri_count: 2,
    });
  });

  // Twelve edges, each the intersection of two faces.
  const edges: TopoEdge[] = [];
  const addEdge = (fa: string, fb: string, c0: [0 | 1, 0 | 1, 0 | 1], c1: [0 | 1, 0 | 1, 0 | 1]): void => {
    const start = corner(...c0);
    const end = corner(...c1);
    const name = `edge:${fa.slice(5)}/${fb.slice(5)}`;
    edges.push({
      name,
      curve_kind: "line",
      start,
      end,
      length: dist(start, end),
      faces: [fa, fb],
      polyline: [start, end],
    });
    for (const f of faces) if (f.name === fa || f.name === fb) f.edges.push(name);
  };
  addEdge("face:top", "face:front", [0, 0, 1], [1, 0, 1]);
  addEdge("face:top", "face:back", [0, 1, 1], [1, 1, 1]);
  addEdge("face:top", "face:left", [0, 0, 1], [0, 1, 1]);
  addEdge("face:top", "face:right", [1, 0, 1], [1, 1, 1]);
  addEdge("face:bottom", "face:front", [0, 0, 0], [1, 0, 0]);
  addEdge("face:bottom", "face:back", [0, 1, 0], [1, 1, 0]);
  addEdge("face:bottom", "face:left", [0, 0, 0], [0, 1, 0]);
  addEdge("face:bottom", "face:right", [1, 0, 0], [1, 1, 0]);
  addEdge("face:front", "face:left", [0, 0, 0], [0, 0, 1]);
  addEdge("face:front", "face:right", [1, 0, 0], [1, 0, 1]);
  addEdge("face:back", "face:left", [0, 1, 0], [0, 1, 1]);
  addEdge("face:back", "face:right", [1, 1, 0], [1, 1, 1]);

  const vertices: TopoVertex[] = [];
  for (const x of [0, 1] as const)
    for (const y of [0, 1] as const)
      for (const z of [0, 1] as const) vertices.push({ name: cname(x, y, z), position: corner(x, y, z) });

  return { mesh: mb.build(), faces, edges, vertices, bbox: { min: v3(x0, y0, z0), max: v3(x1, y1, z1) } };
}

/** Cylinder of radius r, axis +Z, z in [z0, z0+h], centred on (cx, cy). */
function buildCylinder(cx: number, cy: number, r: number, z0: number, h: number, segments: number): BuiltBody {
  const z1 = z0 + h;
  const mb = new MeshBuilder();
  const faces: TopoFace[] = [];
  const ring = (z: number): Vec3[] => {
    const out: Vec3[] = [];
    for (let i = 0; i <= segments; i++) {
      const t = (i / segments) * Math.PI * 2;
      out.push(v3(cx + r * Math.cos(t), cy + r * Math.sin(t), z));
    }
    return out;
  };
  const bottomRing = ring(z0);
  const topRing = ring(z1);

  // face 0: side (cylinder). Seam at angle 0 (i = 0 and i = segments share a position).
  {
    const triStart = mb.triCount();
    const lower: number[] = [];
    const upper: number[] = [];
    for (let i = 0; i <= segments; i++) {
      const t = (i / segments) * Math.PI * 2;
      const n = v3(Math.cos(t), Math.sin(t), 0);
      lower.push(mb.vertex(bottomRing[i] as Vec3, n));
      upper.push(mb.vertex(topRing[i] as Vec3, n));
    }
    for (let i = 0; i < segments; i++) {
      const a = lower[i] as number;
      const b = lower[i + 1] as number;
      const c = upper[i + 1] as number;
      const d = upper[i] as number;
      mb.tri(a, b, c, 0);
      mb.tri(a, c, d, 0);
    }
    faces.push({
      name: "face:side",
      surface_kind: "cylinder",
      centroid: v3(cx, cy, z0 + h / 2),
      normal: v3(0, 0, 0),
      area: 2 * Math.PI * r * h,
      edges: ["edge:bottom", "edge:top", "edge:seam"],
      tri_start: triStart,
      tri_count: mb.triCount() - triStart,
    });
  }
  // face 1: top cap, face 2: bottom cap (triangle fans).
  const cap = (name: string, z: number, nz: 1 | -1, faceId: number, pts: Vec3[]): void => {
    const triStart = mb.triCount();
    const n = v3(0, 0, nz);
    const center = mb.vertex(v3(cx, cy, z), n);
    const ids = pts.map((p) => mb.vertex(p, n));
    for (let i = 0; i < segments; i++) {
      const a = ids[i] as number;
      const b = ids[i + 1] as number;
      if (nz > 0) mb.tri(center, a, b, faceId);
      else mb.tri(center, b, a, faceId);
    }
    faces.push({
      name,
      surface_kind: "plane",
      centroid: v3(cx, cy, z),
      normal: n,
      area: Math.PI * r * r,
      edges: [nz > 0 ? "edge:top" : "edge:bottom"],
      tri_start: triStart,
      tri_count: mb.triCount() - triStart,
    });
  };
  cap("face:top", z1, 1, 1, topRing);
  cap("face:bottom", z0, -1, 2, bottomRing);

  const seamStart = bottomRing[0] as Vec3;
  const seamEnd = topRing[0] as Vec3;
  const edges: TopoEdge[] = [
    {
      name: "edge:bottom",
      curve_kind: "circle",
      start: seamStart,
      end: seamStart,
      length: 2 * Math.PI * r,
      faces: ["face:side", "face:bottom"],
      polyline: bottomRing,
    },
    {
      name: "edge:top",
      curve_kind: "circle",
      start: seamEnd,
      end: seamEnd,
      length: 2 * Math.PI * r,
      faces: ["face:side", "face:top"],
      polyline: topRing,
    },
    {
      name: "edge:seam",
      curve_kind: "line",
      start: seamStart,
      end: seamEnd,
      length: h,
      faces: ["face:side", "face:side"],
      polyline: [seamStart, seamEnd],
    },
  ];
  const vertices: TopoVertex[] = [
    { name: "vertex:seam/bottom", position: seamStart },
    { name: "vertex:seam/top", position: seamEnd },
  ];
  return {
    mesh: mb.build(),
    faces,
    edges,
    vertices,
    bbox: { min: v3(cx - r, cy - r, z0), max: v3(cx + r, cy + r, z1) },
  };
}

// The default is a number, `True`/`False` or a quoted string; a keyword's
// value is a quoted string, a list (`choices=["a", "b"]`) or a bare literal.
const PARAM_RE =
  /param\(\s*"([A-Za-z_][A-Za-z0-9_]*)"\s*,\s*(-?[0-9.]+|True|False|"[^"]*")((?:\s*,\s*[A-Za-z_]+\s*=\s*(?:"[^"]*"|\[[^\]]*\]|-?[A-Za-z0-9_.]+))*)\s*\)/;
const KWARG_RE = /([A-Za-z_]+)\s*=\s*(?:"([^"]*)"|\[([^\]]*)\]|(-?[A-Za-z0-9_.]+))/g;

/** Scrape `param(...)` declarations (with spans) from the script text. */
function scrapeParams(path: string, source: string): ParamSpec[] {
  const out: ParamSpec[] = [];
  const lines = source.split("\n");
  lines.forEach((line, i) => {
    const m = PARAM_RE.exec(line);
    if (!m) return;
    const name = m[1] as string;
    const lit = m[2] as string;
    const base = DEFAULT_PARAMS.find((p) => p.name === name);
    const kind: ParamSpec["kind"] = lit === "True" || lit === "False" ? "bool" : lit.startsWith('"') ? "text" : "number";
    const def = kind === "bool" ? lit === "True" : kind === "text" ? lit.slice(1, -1) : Number(lit);
    const spec: ParamSpec = {
      name,
      kind,
      def: typeof def === "number" && !Number.isFinite(def) ? (base?.def ?? 0) : def,
      min: base?.min ?? -1e9,
      max: base?.max ?? 1e9,
      step: base?.step ?? 1,
      unit: base?.unit ?? "",
      description: base?.description ?? "",
      choices: [],
      span: {
        path,
        start_line: i + 1,
        start_col: (m.index ?? 0) + 1,
        end_line: i + 1,
        end_col: (m.index ?? 0) + m[0].length + 1,
      },
    };
    const kw = m[3] ?? "";
    for (const km of kw.matchAll(KWARG_RE)) {
      const key = km[1] as string;
      const val = km[2] ?? km[3] ?? km[4] ?? "";
      const n = Number(val);
      if (key === "min" && Number.isFinite(n)) spec.min = n;
      else if (key === "max" && Number.isFinite(n)) spec.max = n;
      else if (key === "step" && Number.isFinite(n)) spec.step = n;
      else if (key === "unit") spec.unit = val;
      else if (key === "description" || key === "doc") spec.description = val;
      else if (key === "choices" && km[3] !== undefined) {
        spec.choices = val
          .split(",")
          .map((c) => c.trim().replace(/^"|"$/g, ""))
          .filter((c) => c.length > 0);
      }
    }
    out.push(spec);
  });
  return out;
}

function findCallSpans(path: string, source: string, fn: string): SourceSpan[] {
  const out: SourceSpan[] = [];
  const re = new RegExp(`\\b${fn}\\(`);
  source.split("\n").forEach((line, i) => {
    const m = re.exec(line);
    if (m) {
      out.push({
        path,
        start_line: i + 1,
        start_col: m.index + 1,
        end_line: i + 1,
        end_col: line.trimEnd().length + 1,
      });
    }
  });
  return out;
}

// `units(units.ft_in, fraction=8)`, or the unit as a string. Top level only,
// as the real engine requires of a document's one declaration.
const UNITS_RE = /^units\(\s*(?:units\.([A-Za-z_]+)|"([^"]*)")\s*(?:,\s*fraction\s*=\s*([0-9]+)\s*)?\)/m;

/**
 * The document's display convention, from a `units(...)` line: what
 * the Parameters panel's length readouts, the inspector and the viewport's
 * labels are written in. The fraction defaults as the engine's does --
 * sixteenths of an inch for the imperial units, none for the metric ones.
 */
function scrapeUnits(source: string): DisplayUnits | undefined {
  const m = UNITS_RE.exec(source);
  if (!m) return undefined;
  const unit = lengthUnit(m[1] ?? m[2] ?? "");
  const imperial = unit === "in" || unit === "ft" || unit === "ft-in";
  return { unit, fraction: m[3] !== undefined ? Number(m[3]) : imperial ? 16 : 0 };
}

/**
 * The plates the real engine would lay out, from the script's
 * `export(bodies, "x.3mf", flavor=flavor.bambu, plate=, name=, extruder=)`
 * calls -- a text scrape, like the params, so the plates view can be driven
 * without the wasm engine. It follows the engine's rules in miniature: calls
 * to one path accumulate; `plate=` is a number or a name numbered in order
 * of first appearance after every numbered plate; `name=` makes the call one
 * object; slots are assigned per body in order of first use unless
 * `extruder=` pins one; the 256 mm bed's grid is ceil(sqrt(N)) to a row,
 * 1.2 x the bed apart, rows towards -y; and a plate's contents are
 * moved as a group so that their union footprint is centred on the bed.
 */
const EXPORT_RE = /\bexport\(\s*(\[[^\]]*\]|[A-Za-z_][A-Za-z0-9_]*)\s*,\s*"([^"]+)"([^)]*)\)/g;
const BED = 256;
const STRIDE = 1.2;
/** Stand-in filament colours, slot 1 first: gold, teal, red, blue, grey. */
const SLOT_COLORS: [number, number, number][] = [
  [0.91, 0.72, 0.29],
  [0.18, 0.56, 0.56],
  [0.85, 0.29, 0.24],
  [0.29, 0.5, 0.84],
  [0.6, 0.6, 0.6],
];

function scrapePlates(source: string, bodies: Body[]): Plate[] {
  interface Call {
    path: string;
    plate: number;
    object: string | null;
    bodies: { body: Body; pin: number | null }[];
  }
  const calls: Call[] = [];
  const namesByPath = new Map<string, string[]>();
  for (let m = EXPORT_RE.exec(source); m !== null; m = EXPORT_RE.exec(source)) {
    const [, list, path, rest] = m as unknown as [string, string, string, string];
    if (!/\bflavor\s*=\s*flavor\.bambu\b/.test(rest)) continue;
    const names = namesByPath.get(path) ?? [];
    namesByPath.set(path, names);
    const plateNum = /\bplate\s*=\s*(\d+)/.exec(rest);
    const plateName = /\bplate\s*=\s*"([^"]*)"/.exec(rest);
    let plate = 1;
    if (plateNum) plate = Math.max(1, Number(plateNum[1]));
    else if (plateName) {
      const at = names.indexOf(plateName[1] as string);
      if (at >= 0) plate = at + 1;
      else {
        names.push(plateName[1] as string);
        plate = names.length;
      }
    }
    while (names.length < plate) names.push("");
    const object = /\bname\s*=\s*"([^"]*)"/.exec(rest);
    const pinList = /\bextruder\s*=\s*\[([^\]]*)\]/.exec(rest);
    const pinOne = /\bextruder\s*=\s*(\d+)/.exec(rest);
    const pins = pinList ? pinList[1]!.split(",").map((s) => Number(s.trim())) : [];
    const ids = list.replace(/[[\]]/g, "").split(",").map((s) => s.trim()).filter(Boolean);
    const found = ids
      .map((id, k) => {
        const body = bodies.find((b) => b.name === id);
        if (!body) return null;
        const pin = pinList ? (pins[k] ?? null) : pinOne ? Number(pinOne[1]) : null;
        return { body, pin: pin !== null && Number.isFinite(pin) && pin > 0 ? pin : null };
      })
      .filter((x): x is { body: Body; pin: number | null } => x !== null);
    calls.push({ path, plate, object: object ? (object[1] as string) : null, bodies: found });
  }
  const out: Plate[] = [];
  for (const [path, names] of namesByPath) {
    const mine = calls.filter((c) => c.path === path);
    // Slots: one per body in order of first unpinned use; a pin is the
    // call's own.
    const slotOf = new Map<string, number>();
    let next = 1;
    for (const c of mine) {
      for (const { body, pin } of c.bodies) {
        if (pin === null && !slotOf.has(body.id)) slotOf.set(body.id, next++);
      }
    }
    for (let p = 1; p <= names.length; p++) {
      const placed: { body: Body; slot: number; object: string }[] = [];
      let lo = [Infinity, Infinity];
      let hi = [-Infinity, -Infinity];
      for (const c of mine.filter((c) => c.plate === p)) {
        for (const { body, pin } of c.bodies) {
          placed.push({ body, slot: pin ?? slotOf.get(body.id) ?? 1, object: c.object ?? body.name });
          lo = [Math.min(lo[0]!, body.bbox.min.x), Math.min(lo[1]!, body.bbox.min.y)];
          hi = [Math.max(hi[0]!, body.bbox.max.x), Math.max(hi[1]!, body.bbox.max.y)];
        }
      }
      const i = p - 1;
      // Bambu's grid: ceil(sqrt(N)) plates to a row.
      const cols = Math.max(1, Math.ceil(Math.sqrt(names.length)));
      const origin = { x: (i % cols) * STRIDE * BED, y: -Math.floor(i / cols) * STRIDE * BED, z: 0 };
      const shift = placed.length ? [BED / 2 - (lo[0]! + hi[0]!) / 2, BED / 2 - (lo[1]! + hi[1]!) / 2] : [0, 0];
      out.push({
        index: p,
        name: names[i] ?? "",
        path,
        bed: { x: BED, y: BED },
        origin,
        bodies: placed.map(({ body, slot, object }) => ({
          body_id: body.id,
          object,
          transform: { translation: { x: origin.x + shift[0]!, y: origin.y + shift[1]!, z: 0 }, rotation: { x: 0, y: 0, z: 0, w: 1 } },
          slot,
          color: SLOT_COLORS[(slot - 1) % SLOT_COLORS.length] as [number, number, number],
          filament: "",
        })),
      });
    }
  }
  return out;
}

/**
 * The display layers the real engine would list, from the script's
 * `x = layer("name", doc=, visible=, opacity=, color=)` calls -- a text
 * scrape, like the params -- and, for each `extrude(` in order, the layer
 * its `layer=` names (a variable from those calls, or a quoted name).
 */
const LAYER_RE = /\b(?:([A-Za-z_]\w*)\s*=\s*)?layer\(\s*"([^"]+)"([^)]*)\)/g;

function scrapeLayers(source: string): { layers: Layer[]; byVar: Map<string, string> } {
  const layers: Layer[] = [];
  const byVar = new Map<string, string>();
  for (let m = LAYER_RE.exec(source); m !== null; m = LAYER_RE.exec(source)) {
    const [, variable, name, rest] = m as unknown as [string, string | undefined, string, string];
    if (layers.some((l) => l.name === name)) continue;
    const doc = /\bdoc\s*=\s*"([^"]*)"/.exec(rest);
    const opacity = /\bopacity\s*=\s*([0-9.]+)/.exec(rest);
    const color = /\bcolor\s*=\s*"#([0-9a-fA-F]{6})"/.exec(rest);
    const hex = color ? color[1] as string : null;
    const chan = (i: number): number => {
      const c = Number.parseInt((hex as string).slice(i, i + 2), 16) / 255;
      return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
    };
    layers.push({
      name,
      doc: doc ? (doc[1] as string) : "",
      visible: !/\bvisible\s*=\s*False\b/.test(rest),
      opacity: opacity ? Math.max(0, Math.min(1, Number(opacity[1]))) : 1,
      color: hex ? [chan(0), chan(2), chan(4)] : null,
      order: layers.length,
    });
    if (variable) byVar.set(variable, name);
  }
  return { layers, byVar };
}

/** The layer the k-th `extrude(` line puts its body on, or "". */
function scrapeBodyLayer(source: string, k: number, byVar: Map<string, string>): string {
  const lines = source.split("\n").filter((l) => /\bextrude\(/.test(l));
  const line = lines[k];
  if (!line) return "";
  const m = /\blayer\s*=\s*(?:"([^"]+)"|([A-Za-z_]\w*))/.exec(line);
  if (!m) return "";
  if (m[1]) return m[1];
  return byVar.get(m[2] as string) ?? "";
}

function asNumber(v: ParamValue | undefined, fallback: number): number {
  return typeof v === "number" && Number.isFinite(v) ? v : fallback;
}

export class MockEngine implements EngineLike {
  readonly kind = "mock" as const;
  /** A failure reported against this build is a bug in the UI, not the engine. */
  readonly buildId = "mock (no wasm engine)";
  private meshes: MeshData[] = [];

  evaluate(req: EvaluateRequest, signal?: AbortSignal): Promise<Evaluated> {
    throwIfAborted(signal);
    const t0 = performance.now();
    const model = this.build(req);
    return Promise.resolve({
      model,
      meshes: this.meshes,
      tolerance: req.tolerance ?? DEFAULT_TOLERANCE,
      kind: this.kind,
      elapsedMs: performance.now() - t0,
    });
  }

  dispose(): void {
    this.meshes = [];
  }

  private build(req: EvaluateRequest): Model {
    const t0 = performance.now();
    const main = req.scripts[0] ?? { path: "main.star", source: "" };
    const scraped = scrapeParams(main.path, main.source);
    const specs: ParamSpec[] = DEFAULT_PARAMS.map((d) => scraped.find((s) => s.name === d.name) ?? d);
    // Extra params declared by the script that the mock does not know about
    // are still surfaced so the Parameters panel reflects the script.
    for (const s of scraped) if (!specs.some((p) => p.name === s.name)) specs.push(s);

    const diagnostics: Diagnostic[] = [];
    const params: ParamInfo[] = specs.map((s) => {
      const ov = req.overrides[s.name];
      // An override of the wrong type is ignored, as the real engine ignores
      // it (with a warning there); a choice that is not one of the choices too.
      const overridden = ov !== undefined;
      let value: ParamValue = s.def;
      if (s.kind === "number") {
        const n = overridden ? asNumber(ov, s.def as number) : (s.def as number);
        value = n;
        if (overridden && (n < s.min || n > s.max)) {
          diagnostics.push({
            severity: "warning",
            message: `${s.name} = ${n} is outside its range [${s.min}, ${s.max}]`,
            span: s.span,
            feature_id: "",
          });
        }
      } else if (s.kind === "bool") {
        value = typeof ov === "boolean" ? ov : s.def;
      } else if (typeof ov === "string" && (s.choices.length === 0 || s.choices.includes(ov))) {
        value = ov;
      }
      const info: ParamInfo = {
        name: s.name,
        default_value: s.def,
        value,
        overridden,
        description: s.description,
        unit: s.kind === "number" ? s.unit : "",
        min: s.kind === "number" ? s.min : null,
        max: s.kind === "number" ? s.max : null,
        step: s.kind === "number" ? s.step : null,
        choices: s.choices,
        span: s.span,
      };
      return info;
    });
    const val = (name: string): number => {
      const p = params.find((x) => x.name === name);
      return p ? asNumber(p.value, 0) : 0;
    };
    const flag = (name: string, fallback: boolean): boolean => {
      const p = params.find((x) => x.name === name);
      return p && typeof p.value === "boolean" ? p.value : fallback;
    };
    const width = Math.max(0.1, val("width"));
    const depth = Math.max(0.1, val("depth"));
    const height = Math.max(0.1, val("height"));
    const bossD = Math.max(0.1, val("boss_d"));
    // `with_boss` is the gallery's switch: off leaves the boss out, as the
    // real script's `if with_boss and boss_h > 0` does.
    const bossH = flag("with_boss", true) ? Math.max(0, val("boss_h")) : 0;

    const sketchSpans = findCallSpans(main.path, main.source, "sketch");
    const extrudeSpans = findCallSpans(main.path, main.source, "extrude");

    // `fail("...")` anywhere in the script becomes an error diagnostic, so the
    // diagnostics strip can be exercised by editing the script.
    main.source.split("\n").forEach((line, i) => {
      const m = /^\s*fail\((?:"([^"]*)")?/.exec(line);
      if (m) {
        diagnostics.push({
          severity: "error",
          message: m[1] || "fail() called",
          span: { path: main.path, start_line: i + 1, start_col: 1, end_line: i + 1, end_col: line.length + 1 },
          feature_id: "",
        });
      }
    });

    const features: Feature[] = [
      {
        id: "sketch_1",
        kind: "sketch",
        name: "base_profile",
        inputs: [],
        outputs: [],
        args: { plane: "XY", rect: `${width} × ${depth}`, center: "True" },
        span: sketchSpans[0] ?? null,
        suppressed: false,
        diagnostics: [],
      },
      {
        id: "extrude_1",
        kind: "extrude",
        name: "base",
        inputs: ["sketch_1"],
        outputs: ["body_1"],
        args: { profile: "sketch_1", distance: String(height) },
        span: extrudeSpans[0] ?? null,
        suppressed: false,
        diagnostics: [],
      },
      {
        id: "sketch_2",
        kind: "sketch",
        name: "boss_profile",
        inputs: ["extrude_1"],
        outputs: [],
        args: { plane: "base.face:top", circle: `d=${bossD}` },
        span: sketchSpans[1] ?? null,
        suppressed: false,
        diagnostics: [],
      },
      {
        id: "extrude_2",
        kind: "extrude",
        name: "boss",
        inputs: ["sketch_2"],
        outputs: ["body_2"],
        args: { profile: "sketch_2", distance: String(bossH) },
        span: extrudeSpans[1] ?? null,
        suppressed: bossH <= 0,
        diagnostics: [],
      },
    ];

    if (bossD > Math.min(width, depth)) {
      const d: Diagnostic = {
        severity: "error",
        message: `boss diameter ${bossD} does not fit on the ${width} × ${depth} base`,
        span: extrudeSpans[1] ?? null,
        feature_id: "extrude_2",
      };
      diagnostics.push(d);
      (features[3] as Feature).diagnostics.push(d);
    }

    const base = buildBox(width, depth, height, 0);
    const bodies: Body[] = [];
    this.meshes = [base.mesh];
    const { layers, byVar } = scrapeLayers(main.source);
    bodies.push({
      id: "body_1",
      feature_id: "extrude_1",
      name: "base",
      bbox: base.bbox,
      topology: { faces: base.faces, edges: base.edges, vertices: base.vertices },
      mesh_index: 0,
      visible: true,
      material: null,
      part_body: "",
      transform: null,
      instance: "",
      layer: scrapeBodyLayer(main.source, 0, byVar),
    });
    if (bossH > 0) {
      const boss = buildCylinder(0, 0, bossD / 2, height, bossH, 64);
      this.meshes.push(boss.mesh);
      bodies.push({
        id: "body_2",
        feature_id: "extrude_2",
        name: "boss",
        bbox: boss.bbox,
        topology: { faces: boss.faces, edges: boss.edges, vertices: boss.vertices },
        mesh_index: 1,
        visible: true,
        material: null,
        part_body: "",
        transform: null,
        instance: "",
        layer: scrapeBodyLayer(main.source, 1, byVar),
      });
    }

    const model: Model = { params, features, bodies, diagnostics, eval_millis: performance.now() - t0, plates: scrapePlates(main.source, bodies), layers };
    const units = scrapeUnits(main.source);
    if (units) model.display_units = units;
    return model;
  }

  apiDoc(): Promise<ApiDoc> {
    return Promise.resolve(this.apiDocSync());
  }

  private apiDocSync(): ApiDoc {
    const p = (name: string, type: string, doc: string, required = true, default_value = ""): ApiDoc["functions"][0]["params"][0] => ({
      name,
      type,
      doc,
      required,
      default_value,
    });
    return {
      functions: [
        {
          name: "param",
          doc: "Declare a user-adjustable parameter. Returns its effective value (the script default unless overridden in the document).",
          params: [
            p("name", "str", "Stable parameter name."),
            p("default", "float | bool | str | vec3", "Default value."),
            p("min", "float", "Lower bound (numbers only).", false, "None"),
            p("max", "float", "Upper bound (numbers only).", false, "None"),
            p("step", "float", "Slider step.", false, "None"),
            p("unit", "str", "Display unit, e.g. \"mm\".", false, "\"\""),
            p("description", "str", "Shown in the parameters panel.", false, "\"\""),
          ],
          returns: "float | bool | str | vec3",
          on_type: "",
        },
        {
          name: "sketch",
          doc: "Start a 2D sketch on a plane (\"XY\", \"XZ\", \"YZ\") or on a planar face.",
          params: [p("plane", "str | Face", "Sketch plane.")],
          returns: "Sketch",
          on_type: "",
        },
        {
          name: "extrude",
          doc: "Extrude a closed sketch profile along its plane normal.",
          params: [
            p("profile", "Sketch", "Closed profile to extrude."),
            p("distance", "float", "Extrusion distance in mm."),
            p("name", "str", "Feature name.", false, "\"\""),
          ],
          returns: "Body",
          on_type: "",
        },
        {
          name: "fail",
          doc: "Abort evaluation with an error diagnostic at this location.",
          params: [p("message", "str", "Error message.")],
          returns: "None",
          on_type: "",
        },
      ],
      types: [
        {
          name: "Sketch",
          doc: "A 2D profile under construction.",
          methods: [
            {
              name: "rect",
              doc: "Add an axis-aligned rectangle.",
              params: [
                p("width", "float", "Size along the sketch x axis."),
                p("height", "float", "Size along the sketch y axis."),
                p("center", "bool", "Centre on the origin.", false, "False"),
              ],
              returns: "Sketch",
              on_type: "Sketch",
            },
            {
              name: "circle",
              doc: "Add a circle centred on the origin.",
              params: [p("d", "float", "Diameter.")],
              returns: "Sketch",
              on_type: "Sketch",
            },
          ],
          attrs: [],
        },
        {
          name: "Body",
          doc: "A solid produced by a feature.",
          methods: [
            {
              name: "face",
              doc: "Look up a face by topological name, e.g. \"top\".",
              params: [p("name", "str", "Face name.")],
              returns: "Face",
              on_type: "Body",
            },
          ],
          attrs: [p("name", "str", "Body name.")],
        },
      ],
    };
  }
}
