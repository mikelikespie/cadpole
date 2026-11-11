// The 3D viewport: three.js WebGPURenderer, studio lighting, ground grid,
// body meshes straight from the engine's typed arrays, one batched
// LineSegments2 for all feature edges, hover/selection overlays with soft
// outlines, inline labels, display modes, perspective/ortho, bbox overlay,
// view presets, fit / focus and PNG snapshots. Rendering is on demand: a
// frame is drawn only when the camera, model or an overlay changed -- and at
// once, in the same task, when the drawing buffer changed size, because a
// resized buffer is a cleared one (drawing-buffer.ts).

import {
  Box3,
  BufferAttribute,
  BufferGeometry,
  ClippingGroup,
  Color,
  DataTexture,
  DataUtils,
  DirectionalLight,
  DoubleSide,
  EquirectangularReflectionMapping,
  Float32BufferAttribute,
  Group,
  HalfFloatType,
  LinearFilter,
  LinearSRGBColorSpace,
  Line2NodeMaterial,
  LineBasicMaterial,
  LineSegments,
  Matrix4,
  Mesh,
  MeshBasicMaterial,
  MeshStandardMaterial,
  NeutralToneMapping,
  OrthographicCamera,
  PerspectiveCamera,
  Plane,
  PlaneGeometry,
  Quaternion,
  RGBAFormat,
  SRGBColorSpace,
  Scene,
  Sphere,
  Vector3,
  WebGPURenderer,
  type Camera,
  type Material,
  type Object3D,
} from "three/webgpu";
import { LineSegmentsGeometry } from "three/examples/jsm/lines/LineSegmentsGeometry.js";
import { LineSegments2 } from "three/examples/jsm/lines/webgpu/LineSegments2.js";
import { CameraController, type ExactPose, type WheelMode } from "./controls.js";
import { DrawingBuffer, type BufferSize } from "./drawing-buffer.js";
import type { Evaluated, MeshData } from "./engine.js";
import { count, record, span } from "./perf.js";
import { Picker, type Pickable } from "./picking.js";
import { bodiesForRef, bodyRef, edgeRef, faceRef, featureRef, refKey } from "./selection.js";
import type { Attachment, Body, CameraState, EntityKind, EntityRef, LengthUnit, Model, Plate, PlateBody, Transform } from "./types.js";
import { plateLabel } from "./ui/plates.js";
import { fmtLen } from "./util.js";
import { STUDIO, STUDIO_AZIMUTH, STUDIO_SURROUND_SIZE, studioSurround } from "./studio.js";
import { capTriangles, cutColors, formatSection, joinLoops, lift, NO_SECTION, planesOf, sliceMesh, type CutPlane, type SectionState } from "./section.js";

export type ViewPreset = "home" | "iso" | "top" | "bottom" | "front" | "back" | "left" | "right";
export type ViewMode = "shaded" | "shaded_edges" | "wireframe" | "xray" | "faces";

export const VIEW_MODES: { id: ViewMode; label: string }[] = [
  { id: "shaded", label: "Shaded" },
  { id: "shaded_edges", label: "Shaded + edges" },
  { id: "wireframe", label: "Wireframe" },
  { id: "xray", label: "X-ray" },
  { id: "faces", label: "Faces" },
];

/** Something under the cursor that could be selected. */
export interface Candidate {
  ref: EntityRef;
  kind: EntityKind;
  label: string;
  detail: string;
}

/**
 * A display layer's state as the viewer draws it: the script's
 * `layer(visible=, opacity=)` with the Layers panel's overrides written
 * over them. Keyed by `Body.layer`; "" is the implicit layer.
 */
export interface LayerDisplay {
  visible: boolean;
  opacity: number;
}

export interface FrameStats {
  frameMs: number;
  triangles: number;
  bodies: number;
  backend: "webgpu" | "webgl";
}

export interface ViewerCallbacks {
  onHover(candidate: Candidate | null): void;
  onClick(candidates: Candidate[], additive: boolean, x: number, y: number): void;
  onContext(candidates: Candidate[], x: number, y: number): void;
  /** A context request turned out to be a press held and then dragged: put the menu away. */
  onContextDismiss?(): void;
  onFrame?(stats: FrameStats): void;
}

interface BodyView {
  body: Body;
  mesh: Mesh;
  geometry: BufferGeometry;
  /// The four buffers this view's geometry was built from, kept so the next
  /// evaluation can ask whether the new ones are the same bytes.
  mesh0: MeshData;
  indices: Uint32Array;
  faceIds: Uint32Array;
  faceTris: Uint32Array[] | null;
  hover: Mesh;
  selected: Mesh;
  pickable: Pickable;
  segRange: [number, number];
  colors: BufferAttribute | null;
  /// A material built for this body alone, from `body.material`, or null when
  /// it is on the shared default. Reused while `materialKey` is unchanged.
  owned: MeshStandardMaterial | null;
  /// `body.material` as a string, so "did the script change this body's
  /// appearance" is one comparison rather than five.
  materialKey: string;
  /// The edge segments where the model puts them, kept so the plates view
  /// can move them and the model view can put them back.
  segments0: Float32Array;
  /// In the plates view: the matrix that takes this body's *display*
  /// coordinates (its topology, its segments) to its place on the plate;
  /// null in the model view, or for a body on no plate.
  plateMatrix: Matrix4 | null;
  /// In the plates view: the body's filament colour, shared per slot colour.
  plateMaterial: MeshStandardMaterial | null;
}

interface Label {
  el: HTMLElement;
  anchor: Vector3;
  text: string;
}

const COLORS = {
  body: 0xb6b9bf,
  edge: 0x141519,
  hover: 0xa8d5ff,
  selected: 0x3b8cff,
  gridMinor: 0x35373d,
  gridMajor: 0x45484f,
  axisX: 0xa64545,
  axisY: 0x3f8a4a,
  bbox: 0x9aa0ad,
  /// A print plate's outline and its faint fill.
  bed: 0xdfe4ee,
  /// A body that was not in the previous model at all.
  flashNew: 0x46c26a,
  /// A body that was there but whose geometry moved.
  flashChanged: 0x4fb3d9,
};

/// The time constant of the picture's move when the covered edges change, in
/// milliseconds: it arrives in about the length of the sheet's own slide.
const SHIFT_TAU_MS = 60;
/// The least width or height, in CSS pixels, worth centring the model in.
const MIN_CLEAR_PX = 120;

/** How much of each edge of the viewport is covered, in CSS pixels. */
export interface ViewportInsets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

/// How long a change flash takes to fade out, in milliseconds.
///
/// Long enough to catch the eye if you were looking elsewhere when the model
/// re-evaluated, short enough not to sit on top of the thing you are then
/// trying to look at.
const FLASH_MS = 1400;

/// What a body looked like last time, cheaply enough to compare every
/// evaluation.
///
/// Keyed by the body's *name* rather than its id: an id carries the feature
/// index (`baselid#43`), which moves whenever a feature is inserted earlier in
/// the script, so keying on it would flash the whole model on any edit. The
/// name is what the script called it.
interface BodySignature {
  triangles: number;
  bbox: string;
}

/// The stable half of a body id: `baselid#43` is `baselid`.
function bodyKey(body: { id: string; name?: string }): string {
  if (body.name) return body.name;
  const hash = body.id.lastIndexOf("#");
  return hash > 0 ? body.id.slice(0, hash) : body.id;
}

/**
 * Whether two typed arrays hold the same bytes.
 *
 * Compared as 32-bit words, which is what every buffer here is made of, and as
 * *bit patterns* rather than as numbers: two float arrays differing only in
 * `-0` versus `0`, or in which NaN they carry, are reported as different. That
 * is the conservative direction -- it costs a rebuild, never a wrong picture.
 */
function sameBytes(a: { buffer: ArrayBufferLike; byteOffset: number; byteLength: number }, b: typeof a): boolean {
  if (a.byteLength !== b.byteLength) return false;
  if (a === b) return true;
  // A word at a time where the buffers allow it, which is every buffer either
  // engine produces (`.slice()` in the worker and `new Float32Array(...)` in
  // the wire decoder both start at offset zero). A view that did not start on
  // a word boundary cannot be read as `Uint32Array` at all, so that case
  // compares bytes rather than refusing to look.
  if (((a.byteOffset | b.byteOffset | a.byteLength) & 3) === 0) {
    const n = a.byteLength >>> 2;
    const wa = new Uint32Array(a.buffer, a.byteOffset, n);
    const wb = new Uint32Array(b.buffer, b.byteOffset, n);
    for (let i = 0; i < n; i++) if (wa[i] !== wb[i]) return false;
    return true;
  }
  const ba = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
  const bb = new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
  for (let i = 0; i < ba.length; i++) if (ba[i] !== bb[i]) return false;
  return true;
}

/**
 * Whether the viewer would build exactly the same geometry from `b` as it did
 * from `a`.
 *
 * This is the whole basis of reuse, so it is a byte comparison and not a
 * summary: a digest would need an argument about collisions, and a signature
 * of counts and a rounded bounding box (which is what the change flash uses,
 * and all it needs) cannot tell a moved hole from an unmoved one. Comparing
 * the bytes needs no argument at all -- if every word matches, the buffers the
 * GPU already holds *are* the answer.
 *
 * It is also cheap in the direction that matters: a body that changed usually
 * differs early and the loop stops there, and a body that did not costs one
 * linear pass over about 300 KB, against the rebuild and re-upload it saves.
 */
function sameMesh(a: MeshData, b: MeshData): boolean {
  // Lengths first, all of them: a body that gained or lost a triangle
  // answers in nanoseconds instead of after a full pass over its positions.
  const ca = a.colors ?? NO_COLORS;
  const cb = b.colors ?? NO_COLORS;
  return (
    a.indices.length === b.indices.length &&
    a.positions.length === b.positions.length &&
    a.normals.length === b.normals.length &&
    a.faceIds.length === b.faceIds.length &&
    ca.length === cb.length &&
    sameBytes(a.indices, b.indices) &&
    sameBytes(a.positions, b.positions) &&
    sameBytes(a.normals, b.normals) &&
    sameBytes(a.faceIds, b.faceIds) &&
    sameBytes(ca, cb)
  );
}

const NO_COLORS = new Uint8Array(0);

/** sRGB 8-bit -> display-linear, the 256 values once. */
const SRGB_TO_LINEAR = (() => {
  const t = new Float32Array(256);
  for (let i = 0; i < 256; i++) {
    const s = i / 255;
    t[i] = s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  }
  return t;
})();

/**
 * A scan's per-vertex colour as the `color` attribute three.js
 * multiplies a `vertexColors` material's colour by: display-linear floats,
 * three a vertex. Null when the mesh has none, or colours that do not match
 * its vertices -- which the engine never sends, and which would otherwise
 * read past the buffer.
 */
function scanColorAttribute(mesh: MeshData): BufferAttribute | null {
  const c = mesh.colors;
  if (!c || c.length === 0 || c.length !== mesh.positions.length) return null;
  const out = new Float32Array(c.length);
  for (let i = 0; i < c.length; i++) out[i] = SRGB_TO_LINEAR[c[i] as number] as number;
  return new BufferAttribute(out, 3);
}

/**
 * Take the view that can be reused for `key`, if one of them holds this mesh.
 *
 * The key is only where to look: reuse is decided by the bytes, so a key that
 * matched the wrong view still cannot serve the wrong geometry -- the geometry
 * it would serve is bit-identical to the one that would have been built. The
 * key exists to keep the comparison to one candidate rather than all of them,
 * and it is the body's *name* for the same reason the change flash uses the
 * name: an id carries the feature index, which moves whenever a line is added
 * earlier in the script.
 */
function takeReusable(spare: Map<string, BodyView[]>, key: string, mesh0: MeshData): BodyView | null {
  const list = spare.get(key);
  if (!list || list.length === 0) return null;
  // The one holding these exact triangles if there is one, and otherwise the
  // first: even when the triangles changed, the scene node, its two overlays
  // and its place in the renderer's caches are worth keeping, and only the
  // buffers behind them have to be replaced.
  let at = list.findIndex((v) => sameMesh(v.mesh0, mesh0));
  if (at < 0) at = 0;
  const v = list[at] as BodyView;
  list.splice(at, 1);
  if (list.length === 0) spare.delete(key);
  return v;
}

/// `body.material` as one comparable string; "" when the body has none.
function materialKeyOf(body: Body, vertexColors = false): string {
  const m = body.material;
  const vc = vertexColors ? "|vc" : "";
  if (!m) return vc;
  const c = m.color;
  const key = `${c ? `${c[0]},${c[1]},${c[2]}` : ""}|${m.roughness ?? ""}|${m.metallic ?? ""}|${m.opacity ?? ""}`;
  // `materialFor` returns null when a material message says nothing that would
  // change the default, and the key has to agree with it or a body would be
  // handed a material it does not have. A body with its own vertex colours
  // always has a material of its own: the shared default does not read them.
  return key === "|||" ? vc : key + vc;
}

/// Let go of the material a view built for itself, if it has one.
function disposeOwned(v: BodyView | null): void {
  v?.owned?.dispose();
}

/**
 * Put a scene node where the model says its body is drawn.
 *
 * An instance body's triangles are its part body's, uploaded once; what
 * makes it that instance is this matrix -- the pose plus the exploded-view
 * offset -- and a re-pose is sixteen numbers, not a mesh. An ordinary body's
 * triangles are already where they belong, so it sits at the identity; a
 * node reused from a body that had a transform is put back there too.
 */
function applyTransform(o: Object3D, t: Transform | null): void {
  if (t) {
    o.position.set(t.translation.x, t.translation.y, t.translation.z);
    o.quaternion.set(t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w);
  } else {
    o.position.set(0, 0, 0);
    o.quaternion.identity();
  }
  o.updateMatrix();
}

/** A `cadpole.v1.Transform` as a matrix; the identity for none. */
function matrixOf(t: Transform | null, out: Matrix4): Matrix4 {
  if (!t) return out.identity();
  return out.compose(
    new Vector3(t.translation.x, t.translation.y, t.translation.z),
    new Quaternion(t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w),
    new Vector3(1, 1, 1),
  );
}

/** Edge segments (six floats each) moved by `m`. */
function movedSegments(src: Float32Array, m: Matrix4, tmp: Vector3): Float32Array {
  const out = new Float32Array(src.length);
  for (let i = 0; i + 2 < src.length; i += 3) {
    tmp.set(src[i] as number, src[i + 1] as number, src[i + 2] as number).applyMatrix4(m);
    out[i] = tmp.x;
    out[i + 1] = tmp.y;
    out[i + 2] = tmp.z;
  }
  return out;
}

export const VIEW_DIRS: Record<Exclude<ViewPreset, "home">, [number, number, number]> = {
  iso: [1, -0.95, 0.8],
  top: [0, -0.0004, 1],
  bottom: [0, -0.0004, -1],
  front: [0, -1, 0],
  back: [0, 1, 0],
  left: [-1, 0, 0],
  right: [1, 0, 0],
};

const TIGHT_EDGE_PX = 6;

/**
 * The surround as an environment map (`studioSurround`); the renderer
 * prefilters it once for rough and smooth surfaces alike. Rows run from
 * straight down to straight up along the map's y, which
 * `environmentRotation` stands on the model's z.
 */
function studioEnvironment(): DataTexture {
  const { width: W, height: H } = STUDIO_SURROUND_SIZE;
  const data = studioSurround((v) => DataUtils.toHalfFloat(v));
  const tex = new DataTexture(data, W, H, RGBAFormat, HalfFloatType);
  tex.mapping = EquirectangularReflectionMapping;
  tex.colorSpace = LinearSRGBColorSpace;
  tex.minFilter = LinearFilter;
  tex.magFilter = LinearFilter;
  tex.needsUpdate = true;
  return tex;
}

/**
 * A cut as a three.js clipping plane: three keeps the side where
 * `normal . p + constant >= 0`, so the normal points into the kept side.
 */
function planeOf(p: CutPlane): Plane {
  const n = new Vector3(p.axis === 0 ? 1 : 0, p.axis === 1 ? 1 : 0, p.axis === 2 ? 1 : 0);
  // `z=40` removes the positive side: keep -z + 40 >= 0.
  return p.removePositive ? new Plane(n.negate(), p.at) : new Plane(n, -p.at);
}

