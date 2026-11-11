// Reading a screenshot's pixels in a test.

import { inflateSync } from "node:zlib";

/**
 * A PNG as RGBA pixels: as much of the format as a Playwright screenshot
 * uses (8-bit RGB or RGBA, not interlaced), so a test can ask what colour
 * the viewport drew.
 */
export function decodePng(png: Buffer): { width: number; height: number; rgba: Uint8Array } {
  let off = 8;
  let width = 0;
  let height = 0;
  let depth = 0;
  let type = 0;
  const idat: Buffer[] = [];
  while (off + 8 <= png.length) {
    const len = png.readUInt32BE(off);
    const kind = png.toString("ascii", off + 4, off + 8);
    const data = png.subarray(off + 8, off + 8 + len);
    if (kind === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      depth = data[8] ?? 0;
      type = data[9] ?? 0;
    } else if (kind === "IDAT") idat.push(data);
    else if (kind === "IEND") break;
    off += 12 + len;
  }
  if (depth !== 8 || (type !== 6 && type !== 2)) throw new Error(`a ${depth}-bit PNG of colour type ${type}`);
  const bpp = type === 6 ? 4 : 3;
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * bpp;
  const rgba = new Uint8Array(width * height * 4);
  let prev = new Uint8Array(stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)] ?? 0;
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const cur = new Uint8Array(stride);
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? (cur[x - bpp] ?? 0) : 0;
      const b = prev[x] ?? 0;
      const c = x >= bpp ? (prev[x - bpp] ?? 0) : 0;
      let v = line[x] ?? 0;
      if (filter === 1) v += a;
      else if (filter === 2) v += b;
      else if (filter === 3) v += (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a);
        const pb = Math.abs(p - b);
        const pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      cur[x] = v & 255;
    }
    for (let x = 0; x < width; x++) {
      for (let k = 0; k < 3; k++) rgba[(y * width + x) * 4 + k] = cur[x * bpp + k] ?? 0;
      rgba[(y * width + x) * 4 + 3] = bpp === 4 ? (cur[x * bpp + 3] ?? 255) : 255;
    }
    prev = cur;
  }
  return { width, height, rgba };
}
