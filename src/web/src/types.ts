// Data shapes used by the UI. They mirror proto/cadpole/v1/*.proto (JSON uses the
// same snake_case field names); `normalize.ts` coerces whatever the engine /
// server emits into these exact shapes so the rest of the app can be strict.

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

/** cadpole.v1.ParamValue, flattened: number | bool | text | vector. */
export type ParamValue = number | boolean | string | Vec3;

export interface ScriptFile {
  path: string;
  source: string;
}

export interface SourceSpan {
  path: string;
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
}

export type Severity = "info" | "warning" | "error";

export interface Diagnostic {
  severity: Severity;
  message: string;
  span: SourceSpan | null;
  feature_id: string;
}

export interface ParamInfo {
  name: string;
  default_value: ParamValue;
  value: ParamValue;
  overridden: boolean;
  description: string;
  unit: string;
  min: number | null;
  max: number | null;
  step: number | null;
  /** For a text parameter: the values it may take (`param(choices=)`), else empty. */
  choices: string[];
  span: SourceSpan | null;
}

export interface Feature {
  id: string;
  kind: string;
  name: string;
  inputs: string[];
  outputs: string[];
  args: Record<string, string>;
  span: SourceSpan | null;
  suppressed: boolean;
  diagnostics: Diagnostic[];
}

export interface BBox {
  min: Vec3;
  max: Vec3;
}

/** cadpole.v1.Transform: a rigid transform, rotate (a unit quaternion) then translate. */
export interface Transform {
  translation: Vec3;
  rotation: { x: number; y: number; z: number; w: number };
}

export interface TopoFace {
  name: string;
  surface_kind: string;
  centroid: Vec3;
  normal: Vec3;
  area: number;
  edges: string[];
  tri_start: number;
  tri_count: number;
}

export interface TopoEdge {
  name: string;
  curve_kind: string;
  start: Vec3;
  end: Vec3;
  length: number;
  faces: string[];
  polyline: Vec3[];
}

export interface TopoVertex {
  name: string;
  position: Vec3;
}

export interface Topology {
  faces: TopoFace[];
  edges: TopoEdge[];
  vertices: TopoVertex[];
}

/** What a body looks like: `body.material(...)` in a script. */
export interface BodyMaterial {
  /** Linear RGB in 0..1, or null to keep the viewer's default. */
  color: [number, number, number] | null;
  roughness: number | null;
  metallic: number | null;
  /** 0..1. Below 0.999 the body draws translucent. */
  opacity: number | null;
}

export interface Body {
  id: string;
  feature_id: string;
  name: string;
  bbox: BBox;
  topology: Topology;
  /**
   * Index for the engine's mesh_* accessors (replaces cadpole.v1.Body.mesh).
   * For an instance body it is the *part body's* index: the
   * triangles arrive once and every instance points at them.
   */
  mesh_index: number;
  visible: boolean;
  /**
   * cadpole.v1.Body.part_body: the part body this instance body shares a mesh
   * with, or "" for an ordinary body. That body is in the model too,
   * invisible, only to carry the mesh.
   */
  part_body: string;
  /**
   * cadpole.v1.Body.transform: how the shared mesh is displayed -- the
   * instance's pose plus its exploded-view offset -- or null for an
   * ordinary body, which is drawn where its own triangles are.
   */
  transform: Transform | null;
  /** cadpole.v1.Body.instance: the instance this body belongs to, or "". */
  instance: string;
  /**
   * Appearance from the script, or null when it said nothing.
   *
   * The offline renderer has honoured this since the beginning, which is why a
   * `cadpole render` of the planter comes out terracotta; the live viewport
   * ignored it and drew everything in one shared grey. That is also why there
   * appeared to be no way to make a *single* body translucent, even though
   * `material(opacity=)` has always existed.
   */
  material: BodyMaterial | null;
  /**
   * cadpole.v1.Body.layer: the display layer this body is on, a
   * `Layer.name` from `Model.layers`, or "" for the implicit layer. Read by
   * the viewer's per-layer state, never by an evaluation.
   */
  layer: string;
}

/**
 * cadpole.v1.Layer: one `layer(name, doc=, visible=, opacity=, color=)`
 * declaration, with the layer's own defaults. What the Layers panel lists,
 * in `order`.
 */
export interface Layer {
  name: string;
  doc: string;
  visible: boolean;
  opacity: number;
  color: [number, number, number] | null;
  order: number;
}

/**
 * What the user changed about one layer in this browser: a field
 * left out keeps the script's own default. Never sent to the engine.
 */
export interface LayerOverride {
  visible?: boolean;
  opacity?: number;
}

/**
 * cadpole.v1.PlateBody: one body on a print plate -- which body, the
 * transform that puts its mesh (the shared part mesh, for an instance) where
 * the export puts it, and the filament it prints in.
 */
export interface PlateBody {
  body_id: string;
  /** The print object: `export(name=)`, else the body's own name. */
  object: string;
  /** Pose (no exploded-view offset) then the plate's placement. */
  transform: Transform;
  /** 1-based filament slot. */
  slot: number;
  /** The slot's colour, linear RGB. */
  color: [number, number, number];
  /** The filament's label: a material name, else `#rrggbb`. */
  filament: string;
}

/**
 * cadpole.v1.Plate: one print plate of a Bambu Studio project, laid out as
 * `cadpole export` lays it out: on Bambu's grid, its contents centred on the bed.
 */