/** Grid spacings for a scene drawn in feet and inches: 1, 2, 5, 10, 20, 50, 100 ft. */
const FT = 304.8;
const IMPERIAL_STEPS = [1, 2, 5, 10, 20, 50, 100, 200].map((f) => f * FT);
/** And in millimetres, past the three the small scenes have always had. */
const METRIC_STEPS = [100, 250, 500, 1000, 2500, 5000, 10000, 25000, 50000];
/** About this many minor lines from the centre to an edge, at most. */
const GRID_LINES = 40;

/**
 * The ground grid's minor spacing and major multiple for a half-width,
 * mm. The small scenes keep the spacing they always had (10, 50, 100 mm:
 * every screenshot test is one); a large scene -- a site, a house -- gets a
 * spacing that leaves the grid a grid rather than a moiré, in feet when
 * the document reads in feet.
 */
export function gridSpacing(half: number, unit: LengthUnit): { minor: number; major: number } {
  const imperial = unit === "ft" || unit === "ft-in" || unit === "in";
  if (imperial && half > 2000) {
    const minor = IMPERIAL_STEPS.find((s) => half / s <= GRID_LINES) ?? (IMPERIAL_STEPS[IMPERIAL_STEPS.length - 1] as number);
    return { minor, major: minor * 5 };
  }
  if (half <= 400) return { minor: 10, major: 50 };
  if (half <= 1000) return { minor: 50, major: 250 };
  if (half / 100 <= GRID_LINES * 2) return { minor: 100, major: 500 };
  const minor = METRIC_STEPS.find((s) => half / s <= GRID_LINES) ?? (METRIC_STEPS[METRIC_STEPS.length - 1] as number);
  return { minor, major: minor * (String(minor).startsWith("25") ? 4 : 5) };
}

function hashHue(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return ((h >>> 0) % 360) / 360;
}

export class Viewer {
  readonly canvas: HTMLCanvasElement;
  readonly controls: CameraController;
  private unit: LengthUnit = "mm";

  /// How lengths are written, and what the grid is spaced in: a house in
  /// feet gets a grid of feet, a part in millimetres the usual one.
  get displayUnit(): LengthUnit {
    return this.unit;
  }

  set displayUnit(u: LengthUnit) {
    if (u === this.unit) return;
    this.unit = u;
    this.buildGrid(this.gridHalf);
    this.rebuildBBox();
    this.requestRender();
  }
  private renderer: WebGPURenderer;
  private readonly scene = new Scene();
  private readonly camera: PerspectiveCamera;
  private readonly ortho: OrthographicCamera;
  private orthographic = false;
  private readonly picker = new Picker();
  private readonly views: BodyView[] = [];
  private readonly pickables: Pickable[] = [];
  private readonly bodyMaterial: MeshStandardMaterial;
  private readonly xrayMaterial: MeshStandardMaterial;
  private readonly wireMaterial: MeshBasicMaterial;
  private readonly facesMaterial: MeshStandardMaterial;
  private readonly hoverMaterial: MeshBasicMaterial;
  private readonly selectedMaterial: MeshBasicMaterial;
  private readonly edgeMaterial: Line2NodeMaterial;
  private readonly softEdgeMaterial: Line2NodeMaterial;
  private readonly hoverEdgeMaterial: Line2NodeMaterial;
  private readonly selectedEdgeMaterial: Line2NodeMaterial;
  private edges: LineSegments2 | null = null;
  private hoverEdges: LineSegments2 | null = null;
  private softEdges: LineSegments2 | null = null;
  private selectedEdges: LineSegments2 | null = null;
  private grid: LineSegments | null = null;
  private gridHalf = 0;
  private bboxLines: LineSegments | null = null;
  private bboxVisible = false;
  private dimLabels: Label[] = [];
  private ready = false;
  private disposed = false;
  private needsRender = false;
  /// Set by `setModel` when geometry changed, so the next frame -- the one
  /// that has to create and fill the GPU buffers -- is measured separately.
  private freshGeometry = false;
  private rafId = 0;
  /// Body signatures from the previous `setModel`, for change detection.
  private lastSignatures = new Map<string, BodySignature>();
  /// One material per flash colour, shared by every body flashing in it.
  ///
  /// Per body, which is what this was, meant forty new `MeshStandardMaterial`
  /// instances for a forty-body edit, and a new material is a new render
  /// pipeline for the renderer to build: measured at 3.6 ms each, 145 ms of
  /// the 159 ms frame that followed. Sharing works because every body
  /// in one flash starts at the same instant and fades identically -- a body
  /// with a material of its own still flashes on that, so its colour survives.
  private readonly flashMaterials = new Map<number, MeshStandardMaterial>();
  /// Bodies currently flashing: mesh -> when the flash started and its colour.
  private flashes = new Map<Mesh, { start: number; color: number; material: MeshStandardMaterial }>();
  private hidden: ReadonlySet<string> = new Set();
  /// Per-layer display state, from `setLayers`. A body whose layer
  /// is not in the map is drawn as the model says.
  private layers: ReadonlyMap<string, LayerDisplay> = new Map();
  /// The translucent materials a ghosted layer's bodies wear, one per
  /// (body material, layer opacity): the same cache-by-appearance idea as
  /// `owned`, extended to the layer, so a slider step is a lookup and not
  /// a new render pipeline per body.
  private readonly ghostMaterials = new Map<string, MeshStandardMaterial>();
  private model: Model | null = null;
  /// The plates view: bodies where the export puts them, on their
  /// beds, in their filaments' colours. Everything it needs is in the
  /// model's `plates`; it adds no geometry, only transforms and materials.
  private platesOn = false;
  /// The beds: an outline and a faint fill per plate.
  private beds: Object3D[] = [];
  private plateLabels: Label[] = [];
  /// One material per filament colour, shared by every body printing in it.
  private readonly plateMaterials = new Map<string, MeshStandardMaterial>();
  /// A body exported to two plates is drawn on the second as a ghost mesh
  /// sharing its geometry: shown, not pickable, in its filament's colour.
  private ghosts: { mesh: Mesh; material: MeshStandardMaterial }[] = [];
  /// The model view's camera, put back bit for bit when the plates view is
  /// left: the orbit, and the near and far planes, which follow the orbit
  /// distance with a 20 % hysteresis and would otherwise settle a hair
  /// away from where they were -- a different depth range is a different
  /// picture along every edge.
  private savedCamera: { pose: ExactPose; near: number; far: number } | null = null;
  private triangles = 0;
  private sceneRadius = 60;
  private readonly sceneCenter = new Vector3(0, 0, 10);
  private readonly sceneBox = new Box3();
  private mode: ViewMode = "shaded_edges";
  private gridVisible = true;
  private readonly labelsEl: HTMLElement;
  private labels: Label[] = [];
  private readonly tmpV = new Vector3();
  private readonly tmpV2 = new Vector3();
  private readonly tmpQ = new Quaternion();
  private readonly tmpBox = new Box3();
  private readonly tmpColor = new Color();
  private readonly tmpSphere = new Sphere();
  private pendingHover: { x: number; y: number } | null = null;
  private hoverScheduled = false;
  private lastHoverKey: string | null = null;
  private triad: {
    svg: SVGSVGElement;
    axes: { line: SVGLineElement | null; dot: SVGCircleElement; label: SVGTextElement | null; dir: Vector3 }[];
  } | null = null;
  private readonly resizeObserver: ResizeObserver;
  /// Everything the model draws -- bodies, their overlays, the edge
  /// batches, the plates view's ghosts -- under one clipping group, so a
  /// section is the group's planes and nothing per material.
  private readonly content = new ClippingGroup();
  /// The section's caps and cut outlines, one group per plane: a cap lies
  /// in its own plane, so it is clipped by the *other* plane only.
  private readonly capGroups: [ClippingGroup, ClippingGroup] = [new ClippingGroup(), new ClippingGroup()];
  private section: SectionState = NO_SECTION;
  /// The layers the camera frames (`setFrameLayers`); empty frames all.
  private frameLayers: ReadonlySet<string> = new Set();
  /// What Home, Iso and Fit fit: `sceneBox`, or the framing layers' part of it.
  private readonly frameBox = new Box3();
  /// Where Home looks from, when a published page says; iso otherwise.
  private homeDir: Vector3 | null = null;
  /// A cut face: each body's own colour (a vertex colour; `cutColors`),
  /// unlit, so it is one flat colour beside the lit surface it cuts. The
  /// frame's exposure and tone curve apply to it as to everything drawn,
  /// which lifts it a little further. It tests depth and does not write it:
  /// where two bodies overlap their cut faces are the same plane, and
  /// depths that differ only by rounding would speckle one through the
  /// other. Drawn in order, the later body's face covers the earlier's.
  private readonly capMaterial = new MeshBasicMaterial({
    vertexColors: true,
    side: DoubleSide,
    depthWrite: false,
    polygonOffset: true,
    polygonOffsetFactor: 1,
    polygonOffsetUnits: 1,
  });
  /// The depth the cut faces did not write, laid down once the plane's
  /// faces are drawn: what the other plane's faces, the outlines and
  /// anything translucent are hidden by.
  private readonly capDepthMaterial = new MeshBasicMaterial({
    colorWrite: false,
    side: DoubleSide,
    polygonOffset: true,
    polygonOffsetFactor: 1,
    polygonOffsetUnits: 1,
  });
  /// A cut face's outline: a hairline in a shade of the face's colour.
  private readonly cutLineMaterial = new Line2NodeMaterial({ vertexColors: true, linewidth: 1, worldUnits: false });
  /// What each plane's cap draws, and which body each cap triangle belongs
  /// to, so a click on a cut face selects the body it cuts.
  private caps: { mesh: Mesh | null; depth: Mesh | null; lines: LineSegments2 | null; bodies: Body[] }[] = [
    { mesh: null, depth: null, lines: null, bodies: [] },
    { mesh: null, depth: null, lines: null, bodies: [] },
  ];
  /// The key, fill and rim, turned about z to follow the camera.
  private readonly rig = new Group();
  /// How much of each edge of the viewport something else covers
  /// (`setViewportInsets`), in CSS pixels.
  private insets: ViewportInsets = { top: 0, right: 0, bottom: 0, left: 0 };
  /// Where the picture's centre is drawn, in CSS pixels from the middle of
  /// the canvas (x right, y down): the middle of what the insets leave. It
  /// closes on `to` a fixed fraction at a time, so a target that is itself
  /// moving -- a sheet under a finger -- is followed without starting over.
  private readonly shift = { x: 0, y: 0, toX: 0, toY: 0, last: 0, moving: false };
  private readonly reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  /// How faint the ground grid is drawn: 1 as usual, less for `--grid dim`.
  private gridStrength = 1;

  constructor(
    private readonly container: HTMLElement,
    private readonly callbacks: ViewerCallbacks,
  ) {
    this.canvas = document.createElement("canvas");
    this.canvas.className = "viewport-canvas";
    this.canvas.setAttribute("aria-label", "3D model viewport");
    this.canvas.setAttribute("role", "img");
    this.canvas.tabIndex = -1;
    container.appendChild(this.canvas);
    this.labelsEl = document.createElement("div");
    this.labelsEl.className = "vlabels";
    this.labelsEl.setAttribute("aria-hidden", "true");
    container.appendChild(this.labelsEl);

    this.renderer = this.makeRenderer();
    this.camera = new PerspectiveCamera(40, 1, 0.1, 5000);
    this.camera.up.set(0, 0, 1);
    this.ortho = new OrthographicCamera(-1, 1, 1, -1, 0.1, 5000);
    this.ortho.up.set(0, 0, 1);

    this.controls = new CameraController(this.camera, this.canvas, {
      onChange: () => this.requestRender(),
      onClick: (x, y, _button, additive) => this.callbacks.onClick(this.candidatesAt(x, y), additive, x, y),
      onDoubleClick: (x, y) => {
        // On the model: orbit about that point. Off it: bring it all back.
        if (!this.focusAt(x, y)) this.fit();
      },
      onContext: (x, y) => this.callbacks.onContext(this.candidatesAt(x, y), x, y),
      onContextDismiss: () => this.callbacks.onContextDismiss?.(),
    });

    // Faces stand a constant two depth units behind the edges drawn on them
    // and no more. A slope-scaled offset would hide an edge's whole width on
    // a steep face, but it also moves two faces a hair apart by different
    // amounts wherever they are meshed differently -- artwork 0.04 mm proud
    // of a mug's wall came out striped with the wall from across a table.
    // The edges are pulled forward by their own slope instead (below).
    const offset = { polygonOffset: true, polygonOffsetFactor: 0, polygonOffsetUnits: 2 };
    this.bodyMaterial = new MeshStandardMaterial({ color: COLORS.body, roughness: 0.58, metalness: 0.06, ...offset });
    this.xrayMaterial = new MeshStandardMaterial({
      color: 0xc7d3e6,
      roughness: 0.5,
      metalness: 0.0,
      transparent: true,
      opacity: 0.3,
      depthWrite: false,
      ...offset,
    });
    this.wireMaterial = new MeshBasicMaterial({ color: 0x9a9da5, wireframe: true, transparent: true, opacity: 0.6 });
    this.facesMaterial = new MeshStandardMaterial({ vertexColors: true, roughness: 0.7, metalness: 0.02, ...offset });
    this.hoverMaterial = new MeshBasicMaterial({
      color: COLORS.hover,
      transparent: true,
      opacity: 0.28,
      depthWrite: false,
      polygonOffset: true,
      polygonOffsetFactor: -1,
      polygonOffsetUnits: -2,
    });
    this.selectedMaterial = new MeshBasicMaterial({
      color: COLORS.selected,
      transparent: true,
      opacity: 0.5,
      depthWrite: false,
      polygonOffset: true,
      polygonOffsetFactor: -1,
      polygonOffsetUnits: -3,
    });
    // A hairline on a phone: its pixels are a third the size, and a model
    // with a few thousand edges reads as a drawing at 1 px and as a scribble
    // at more.
    const fine = window.matchMedia("(pointer: coarse)").matches;
    this.edgeMaterial = new Line2NodeMaterial({ color: COLORS.edge, linewidth: fine ? 1 : 1.25, worldUnits: false });
    Object.assign(this.edgeMaterial, { polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -2 });
    this.softEdgeMaterial = new Line2NodeMaterial({ color: COLORS.hover, linewidth: 4, worldUnits: false, transparent: true, opacity: 0.45 });
    this.hoverEdgeMaterial = new Line2NodeMaterial({ color: COLORS.hover, linewidth: 3.5, worldUnits: false });
    this.selectedEdgeMaterial = new Line2NodeMaterial({ color: COLORS.selected, linewidth: 3, worldUnits: false });
    for (const m of [this.softEdgeMaterial, this.hoverEdgeMaterial, this.selectedEdgeMaterial]) {
      m.depthTest = false;
      m.depthWrite = false;
    }

    // The studio `cadpole render` lights a model with (`Lights::studio`): a
    // surround for the ambient and for a metal to reflect, and a warm key, a
    // cool fill and a rim. The rig turns with the camera, so the side being
    // looked at is the lit one from every angle.
    this.scene.environment = studioEnvironment();
    this.scene.environmentRotation.set(-Math.PI / 2, 0, 0);
    for (const l of STUDIO.lights) {
      const light = new DirectionalLight(new Color().setRGB(l.color[0], l.color[1], l.color[2]), l.intensity);
      light.position.set(l.dir[0], l.dir[1], l.dir[2]);
      this.rig.add(light);
    }
    this.scene.add(this.rig);

    // No planes until a section is set: an empty clipping group clips nothing.
    this.content.enabled = false;
    this.scene.add(this.content);
    for (const g of this.capGroups) {
      g.enabled = false;
      this.scene.add(g);
    }

    this.buildGrid(150);
    this.buildTriad();

    this.canvas.addEventListener("pointermove", this.onPointerMove);
    this.canvas.addEventListener("pointerleave", () => this.emitHover(null));

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(container);
  }

