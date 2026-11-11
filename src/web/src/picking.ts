// Picking: faces via three's Raycaster (triangle -> face id from the engine's
// mesh_face_ids), edges via a screen-space distance test against the edge
// polylines (occlusion-checked against the face hit).
//
// Allocation-free per call: scratch objects are reused, and `PickResult.point`
// is a shared Vector3 that is overwritten by the next pick().

import { Matrix4, Raycaster, Vector2, Vector3, type Camera, type Intersection, type Mesh } from "three/webgpu";
import { keeps, type CutPlane } from "./section.js";
import type { Body } from "./types.js";

export interface Pickable {
  body: Body;
  mesh: Mesh;
  /** One topology face index per triangle. */
  faceIds: Uint32Array;
  /** Edge polyline segments as world-space endpoint pairs (6 floats each). */
  segments: Float32Array;
  /** Topology edge index per segment. */
  segmentEdge: Uint32Array;
}

export interface PickHit {
  body: Body;
  kind: "face" | "edge";
  /** Index into body.topology.faces / .edges. */
  index: number;
  /** Distance from the camera to the picked point. */
  distance: number;
}

export interface PickResult {
  face: PickHit | null;
  /** Nearest visible edge within `loosePx`, or null. */
  edge: PickHit | null;
  /** Screen distance to `edge` in CSS px (Infinity when none). */
  edgePx: number;
  /** A section's cut face under the cursor, in front of any face, or null. */
  cut: { body: Body; distance: number } | null;
  /** World point under the cursor (face hit, else point on the edge), or null. */
  point: Vector3 | null;
}

export class Picker {
  private readonly raycaster = new Raycaster();
  private readonly ndc = new Vector2();
  private readonly viewProj = new Matrix4();
  private readonly hits: Intersection[] = [];
  private readonly meshes: Mesh[] = [];
  private readonly camPos = new Vector3();
  private readonly edgePoint = new Vector3();
  private readonly point = new Vector3();
  private readonly result: PickResult = { face: null, edge: null, edgePx: Infinity, cut: null, point: null };
  private readonly capHits: Intersection[] = [];

