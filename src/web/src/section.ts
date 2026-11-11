// Section planes in the viewer: the model cut by up to one
// horizontal and one vertical plane, client-side, with no engine call.
//
// The spelling is `cadpole render --section`'s, extended to a pair:
//
//   z=2400          remove everything above z = 2400 mm (a plan cut)
//   -z=2400         remove everything below it instead (looking up)
//   x=1200, y=..    a vertical cut, removing the +x (+y) side
//   -x=1200         ... or the -x (-y) side ("flip")
//   z=2400,-y=3000  both: one horizontal and one vertical plane
//
// Coordinates are millimetres, like every number the model carries; the
// panel writes and reads them in the document's display unit.
//
// Everything here is a function of its inputs -- parse, format, slice a
// mesh, close the cut into loops and fill them -- so it runs under
// `//src/web:unit_test` without a browser. The viewer does the drawing:
// clipping is the GPU's (three's `ClippingGroup`), the caps are the
// triangles `capTriangles` returns.

// The pure module, not `three/webgpu`: Earcut and nothing else, so this file
// runs under node and does not pull a second copy of three into the bundle.
import { Earcut } from "three/src/extras/Earcut.js";

/** One cutting plane. `axis` 0, 1, 2 is x, y, z. */
export interface CutPlane {
  axis: 0 | 1 | 2;
  /** Where the plane is, mm. */
  at: number;
  /** The side removed: `true` the positive side (`z=40`), `false` the negative (`-z=40`). */
  removePositive: boolean;
}

/** The viewer's section: at most one horizontal and one vertical plane. */
export interface SectionState {
  horizontal: CutPlane | null;
  vertical: CutPlane | null;
}

export const NO_SECTION: SectionState = { horizontal: null, vertical: null };

const AXES = ["x", "y", "z"] as const;

/** The active planes, horizontal first. */
export function planesOf(s: SectionState): CutPlane[] {
  const out: CutPlane[] = [];
  if (s.horizontal) out.push(s.horizontal);
  if (s.vertical) out.push(s.vertical);
  return out;
}

export function isSectioned(s: SectionState): boolean {
  return s.horizontal !== null || s.vertical !== null;
}

/**
 * `z=40`, `-x=22`, `z=2400,-y=3000` as a section. Empty is no section.
 * Throws with a message for anything else, as `cadpole render` would.
 */
export function parseSection(spec: string): SectionState {
  const out: SectionState = { horizontal: null, vertical: null };
  for (const raw of spec.split(",").map((p) => p.trim()).filter((p) => p.length > 0)) {
    const neg = raw.startsWith("-");
    const rest = neg ? raw.slice(1) : raw;
    const eq = rest.indexOf("=");
    if (eq < 0) throw new Error(`section ${JSON.stringify(raw)}: expected <axis>=<value>, as in z=40 or -x=22`);
    const name = rest.slice(0, eq).trim().toLowerCase();
    const axis = AXES.indexOf(name as (typeof AXES)[number]);
    if (axis < 0) throw new Error(`section ${JSON.stringify(raw)}: the axis must be x, y or z`);
    const valueText = rest.slice(eq + 1).trim();
    const at = Number(valueText);
    if (valueText === "" || !Number.isFinite(at)) throw new Error(`section ${JSON.stringify(raw)}: ${JSON.stringify(valueText)} is not a number`);
    const plane: CutPlane = { axis: axis as 0 | 1 | 2, at, removePositive: !neg };
    if (axis === 2) {
      if (out.horizontal) throw new Error("a section has at most one horizontal (z=) plane");
      out.horizontal = plane;
    } else {
      if (out.vertical) throw new Error("a section has at most one vertical (x= or y=) plane");
      out.vertical = plane;
    }
  }
  return out;
}

/** A section in the same spelling `parseSection` reads; "" for none. */
export function formatSection(s: SectionState): string {
  return planesOf(s)
    .map((p) => `${p.removePositive ? "" : "-"}${AXES[p.axis]}=${roundMm(p.at)}`)
    .join(",");
}

/** A micron is the finest the spelling carries: 304.8 rather than 304.80000000000001. */
function roundMm(v: number): number {
  return Math.round(v * 1000) / 1000;
}

/**
 * Whether a point is kept by a plane: on the plane counts as kept, which is
 * also what the GPU's clip does (it discards a strictly negative distance).
 */