  /** Must be awaited before the first render. */
  async init(): Promise<void> {
    await this.startRenderer();
    this.controls.onGesture = (active) => this.setGesture(active);
    this.canvas.addEventListener("webglcontextrestored", this.onContextRestored);
    this.watchPixelRatio();
  }

  private makeRenderer(): WebGPURenderer {
    const renderer = new WebGPURenderer({ canvas: this.canvas, antialias: true, alpha: true });
    renderer.onDeviceLost = (info) => {
      // An old renderer's device going away after it was replaced is not news.
      if (renderer === this.renderer) this.deviceLost(info.api === "WebGL" ? "webgl" : "webgpu", info.message);
    };
    return renderer;
  }

  /** Bring the current renderer up and draw the first frame with it. */
  private async startRenderer(): Promise<void> {
    const renderer = this.renderer;
    await renderer.init();
    if (renderer !== this.renderer || this.disposed) return;
    renderer.toneMapping = NeutralToneMapping;
    renderer.toneMappingExposure = STUDIO.exposure;
    renderer.setClearColor(new Color(0x000000), 0);
    this.buffer.reset();
    this.ready = true;
    this.resize();
    this.requestRender();
  }

  get backend(): "webgpu" | "webgl" {
    const b = this.renderer.backend as { isWebGPUBackend?: boolean };
    return b.isWebGPUBackend ? "webgpu" : "webgl";
  }

  get triangleCount(): number {
    return this.triangles;
  }

  get wheelMode(): WheelMode {
    return this.controls.wheelMode;
  }

  set wheelMode(m: WheelMode) {
    this.controls.wheelMode = m;
  }

  private activeCamera(): Camera {
    return this.orthographic ? this.ortho : this.camera;
  }

  // ---------------------------------------------------------------- model

  /**
   * Show one evaluation.
   *
   * `flash` says whether this result is the model's new final word. A
   * progressive refinement calls here twice for one evaluation --
   * coarse, then full -- and only the second is a change worth marking: the
   * flash compares triangle counts, and a refinement changes every count
   * without changing the model at all. The signatures it compares against are
   * only recorded for a final result too, so a coarse pass cannot poison the
   * comparison the next real edit makes.
   *
   * Nothing here touches the camera, the selection or the hover; they live
   * outside the meshes. The caller re-applies the highlight, which is what
   * makes a refinement invisible to a selection: a selected face is a
   * *topological name*, which is generated by the feature that made the
   * geometry and does not depend on how finely it was tessellated.
   *
   * **A body whose triangles did not change keeps everything**: its
   * `BufferGeometry`, the GPU buffers behind it, its two overlay meshes and
   * its place in the scene. What decides that is a byte comparison of the four
   * buffers the engine just sent against the four this view was built from --
   * not a digest and not the coarse signature the change flash uses, because
   * the cost of being wrong here is a stale mesh on screen. Everything that
   * hangs off the *model* rather than off the triangles -- the body's id, its
   * topology, its material, its pick segments -- is rebuilt either way, and
   * costs microseconds.
   */
  setModel(evaluated: Evaluated, opts: { flash?: boolean } = {}): void {
    const model = evaluated.model;
    const flashing = opts.flash !== false;
    const previous = this.lastSignatures;
    const signatures = new Map<string, BodySignature>();
    // Nothing flashes on the very first model: everything would be new, and a
    // whole scene lighting up says nothing about what just changed.
    const first = previous.size === 0;
    // What is on screen now, indexed by the key the new model will look it up
    // under, and consumed as it is matched -- so one view is reused at most
    // once even if two bodies share a name.
    const spare = new Map<string, BodyView[]>();
    for (const v of this.views) {
      const k = bodyKey(v.body);
      const list = spare.get(k);
      if (list) list.push(v);
      else spare.set(k, [v]);
    }
    this.endFlashes();
    this.views.length = 0;
    this.pickables.length = 0;
    this.lastHoverKey = null;
    this.model = model;
    // Instances of one part share one `MeshData`, and here one
    // `BufferGeometry`: the first body to need a geometry for those bytes
    // builds (or keeps) it and every later one points at it. A geometry a
    // body lets go of is retired rather than disposed on the spot, because
    // another body in this pass may still be drawing it; what is left over
    // at the end, and not in use, is disposed then.
    const built = new Map<MeshData, BufferGeometry>();
    const retired: BufferGeometry[] = [];
    let tris = 0;
    let msGeometry = 0;
    let msBounds = 0;
    let msCompare = 0;
    let msOverlays = 0;
    let msSegments = 0;
    for (const body of model.bodies) {
      const mesh0 = evaluated.meshes[body.mesh_index];
      if (!mesh0) {
        console.warn("cadpole: no mesh for", body.id, "at index", body.mesh_index);
        continue;
      }
      const { positions, normals, indices } = mesh0;
      const key = bodyKey(body);
      const tCompare = performance.now();
      const prior = takeReusable(spare, key, mesh0);
      const keepGeometry = prior !== null && sameMesh(prior.mesh0, mesh0);
      msCompare += performance.now() - tCompare;

      let geometry: BufferGeometry;
      const shared = built.get(mesh0);
      if (shared) {
        count("viewer.bodyShared");
        geometry = shared;
        if (prior && prior.geometry !== shared) retired.push(prior.geometry);
      } else if (keepGeometry && prior) {
        count("viewer.bodyReused");
        geometry = prior.geometry;
      } else {
        count("viewer.bodyRebuilt");
        const tGeom = performance.now();
        geometry = new BufferGeometry();
        geometry.setAttribute("position", new BufferAttribute(positions, 3));
        if (normals.length === positions.length) geometry.setAttribute("normal", new BufferAttribute(normals, 3));
        else geometry.computeVertexNormals();
        geometry.setIndex(new BufferAttribute(indices, 1));
        // A scan's own colour, kept with the geometry so the faces
        // mode, which paints the same attribute, can hand it back.
        const scan = scanColorAttribute(mesh0);
        if (scan) {
          count("viewer.scanColors");
          geometry.setAttribute("color", scan);
          geometry.userData["scanColors"] = scan;
        }
        const tBounds = performance.now();
        msGeometry += tBounds - tGeom;
        // One pass over the positions, not three: `computeBoundingSphere`
        // walks them twice and then `computeBoundingBox` walks them again.
        // A sphere around the box is bigger than the tightest one, and the
        // only thing either is used for is frustum culling, where bigger is
        // the safe direction.
        geometry.computeBoundingBox();
        geometry.boundingSphere = (geometry.boundingBox ?? new Box3()).getBoundingSphere(new Sphere());
        msBounds += performance.now() - tBounds;
        if (prior) retired.push(prior.geometry);
      }
      built.set(mesh0, geometry);
      // The overlays index this geometry's vertices: they follow it when it
      // is not the one they were built on, whether rebuilt or now shared.
      const sameGeometry = prior !== null && prior.geometry === geometry;

      // The scene node and its two overlays survive even when the triangles
      // did not: they carry this body's place in the scene and in the
      // renderer's per-object caches, and only the buffers behind them have
      // to be replaced.
      const tOverlay = performance.now();
      const mesh = prior ? prior.mesh : new Mesh(geometry, this.bodyMaterial);
      let hover: Mesh;
      let selected: Mesh;
      if (prior) {
        mesh.geometry = geometry;
        hover = prior.hover;
        selected = prior.selected;
        if (!sameGeometry) {
          hover.geometry.dispose();
          selected.geometry.dispose();
          hover.geometry = this.overlayGeometry(geometry);
          selected.geometry = this.overlayGeometry(geometry);
          // Their index addressed the old triangles. The caller re-applies the
          // highlight in this same task, so nothing is drawn in between, but a
          // scene that is only coherent because of when it is looked at is not
          // coherent.
          hover.visible = false;
          selected.visible = false;
        }
      } else {
        hover = this.makeOverlay(geometry, this.hoverMaterial, 10);
        selected = this.makeOverlay(geometry, this.selectedMaterial, 11);
      }
      msOverlays += performance.now() - tOverlay;
      const reused = prior;

      const box = geometry.boundingBox;
      // An instance's geometry is its part's, so where it *is* comes from
      // its transform; a re-pose is a change worth a flash.
      const t = body.transform;
      const where = t
        ? `|${[t.translation.x, t.translation.y, t.translation.z, t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w].map((v) => v.toFixed(3)).join(",")}`
        : "";
      const signature: BodySignature = {
        triangles: indices.length / 3,
        // Rounded to a micron: a bounding box read off f32 mesh positions
        // wobbles in its last bits between runs, and a flash that fires on
        // that is worse than no flash at all.
        bbox: (box ? [box.min.x, box.min.y, box.min.z, box.max.x, box.max.y, box.max.z].map((v) => v.toFixed(3)).join(",") : "") + where,
      };
      signatures.set(key, signature);
      const was = previous.get(key);
      const flash =
        !flashing || first
          ? null
          : !was
            ? COLORS.flashNew
            : was.triangles !== signature.triangles || was.bbox !== signature.bbox
              ? COLORS.flashChanged
              : null;

      // A body's own appearance, if the script gave it one, and otherwise the
      // shared default. The instance survives an evaluation that did not
      // change it: a fresh `MeshStandardMaterial` is a fresh render pipeline
      // for the renderer to build, which is milliseconds, not microseconds.
      const materialKey = materialKeyOf(body, geometry.userData["scanColors"] !== undefined);
      let own: MeshStandardMaterial | null;
      if (reused && reused.materialKey === materialKey) {
        own = reused.owned;
      } else {
        disposeOwned(reused);
        own = materialKey === "" ? null : this.materialFor(body, geometry.userData["scanColors"] !== undefined);
      }
      const material = own ?? (flash === null ? this.bodyMaterial : this.flashMaterial(flash));
      mesh.material = material;
      mesh.name = body.id;
      mesh.visible = this.bodyShown(body) && body.visible;
      applyTransform(mesh, body.transform);
      applyTransform(hover, body.transform);
      applyTransform(selected, body.transform);
      // After the opaque ones, or a translucent body blends against whatever
      // happened to be drawn first.
      mesh.renderOrder = own?.transparent ? 5 : 0;
      // Re-adding moves an object to the end of the scene's children, so the
      // scene ends up in model order whatever was reused: the picture is a
      // function of the model and not of the sequence of edits that reached it.
      this.content.add(mesh, hover, selected);
      if (flash !== null) {
        count("viewer.bodyFlashed");
        this.flashes.set(mesh, { start: performance.now(), color: flash, material });
      }
      tris += indices.length / 3;

      const tSegs = performance.now();
      let segCount = 0;
      for (const e of body.topology.edges) segCount += Math.max(0, e.polyline.length - 1);
      const segments = new Float32Array(segCount * 6);
      const segmentEdge = new Uint32Array(segCount);
      let s = 0;
      body.topology.edges.forEach((e, ei) => {
        for (let i = 0; i + 1 < e.polyline.length; i++) {
          const a = e.polyline[i] as { x: number; y: number; z: number };
          const b = e.polyline[i + 1] as { x: number; y: number; z: number };
          segments[s * 6] = a.x;
          segments[s * 6 + 1] = a.y;
          segments[s * 6 + 2] = a.z;
          segments[s * 6 + 3] = b.x;
          segments[s * 6 + 4] = b.y;
          segments[s * 6 + 5] = b.z;
          segmentEdge[s] = ei;
          s++;
        }
      });
      msSegments += performance.now() - tSegs;
      // A view whose geometry survived keeps the buffers that geometry points
      // at, and the ones that just arrived -- byte for byte the same -- are
      // dropped.
      const kept = keepGeometry && prior ? prior.mesh0 : mesh0;
      const pickable: Pickable = { body, mesh, faceIds: kept.faceIds, segments, segmentEdge };
      this.views.push({
        body,
        mesh,
        geometry,
        mesh0: kept,
        indices: kept.indices,
        faceIds: kept.faceIds,
        // The triangle-to-face index depends on `faceIds` and on how many
        // faces there are, and nothing else. Identical bytes mean identical
        // `faceIds`, so it survives exactly when the geometry and the face
        // count both do.
        faceTris:
          keepGeometry && prior && prior.body.topology.faces.length === body.topology.faces.length ? prior.faceTris : null,
        hover,
        selected,
        pickable,
        segRange: [0, 0],
        // Per-face colours are keyed by the body's id and its face names, and
        // are only ever built in `faces` mode. Rebuilding them lazily there is
        // cheaper than proving they are still right here.
        colors: null,
        owned: own,
        materialKey,
        segments0: segments,
        plateMatrix: null,
        plateMaterial: null,
      });
    }
    // Whatever the new model did not claim is gone from it -- except a
    // geometry a body of the new model shares, which stays.
    const live = new Set(this.views.map((v) => v.geometry));
    for (const list of spare.values()) for (const v of list) this.disposeView(v, live);
    for (const g of retired) if (!live.has(g)) g.dispose();
    // A coarse pass is not the model's last word, so it is not what the next
    // edit should be compared against.
    if (flashing) this.lastSignatures = signatures;
    this.triangles = tris;
    count("viewer.triangles", tris);
    count("viewer.setModel");
    record("setModel.geometry", msGeometry);
    record("setModel.meshBounds", msBounds);
    record("setModel.compare", msCompare);
    record("setModel.overlays", msOverlays);
    record("setModel.segments", msSegments);
    // A model that arrives while the plates view is on is placed on its
    // plates before the edges, the pickables and the bounds are built from
    // it, so that all three see the bodies where they are drawn.
    if (this.platesOn) span("setModel.plates", () => this.placeOnPlates());
    span("setModel.mode", () => this.applyMode());
    span("setModel.edgeBatch", () => this.rebuildEdgeBatch());
    span("setModel.pickables", () => this.rebuildPickables());
    span("setModel.bounds", () => this.updateSceneBounds());
    this.freshGeometry = true;
    this.requestRender();
  }

  /**
   * A material of this body's own, or null to use the shared default.
   *
   * Only built when the script actually said something, so the common case
   * still shares one instance across every body and costs nothing.
   *
   * Translucency is the part that needs care. `transparent` without
   * `depthWrite: false` makes a body hide the parts of *itself* that are
   * behind its own front faces, which looks like a hole rather than like
   * glass; and a translucent body has to draw after the opaque ones, which is
   * what `renderOrder` buys.
   */
  private materialFor(body: Body, vertexColors = false): MeshStandardMaterial | null {
    const m = body.material;
    if (!m && !vertexColors) return null;
    const hasColour = m?.color != null;
    const opacity = m?.opacity == null ? 1 : m.opacity;
    const translucent = opacity < 0.999;
    if (!vertexColors && !hasColour && !translucent && m?.roughness == null && m?.metallic == null) return null;
    const mat = this.bodyMaterial.clone() as MeshStandardMaterial;
    // A scan's own colour multiplies the base colour, as `cadpole render`
    // does; the importer makes that base white.
    if (vertexColors) {
      mat.vertexColors = true;
      mat.color.setRGB(1, 1, 1);
    }
    if (!m) return mat;
    if (m.color) mat.color.setRGB(m.color[0], m.color[1], m.color[2]);
    if (m.roughness !== null) mat.roughness = m.roughness;
    if (m.metallic !== null) mat.metalness = m.metallic;
    if (translucent) {
      mat.transparent = true;
      mat.opacity = Math.max(0.03, opacity);
      mat.depthWrite = false;
    }
    return mat;
  }

  /** The shared material a body flashing in `color` borrows for the fade. */
  private flashMaterial(color: number): MeshStandardMaterial {
    const held = this.flashMaterials.get(color);
    if (held) return held;
    const m = this.bodyMaterial.clone() as MeshStandardMaterial;
    this.flashMaterials.set(color, m);
    return m;
  }

  /**
   * Stop every flash and leave the materials as they were found.
   *
   * The emissive has to be put back rather than the material thrown away: a
   * body with a material of its own flashes *on that material*, and that
   * material outlives both the flash and the evaluation.
   */
  private endFlashes(): void {
    for (const [, f] of this.flashes) {
      f.material.emissive.setHex(0x000000);
      f.material.emissiveIntensity = 1;
    }
    this.flashes.clear();
  }

