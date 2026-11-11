// A published model (`.cadpole`) as a mesh-only `.cpmesh` for the page's own 3D.
//
// A published model carries every body's topology, tens of megabytes for an
// assembly; the page only draws, so it gets the bodies and quantized meshes.
// Layout, zlib-deflated: "CPM1", u32 header bytes, the header as JSON (padded
// to 4), then per mesh: positions u16 x 3 (quantized to the mesh's box),
// normals i8 x 3, indices u16 or u32, each block padded to 4 bytes. The
// reader is `decodeMeshes` in the web package's src/site/site3d.ts.
import { deflateSync } from "node:zlib";

// The web app's own decoder, as the build transpiled it.
import { decodeBaked } from "../../src/web/src/baked.js";

const pad = (n) => (4 - (n % 4)) % 4;
const r3 = (x) => Math.round(x * 1000) / 1000;

/** @param {Uint8Array} bytes a published model. @returns {Promise<Buffer>} the `.cpmesh`. */
export async function slim(bytes) {
  const b = await decodeBaked(new Uint8Array(bytes));
  // A body on a layer the script declared hidden is not drawn: the app's
  // viewer hides it through its layers, and a print-orientation copy drawn
  // here would sit on top of the model.
  const hidden = new Set((b.model.layers ?? []).filter((l) => l.visible === false).map((l) => l.name));
  const shown = (x) => x.visible && !hidden.has(x.layer ?? "");
  const used = new Set(b.model.bodies.filter(shown).map((x) => x.mesh_index));
  const remap = new Map(), parts = [], meshes = [];
  const push = (a) => {
    parts.push(Buffer.from(a.buffer, a.byteOffset, a.byteLength));
    if (pad(a.byteLength)) parts.push(Buffer.alloc(pad(a.byteLength)));
  };
  b.meshes.forEach((m, i) => {
    if (!used.has(i) || m.indices.length === 0) return;
    const nv = m.positions.length / 3;
    const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
    for (let k = 0; k < m.positions.length; k++) { const v = m.positions[k]; if (v < lo[k % 3]) lo[k % 3] = v; if (v > hi[k % 3]) hi[k % 3] = v; }
    const q = new Uint16Array(nv * 3);
    for (let k = 0; k < q.length; k++) { const s = hi[k % 3] - lo[k % 3]; q[k] = s > 0 ? Math.round(((m.positions[k] - lo[k % 3]) / s) * 65535) : 0; }
    const n = new Int8Array(nv * 3);
    for (let k = 0; k < n.length; k++) n[k] = Math.max(-127, Math.min(127, Math.round(m.normals[k] * 127)));
    const wide = nv > 65535;
    const idx = wide ? new Uint32Array(m.indices) : new Uint16Array(m.indices);
    remap.set(i, meshes.length);
    meshes.push({ nv, ni: idx.length, wide, lo, hi });
    push(q); push(n); push(idx);
  });
  const bodies = b.model.bodies.filter((x) => shown(x) && remap.has(x.mesh_index)).map((x) => ({
    name: x.name,
    mesh_index: remap.get(x.mesh_index),
    visible: true,
    material: x.material && { color: x.material.color?.map(r3) ?? null, roughness: x.material.roughness, metallic: x.material.metallic, opacity: x.material.opacity },
    transform: x.transform,
  }));
  let head = Buffer.from(JSON.stringify({ name: b.name, tolerance: b.tolerance, bodies, meshes }), "utf8");
  if (pad(head.length)) head = Buffer.concat([head, Buffer.alloc(pad(head.length), 0x20)]);
  const pre = Buffer.alloc(8);
  pre.write("CPM1", 0, "latin1");
  pre.writeUInt32LE(head.length, 4);
  return deflateSync(Buffer.concat([pre, head, ...parts]), { level: 9 });
}