export function keeps(p: CutPlane, x: number, y: number, z: number, eps = 0): boolean {
  const c = p.axis === 0 ? x : p.axis === 1 ? y : z;
  return p.removePositive ? c <= p.at + eps : c >= p.at - eps;
}

/**
 * The cut of one mesh: every triangle that straddles the plane contributes
 * one segment, in the plane's own 2D coordinates (`u`, `v` are the two other
 * axes in order: y,z for an x cut, x,z for y, x,y for z).
 *
 * Positions are the mesh's own; `m` (column-major 4x4, three's
 * `Matrix4.elements`) moves them where the body is drawn, or null for the
 * identity. The returned array is four floats a segment: u0, v0, u1, v1.
 *
 * An edge's crossing point is computed from its two endpoints in a fixed
 * order -- lexicographic by position, not by vertex index -- so the two
 * triangles sharing an edge produce bit-identical points even when the mesh
 * duplicates its vertices along a crease, and the loops close by exact match.
 */
export function sliceMesh(positions: Float32Array, indices: Uint32Array, m: ArrayLike<number> | null, plane: CutPlane): Float32Array {
  const n = positions.length / 3;
  // World positions only when the body is moved (an instance).
  let P: ArrayLike<number> = positions;
  if (m) {
    const w = new Float64Array(positions.length);
    for (let i = 0; i < n; i++) {
      const x = positions[i * 3] as number;
      const y = positions[i * 3 + 1] as number;
      const z = positions[i * 3 + 2] as number;
      w[i * 3] = (m[0] as number) * x + (m[4] as number) * y + (m[8] as number) * z + (m[12] as number);
      w[i * 3 + 1] = (m[1] as number) * x + (m[5] as number) * y + (m[9] as number) * z + (m[13] as number);
      w[i * 3 + 2] = (m[2] as number) * x + (m[6] as number) * y + (m[10] as number) * z + (m[14] as number);
    }
    P = w;
  }
  const a = plane.axis;
  const ua = a === 0 ? 1 : 0;
  const va = a === 2 ? 1 : 2;
  const at = plane.at;
  const out: number[] = [];
  const pt: number[] = [0, 0];
  // The crossing of edge (i, j), written into `pt`: i and j ordered by
  // position so both triangles of a shared edge compute the same bits.
  const cross = (i: number, j: number): void => {
    let p = i;
    let q = j;
    if (before(P, j, i)) {
      p = j;
      q = i;
    }
    const dp = (P[p * 3 + a] as number) - at;
    const dq = (P[q * 3 + a] as number) - at;
    const t = dp / (dp - dq);
    pt[0] = (P[p * 3 + ua] as number) + ((P[q * 3 + ua] as number) - (P[p * 3 + ua] as number)) * t;
    pt[1] = (P[p * 3 + va] as number) + ((P[q * 3 + va] as number) - (P[p * 3 + va] as number)) * t;
  };
  for (let t = 0; t + 2 < indices.length; t += 3) {
    const i0 = indices[t] as number;
    const i1 = indices[t + 1] as number;
    const i2 = indices[t + 2] as number;
    // A vertex exactly on the plane counts as below it: a consistent rule,
    // so a face lying in the plane makes no segment and its neighbours'
    // segments still meet.
    const s0 = (P[i0 * 3 + a] as number) > at;
    const s1 = (P[i1 * 3 + a] as number) > at;
    const s2 = (P[i2 * 3 + a] as number) > at;
    if (s0 === s1 && s1 === s2) continue;
    // The odd vertex out, and the two edges from it that cross.
    let o: number, b: number, c: number;
    if (s0 !== s1 && s0 !== s2) [o, b, c] = [i0, i1, i2];
    else if (s1 !== s0 && s1 !== s2) [o, b, c] = [i1, i2, i0];
    else [o, b, c] = [i2, i0, i1];
    cross(o, b);
    const u0 = pt[0] as number;
    const v0 = pt[1] as number;
    cross(o, c);
    out.push(u0, v0, pt[0] as number, pt[1] as number);
  }
  return new Float32Array(out);
}

/** Whether vertex `i` sorts before vertex `j` by (x, y, z). */
function before(P: ArrayLike<number>, i: number, j: number): boolean {
  for (let k = 0; k < 3; k++) {
    const a = P[i * 3 + k] as number;
    const b = P[j * 3 + k] as number;
    if (a !== b) return a < b;
  }
  return false;
}