  /** An overlay's geometry: the body's own vertices, with an index of its own. */
  private overlayGeometry(geometry: BufferGeometry): BufferGeometry {
    const g = new BufferGeometry();
    g.setAttribute("position", geometry.getAttribute("position"));
    g.setIndex(new BufferAttribute(new Uint32Array(0), 1));
    g.boundingSphere = geometry.boundingSphere;
    return g;
  }

  /** One body's hover / selection overlay: the same vertices, a rebuilt index. */
  private makeOverlay(geometry: BufferGeometry, mat: MeshBasicMaterial, order: number): Mesh {
    const g = this.overlayGeometry(geometry);
    const m = new Mesh(g, mat);
    m.visible = false;
    m.renderOrder = order;
    m.frustumCulled = false;
    return m;
  }

  /**
   * Let go of a body that the new model does not have. `live` is the set
   * of geometries the new model still draws: one this view shares with a
   * surviving body (an instance's part mesh) is kept.
   */
  private disposeView(v: BodyView, live?: Set<BufferGeometry>): void {
    this.content.remove(v.mesh, v.hover, v.selected);
    if (!live || !live.has(v.geometry)) v.geometry.dispose();
    v.hover.geometry.dispose();
    v.selected.geometry.dispose();
    v.owned?.dispose();
  }

  private clearModel(): void {
    this.endFlashes();
    this.clearBeds();
    for (const v of this.views) this.disposeView(v);
    this.views.length = 0;
    this.pickables.length = 0;
    this.lastHoverKey = null;
    if (this.edges) {
      this.content.remove(this.edges);
      this.edges.geometry.dispose();
      this.edges = null;
    }
    this.hoverEdges = this.applyEdgeOverlay(this.hoverEdges, this.hoverEdgeMaterial, []);
    this.softEdges = this.applyEdgeOverlay(this.softEdges, this.softEdgeMaterial, []);
    this.selectedEdges = this.applyEdgeOverlay(this.selectedEdges, this.selectedEdgeMaterial, []);
  }

  private rebuildEdgeBatch(): void {
    if (this.edges) {
      this.content.remove(this.edges);
      this.edges.geometry.dispose();
      this.edges = null;
    }
    let total = 0;
    for (const v of this.views) if (v.mesh.visible) total += v.pickable.segments.length;
    let off = 0;
    const all = new Float32Array(total);
    for (const v of this.views) {
      if (!v.mesh.visible) {
        v.segRange = [0, 0];
        continue;
      }
      all.set(v.pickable.segments, off);
      v.segRange = [off / 6, v.pickable.segments.length / 6];
      off += v.pickable.segments.length;
    }
    if (total === 0) return;
    const geom = new LineSegmentsGeometry();
    geom.setPositions(all);
    this.edges = new LineSegments2(geom, this.edgeMaterial);
    this.edges.visible = this.mode !== "shaded";
    this.edges.frustumCulled = false;
    this.edges.renderOrder = 5;
    this.content.add(this.edges);
  }

  private rebuildPickables(): void {
    this.pickables.length = 0;
    for (const v of this.views) if (v.mesh.visible) this.pickables.push(v.pickable);
    // How many bodies are drawn, stamped where a test can read it without a
    // frame (the HUD writes only on one): `data-evals`'s companion.
    document.documentElement.dataset["bodiesDrawn"] = String(this.pickables.length);
  }

  private updateSceneBounds(): void {
    this.sceneBox.makeEmpty();
    for (const v of this.views) {
      if (!v.mesh.visible) continue;
      const bb = v.geometry.boundingBox;
      // Where the body is drawn, not where its triangles are: an instance's
      // geometry is its part's, and a body on a plate is moved there.
      if (bb) this.sceneBox.union(this.tmpBox.copy(bb).applyMatrix4(v.mesh.matrix));
    }
    if (this.platesOn && this.model) {
      for (const p of this.model.plates) {
        this.sceneBox.expandByPoint(this.tmpV.set(p.origin.x, p.origin.y, 0));
        this.sceneBox.expandByPoint(this.tmpV.set(p.origin.x + p.bed.x, p.origin.y + p.bed.y, 0));
      }
    }
    if (this.sceneBox.isEmpty()) {
      this.sceneBox.min.set(-40, -40, 0);
      this.sceneBox.max.set(40, 40, 30);
    }
    // What the camera frames: the drawn bodies of the framing
    // layers when the publisher named some and any of them is drawn, else
    // every drawn body. The grid, the bounding box and the section span
    // the whole drawn scene either way.
    this.frameBox.makeEmpty();
    if (this.frameLayers.size > 0 && !this.platesOn) {
      for (const v of this.views) {
        if (!v.mesh.visible || !this.frameLayers.has(v.body.layer)) continue;
        const bb = v.geometry.boundingBox;
        if (bb) this.frameBox.union(this.tmpBox.copy(bb).applyMatrix4(v.mesh.matrix));
      }
    }
    if (this.frameBox.isEmpty()) this.frameBox.copy(this.sceneBox);
    this.frameBox.getBoundingSphere(this.tmpSphere);
    this.sceneRadius = Math.max(1, this.tmpSphere.radius);
    this.sceneCenter.copy(this.tmpSphere.center);
    // Pan and zoom stay on the whole drawn scene, never only the framed part.
    this.sceneBox.getBoundingSphere(this.tmpSphere);
    this.controls.setBounds(this.tmpSphere.center, Math.max(1, this.tmpSphere.radius));
    const extent = Math.max(
      Math.abs(this.sceneBox.min.x),
      Math.abs(this.sceneBox.max.x),
      Math.abs(this.sceneBox.min.y),
      Math.abs(this.sceneBox.max.y),
    );
    const half = Math.ceil(Math.max(extent * 1.6, 100) / 50) * 50;
    if (half !== this.gridHalf) this.buildGrid(half);
    this.rebuildBBox();
    // What Home, Iso and Fit frame, stamped where a test can read it: the
    // drawn bodies only, never a hidden layer's.
    const r = (v: number): number => Math.round(v);
    const b = this.sceneBox;
    document.documentElement.dataset["sceneBox"] = [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z].map(r).join(",");
    const f = this.frameBox;
    document.documentElement.dataset["frameBox"] = [f.min.x, f.min.y, f.min.z, f.max.x, f.max.y, f.max.z].map(r).join(",");
    // The caps follow what is drawn: a body hidden, a layer switched off, a
    // new model or the plates view all change what the section cuts.
    this.rebuildCaps();
  }

  /**
   * The layers the camera frames (`cadpole publish --frame-layer`):
   * Home, Iso and Fit fit these layers' drawn bodies; empty fits them all.
   */
  setFrameLayers(names: readonly string[]): void {
    this.frameLayers = new Set(names);
    this.updateSceneBounds();
  }

  /**
   * What the camera frames, mm (`setFrameLayers`, else every drawn body):
   * where the Section panel's sliders run and a new cut starts.
   */
  get frameBounds(): Box3 {
    return (this.frameBox.isEmpty() ? this.sceneBox : this.frameBox).clone();
  }

  /** The drawn bodies' bounds, mm. */
  get bounds(): Box3 {
    return this.sceneBox.clone();
  }

  // ------------------------------------------------------------- section

  /**
   * Cut the model: up to one horizontal and one vertical plane, as
   * `cadpole render --section` spells them. Client-side only -- the clipping
   * is the GPU's and the caps are sliced here from the meshes the viewer
   * already holds -- so it is a redraw on any page, published ones
   * included, and never an evaluation.
   */
  setSection(s: SectionState): void {
    this.section = s;
    const planes = [s.horizontal, s.vertical];
    const three = planes.map((p) => (p ? planeOf(p) : null));
    const all = three.filter((p): p is Plane => p !== null);
    this.content.clippingPlanes = all;
    this.content.enabled = all.length > 0;
    // A plane's cap is clipped by the other plane only.
    this.capGroups.forEach((g, i) => {
      const other = three[1 - i] ?? null;
      g.clippingPlanes = other ? [other] : [];
      g.enabled = other !== null;
    });
    document.documentElement.dataset["section"] = formatSection(s);
    this.rebuildCaps();
    this.requestRender();
  }

  get sectionState(): SectionState {
    return this.section;
  }

  private clearCap(i: number): void {
    const c = this.caps[i];
    if (!c) return;
    const g = this.capGroups[i] as ClippingGroup;
    if (c.mesh) {
      g.remove(c.mesh);
      c.mesh.geometry.dispose();
    }
    if (c.depth) g.remove(c.depth);
    if (c.lines) {
      g.remove(c.lines);
      c.lines.geometry.dispose();
    }
    this.caps[i] = { mesh: null, depth: null, lines: null, bodies: [] };
  }

  /**
   * Slice every drawn body by each plane and build that plane's cap: its
   * closed loops filled with the body's colour, every loop and open chain
   * outlined in a shade of it. A body on a ghosted layer is outlined but
   * not filled -- an opaque cut face on a see-through body would be the
   * only solid thing about it.
   */
  private rebuildCaps(): void {
    const t0 = performance.now();
    const planes = [this.section.horizontal, this.section.vertical];
    let tris = 0;
    for (let i = 0; i < 2; i++) {
      this.clearCap(i);
      const plane = planes[i];
      if (!plane) continue;
      const pos: number[] = [];
      const fills: number[] = [];
      const lines: number[] = [];
      const lineColors: number[] = [];
      const bodies: Body[] = [];
      const fillColor = new Color();
      const lineColor = new Color();
      const p3: [number, number, number] = [0, 0, 0];
      const q3: [number, number, number] = [0, 0, 0];
      const identity = new Matrix4();
      for (const v of this.views) {
        if (!v.mesh.visible) continue;
        const bb = v.geometry.boundingBox;
        if (!bb) continue;
        const wb = this.tmpBox.copy(bb).applyMatrix4(v.mesh.matrix);
        const lo = plane.axis === 0 ? wb.min.x : plane.axis === 1 ? wb.min.y : wb.min.z;
        const hi = plane.axis === 0 ? wb.max.x : plane.axis === 1 ? wb.max.y : wb.max.z;
        if (!(lo < plane.at && hi > plane.at)) continue;
        const m = v.mesh.matrix.equals(identity) ? null : v.mesh.matrix.elements;
        const segs = sliceMesh(v.mesh0.positions, v.indices, m, plane);
        if (segs.length === 0) continue;
        const { loops, open } = joinLoops(segs);
        const cut = cutColors(this.drawnColor(v));
        fillColor.setRGB(cut.fill[0], cut.fill[1], cut.fill[2], SRGBColorSpace);
        lineColor.setRGB(cut.line[0], cut.line[1], cut.line[2], SRGBColorSpace);
        for (const chain of [...loops, ...open]) {
          const n = chain.length / 2;
          const closed = loops.includes(chain);
          for (let k = 0; k + 1 < n + (closed ? 1 : 0); k++) {
            const a = k;
            const b = (k + 1) % n;
            lift(plane, chain[a * 2] as number, chain[a * 2 + 1] as number, p3);
            lift(plane, chain[b * 2] as number, chain[b * 2 + 1] as number, q3);
            lines.push(p3[0], p3[1], p3[2], q3[0], q3[1], q3[2]);
            lineColors.push(lineColor.r, lineColor.g, lineColor.b, lineColor.r, lineColor.g, lineColor.b);
          }
        }
        if (this.ghostFor(v) !== null) continue;
        const fill = capTriangles(loops);
        for (let k = 0; k + 1 < fill.length; k += 2) {
          lift(plane, fill[k] as number, fill[k + 1] as number, p3);
          pos.push(p3[0], p3[1], p3[2]);
          fills.push(fillColor.r, fillColor.g, fillColor.b);
        }
        for (let k = 0; k < fill.length / 6; k++) bodies.push(v.body);
      }
      const g = this.capGroups[i] as ClippingGroup;
      const cap = this.caps[i] as { mesh: Mesh | null; depth: Mesh | null; lines: LineSegments2 | null; bodies: Body[] };
      if (pos.length > 0) {
        const geom = new BufferGeometry();
        geom.setAttribute("position", new Float32BufferAttribute(pos, 3));
        geom.setAttribute("color", new Float32BufferAttribute(fills, 3));
        geom.computeBoundingBox();
        geom.computeBoundingSphere();
        const mesh = new Mesh(geom, this.capMaterial);
        // One plane's faces and then their depth, before the other plane's.
        mesh.renderOrder = 1 + i * 0.5;
        mesh.name = `section-cap-${"xyz"[plane.axis]}`;
        g.add(mesh);
        cap.mesh = mesh;
        const depth = new Mesh(geom, this.capDepthMaterial);
        depth.renderOrder = 1.25 + i * 0.5;
        g.add(depth);
        cap.depth = depth;
        cap.bodies = bodies;
        tris += pos.length / 9;
      }
      if (lines.length > 0) {
        const geom = new LineSegmentsGeometry();
        geom.setPositions(new Float32Array(lines));
        geom.setColors(new Float32Array(lineColors));
        const l = new LineSegments2(geom, this.cutLineMaterial);
        l.frustumCulled = false;
        l.renderOrder = 6;
        g.add(l);
        cap.lines = l;
      }
    }
    // How much the cut filled, for a test to wait on and the perf panel to quote.
    document.documentElement.dataset["sectionTris"] = String(tris);
    record("section.caps", performance.now() - t0);
  }

  /**
   * The colour a body is drawn in, sRGB-encoded: its filament's on a plate,
   * its material's, or the default. A scan's colours are its vertices' and
   * its material white, so its cut takes the default.
   */
  private drawnColor(v: BodyView): [number, number, number] {
    const plate = this.platesOn ? v.plateMaterial : null;
    const own = v.owned && !v.owned.vertexColors ? v.owned : null;
    const c = this.tmpColor.copy((plate ?? own ?? this.bodyMaterial).color).convertLinearToSRGB();
    return [c.r, c.g, c.b];
  }

  /** The cap meshes and their bodies, for the picker. */
  private capPickables(): { mesh: Mesh; bodies: Body[] }[] {
    const out: { mesh: Mesh; bodies: Body[] }[] = [];
    for (const c of this.caps) if (c.mesh) out.push({ mesh: c.mesh, bodies: c.bodies });
    return out;
  }

  /** The active section's planes, for the picker. */
  private cutPlanes(): CutPlane[] {
    return planesOf(this.section);
  }

  /** Whether the dim grid / normal grid. */
  setGridStrength(k: number): void {
    this.gridStrength = Math.max(0.05, Math.min(1, k));
    if (this.grid) (this.grid.material as LineBasicMaterial).opacity = 0.9 * this.gridStrength;
    this.requestRender();
  }

  setHidden(hidden: ReadonlySet<string>): void {
    this.hidden = hidden;
    for (const v of this.views) v.mesh.visible = this.shouldShow(v);
    for (const g of this.ghosts) g.mesh.visible = !hidden.has(g.mesh.name);
    this.rebuildEdgeBatch();
    this.rebuildPickables();
    this.updateSceneBounds();
    this.requestRender();
  }

  /**
   * The display layers: which are drawn and how opaque, by name.
   * Touches the scene and nothing else -- visibility, the edge batch, the
   * pickables, the bounds, and the ghost material of every body on a
   * ghosted layer -- so a switch or a slider in the Layers panel is a
   * redraw, never an evaluation.
   */
  setLayers(layers: ReadonlyMap<string, LayerDisplay>): void {
    this.layers = layers;
    // Ghost materials for opacities no layer has any more can go; the ones
    // still in use are picked up again by `applyMode`.
    const inUse = new Set<string>();
    for (const l of layers.values()) inUse.add(l.opacity.toFixed(3));
    for (const [key, m] of this.ghostMaterials) {
      if (!inUse.has(key.slice(key.lastIndexOf("|") + 1))) {
        m.dispose();
        this.ghostMaterials.delete(key);
      }
    }
    for (const v of this.views) v.mesh.visible = this.shouldShow(v);
    this.applyMode();
    this.rebuildEdgeBatch();
    this.rebuildPickables();
    this.updateSceneBounds();
    this.requestRender();
  }

  /** Not hidden by the user, and on a layer that is shown (or on none). */
  private bodyShown(body: Body): boolean {
    if (this.hidden.has(body.id)) return false;
    const layer = this.layers.get(body.layer);
    return layer ? layer.visible : true;
  }