  /**
   * @param px,py pointer position in CSS pixels relative to the canvas
   * @param w,h canvas size in CSS pixels
   * @param loosePx maximum screen distance for edge candidates
   */
  pick(
    camera: Camera,
    pickables: readonly Pickable[],
    px: number,
    py: number,
    w: number,
    h: number,
    loosePx = 22,
    section: { planes: readonly CutPlane[]; caps: readonly { mesh: Mesh; bodies: readonly Body[] }[] } = { planes: [], caps: [] },
  ): PickResult {
    const r = this.result;
    r.face = null;
    r.edge = null;
    r.edgePx = Infinity;
    r.cut = null;
    r.point = null;
    if (w <= 0 || h <= 0) return r;
    this.ndc.set((px / w) * 2 - 1, -(py / h) * 2 + 1);
    camera.updateMatrixWorld();
    this.camPos.setFromMatrixPosition(camera.matrixWorld);
    // What a section removed is not there to pick: the GPU does not
    // draw it, so a click goes through to whatever it does draw.
    const planes = section.planes;
    const kept = (p: Vector3, eps = 0): boolean => {
      for (const c of planes) if (!keeps(c, p.x, p.y, p.z, eps)) return false;
      return true;
    };

    // Faces.
    this.raycaster.setFromCamera(this.ndc, camera);
    this.meshes.length = 0;
    for (const p of pickables) if (p.mesh.visible) this.meshes.push(p.mesh);
    this.hits.length = 0;
    this.raycaster.intersectObjects(this.meshes, false, this.hits);
    const hit = planes.length === 0 ? this.hits[0] : this.hits.find((x) => kept(x.point));
    // A cut face in front of the first face left standing selects the body
    // it cuts. A cap lies in its own plane, so it is tested with a margin.
    if (section.caps.length > 0) {
      this.capHits.length = 0;
      this.raycaster.intersectObjects(
        section.caps.map((c) => c.mesh),
        false,
        this.capHits,
      );
      const scale = Math.max(1, this.camPos.length()) * 1e-6;
      const capHit = this.capHits.find((x) => kept(x.point, scale + 1e-3));
      if (capHit && capHit.faceIndex !== undefined && capHit.faceIndex !== null && (!hit || capHit.distance < hit.distance)) {
        const c = section.caps.find((x) => x.mesh === capHit.object);
        const body = c?.bodies[capHit.faceIndex];
        if (body) {
          r.cut = { body, distance: capHit.distance };
          r.point = this.point.copy(capHit.point);
        }
      }
    }
    if (!r.cut && hit && hit.faceIndex !== undefined && hit.faceIndex !== null) {
      const p = pickables.find((x) => x.mesh === hit.object);
      if (p) {
        const fi = p.faceIds[hit.faceIndex];
        if (fi !== undefined) {
          r.face = { body: p.body, kind: "face", index: fi, distance: hit.distance };
          r.point = this.point.copy(hit.point);
        }
      }
    }

    // Edges: screen-space distance to each segment.
    this.viewProj.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    const m = this.viewProj.elements;
    let bestD2 = loosePx * loosePx;
    let bestBody: Pickable | null = null;
    let bestSeg = -1;
    let bestT = 0;
    for (const p of pickables) {
      if (!p.mesh.visible) continue;
      const s = p.segments;
      const n = s.length / 6;
      for (let i = 0; i < n; i++) {
        const o = i * 6;
        const ax = s[o] as number;
        const ay = s[o + 1] as number;
        const az = s[o + 2] as number;
        const bx = s[o + 3] as number;
        const by = s[o + 4] as number;
        const bz = s[o + 5] as number;
        const aw = m[3]! * ax + m[7]! * ay + m[11]! * az + m[15]!;
        const bw = m[3]! * bx + m[7]! * by + m[11]! * bz + m[15]!;
        if (aw <= 1e-6 || bw <= 1e-6) continue; // behind the near plane
        const asx = ((m[0]! * ax + m[4]! * ay + m[8]! * az + m[12]!) / aw + 1) * 0.5 * w;
        const asy = (1 - (m[1]! * ax + m[5]! * ay + m[9]! * az + m[13]!) / aw) * 0.5 * h;
        const bsx = ((m[0]! * bx + m[4]! * by + m[8]! * bz + m[12]!) / bw + 1) * 0.5 * w;
        const bsy = (1 - (m[1]! * bx + m[5]! * by + m[9]! * bz + m[13]!) / bw) * 0.5 * h;
        const dx = bsx - asx;
        const dy = bsy - asy;
        const len2 = dx * dx + dy * dy;
        let t = len2 > 0 ? ((px - asx) * dx + (py - asy) * dy) / len2 : 0;
        t = t < 0 ? 0 : t > 1 ? 1 : t;
        const cx = asx + dx * t - px;
        const cy = asy + dy * t - py;
        const d2 = cx * cx + cy * cy;
        if (d2 < bestD2 && (planes.length === 0 || kept(this.edgePoint.set(ax + (bx - ax) * t, ay + (by - ay) * t, az + (bz - az) * t)))) {
          bestD2 = d2;
          bestBody = p;
          bestSeg = i;
          bestT = t;
        }
      }
    }

    if (bestBody && bestSeg >= 0) {
      const s = bestBody.segments;
      const o = bestSeg * 6;
      this.edgePoint.set(
        (s[o] as number) + ((s[o + 3] as number) - (s[o] as number)) * bestT,
        (s[o + 1] as number) + ((s[o + 4] as number) - (s[o + 1] as number)) * bestT,
        (s[o + 2] as number) + ((s[o + 5] as number) - (s[o + 2] as number)) * bestT,
      );
      const edgeDist = this.edgePoint.distanceTo(this.camPos);
      // Accept the edge unless a face is clearly in front of it.
      const front = r.cut ?? r.face;
      const tol = front ? front.distance * 0.004 + 0.05 : 0;
      if (!front || edgeDist <= front.distance + tol) {
        const ei = bestBody.segmentEdge[bestSeg];
        if (ei !== undefined) {
          r.edge = { body: bestBody.body, kind: "edge", index: ei, distance: edgeDist };
          r.edgePx = Math.sqrt(bestD2);
          if (!r.point) r.point = this.point.copy(this.edgePoint);
        }
      }
    }
    return r;
  }
}