/** What a cut closes into: loops (filled) and chains that do not close (outlined only). */
export interface CutLoops {
  loops: Float32Array[];
  open: Float32Array[];
}

/**
 * Join a cut's segments (`sliceMesh`'s output) into closed loops by exact
 * endpoint match. A watertight body's cut is all loops; anything else -- a
 * terrain sheet, a scan, a mesh with a crack -- leaves chains, which are
 * outlined but never filled, so an open surface cannot flood the cap.
 */
export function joinLoops(segs: Float32Array): CutLoops {
  const nSeg = segs.length / 4;
  const key = (u: number, v: number): string => `${u},${v}`;
  const ends = new Map<string, number[]>();
  for (let s = 0; s < nSeg; s++) {
    for (const e of [0, 1]) {
      const k = key(segs[s * 4 + e * 2] as number, segs[s * 4 + e * 2 + 1] as number);
      const list = ends.get(k);
      if (list) list.push(s * 2 + e);
      else ends.set(k, [s * 2 + e]);
    }
  }
  const used = new Uint8Array(nSeg);
  const loops: Float32Array[] = [];
  const open: Float32Array[] = [];
  // The unused segment end at the same point as `end`, other than `end` itself.
  const next = (endpoint: number): number => {
    const s = endpoint >> 1;
    const e = endpoint & 1;
    const list = ends.get(key(segs[s * 4 + e * 2] as number, segs[s * 4 + e * 2 + 1] as number)) ?? [];
    for (const o of list) if (o >> 1 !== s && !used[o >> 1]) return o;
    return -1;
  };
  for (let s0 = 0; s0 < nSeg; s0++) {
    if (used[s0]) continue;
    used[s0] = 1;
    // Walk forward from the segment's far end, then backward from its near end.
    const fwd: number[] = [segs[s0 * 4] as number, segs[s0 * 4 + 1] as number, segs[s0 * 4 + 2] as number, segs[s0 * 4 + 3] as number];
    let at = s0 * 2 + 1;
    let closed = false;
    for (;;) {
      const o = next(at);
      if (o < 0) break;
      const s = o >> 1;
      used[s] = 1;
      const far = (o & 1) ^ 1;
      const fu = segs[s * 4 + far * 2] as number;
      const fv = segs[s * 4 + far * 2 + 1] as number;
      if (fu === fwd[0] && fv === fwd[1]) {
        closed = true;
        break;
      }
      fwd.push(fu, fv);
      at = s * 2 + far;
    }
    if (closed) {
      if (fwd.length >= 6) loops.push(new Float32Array(fwd));
      continue;
    }
    const back: number[] = [];
    at = s0 * 2;
    for (;;) {
      const o = next(at);
      if (o < 0) break;
      const s = o >> 1;
      used[s] = 1;
      const far = (o & 1) ^ 1;
      back.push(segs[s * 4 + far * 2] as number, segs[s * 4 + far * 2 + 1] as number);
      at = s * 2 + far;
    }
    const chain: number[] = [];
    for (let i = back.length - 2; i >= 0; i -= 2) chain.push(back[i] as number, back[i + 1] as number);
    open.push(new Float32Array([...chain, ...fwd]));
  }
  return { loops, open };
}

/** Twice the signed area of a loop (u, v pairs): positive counter-clockwise. */
export function signedArea2(loop: Float32Array): number {
  let a = 0;
  const n = loop.length / 2;
  for (let i = 0, j = n - 1; i < n; j = i++) {
    a += (loop[j * 2] as number) * (loop[i * 2 + 1] as number) - (loop[i * 2] as number) * (loop[j * 2 + 1] as number);
  }
  return a;
}

/** Whether (u, v) is inside a loop (even-odd). */
export function inside(loop: Float32Array, u: number, v: number): boolean {
  let c = false;
  const n = loop.length / 2;
  for (let i = 0, j = n - 1; i < n; j = i++) {
    const ui = loop[i * 2] as number;
    const vi = loop[i * 2 + 1] as number;
    const uj = loop[j * 2] as number;
    const vj = loop[j * 2 + 1] as number;
    if (vi > v !== vj > v && u < ((uj - ui) * (v - vi)) / (vj - vi) + ui) c = !c;
  }
  return c;
}