  /**
   * The translucent material a body on a ghosted layer wears, or null when
   * its layer is opaque: its own material (or the shared default) cloned
   * at the layer's opacity, drawn after the opaque bodies and without
   * writing depth, exactly as `materialFor` treats `material(opacity=)`.
   */
  private ghostFor(v: BodyView): MeshStandardMaterial | null {
    const layer = this.layers.get(v.body.layer);
    if (!layer || layer.opacity >= 0.999) return null;
    const key = `${v.materialKey}|${layer.opacity.toFixed(3)}`;
    let m = this.ghostMaterials.get(key);
    if (!m) {
      const base = v.owned ?? this.bodyMaterial;
      m = base.clone() as MeshStandardMaterial;
      // A flash is an emissive on the body's own material; a ghost cloned
      // mid-flash must not keep it.
      m.emissive.setHex(0);
      m.transparent = true;
      m.opacity = Math.max(0.03, layer.opacity * (base.transparent ? base.opacity : 1));
      m.depthWrite = false;
      this.ghostMaterials.set(key, m);
    }
    return m;
  }

  /**
   * Whether a body is drawn: not hidden by the user, on a layer that is
   * shown, and visible in the model -- or, in the plates view, on a plate,
   * whatever the model says: a part body exported directly is invisible in
   * the model view (its instances stand in for it) and a print object in
   * the plates view.
   */
  private shouldShow(v: BodyView): boolean {
    if (!this.bodyShown(v.body)) return false;
    return this.platesOn ? v.plateMatrix !== null : v.body.visible;
  }

  // ---------------------------------------------------------- plates view

  get platesView(): boolean {
    return this.platesOn;
  }

  /**
   * Switch between the model view and the plates view.
   *
   * Entering it moves every body that is on a plate to where `cadpole export`
   * puts it -- by transform, on the geometry the viewer already has -- in
   * its filament's colour, draws the beds, and hides bodies that are on no
   * plate. Leaving it puts everything back, the camera included: the model
   * view is exactly what it was, which is what makes the toggle safe to
   * flick.
   */
  setPlatesView(on: boolean): void {
    if (on === this.platesOn) return;
    if (on) this.savedCamera = { pose: this.controls.exactPose(), near: this.camera.near, far: this.camera.far };
    this.platesOn = on;
    this.placeOnPlates();
    this.applyMode();
    this.rebuildEdgeBatch();
    this.rebuildPickables();
    this.updateSceneBounds();
    if (!on && this.savedCamera) {
      this.controls.restoreExact(this.savedCamera.pose);
      this.camera.near = this.savedCamera.near;
      this.camera.far = this.savedCamera.far;
      this.camera.updateProjectionMatrix();
      this.savedCamera = null;
    }
    this.requestRender();
  }

  /** The placements of a body on the model's plates, in plate order. */
  private placementsOf(bodyId: string): { plate: Plate; body: PlateBody }[] {
    const out: { plate: Plate; body: PlateBody }[] = [];
    if (!this.model) return out;
    for (const plate of this.model.plates) for (const body of plate.bodies) if (body.body_id === bodyId) out.push({ plate, body });
    return out;
  }

  /**
   * Put every body where the current view wants it: on its plate, or back
   * where the model put it. Transforms, materials, visibility and the edge
   * segments only; the caller rebuilds what hangs off those.
   */
  private placeOnPlates(): void {
    for (const g of this.ghosts) this.content.remove(g.mesh);
    this.ghosts = [];
    this.clearBeds();
    const bodyMatrix = new Matrix4();
    const plateMatrix = new Matrix4();
    for (const v of this.views) {
      const placements = this.platesOn ? this.placementsOf(v.body.id) : [];
      const first = placements[0];
      if (first) {
        // The mesh is placed by the export's transform outright; its
        // display coordinates (topology, segments) get there through the
        // inverse of where the model view had them.
        matrixOf(first.body.transform, plateMatrix);
        matrixOf(v.body.transform, bodyMatrix).invert();
        const display = plateMatrix.clone().multiply(bodyMatrix);
        v.plateMatrix = display;
        v.plateMaterial = this.plateMaterialFor(first.body.color);
        for (const o of [v.mesh, v.hover, v.selected]) {
          o.matrix.copy(plateMatrix);
          o.matrix.decompose(o.position, o.quaternion, o.scale);
          o.updateMatrix();
        }
        v.pickable.segments = movedSegments(v.segments0, display, this.tmpV);
        for (const extra of placements.slice(1)) {
          const material = this.plateMaterialFor(extra.body.color);
          const ghost = new Mesh(v.geometry, material);
          ghost.name = v.body.id;
          matrixOf(extra.body.transform, ghost.matrix);
          ghost.matrix.decompose(ghost.position, ghost.quaternion, ghost.scale);
          ghost.updateMatrix();
          ghost.visible = !this.hidden.has(v.body.id);
          this.content.add(ghost);
          this.ghosts.push({ mesh: ghost, material });
        }
      } else {
        v.plateMatrix = null;
        v.plateMaterial = null;
        applyTransform(v.mesh, v.body.transform);
        applyTransform(v.hover, v.body.transform);
        applyTransform(v.selected, v.body.transform);
        v.pickable.segments = v.segments0;
      }
      v.mesh.visible = this.shouldShow(v);
      if (!v.mesh.visible) {
        v.hover.visible = false;
        v.selected.visible = false;
      }
    }
    if (this.platesOn && this.model) this.buildBeds(this.model.plates);
  }

  /** The material a body prints in: one per colour, shared. */
  private plateMaterialFor(color: [number, number, number]): MeshStandardMaterial {
    const key = color.map((c) => c.toFixed(4)).join(",");
    const held = this.plateMaterials.get(key);
    if (held) return held;
    const m = this.bodyMaterial.clone() as MeshStandardMaterial;
    m.color.setRGB(color[0], color[1], color[2]);
    this.plateMaterials.set(key, m);
    return m;
  }

  /** The beds: a light outline and a faint fill per plate, and a label with its number and name. */
  private buildBeds(plates: readonly Plate[]): void {
    if (plates.length === 0) return;
    const pos: number[] = [];
    for (const p of plates) {
      const { x: x0, y: y0 } = p.origin;
      const x1 = x0 + p.bed.x;
      const y1 = y0 + p.bed.y;
      pos.push(x0, y0, 0, x1, y0, 0, x1, y0, 0, x1, y1, 0, x1, y1, 0, x0, y1, 0, x0, y1, 0, x0, y0, 0);
      const fill = new Mesh(
        new PlaneGeometry(p.bed.x, p.bed.y),
        new MeshBasicMaterial({ color: COLORS.bed, transparent: true, opacity: 0.07, depthWrite: false }),
      );
      fill.position.set((x0 + x1) / 2, (y0 + y1) / 2, -0.01);
      fill.renderOrder = -1;
      this.scene.add(fill);
      this.beds.push(fill);
      const el = document.createElement("div");
      el.className = "vlabel platelabel";
      el.textContent = plateLabel(p);
      this.labelsEl.appendChild(el);
      this.plateLabels.push({ el, anchor: new Vector3(x0 + p.bed.x / 2, y1, 0), text: el.textContent });
    }
    const geom = new BufferGeometry();
    geom.setAttribute("position", new Float32BufferAttribute(pos, 3));
    const outline = new LineSegments(geom, new LineBasicMaterial({ color: COLORS.bed, transparent: true, opacity: 0.9 }));
    outline.position.z = 0.02;
    outline.frustumCulled = false;
    this.scene.add(outline);
    this.beds.push(outline);
  }

  private clearBeds(): void {
    for (const b of this.beds) {
      this.scene.remove(b);
      const m = b as Mesh | LineSegments;
      m.geometry.dispose();
      (m.material as Material).dispose();
    }
    this.beds = [];
    for (const l of this.plateLabels) l.el.remove();
    this.plateLabels = [];
  }

  /**
   * Frame one plate of the model's `plates` (its bed and what stands on
   * it), or every plate for `null`. A camera move, nothing else.
   */
  framePlate(index: number | null): void {
    const plates = this.model?.plates ?? [];
    const chosen = index === null ? plates : plates[index] ? [plates[index] as Plate] : plates;
    if (chosen.length === 0) {
      this.fit();
      return;
    }
    this.tmpBox.makeEmpty();
    for (const p of chosen) {
      this.tmpBox.expandByPoint(this.tmpV.set(p.origin.x, p.origin.y, 0));
      this.tmpBox.expandByPoint(this.tmpV.set(p.origin.x + p.bed.x, p.origin.y + p.bed.y, 0));
      for (const b of p.bodies) {
        const v = this.views.find((x) => x.body.id === b.body_id);
        const bb = v?.geometry.boundingBox;
        if (v && bb && v.mesh.visible) this.tmpBox.union(new Box3().copy(bb).applyMatrix4(v.mesh.matrix));
      }
    }
    this.tmpBox.getBoundingSphere(this.tmpSphere);
    this.controls.setPose({ distance: this.fitDistance(Math.max(1, this.tmpSphere.radius)), target: this.tmpSphere.center.clone() });
    this.requestRender();
  }

  // ------------------------------------------------------- display modes

  setViewMode(mode: ViewMode): void {
    if (mode === this.mode) return;
    this.mode = mode;
    this.applyMode();
    this.requestRender();
  }

  get viewMode(): ViewMode {
    return this.mode;
  }

  private applyMode(): void {
    let mat: Material;
    switch (this.mode) {
      case "shaded":
      case "shaded_edges":
        mat = this.bodyMaterial;
        break;
      case "wireframe":
        mat = this.wireMaterial;
        break;
      case "xray":
        mat = this.xrayMaterial;
        break;
      case "faces":
        mat = this.facesMaterial;
        break;
    }
    const shaded = this.mode === "shaded" || this.mode === "shaded_edges";
    for (const v of this.views) {
      // The faces mode paints the `color` attribute; a scan's own colour
      // is put back when it is left.
      const scan = v.geometry.userData["scanColors"] as BufferAttribute | undefined;
      if (this.mode === "faces") {
        if (!v.colors) v.colors = this.buildFaceColors(v);
        else if (scan) v.geometry.setAttribute("color", v.colors);
      } else if (scan && v.geometry.getAttribute("color") !== scan) {
        v.geometry.setAttribute("color", scan);
      }
      // A body mid-flash is on a material carrying the fading emissive, and
      // `mat` here is the shared one. Reassigning it would cancel the flash
      // the instant `setModel` finished, which is precisely when the flash
      // starts. Any other mode wins: an x-ray or wireframe request is the
      // user asking for something specific.
      const flash = this.flashes.get(v.mesh);
      const own = shaded ? v.owned : null;
      // On a plate a body wears its filament's colour, over its own material
      // and over a flash: the plates view is about what comes off the
      // printer, and a change flash there would be a grey body for a second.
      const plate = shaded && this.platesOn ? v.plateMaterial : null;
      // A ghosted layer wins over a flash: the layer is a standing
      // choice, the flash a second's notice, and an opaque second in the
      // middle of a ghost is the wrong way round.
      const ghost = shaded && !this.platesOn ? this.ghostFor(v) : null;
      v.mesh.material = plate ?? (shaded ? (ghost ?? (flash ? flash.material : (own ?? mat))) : mat);
      v.mesh.renderOrder = ghost || own?.transparent ? 5 : 0;
    }
    for (const g of this.ghosts) g.mesh.material = shaded ? g.material : mat;
    if (this.edges) this.edges.visible = this.mode !== "shaded";
  }

  /** Stable per-face colours (hash of the topological name). */
  private buildFaceColors(v: BodyView): BufferAttribute {
    const n = v.geometry.getAttribute("position").count;
    const colors = new Float32Array(n * 3);
    const faceColor = new Map<number, [number, number, number]>();
    const c = new Color();
    v.body.topology.faces.forEach((f, i) => {
      c.setHSL(hashHue(`${v.body.id}/${f.name}`), 0.55, 0.62);
      faceColor.set(i, [c.r, c.g, c.b]);
    });
    for (let t = 0; t < v.faceIds.length; t++) {
      const rgb = faceColor.get(v.faceIds[t] as number) ?? [0.6, 0.6, 0.6];
      for (let k = 0; k < 3; k++) {
        const vi = v.indices[t * 3 + k] as number;
        colors[vi * 3] = rgb[0];
        colors[vi * 3 + 1] = rgb[1];
        colors[vi * 3 + 2] = rgb[2];
      }
    }
    const attr = new BufferAttribute(colors, 3);
    v.geometry.setAttribute("color", attr);
    return attr;
  }

  setGridVisible(on: boolean): void {
    this.gridVisible = on;
    if (this.grid) this.grid.visible = on;
    this.requestRender();
  }

  setLabelsVisible(on: boolean): void {
    this.labelsEl.classList.toggle("labels-off", !on);
    this.requestRender();
  }

  setBBoxVisible(on: boolean): void {
    this.bboxVisible = on;
    this.rebuildBBox();
    this.requestRender();
  }

  setProjection(orthographic: boolean): void {
    if (this.orthographic === orthographic) return;
    this.orthographic = orthographic;
    this.requestRender();
  }

  get isOrthographic(): boolean {
    return this.orthographic;
  }

  private rebuildBBox(): void {
    if (this.bboxLines) {
      this.scene.remove(this.bboxLines);
      // Both: the renderer keeps a render object per (object, material) until
      // the material's `dispose` event, and removing it from the scene fires
      // none, so a material left undisposed leaks one per rebuild.
      this.bboxLines.geometry.dispose();
      (this.bboxLines.material as Material).dispose();
      this.bboxLines = null;
    }
    for (const l of this.dimLabels) l.el.remove();
    this.dimLabels = [];
    if (!this.bboxVisible || this.views.length === 0) return;
    const b = this.sceneBox;
    const { x: x0, y: y0, z: z0 } = b.min;
    const { x: x1, y: y1, z: z1 } = b.max;
    const pos = [
      x0, y0, z0, x1, y0, z0, x1, y0, z0, x1, y1, z0, x1, y1, z0, x0, y1, z0, x0, y1, z0, x0, y0, z0,
      x0, y0, z1, x1, y0, z1, x1, y0, z1, x1, y1, z1, x1, y1, z1, x0, y1, z1, x0, y1, z1, x0, y0, z1,
      x0, y0, z0, x0, y0, z1, x1, y0, z0, x1, y0, z1, x1, y1, z0, x1, y1, z1, x0, y1, z0, x0, y1, z1,
    ];
    const geom = new BufferGeometry();
    geom.setAttribute("position", new Float32BufferAttribute(pos, 3));
    const mat = new LineBasicMaterial({ color: COLORS.bbox, transparent: true, opacity: 0.75, depthTest: false });
    this.bboxLines = new LineSegments(geom, mat);
    this.bboxLines.renderOrder = 15;
    this.bboxLines.frustumCulled = false;
    this.scene.add(this.bboxLines);
    const mk = (text: string, x: number, y: number, z: number): void => {
      const el = document.createElement("div");
      el.className = "vlabel dimlabel";
      el.textContent = text;
      this.labelsEl.appendChild(el);
      this.dimLabels.push({ el, anchor: new Vector3(x, y, z), text });
    };
    mk(`X ${fmtLen(x1 - x0, this.displayUnit)}`, (x0 + x1) / 2, y0, z0);
    mk(`Y ${fmtLen(y1 - y0, this.displayUnit)}`, x1, (y0 + y1) / 2, z0);
    mk(`Z ${fmtLen(z1 - z0, this.displayUnit)}`, x1, y0, (z0 + z1) / 2);
  }

  // ------------------------------------------------------------ picking