export interface Plate {
  /** 1-based plate number. */
  index: number;
  /** `export(plate="...")`'s name, or "". */
  name: string;
  /** The `export()` path the plate belongs to, e.g. "gear_toy.3mf". */
  path: string;
  /** The bed, in mm. */
  bed: { x: number; y: number };
  /** The bed's corner in the export's world space. */
  origin: Vec3;
  bodies: PlateBody[];
}

/**
 * The document's display convention, as the script declared it with
 * `units(...)`: the unit readouts are written in and, for the imperial
 * units, the fraction of an inch a dimension rounds to. Every number the
 * model carries is still a millimetre.
 */
export interface DisplayUnits {
  unit: LengthUnit;
  fraction: number;
}

export interface Model {
  params: ParamInfo[];
  features: Feature[];
  bodies: Body[];
  diagnostics: Diagnostic[];
  eval_millis: number;
  /** Absent when the script declared no `units(...)`, which reads as millimetres. */
  display_units?: DisplayUnits;
  /**
   * The print plates of the document's Bambu projects, empty for a
   * document without `export(..., flavor=flavor.bambu)` calls. What the
   * plates view draws, by placing the meshes the model already carries.
   */
  plates: Plate[];
  /**
   * The display layers the script declared, in declaration order;
   * empty for a document that declares none, in which case no Layers
   * panel is shown.
   */
  layers: Layer[];
}

export type EntityKind =
  | "feature"
  | "body"
  | "face"
  | "edge"
  | "vertex"
  | "sketch"
  | "sketch_entity"
  | "param"
  | "asset";

export interface EntityRef {
  feature_id: string;
  body_id?: string;
  kind: EntityKind;
  topo_name?: string;
}

export interface Selection {
  selected: EntityRef[];
  hovered: EntityRef | null;
  note: string;
}

export type MessageFrom = "user" | "assistant";

/** cadpole.v1.ViewState-shaped camera description attached to snapshots. */
export interface CameraState {
  eye: Vec3;
  target: Vec3;
  up: Vec3;
  orthographic: boolean;
  fov_degrees: number;
}

export interface Attachment {
  kind: "screenshot";
  png_base64: string;
  camera: CameraState;
  /** Whether selection labels were drawn into the image. */
  annotated?: boolean;
  width?: number;
  height?: number;
}

export interface Message {
  seq: number;
  from: MessageFrom;
  text: string;
  at: string;
  attachments?: Attachment[];
  /**
   * Absent for a message posted whole. For a reply written a piece at a time:
   * "streaming" while it grows, then "done" or "failed" (reply-stream.ts).
   */
  state?: "streaming" | "done" | "failed";
  /** What the assistant is doing right now, while `state` is "streaming". */
  activity?: string;
}

export type LengthUnit = "mm" | "cm" | "m" | "in" | "ft" | "ft-in";

export interface CadpoleDocument {
  id: string;
  name: string;
  display_unit: LengthUnit;
  scripts: ScriptFile[];
  param_overrides: Record<string, ParamValue>;
  /**
   * The names of the document's assets, never their bytes: the
   * browser holds none, so a document with any can only be evaluated by the
   * server. Absent where there is no session.
   */
  asset_names?: string[];
}

export interface Session {
  id: string;
  revision: number;
  /**
   * Sequence of the newest event in the log when this session was read.
   *
   * Distinct from `revision`, and the distinction matters: a revision counts
   * *content* changes, a sequence counts *events*. A client joining, a
   * message, or a `build_changed` moves one and not the other. Using the
   * revision as an event watermark therefore compares two different
   * counters, and the guard sits below the sequence it is checked against.
   */
  lastEventSeq: number;
  /**
   * Fingerprint of the web bundle the server is serving right now.
   *
   * The page records it on load and compares it to every `build_changed`,
   * which is what makes live reload survive a server restart. A sequence
   * number cannot: the log restarts at 1, so a page holding a watermark from
   * the previous run ignores every event of the new one, and a page holding
   * none reloads on the replayed backlog for ever. The build means the same
   * thing on both sides of a restart.
   */
  webBuild: string;
  document: CadpoleDocument;
  selection: Selection;
  messages: Message[];
  clients: number;
  /**
   * The Listen switch: whether what the user types is passed to an attached
   * assistant. Absent where there is no session server.
   */
  assistantListening?: boolean;
}

export type EventKind =
  | "script_changed"
  | "params_changed"
  | "selection_changed"
  | "message"
  /** A reply in progress grew. Not in the event log: its `seq` is 0. */
  | "reply_stream"
  | "client_joined"
  | "client_left"
  | "eval_finished"
  | "build_changed"
  /** A front end crashed and said so (`ReportClientFailure`). */
  | "client_failure"
  /** A solve too long for one evaluation started as a job. */
  | "solve_started"
  /** A solve job ended, and the server evaluated the document again. */
  | "solve_finished";

export interface CadpoleEvent {
  seq: number;
  at: string;
  kind: EventKind | string;
  data: unknown;
}

export interface ApiParam {
  name: string;
  type: string;
  doc: string;
  required: boolean;
  default_value: string;
}

export interface ApiFunction {
  name: string;
  doc: string;
  params: ApiParam[];
  returns: string;
  on_type: string;
  /** A value to read (`math.pi`, `XY`), not a function to call. */
  constant?: boolean;
}

export interface ApiType {
  name: string;
  doc: string;
  methods: ApiFunction[];
  attrs: ApiParam[];
}

export interface ApiDoc {
  functions: ApiFunction[];
  types: ApiType[];
}