/**
 * Fill a body's closed loops: each loop's nesting depth decides what it is
 * (even: an outline of material, odd: a hole in the one around it), and
 * each outline is triangulated with its holes. Returns u, v triples, two
 * floats a vertex, three vertices a triangle.
 */
export function capTriangles(loops: readonly Float32Array[]): Float32Array {
  const usable = loops.filter((l) => l.length >= 6 && Math.abs(signedArea2(l)) > 0);
  const depth = usable.map((l, i) => {
    const u = l[0] as number;
    const v = l[1] as number;
    let d = 0;
    usable.forEach((o, j) => {
      if (j !== i && inside(o, u, v)) d++;
    });
    return d;
  });
  const area = usable.map((l) => Math.abs(signedArea2(l)));
  const out: number[] = [];
  usable.forEach((outer, i) => {
    if ((depth[i] as number) % 2 !== 0) return;
    // Holes: the loops one level deeper whose smallest enclosing outline is this one.
    const holes: Float32Array[] = [];
    usable.forEach((hole, j) => {
      if (depth[j] !== (depth[i] as number) + 1) return;
      const u = hole[0] as number;
      const v = hole[1] as number;
      if (!inside(outer, u, v)) return;
      // The innermost outline at this depth that holds it.
      let best = i;
      usable.forEach((o, k) => {
        if (k !== i && depth[k] === depth[i] && (area[k] as number) < (area[best] as number) && inside(o, u, v)) best = k;
      });
      if (best === i) holes.push(hole);
    });
    // Earcut takes the outline and its holes as one flat list, with where
    // each hole starts; it does not care which way any of them winds.
    const flat: number[] = Array.from(outer);
    const holeStarts: number[] = [];
    for (const hole of holes) {
      holeStarts.push(flat.length / 2);
      for (const x of hole) flat.push(x);
    }
    for (const k of Earcut.triangulate(flat, holeStarts, 2)) out.push(flat[k * 2] as number, flat[k * 2 + 1] as number);
  });
  return new Float32Array(out);
}

/** A 2D point in a plane's (u, v) back to model x, y, z. */
export function lift(plane: CutPlane, u: number, v: number, out: [number, number, number]): [number, number, number] {
  if (plane.axis === 0) {
    out[0] = plane.at;
    out[1] = u;
    out[2] = v;
  } else if (plane.axis === 1) {
    out[0] = u;
    out[1] = plane.at;
    out[2] = v;
  } else {
    out[0] = u;
    out[1] = v;
    out[2] = plane.at;
  }
  return out;
}

/** The pale grey a cut face's colour moves toward, and how far it goes. */
export const CUT_PALE = 0.88;
export const CUT_TOWARD = 0.25;
/** The fill luma under which the outline is lighter than the face, not darker. */
const CUT_LINE_FLIP = 0.32;
/** A darker outline: the face's colour times this. */
const CUT_LINE_DARKEN = 0.58;
/** A lighter outline: the face's colour this far toward white. */
const CUT_LINE_LIGHTEN = 0.3;

/**
 * What a cut through a body of colour `rgb` is drawn in: `fill` for the cut
 * face, `line` for its outline. All three are sRGB-encoded, each channel
 * 0..1, and are what the viewer draws before its exposure and tone curve.
 *
 * The face is the body's own colour moved part of the way toward a pale grey
 * and drawn unlit: lighter than the body for every colour but the palest,
 * which have no room to lighten and go a shade greyer instead. So it is told
 * from the lit surface beside it by being flat and a shade off, and from the
 * next body's by hue. The outline is a shade of the same colour -- darker, or
 * lighter where the face is too dark for a darker line to show -- so two
 * bodies of one colour still part at the cut.
 */
export function cutColors(rgb: readonly [number, number, number]): { fill: [number, number, number]; line: [number, number, number] } {
  const fill = rgb.map((c) => c + (CUT_PALE - c) * CUT_TOWARD) as [number, number, number];
  const luma = 0.2126 * fill[0] + 0.7152 * fill[1] + 0.0722 * fill[2];
  const line = fill.map((c) => (luma >= CUT_LINE_FLIP ? c * CUT_LINE_DARKEN : c + (1 - c) * CUT_LINE_LIGHTEN)) as [number, number, number];
  return { fill, line };
}