  candidatesAt(x: number, y: number): Candidate[] {
    const model = this.model;
    if (!model) return [];
    const rect = this.canvas.getBoundingClientRect();
    const r = this.picker.pick(this.activeCamera(), this.pickables, x, y, rect.width, rect.height, 22, { planes: this.cutPlanes(), caps: this.capPickables() });
    const out: Candidate[] = [];
    const seen = new Set<string>();
    const push = (c: Candidate): void => {
      const k = refKey(c.ref);
      if (seen.has(k)) return;
      seen.add(k);
      out.push(c);
    };
    const edgeCand = (): Candidate | null => {
      if (!r.edge) return null;
      const e = r.edge.body.topology.edges[r.edge.index];
      if (!e) return null;
      return { ref: edgeRef(r.edge.body, e), kind: "edge", label: e.name.replace(/^edge:/, ""), detail: e.curve_kind };
    };
    if (r.edge && r.edgePx <= TIGHT_EDGE_PX) {
      const c = edgeCand();
      if (c) push(c);
    }
    if (r.face) {
      const f = r.face.body.topology.faces[r.face.index];
      if (f) push({ ref: faceRef(r.face.body, f), kind: "face", label: f.name.replace(/^face:/, ""), detail: f.surface_kind });
    }
    if (r.edge) {
      const c = edgeCand();
      if (c) push(c);
    }
    const body = r.face?.body ?? r.edge?.body ?? r.cut?.body ?? null;
    if (body) {
      push({ ref: bodyRef(body), kind: "body", label: body.name || body.id, detail: "body" });
      const f = model.features.find((x) => x.id === body.feature_id);
      if (f) push({ ref: featureRef(f), kind: "feature", label: f.name || f.id, detail: f.kind });
    }
    return out;
  }

  /** Double-click / double-tap: orbit around the point under the cursor. */
  focusAt(x: number, y: number): boolean {
    const rect = this.canvas.getBoundingClientRect();
    const r = this.picker.pick(this.activeCamera(), this.pickables, x, y, rect.width, rect.height, 22, { planes: this.cutPlanes(), caps: this.capPickables() });
    if (r.point) this.controls.focus(r.point.clone());
    return r.point !== null && r.point !== undefined;
  }

  private readonly onPointerMove = (e: PointerEvent): void => {
    if (e.pointerType === "touch") return;
    if (e.buttons !== 0) {
      this.emitHover(null);
      return;
    }
    const rect = this.canvas.getBoundingClientRect();
    this.pendingHover = { x: e.clientX - rect.left, y: e.clientY - rect.top };
    if (!this.hoverScheduled) {
      this.hoverScheduled = true;
      requestAnimationFrame(() => {
        this.hoverScheduled = false;
        const p = this.pendingHover;
        this.pendingHover = null;
        if (p) this.emitHover(this.candidatesAt(p.x, p.y)[0] ?? null);
      });
    }
  };

  private emitHover(c: Candidate | null): void {
    const key = c ? refKey(c.ref) : null;
    if (key === this.lastHoverKey) return;
    this.lastHoverKey = key;
    this.canvas.style.cursor = c ? "pointer" : "";
    this.callbacks.onHover(c);
  }

  // ------------------------------------------------------------ highlight

  setHighlight(selected: readonly EntityRef[], hovered: EntityRef | null): void {
    if (!this.model) return;
    const model = this.model;
    const selTris = new Map<BodyView, number[]>();
    const hovTris = new Map<BodyView, number[]>();
    const selSegs: number[] = [];
    const hovSegs: number[] = [];
    const softSegs: number[] = [];

    const edgeSegs = (v: BodyView, ei: number, into: number[]): void => {
      const [start, count] = v.segRange;
      const se = v.pickable.segmentEdge;
      for (let i = 0; i < count; i++) if (se[i] === ei) into.push(start + i);
    };
    const faceOutline = (v: BodyView, faceIndex: number, into: number[]): void => {
      const f = v.body.topology.faces[faceIndex];
      if (!f) return;
      for (const en of f.edges) {
        const ei = v.body.topology.edges.findIndex((e) => e.name === en);
        if (ei >= 0) edgeSegs(v, ei, into);
      }
    };
    const collect = (ref: EntityRef, tris: Map<BodyView, number[]>, segs: number[], soft: number[] | null): void => {
      if (ref.kind === "face" || ref.kind === "edge") {
        const v = this.views.find((x) => x.body.id === ref.body_id);
        if (!v || !v.mesh.visible) return;
        if (ref.kind === "face") {
          const fi = v.body.topology.faces.findIndex((f) => f.name === ref.topo_name);
          if (fi < 0) return;
          const list = tris.get(v) ?? [];
          for (const t of this.faceTriangles(v, fi)) list.push(t);
          tris.set(v, list);
          faceOutline(v, fi, soft ?? segs);
        } else {
          const ei = v.body.topology.edges.findIndex((e) => e.name === ref.topo_name);
          if (ei >= 0) edgeSegs(v, ei, segs);
        }
        return;
      }
      for (const body of bodiesForRef(model, ref)) {
        const v = this.views.find((x) => x.body.id === body.id);
        if (!v || !v.mesh.visible) continue;
        tris.set(v, [-1]);
        const [start, count] = v.segRange;
        const into = soft ?? segs;
        for (let i = 0; i < count; i++) into.push(start + i);
      }
    };
    for (const r of selected) collect(r, selTris, selSegs, null);
    if (hovered) collect(hovered, hovTris, hovSegs, softSegs);

    for (const v of this.views) {
      this.applyOverlay(v, v.selected, selTris.get(v));
      this.applyOverlay(v, v.hover, hovTris.get(v));
    }
    this.selectedEdges = this.applyEdgeOverlay(this.selectedEdges, this.selectedEdgeMaterial, selSegs);
    this.hoverEdges = this.applyEdgeOverlay(this.hoverEdges, this.hoverEdgeMaterial, hovSegs);
    this.softEdges = this.applyEdgeOverlay(this.softEdges, this.softEdgeMaterial, softSegs);
    this.rebuildLabels(selected);
    this.requestRender();
  }

  private faceTriangles(v: BodyView, faceIndex: number): Uint32Array {
    if (!v.faceTris) {
      const nFaces = v.body.topology.faces.length;
      const counts = new Uint32Array(nFaces + 1);
      for (let t = 0; t < v.faceIds.length; t++) {
        const f = v.faceIds[t] as number;
        if (f < nFaces) counts[f] = (counts[f] as number) + 1;
      }
      const lists: Uint32Array[] = [];
      for (let f = 0; f < nFaces; f++) lists.push(new Uint32Array(counts[f] as number));
      counts.fill(0);
      for (let t = 0; t < v.faceIds.length; t++) {
        const f = v.faceIds[t] as number;
        if (f < nFaces) {
          (lists[f] as Uint32Array)[counts[f] as number] = t;
          counts[f] = (counts[f] as number) + 1;
        }
      }
      v.faceTris = lists;
    }
    return v.faceTris[faceIndex] ?? new Uint32Array(0);
  }

  private applyOverlay(v: BodyView, overlay: Mesh, tris: number[] | undefined): void {
    if (!tris || tris.length === 0) {
      overlay.visible = false;
      return;
    }
    let index: Uint32Array;
    if (tris[0] === -1) {
      index = v.indices;
    } else {
      index = new Uint32Array(tris.length * 3);
      for (let i = 0; i < tris.length; i++) {
        const t = (tris[i] as number) * 3;
        index[i * 3] = v.indices[t] as number;
        index[i * 3 + 1] = v.indices[t + 1] as number;
        index[i * 3 + 2] = v.indices[t + 2] as number;
      }
    }
    overlay.geometry.setIndex(new BufferAttribute(index, 1));
    overlay.visible = true;
  }

  private applyEdgeOverlay(existing: LineSegments2 | null, material: Line2NodeMaterial, segs: number[]): LineSegments2 | null {
    if (existing) {
      this.content.remove(existing);
      existing.geometry.dispose();
    }
    if (segs.length === 0) return null;
    const buf = new Float32Array(segs.length * 6);
    let k = 0;
    for (const global of segs) {
      const v = this.views.find((x) => global >= x.segRange[0] && global < x.segRange[0] + x.segRange[1]);
      if (!v) continue;
      const local = (global - v.segRange[0]) * 6;
      for (let i = 0; i < 6; i++) buf[k * 6 + i] = v.pickable.segments[local + i] as number;
      k++;
    }
    const geom = new LineSegmentsGeometry();
    geom.setPositions(k === segs.length ? buf : buf.subarray(0, k * 6));
    const lines = new LineSegments2(geom, material);
    lines.frustumCulled = false;
    lines.renderOrder = 20;
    this.content.add(lines);
    return lines;
  }

  // -------------------------------------------------------------- labels

  private rebuildLabels(selected: readonly EntityRef[]): void {
    const model = this.model;
    for (const l of this.labels) l.el.remove();
    this.labels = [];
    if (!model) return;
    for (const ref of selected.slice(0, 8)) {
      const anchor = this.anchorFor(ref);
      if (!anchor) continue;
      const kind = ref.kind;
      const name =
        kind === "face" || kind === "edge" || kind === "vertex"
          ? (ref.topo_name ?? "").replace(/^(face|edge|vertex):/, "")
          : kind === "body"
            ? (model.bodies.find((b) => b.id === ref.body_id)?.name ?? ref.body_id ?? "")
            : (model.features.find((f) => f.id === ref.feature_id)?.name ?? ref.feature_id);
      const text = `${kind} · ${name}`;
      const el = document.createElement("div");
      el.className = `vlabel vlabel-${ref.kind}`;
      el.textContent = text;
      this.labelsEl.appendChild(el);
      this.labels.push({ el, anchor, text });
    }
  }

  /** Where a body's display coordinates are drawn: moved in the plates view, in place otherwise. */
  private displayMatrix(bodyId: string): Matrix4 | null {
    return this.views.find((v) => v.body.id === bodyId)?.plateMatrix ?? null;
  }

  private anchorFor(ref: EntityRef): Vector3 | null {
    const model = this.model;
    if (!model) return null;
    const body = model.bodies.find((b) => b.id === ref.body_id);
    const placed = (p: Vector3, id: string): Vector3 => {
      const m = this.displayMatrix(id);
      return m ? p.applyMatrix4(m) : p;
    };
    if (ref.kind === "face" && body) {
      const f = body.topology.faces.find((x) => x.name === ref.topo_name);
      return f ? placed(new Vector3(f.centroid.x, f.centroid.y, f.centroid.z), body.id) : null;
    }
    if (ref.kind === "edge" && body) {
      const e = body.topology.edges.find((x) => x.name === ref.topo_name);
      if (!e) return null;
      const mid = e.polyline[Math.floor(e.polyline.length / 2)] ?? e.start;
      return placed(new Vector3(mid.x, mid.y, mid.z), body.id);
    }
    const b = bodiesForRef(model, ref)[0];
    if (!b) return null;
    return placed(new Vector3((b.bbox.min.x + b.bbox.max.x) / 2, (b.bbox.min.y + b.bbox.max.y) / 2, b.bbox.max.z), b.id);
  }

  /** Project an anchor with the active camera into CSS pixels; null when behind/off-screen. */
  private project(anchor: Vector3, out: { x: number; y: number }): boolean {
    const w = this.container.clientWidth;
    const h = this.container.clientHeight;
    this.tmpV.copy(anchor).project(this.activeCamera());
    if (!(this.tmpV.z < 1 && Math.abs(this.tmpV.x) < 1.2 && Math.abs(this.tmpV.y) < 1.2)) return false;
    out.x = ((this.tmpV.x + 1) / 2) * w;
    out.y = ((1 - this.tmpV.y) / 2) * h;
    return true;
  }

  private readonly projOut = { x: 0, y: 0 };

  private updateLabels(): void {
    for (const l of this.labels) this.placeLabel(l);
    for (const l of this.dimLabels) this.placeLabel(l);
    for (const l of this.plateLabels) this.placeLabel(l);
  }

  private placeLabel(l: Label): void {
    const ok = this.project(l.anchor, this.projOut);
    l.el.hidden = !ok;
    if (ok) l.el.style.transform = `translate(${this.projOut.x.toFixed(1)}px, ${this.projOut.y.toFixed(1)}px)`;
  }

  // -------------------------------------------------------------- camera

  setView(preset: ViewPreset, opts: { fit?: boolean } = {}): void {
    const fit = opts.fit ?? preset === "home";
    const dir = VIEW_DIRS[preset === "home" ? "iso" : preset];
    if (preset === "home" && this.homeDir) this.tmpV.copy(this.homeDir);
    else this.tmpV.set(dir[0], dir[1], dir[2]).normalize();
    const phi = Math.acos(Math.max(-1, Math.min(1, this.tmpV.z)));
    const theta = Math.atan2(this.tmpV.y, this.tmpV.x);
    if (fit) this.controls.setPose({ theta, phi, distance: this.fitBox(this.tmpV2.copy(this.tmpV)), target: this.frameCenter() });
    else this.controls.setPose({ theta, phi });
  }

  /**
   * Look along a direction (from the target toward the eye) and frame what
   * Home frames: a published page's `render_defaults(camera=)`
   * taken for its direction.
   */
  lookFrom(dir: Vector3, opts: { snap?: boolean } = {}): void {
    const d = dir.clone().normalize();
    const phi = Math.acos(Math.max(-1, Math.min(1, d.z)));
    const theta = Math.atan2(d.y, d.x);
    this.controls.setPose({ theta, phi, distance: this.fitBox(d), target: this.frameCenter() }, !opts.snap);
  }

  /** Where Home looks from (toward the target), or null for iso. */
  setHomeDirection(dir: Vector3 | null): void {
    this.homeDir = dir ? dir.clone().normalize() : null;
  }

  /** Stand the eye exactly here, looking at `target` (a published `--eye`). */
  lookAt(eye: Vector3, target: Vector3): void {
    const d = eye.clone().sub(target);
    const distance = Math.max(1e-3, d.length());
    d.normalize();
    this.controls.setPose({ theta: Math.atan2(d.y, d.x), phi: Math.acos(Math.max(-1, Math.min(1, d.z))), distance, target: target.clone() }, false);
  }

  fit(): void {
    const dir = this.camera.position.clone().sub(this.controls.currentTarget(this.tmpV2));
    if (dir.lengthSq() === 0) dir.set(1, -0.95, 0.8);
    this.controls.setPose({ distance: this.fitBox(dir.normalize()), target: this.frameCenter() });
  }

  /** The centre of what Home and Fit frame. */
  private frameCenter(): Vector3 {
    return this.frameBox.isEmpty() ? this.sceneCenter.clone() : this.frameBox.getCenter(new Vector3());
  }

  /**
   * How far back the camera stands, looking along `-dir` at the framed
   * box's centre, for the box's eight corners to fit the view with a small
   * margin. Tighter than a bounding sphere, which leaves a long,
   * low house a third of the viewport: the box is what is seen.
   */
  private fitBox(dir: Vector3): number {
    const box = this.frameBox.isEmpty() ? this.sceneBox : this.frameBox;
    if (box.isEmpty()) return this.fitDistance(this.sceneRadius);
    const c = box.getCenter(new Vector3());
    const back = dir.clone().normalize();
    const up = Math.abs(back.z) > 0.999 ? new Vector3(0, 1, 0) : new Vector3(0, 0, 1);
    const right = new Vector3().crossVectors(up, back).normalize();
    const camUp = new Vector3().crossVectors(back, right).normalize();
    const { tanH, tanV } = this.clearTans();
    let d = 0;
    const p = new Vector3();
    for (let i = 0; i < 8; i++) {
      p.set(i & 1 ? box.max.x : box.min.x, i & 2 ? box.max.y : box.min.y, i & 4 ? box.max.z : box.min.z).sub(c);
      const z = p.dot(back);
      d = Math.max(d, z + Math.abs(p.dot(right)) / tanH, z + Math.abs(p.dot(camUp)) / tanV);
    }
    // A margin, and never inside the box.
    return Math.max(d * 1.06, box.getSize(p).length() * 0.25, 1);
  }

