// Helpers for cadpole.v1.EntityRef values: stable keys, equality, toggling and
// resolving refs against the evaluated model.

import type { Body, EntityRef, Feature, Model, TopoEdge, TopoFace } from "./types.js";

export function refKey(r: EntityRef): string {
  return `${r.kind}|${r.feature_id}|${r.body_id ?? ""}|${r.topo_name ?? ""}`;
}

export function refsEqual(a: EntityRef | null | undefined, b: EntityRef | null | undefined): boolean {
  if (!a || !b) return a === b || (!a && !b);
  return refKey(a) === refKey(b);
}

export function refListsEqual(a: EntityRef[], b: EntityRef[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i];
    const y = b[i];
    if (!x || !y || !refsEqual(x, y)) return false;
  }
  return true;
}

/** Shift-click semantics: add if absent, remove if present. */
export function toggleRef(list: EntityRef[], r: EntityRef): EntityRef[] {
  const k = refKey(r);
  const out = list.filter((x) => refKey(x) !== k);
  return out.length === list.length ? [...list, r] : out;
}

export function faceRef(body: Body, face: TopoFace): EntityRef {
  return { feature_id: body.feature_id, body_id: body.id, kind: "face", topo_name: face.name };
}

export function edgeRef(body: Body, edge: TopoEdge): EntityRef {
  return { feature_id: body.feature_id, body_id: body.id, kind: "edge", topo_name: edge.name };
}

export function bodyRef(body: Body): EntityRef {
  return { feature_id: body.feature_id, body_id: body.id, kind: "body" };
}

export function featureRef(f: Feature): EntityRef {
  return { feature_id: f.id, kind: "feature" };
}

export interface Resolved {
  ref: EntityRef;
  feature: Feature | null;
  body: Body | null;
  face: TopoFace | null;
  edge: TopoEdge | null;
}

export function findBody(model: Model, ref: EntityRef): Body | null {
  if (ref.body_id) {
    const b = model.bodies.find((x) => x.id === ref.body_id);
    if (b) return b;
  }
  return null;
}

export function resolveRef(model: Model | null, ref: EntityRef): Resolved {
  const out: Resolved = { ref, feature: null, body: null, face: null, edge: null };
  if (!model) return out;
  out.feature = model.features.find((f) => f.id === ref.feature_id) ?? null;
  out.body = findBody(model, ref);
  if (out.body && ref.topo_name) {
    if (ref.kind === "face") out.face = out.body.topology.faces.find((f) => f.name === ref.topo_name) ?? null;
    if (ref.kind === "edge") out.edge = out.body.topology.edges.find((e) => e.name === ref.topo_name) ?? null;
  }
  return out;
}

/** Bodies a ref covers in the viewport: the body itself, or a feature's outputs. */
export function bodiesForRef(model: Model, ref: EntityRef): Body[] {
  if (ref.kind === "body" || ref.kind === "face" || ref.kind === "edge" || ref.kind === "vertex") {
    const b = findBody(model, ref);
    return b ? [b] : [];
  }
  if (ref.kind === "feature" || ref.kind === "sketch") {
    const f = model.features.find((x) => x.id === ref.feature_id);
    const ids = new Set(f ? f.outputs : []);
    const out = model.bodies.filter((b) => ids.has(b.id) || b.feature_id === ref.feature_id);
    return out;
  }
  return [];
}

export function refLabel(model: Model | null, ref: EntityRef): string {
  switch (ref.kind) {
    case "face":
    case "edge":
    case "vertex":
      return ref.topo_name ?? ref.kind;
    case "body": {
      const b = model ? findBody(model, ref) : null;
      return b?.name || ref.body_id || "body";
    }
    case "param":
      return `param ${ref.topo_name ?? ""}`;
    default: {
      const f = model?.features.find((x) => x.id === ref.feature_id);
      return f?.name || ref.feature_id || ref.kind;
    }
  }
}