  /** Fit the camera to the given entities; falls back to fit-all. */
  focusSelection(refs: readonly EntityRef[]): void {
    const model = this.model;
    if (!model || refs.length === 0) {
      this.fit();
      return;
    }
    this.tmpBox.makeEmpty();
    const identity = new Matrix4();
    for (const ref of refs) {
      const body = model.bodies.find((b) => b.id === ref.body_id);
      // Display coordinates, moved to the plate when the plates view is on.
      const m = body ? (this.displayMatrix(body.id) ?? identity) : identity;
      if (ref.kind === "face" && body) {
        const f = body.topology.faces.find((x) => x.name === ref.topo_name);
        if (!f) continue;
        const r = Math.max(1, Math.sqrt(Math.max(0, f.area)) * 0.6);
        this.tmpV.set(f.centroid.x, f.centroid.y, f.centroid.z).applyMatrix4(m);
        this.tmpBox.expandByPoint(this.tmpV2.copy(this.tmpV).addScalar(-r));
        this.tmpBox.expandByPoint(this.tmpV2.copy(this.tmpV).addScalar(r));
      } else if (ref.kind === "edge" && body) {
        const e = body.topology.edges.find((x) => x.name === ref.topo_name);
        if (!e) continue;
        for (const p of e.polyline) this.tmpBox.expandByPoint(this.tmpV.set(p.x, p.y, p.z).applyMatrix4(m));
      } else {
        for (const b of bodiesForRef(model, ref)) {
          const mb = this.displayMatrix(b.id) ?? identity;
          this.tmpBox.expandByPoint(this.tmpV.set(b.bbox.min.x, b.bbox.min.y, b.bbox.min.z).applyMatrix4(mb));
          this.tmpBox.expandByPoint(this.tmpV.set(b.bbox.max.x, b.bbox.max.y, b.bbox.max.z).applyMatrix4(mb));
        }
      }
    }
    if (this.tmpBox.isEmpty()) {
      this.fit();
      return;
    }
    this.tmpBox.getBoundingSphere(this.tmpSphere);
    const radius = Math.max(0.5, this.tmpSphere.radius);
    this.controls.setPose({ distance: this.fitDistance(radius), target: this.tmpSphere.center.clone() });
  }

  private fitDistance(radius: number): number {
    const { tanH, tanV } = this.clearTans();
    return (radius / Math.sin(Math.atan(Math.min(tanH, tanV)))) * 1.08;
  }

  // ------------------------------------------------------ covered edges
  //
  // A sheet or a panel that lies over part of the viewport takes that part
  // out of the picture without changing the canvas. The picture is shifted
  // instead: both cameras carry a view offset (`Camera.setViewOffset`) that
  // puts the centre of projection -- where the orbit target is drawn -- in
  // the middle of what is left uncovered. The offset is part of the
  // projection matrix, so everything that goes through the camera agrees
  // with what is drawn without knowing about it: picking, the labels, a
  // section's planes and caps. Nothing moves in the world: the orbit pose,
  // the eye and the target are what they were, so a stored pose or a link to
  // the view opens on the same camera whatever is covering the viewport.

  /**
   * Say how much of each edge of the viewport is covered, in CSS pixels.
   *
   * The model's framed centre then sits in the middle of the uncovered
   * rectangle, and Home, Fit and Focus fit that rectangle. All zeros (the
   * default) is the plain view. The move is eased unless `snap` is set or
   * reduced motion is asked for. An axis left with less than
   * `MIN_CLEAR_PX` uncovered is treated as uncovered: there is nowhere to
   * put the model, and a picture pushed off the canvas helps nobody.
   */
  setViewportInsets(insets: Partial<ViewportInsets>, opts: { snap?: boolean } = {}): void {
    const px = (v: number | undefined): number => (Number.isFinite(v) ? Math.max(0, v as number) : 0);
    const next = { top: px(insets.top), right: px(insets.right), bottom: px(insets.bottom), left: px(insets.left) };
    const was = this.insets;
    if (next.top === was.top && next.right === was.right && next.bottom === was.bottom && next.left === was.left) return;
    this.insets = next;
    this.aimShift(opts.snap === true);
  }

  /** What `setViewportInsets` was last given. */
  viewportInsets(): ViewportInsets {
    return { ...this.insets };
  }

  /**
   * The uncovered part of the viewport: its size as a fraction of the
   * canvas, and where its middle is from the canvas's, in CSS pixels.
   */
  private clearArea(): { fx: number; fy: number; x: number; y: number } {
    const w = Math.max(1, this.container.clientWidth);
    const h = Math.max(1, this.container.clientHeight);
    const i = this.insets;
    const wide = w - i.left - i.right >= MIN_CLEAR_PX;
    const tall = h - i.top - i.bottom >= MIN_CLEAR_PX;
    return {
      fx: wide ? (w - i.left - i.right) / w : 1,
      fy: tall ? (h - i.top - i.bottom) / h : 1,
      // Whole pixels: half of one would resample every edge in the picture.
      x: wide ? Math.round((i.left - i.right) / 2) : 0,
      y: tall ? Math.round((i.top - i.bottom) / 2) : 0,
    };
  }

  /** Half the field of view across and up the uncovered area, as tangents. */
  private clearTans(): { tanH: number; tanV: number } {
    const tanV = Math.tan((this.camera.fov * Math.PI) / 360);
    const a = this.clearArea();
    return { tanH: tanV * this.camera.aspect * a.fx, tanV: tanV * a.fy };
  }

  /** Head for where the insets (or a new canvas size) put the centre. */
  private aimShift(snap: boolean): void {
    const a = this.clearArea();
    const s = this.shift;
    if (a.x === s.toX && a.y === s.toY && !snap) return;
    s.toX = a.x;
    s.toY = a.y;
    if (snap || this.reducedMotion.matches || !this.ready) {
      s.x = a.x;
      s.y = a.y;
      s.moving = false;
      this.applyShift();
    } else if (!s.moving) {
      s.last = 0;
      s.moving = true;
    }
    this.requestRender();
  }

  /** Advance the eased shift; true while it is still under way. */
  private stepShift(now: number): boolean {
    const s = this.shift;
    if (!s.moving) return false;
    const dt = s.last ? Math.min(50, now - s.last) : 16;
    s.last = now;
    const k = 1 - Math.exp(-dt / SHIFT_TAU_MS);
    s.x += (s.toX - s.x) * k;
    s.y += (s.toY - s.y) * k;
    if (Math.abs(s.toX - s.x) < 0.5 && Math.abs(s.toY - s.y) < 0.5) {
      s.x = s.toX;
      s.y = s.toY;
      s.moving = false;
    }
    this.applyShift();
    return s.moving;
  }

  /**
   * Write the shift into both cameras as a view offset. The full view is the
   * canvas and so is the window onto it, moved by the shift: the scale of the
   * picture does not change, only where its centre lands.
   */
  private applyShift(): void {
    const s = this.shift;
    const w = Math.max(1, Math.floor(this.container.clientWidth));
    const h = Math.max(1, Math.floor(this.container.clientHeight));
    for (const cam of [this.camera, this.ortho]) {
      if (s.x === 0 && s.y === 0) {
        if (cam.view?.enabled) cam.clearViewOffset();
      } else {
        cam.setViewOffset(w, h, -s.x, -s.y, w, h);
      }
    }
    // A zoom about the pointer measures from the centre of projection.
    this.controls.centre.x = s.x;
    this.controls.centre.y = s.y;
  }

  /**
   * Where a model point is drawn, in CSS pixels from the viewport's top left
   * corner, or null when it is behind the camera or off the canvas.
   */
  screenPoint(p: { x: number; y: number; z: number }): { x: number; y: number } | null {
    const out = { x: 0, y: 0 };
    return this.project(new Vector3(p.x, p.y, p.z), out) ? out : null;
  }

  private updateNearFar(): void {
    const d = this.controls.currentDistance;
    const near = Math.max(0.05, d * 0.002);
    const far = Math.max(d + this.sceneRadius * 6 + this.gridHalf * 3, 100);
    if (Math.abs(near - this.camera.near) > near * 0.2 || Math.abs(far - this.camera.far) > far * 0.2) {
      this.camera.near = near;
      this.camera.far = far;
      this.camera.updateProjectionMatrix();
    }
  }

  /** Mirror the perspective camera's pose into the orthographic camera. */
  private syncOrtho(): void {
    if (!this.orthographic) return;
    const d = this.controls.currentDistance;
    const half = d * Math.tan((this.camera.fov * Math.PI) / 360);
    this.ortho.position.copy(this.camera.position);
    this.ortho.quaternion.copy(this.camera.quaternion);
    this.ortho.left = -half * this.camera.aspect;
    this.ortho.right = half * this.camera.aspect;
    this.ortho.top = half;
    this.ortho.bottom = -half;
    this.ortho.near = -this.sceneRadius * 4 - this.gridHalf * 2;
    this.ortho.far = d + this.sceneRadius * 6 + this.gridHalf * 3;
    this.ortho.updateProjectionMatrix();
    this.ortho.updateMatrixWorld();
  }

  /** The orbit pose, for storing across a reload. */
  pose(): ReturnType<CameraController["pose"]> {
    return this.controls.pose();
  }

  /**
   * Put the camera back exactly, with no animation.
   *
   * `snap` matters: a restored camera that eases into place looks like the
   * viewport resetting and then flying somewhere, which is precisely the
   * thing being fixed.
   */
  restorePose(p: ReturnType<CameraController["pose"]>): void {
    this.controls.setPose({ theta: p.theta, phi: p.phi, distance: p.distance, target: new Vector3(p.target.x, p.target.y, p.target.z) }, true);
    this.requestRender();
  }

  cameraState(): CameraState {
    const target = this.controls.currentTarget(this.tmpV2);
    return {
      eye: { x: this.camera.position.x, y: this.camera.position.y, z: this.camera.position.z },
      target: { x: target.x, y: target.y, z: target.z },
      up: { x: 0, y: 0, z: 1 },
      orthographic: this.orthographic,
      fov_degrees: this.camera.fov,
    };
  }

  // ------------------------------------------------------------ snapshot

  /** Capture the viewport as PNG, optionally with the selection labels drawn in. */
  async snapshot(opts: { annotate: boolean }): Promise<Attachment> {
    if (!this.ready) throw new Error("viewer not ready");
    const w = this.canvas.width;
    const h = this.canvas.height;
    const out = document.createElement("canvas");
    out.width = w;
    out.height = h;
    const ctx = out.getContext("2d");
    if (!ctx) throw new Error("2d context unavailable");
    const g = ctx.createRadialGradient(w * 0.5, h * 0.35, 0, w * 0.5, h * 0.35, Math.max(w, h) * 0.9);
    g.addColorStop(0, "#3a3c42");
    g.addColorStop(0.55, "#2a2b2f");
    g.addColorStop(1, "#202124");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
    // Render and read back in the same task: the drawing buffer is not
    // preserved across frames (neither by WebGPU nor by the WebGL2 fallback).
    // Drawn without the shift a covered viewport applies: the picture goes
    // with `cameraState()`, which a reader reproduces with a plain camera.
    const held = { ...this.shift };
    const shifted = held.x !== 0 || held.y !== 0;
    if (shifted) {
      this.shift.x = 0;
      this.shift.y = 0;
      this.applyShift();
    }
    this.renderNow();
    ctx.drawImage(this.canvas, 0, 0, w, h);
    if (opts.annotate) {
      const dpr = w / Math.max(1, this.container.clientWidth);
      ctx.font = `${Math.round(12 * dpr)}px system-ui, sans-serif`;
      ctx.textBaseline = "middle";
      for (const l of [...this.labels, ...this.dimLabels]) {
        if (!this.project(l.anchor, this.projOut)) continue;
        const x = this.projOut.x * dpr;
        const y = this.projOut.y * dpr;
        const tw = ctx.measureText(l.text).width;
        const pad = 6 * dpr;
        const bh = 18 * dpr;
        const bx = x - 4 * dpr;
        const by = y - 30 * dpr;
        ctx.fillStyle = "#3b8cff";
        ctx.beginPath();
        ctx.arc(x, y, 3 * dpr, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = "rgba(20,21,25,0.88)";
        ctx.strokeStyle = "#3b8cff";
        ctx.lineWidth = dpr;
        ctx.beginPath();
        ctx.roundRect(bx, by, tw + pad * 2, bh, 9 * dpr);
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = "#dbe9ff";
        ctx.fillText(l.text, bx + pad, by + bh / 2);
      }
    }
    if (shifted) {
      Object.assign(this.shift, held);
      this.applyShift();
      this.requestRender();
    }
    const dataUrl = out.toDataURL("image/png");
    return {
      kind: "screenshot",
      png_base64: dataUrl.slice(dataUrl.indexOf(",") + 1),
      camera: this.cameraState(),
      annotated: opts.annotate,
      width: w,
      height: h,
    };
  }

  // -------------------------------------------------------------- render

  requestRender(): void {
    this.needsRender = true;
    if (!this.rafId && !this.disposed) this.rafId = requestAnimationFrame(this.frame);
  }

  private renderNow(): void {
    this.controls.update(performance.now());
    this.draw();
  }

  /** Draw the scene as it stands into the drawing buffer, now. */
  private draw(): void {
    this.updateNearFar();
    this.syncOrtho();
    this.aimRig();
    this.renderer.render(this.scene, this.activeCamera());
    this.drawn = { width: this.canvas.width, height: this.canvas.height };
    this.updateTriad();
    this.updateLabels();
  }

  /**
   * The size of the drawing buffer when the picture in it was drawn, in
   * device pixels. It is the canvas's own size whenever the page is painted:
   * a buffer that changes size is drawn into in the same task.
   */
  drawnSize(): { width: number; height: number } {
    return { ...this.drawn };
  }

  /// Advance every change flash. Returns true while any is still running, so
  /// the frame loop keeps itself awake for the length of the fade.
  private stepFlashes(now: number): boolean {
    if (this.flashes.size === 0) return false;
    // A material is shared by every body flashing in its colour, so the fade
    // is computed per *material*: the brightest body still flashing on it
    // wins, and a material nobody is flashing on any more is put back.
    const live = new Map<MeshStandardMaterial, { color: number; k: number }>();
    const touched = new Set<MeshStandardMaterial>();
    for (const [mesh, f] of [...this.flashes]) {
      touched.add(f.material);
      const t = (now - f.start) / FLASH_MS;
      if (t >= 1) {
        this.flashes.delete(mesh);
        continue;
      }
      // Ease out: bright immediately, then most of the fade in the first half,
      // which reads as a flash rather than as a slow colour change.
      const k = (1 - t) * (1 - t);
      const prev = live.get(f.material);
      if (!prev || k > prev.k) live.set(f.material, { color: f.color, k });
    }
    for (const m of touched) {
      const l = live.get(m);
      if (l) {
        m.emissive.setHex(l.color);
        m.emissiveIntensity = l.k * 0.85;
      } else {
        m.emissive.setHex(0x000000);
        m.emissiveIntensity = 1;
      }
    }
    return this.flashes.size > 0;
  }

  // ---- drawing lighter while the view moves (touch devices)
  //
  // A phone draws three device pixels a CSS pixel and has a fraction of a
  // laptop's fill rate, so a model that is fine at rest can drop to a slide
  // show under a finger. While a gesture runs, and only once a frame has
  // actually been slow, the viewer steps down: first to one device pixel a
  // CSS pixel, then without the edge lines (the costliest thing drawn: each
  // is a screen-space quad). Everything is put back as the view comes to
  // rest, so what is looked at is always the full picture. A mouse or a
  // trackpad never takes this path.

  private readonly coarse = window.matchMedia("(pointer: coarse)");
  private gesture = false;
  /** The camera moved in the last frame drawn (a gesture's inertia, an ease to a pose). */
  private turning = false;
  /** 0: full; 1: fewer pixels; 2: fewer pixels and no edges. */
  private lean = 0;
  private slowFrames = 0;
  private lastMovingAt = 0;
  private leanEdges: Object3D[] = [];
  private restTimer: number | null = null;

  private setGesture(active: boolean): void {
    this.gesture = active;
    if (active && this.restTimer !== null) {
      window.clearTimeout(this.restTimer);
      this.restTimer = null;
    }
  }

  private setLean(level: number): void {
    if (level === this.lean) return;
    if (level >= 2 && this.lean < 2) {
      this.leanEdges = [];
      this.scene.traverse((o) => {
        const m = (o as unknown as { material?: unknown }).material;
        if (o.visible && m === this.edgeMaterial) this.leanEdges.push(o);
      });
      for (const o of this.leanEdges) o.visible = false;
    } else if (level < 2 && this.lean >= 2) {
      for (const o of this.leanEdges) o.visible = true;
      this.leanEdges = [];
    }
    this.lean = level;
    this.container.dataset["lean"] = String(level);
    // Fewer pixels, or all of them again: a new buffer, drawn into here.
    this.syncBuffer();
  }

  /**
   * After each frame: step down when the view is moving and the frames are
   * coming slowly, and back up at rest. What is timed is the gap between one
   * frame and the next, not the time spent submitting it: a phone that cannot
   * keep up is waiting on its GPU, and that wait is not in this thread.
   *
   * "Moving" is the camera under a finger or easing to a pose. The picture
   * sliding out from under a sheet (the eased view offset) is not: it lasts
   * a fifth of a second, and a drop in resolution for that long is only a
   * flicker each time a sheet opens or closes.
   */
  private adapt(moving: boolean, now: number): void {
    if (!this.coarse.matches) return;
    const frameMs = moving && this.lastMovingAt > 0 ? now - this.lastMovingAt : 0;
    this.lastMovingAt = moving ? now : 0;
    if (moving) {
      // Two slow frames running, so one hitch does not cost the picture.
      this.slowFrames = frameMs > 24 ? this.slowFrames + 1 : 0;
      if (this.slowFrames >= 2 && this.lean < 2) {
        this.slowFrames = 0;
        this.setLean(this.lean + 1);
      }
    } else {
      this.slowFrames = 0;
    }
    // The way back is on a timer from the last frame that moved, not on a
    // frame at rest: drawing is on demand, and the frame that ends a motion
    // may be the last there is.
    if (this.restTimer !== null) {
      window.clearTimeout(this.restTimer);
      this.restTimer = null;
    }
    if (this.lean > 0) this.restTimer = window.setTimeout(this.rested, 160);
  }

  private readonly rested = (): void => {
    this.restTimer = null;
    if (this.disposed) return;
    if (this.gesture || this.turning) {
      // A finger still down and holding still, or a move whose frames are
      // further apart than this timer: look again shortly.
      this.restTimer = window.setTimeout(this.rested, 160);
      return;
    }
    this.setLean(0);
    this.requestRender();
  };

  private readonly frame = (now: number): void => {
    this.rafId = 0;
    if (this.disposed || !this.ready) return;
    // A frame is about to be drawn or skipped: either way, into a buffer of
    // the right size. Usually it already is, and this is two comparisons.
    this.syncBuffer();
    const shifting = this.stepShift(now);
    const turning = this.controls.update(now);
    this.turning = turning;
    const moving = turning || shifting;
    const flashing = this.stepFlashes(now);
    if (flashing) this.needsRender = true;
    if (!(moving || this.needsRender)) return;
    this.needsRender = false;
    const t0 = performance.now();
    span(this.freshGeometry ? "render.gpuFirst" : "render.gpu", () => this.draw());
    const frameMs = performance.now() - t0;
    // The frame *after* a rebuild is where the GPU buffers are created and
    // filled, so it is part of the stall even though it is a different task.
    record(this.freshGeometry ? "render.firstFrame" : "render.frame", frameMs);
    this.freshGeometry = false;
    this.callbacks.onFrame?.({ frameMs, triangles: this.triangles, bodies: this.pickables.length, backend: this.backend });
    this.adapt(turning || this.gesture, now);
    if (moving || flashing) this.requestRender();
  };

  /** Turn the lights to where the camera is looking from. */
  private aimRig(): void {
    const cam = this.activeCamera();
    cam.getWorldDirection(this.tmpV);
    // Looking straight down or up has no azimuth of its own: the screen's
    // up stands in, which is what the orbit leaves there.
    if (Math.hypot(this.tmpV.x, this.tmpV.y) < 1e-3) this.tmpV.set(0, 0, 1).applyQuaternion(cam.quaternion).negate();
    this.rig.rotation.z = Math.atan2(-this.tmpV.y, -this.tmpV.x) - STUDIO_AZIMUTH;
  }

  // ------------------------------------------------- the drawing buffer
  //
  // The canvas's buffer follows three things: the viewport's box, the
  // screen's pixel ratio, and the lighter picture drawn while the view moves.
  // Changing its size clears it, and what is then shown is whatever the
  // buffer holds when the browser next composites -- which, for a size that
  // changed in a `ResizeObserver` callback, is before the next animation
  // frame. So every change goes through `syncBuffer`, and the picture is
  // drawn into the new buffer before that call returns.

  private readonly buffer = new DrawingBuffer({
    resize: (size) => this.resizeBuffer(size),
    draw: () => this.draw(),
  });
  private drawn = { width: 0, height: 0 };

  /** The viewport's box changed (or may have). */
  private resize(): void {
    if (this.ready) {
      this.syncBuffer();
      return;
    }
    // Before the renderer is up there is no buffer, but a fit made
    // meanwhile still needs the viewport's shape.
    const w = Math.floor(this.container.clientWidth);
    const h = Math.floor(this.container.clientHeight);
    if (w >= 1 && h >= 1) this.layout(w, h);
  }

  /** Bring the buffer to the viewport's size, the screen's ratio and the level drawn. */
  private syncBuffer(): void {
    if (!this.ready || this.disposed) return;
    this.buffer.sync(this.container.clientWidth, this.container.clientHeight, window.devicePixelRatio || 1, this.lean > 0);
  }

  private resizeBuffer(size: BufferSize): void {
    this.renderer.setDrawingBufferSize(size.width, size.height, size.ratio);
    this.layout(size.width, size.height);
    this.rebindViewportCopies();
  }

  /** The cameras for a viewport of this many CSS pixels. */
  private layout(w: number, h: number): void {
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    // The offset is in pixels of this canvas: a new size is a new offset.
    // Snapped: the canvas itself jumped, and an ease would trail behind it.
    const a = this.clearArea();
    Object.assign(this.shift, { x: a.x, y: a.y, toX: a.x, toY: a.y, moving: false });
    this.applyShift();
  }

  /**
   * A translucent fat line blends with a copy of the frame drawn so far, and
   * that copy is a texture of the buffer's size: a new size is a new texture.
   * The renderer makes the new one, but re-binds a material to it only when
   * it sees the material change, and a size is not something it watches for
   * a line. So each line material is given a change it does watch:
   * `dashOffset`, which an undashed line carries and never reads. Without
   * this the line goes on sampling the texture that was destroyed, WebGPU
   * rejects every frame that draws it, and the canvas stays empty.
   */
  private rebindViewportCopies(): void {
    for (const m of [this.edgeMaterial, this.softEdgeMaterial, this.hoverEdgeMaterial, this.selectedEdgeMaterial, this.cutLineMaterial]) {
      if (!m.dashed) m.dashOffset = m.dashOffset === 0 ? 1 : 0;
    }
  }

  // A screen's pixel ratio changes without the viewport's box changing: a
  // window dragged to another display, a page zoomed. A media query on the
  // current ratio fires once when it stops being true, and a zoom is a
  // window resize as well. Each frame looks too (`frame`), for a browser
  // that tells the page neither.

  private ratioQuery: MediaQueryList | null = null;

  private watchPixelRatio(): void {
    this.ratioQuery?.removeEventListener("change", this.onPixelRatio);
    this.ratioQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
    this.ratioQuery.addEventListener("change", this.onPixelRatio);
    window.addEventListener("resize", this.onPixelRatio);
  }

  private readonly onPixelRatio = (): void => {
    if (this.disposed) return;
    this.watchPixelRatio();
    this.syncBuffer();
  };

  // ------------------------------------------------ a lost graphics device
  //
  // A browser may take the GPU away: the tab sat in the background, the
  // driver restarted, memory ran short. Everything the renderer held is gone
  // and it will draw nothing more. A WebGPU device is asked for again at
  // once, on the same canvas; a WebGL context comes back when the browser
  // says so. Either way a new renderer is made and the scene, which lives on
  // this side, is drawn with it. Until then, and if it cannot be done, the
  // viewport says so rather than sit empty.

  private lostNotice: HTMLElement | null = null;
  /** When the device was last asked for again, to stop a loop of losses. */
  private recoveries: number[] = [];

  private deviceLost(api: "webgpu" | "webgl", message: string): void {
    if (this.disposed) return;
    this.ready = false;
    console.warn(`viewer: ${api === "webgpu" ? "WebGPU device" : "WebGL context"} lost: ${message}`);
    // A lost WebGL context is restored by the browser or not at all.
    if (api === "webgl") this.showLost("restoring");
    else void this.recover();
  }

  private readonly onContextRestored = (): void => {
    if (!this.disposed && !this.ready) void this.recover();
  };

  private async recover(): Promise<void> {
    const now = performance.now();
    this.recoveries = this.recoveries.filter((t) => now - t < 60_000);
    if (this.recoveries.length >= 3) {
      this.showLost("failed");
      return;
    }
    this.recoveries.push(now);
    this.showLost("restoring");
    // The old renderer is dropped, not disposed: its device is gone already,
    // and disposing one on a restored WebGL context would lose it again.
    this.renderer = this.makeRenderer();
    try {
      await this.startRenderer();
    } catch (err) {
      console.warn("viewer: the graphics device could not be restored", err);
    }
    this.showLost(this.ready ? null : "failed");
  }

  private showLost(state: "restoring" | "failed" | null): void {
    this.container.dataset["graphics"] = state ?? "ok";
    if (state === null) {
      this.lostNotice?.remove();
      this.lostNotice = null;
      return;
    }
    if (!this.lostNotice) {
      this.lostNotice = document.createElement("div");
      this.lostNotice.className = "viewport-notice";
      this.lostNotice.setAttribute("role", "alert");
      this.container.appendChild(this.lostNotice);
    }
    this.lostNotice.replaceChildren();
    const text = document.createElement("p");
    text.textContent = state === "restoring" ? "The graphics device was lost. Restoring the view\u2026" : "The graphics device was lost and could not be restored.";
    this.lostNotice.appendChild(text);
    if (state === "failed") {
      const reload = document.createElement("button");
      reload.type = "button";
      reload.textContent = "Reload";
      reload.addEventListener("click", () => location.reload());
      this.lostNotice.appendChild(reload);
    }
  }

  // ---------------------------------------------------------------- grid

  private buildGrid(half: number): void {
    if (this.grid) {
      this.scene.remove(this.grid);
      // The material too: see rebuildBBox.
      this.grid.geometry.dispose();
      (this.grid.material as Material).dispose();
    }
    this.gridHalf = half;
    const { minor, major } = gridSpacing(half, this.unit);
    const pos: number[] = [];
    const col: number[] = [];
    const cMinor = new Color(COLORS.gridMinor);
    const cMajor = new Color(COLORS.gridMajor);
    const cX = new Color(COLORS.axisX);
    const cY = new Color(COLORS.axisY);
    const push = (x0: number, y0: number, x1: number, y1: number, c: Color): void => {
      pos.push(x0, y0, 0, x1, y1, 0);
      col.push(c.r, c.g, c.b, c.r, c.g, c.b);
    };
    // Counted from the origin rather than accumulated from -half: the lines
    // must land on multiples of the spacing from the origin, and neither a
    // foot (not a whole number of millimetres) nor a half-width that is an
    // odd multiple of 50 under a 100 mm spacing starts there. Where -half is
    // itself on the grid (every small scene) this draws the same lines in the
    // same order as accumulating did.
    const n = Math.floor(half / minor);
    const every = Math.round(major / minor);
    for (let k = -n; k <= n; k++) {
      if (k === 0) continue;
      const i = k * minor;
      const c = k % every === 0 ? cMajor : cMinor;
      push(i, -half, i, half, c);
      push(-half, i, half, i, c);
    }
    push(-half, 0, half, 0, cX);
    push(0, -half, 0, half, cY);
    const geom = new BufferGeometry();
    geom.setAttribute("position", new Float32BufferAttribute(pos, 3));
    geom.setAttribute("color", new Float32BufferAttribute(col, 3));
    const mat = new LineBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.9 * this.gridStrength, depthWrite: false });
    this.grid = new LineSegments(geom, mat);
    this.grid.position.z = -0.02;
    this.grid.renderOrder = -1;
    this.grid.visible = this.gridVisible;
    this.scene.add(this.grid);
  }

  // --------------------------------------------------------------- triad

  /** View-cube equivalent: six clickable axis ends around a small triad. */
  private buildTriad(): void {
    const ns = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("class", "triad");
    svg.setAttribute("viewBox", "-44 -44 88 88");
    svg.setAttribute("aria-hidden", "true");
    const defs: [string, string, Vector3, ViewPreset, boolean][] = [
      ["X", "#e5636a", new Vector3(1, 0, 0), "right", true],
      ["Y", "#5fc26b", new Vector3(0, 1, 0), "back", true],
      ["Z", "#5aa2ff", new Vector3(0, 0, 1), "top", true],
      ["-X", "#e5636a", new Vector3(-1, 0, 0), "left", false],
      ["-Y", "#5fc26b", new Vector3(0, -1, 0), "front", false],
      ["-Z", "#5aa2ff", new Vector3(0, 0, -1), "bottom", false],
    ];
    const axes = defs.map(([name, color, dir, view, positive]) => {
      let line: SVGLineElement | null = null;
      let label: SVGTextElement | null = null;
      if (positive) {
        line = document.createElementNS(ns, "line");
        line.setAttribute("stroke", color);
        line.setAttribute("stroke-width", "2");
        line.setAttribute("x1", "0");
        line.setAttribute("y1", "0");
        svg.appendChild(line);
      }
      const dot = document.createElementNS(ns, "circle");
      dot.setAttribute("r", positive ? "9" : "7");
      dot.setAttribute("fill", positive ? color : "#2c2d31");
      dot.setAttribute("stroke", color);
      dot.setAttribute("stroke-width", positive ? "0" : "1.5");
      dot.style.cursor = "pointer";
      const title = document.createElementNS(ns, "title");
      title.textContent = `${view} view`;
      dot.appendChild(title);
      dot.addEventListener("click", () => this.setView(view));
      svg.appendChild(dot);
      {
        label = document.createElementNS(ns, "text");
        label.setAttribute("text-anchor", "middle");
        label.setAttribute("dominant-baseline", "central");
        label.setAttribute("font-size", positive ? "10" : "7.5");
        label.setAttribute("font-weight", "700");
        label.setAttribute("fill", positive ? "#16171a" : color);
        label.style.pointerEvents = "none";
        label.textContent = positive ? name : `\u2212${name.slice(1)}`;
        svg.appendChild(label);
      }
      return { line, dot, label, dir };
    });
    this.container.appendChild(svg);
    this.triad = { svg, axes };
  }

  private updateTriad(): void {
    if (!this.triad) return;
    this.tmpQ.copy(this.camera.quaternion).invert();
    const order = this.triad.axes
      .map((a, i) => {
        this.tmpV.copy(a.dir).applyQuaternion(this.tmpQ);
        return { i, z: this.tmpV.z, x: this.tmpV.x, y: this.tmpV.y };
      })
      .sort((a, b) => a.z - b.z);
    for (const o of order) {
      const a = this.triad.axes[o.i];
      if (!a) continue;
      const x = (o.x * 30).toFixed(1);
      const y = (-o.y * 30).toFixed(1);
      if (a.line) {
        a.line.setAttribute("x2", x);
        a.line.setAttribute("y2", y);
        this.triad.svg.appendChild(a.line);
      }
      a.dot.setAttribute("cx", x);
      a.dot.setAttribute("cy", y);
      const fade = o.z < 0 ? "0.5" : "1";
      a.dot.setAttribute("opacity", fade);
      a.line?.setAttribute("opacity", fade);
      this.triad.svg.appendChild(a.dot);
      if (a.label) {
        a.label.setAttribute("x", x);
        a.label.setAttribute("y", y);
        this.triad.svg.appendChild(a.label);
      }
    }
  }

  dispose(): void {
    this.disposed = true;
    if (this.rafId) cancelAnimationFrame(this.rafId);
    this.resizeObserver.disconnect();
    this.ratioQuery?.removeEventListener("change", this.onPixelRatio);
    window.removeEventListener("resize", this.onPixelRatio);
    this.canvas.removeEventListener("webglcontextrestored", this.onContextRestored);
    this.controls.dispose();
    this.clearModel();
    this.clearCap(0);
    this.clearCap(1);
    for (const m of this.flashMaterials.values()) m.dispose();
    this.flashMaterials.clear();
    for (const m of this.plateMaterials.values()) m.dispose();
    this.plateMaterials.clear();
    void this.renderer.dispose();
  }
}
