// Annotations on a render, given as points of the model.
//
// A render written with `--camera-json` comes with the camera that drew it
// (docs/views.md, "The camera sidecar"). Everything here is placed by
// projecting model points, in millimetres, through that camera, so an arrow
// between two points of the model lands on the picture where those points
// are, and stays there when the camera, the size or the model changes.
//
// The output is SVG: an overlay the size of the picture, or one standalone
// file with the picture embedded under it.

/** Colours the annotations use unless one names its own. */
export const PALETTE = {
  accent: "#F0531C",
  accentText: "#B93A0E",
  ink: "#1d2733",
  good: "#1FA35B",
  bad: "#D22B2B",
};

const KINDS = ["arrow", "label", "dot", "dim", "note", "outline"];

/**
 * The projection of one frame of a camera sidecar.
 * @param {{width: number, height: number, frames: Array<object>}} sidecar
 * @param {number} [index] which frame (an animation writes one per frame)
 */
export function cameraOf(sidecar, index = 0) {
  if (!sidecar || !Array.isArray(sidecar.frames) || !sidecar.frames[index]) {
    throw new Error(`camera sidecar has no frame ${index}`);
  }
  const f = sidecar.frames[index];
  if (!Array.isArray(f.view) || f.view.length !== 16 || !Array.isArray(f.projection) || f.projection.length !== 16) {
    throw new Error("camera sidecar frame lacks its 4x4 view and projection");
  }
  return { width: sidecar.width, height: sidecar.height, view: f.view, projection: f.projection, eye: f.eye, target: f.target };
}

function mulPoint(m, p) {
  // Row-major 4x4 on a column vector.
  const out = [0, 0, 0, 0];
  for (let r = 0; r < 4; r++) {
    out[r] = m[4 * r] * p[0] + m[4 * r + 1] * p[1] + m[4 * r + 2] * p[2] + m[4 * r + 3] * p[3];
  }
  return out;
}

/**
 * Where a model point lands on the picture, in pixels from the top-left
 * corner. `behind` is true for a point behind the camera, whose pixel
 * position means nothing.
 * @param {ReturnType<typeof cameraOf>} camera
 * @param {[number, number, number]} point model millimetres
 */
export function project(camera, point) {
  if (!Array.isArray(point) || point.length !== 3 || point.some((v) => typeof v !== "number" || !Number.isFinite(v))) {
    throw new Error(`a model point is [x, y, z] in millimetres, got ${JSON.stringify(point)}`);
  }
  const clip = mulPoint(camera.projection, mulPoint(camera.view, [point[0], point[1], point[2], 1]));
  const w = clip[3];
  const behind = !(w > 0);
  const nx = clip[0] / (w || 1e-12);
  const ny = clip[1] / (w || 1e-12);
  return { x: ((nx + 1) / 2) * camera.width, y: ((1 - ny) / 2) * camera.height, behind };
}

function esc(text) {
  return String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

const n1 = (v) => (Math.round(v * 10) / 10).toString();

/** Text with a white halo, so it reads over a dark part and over a white one. */
function haloText(style, x, y, text, { size, color, anchor = "middle", weight = 600 }) {
  // Keep the whole string on the picture: 0.6 em per character is a safe
  // over-estimate for a sans face.
  const tw = 0.6 * size * String(text).length;
  const pad = style.pad;
  const lo = { start: pad, middle: pad + tw / 2, end: pad + tw }[anchor];
  const hi = { start: style.width - pad - tw, middle: style.width - pad - tw / 2, end: style.width - pad }[anchor];
  const cx = Math.min(Math.max(x, lo), Math.max(lo, hi));
  const cy = Math.min(Math.max(y, size + 4), style.height - 6);
  const a = `x="${n1(cx)}" y="${n1(cy)}" font-size="${n1(size)}" text-anchor="${anchor}" font-weight="${weight}"`;
  return (
    `<text ${a} stroke="#fff" stroke-width="${n1(size * 0.24)}" stroke-linejoin="round" fill="#fff">${esc(text)}</text>` +
    `<text ${a} fill="${color}">${esc(text)}</text>`
  );
}

function need(a, i, key) {
  if (a[key] === undefined || a[key] === null) throw new Error(`annotation ${i} (${a.type}): "${key}" is missing`);
  return a[key];
}

/**
 * Check a list of annotations without drawing it. Returns nothing; throws an
 * Error naming the first annotation that is wrong.
 */
export function validateAnnotations(annotations) {
  if (!Array.isArray(annotations)) throw new Error("annotations is a list");
  annotations.forEach((a, i) => {
    if (!a || typeof a !== "object" || !KINDS.includes(a.type)) {
      throw new Error(`annotation ${i}: "type" is one of ${KINDS.join(", ")}, got ${JSON.stringify(a && a.type)}`);
    }
    const pt = (key) => {
      const p = need(a, i, key);
      if (!Array.isArray(p) || p.length !== 3 || p.some((v) => typeof v !== "number")) {
        throw new Error(`annotation ${i} (${a.type}): "${key}" is a model point [x, y, z], got ${JSON.stringify(p)}`);
      }
    };
    if (a.type === "arrow" || a.type === "dim") {
      pt("from");
      pt("to");
    } else if (a.type === "label") {
      pt("at");
      need(a, i, "text");
    } else if (a.type === "dot") {
      pt("at");
    } else if (a.type === "note") {
      need(a, i, "text");
      if (a.xy !== undefined && (!Array.isArray(a.xy) || a.xy.length !== 2)) {
        throw new Error(`annotation ${i} (note): "xy" is [x, y] as fractions of the picture, got ${JSON.stringify(a.xy)}`);
      }
    } else if (a.type === "outline") {
      const pts = need(a, i, "points");
      if (!Array.isArray(pts) || pts.length < 3) throw new Error(`annotation ${i} (outline): "points" is three or more model points`);
    }
  });
}

/**
 * The annotations as SVG elements, for a picture drawn by `camera`.
 *
 * Kinds, every point a model point in millimetres:
 *   arrow   {from, to, text?, color?}          a thick arrow, the way a part moves
 *   label   {at, text, offset?: [dx, dy], anchor?, out?: r}
 *           text with a leader to a dot on the part. `offset` is in units of
 *           a thousandth of the picture's width (so a layout keeps its
 *           proportions at any size); `out` instead pushes the text straight
 *           out from the middle of the picture by that much.
 *   dot     {at, text?, color?}                a round marker, with a character in it
 *   dim     {from, to, text?, offset?: [dx, dy]}  a dimension line with end ticks
 *   note    {text, xy?: [fx, fy], color?, anchor?}  a caption at a fraction of the picture
 *           (default: the bottom left)
 *   outline {points, color?}                   a translucent patch over an area
 *
 * @returns {{svg: string, warnings: string[]}} the elements (no outer <svg>)
 *   and a warning for each annotation whose point is off the picture
 */
export function overlay(camera, annotations) {
  validateAnnotations(annotations);
  const W = camera.width;
  const H = camera.height;
  const u = W / 1000; // one layout unit
  const style = { width: W, height: H, pad: 12 * u };
  const text = 20 * u;
  const out = [];
  const warnings = [];
  const at = (a, i, key, p) => {
    const q = project(camera, p);
    if (q.behind || q.x < 0 || q.y < 0 || q.x > W || q.y > H) {
      warnings.push(`annotation ${i} (${a.type}): "${key}" ${JSON.stringify(p)} is off the picture`);
    }
    return q;
  };
  annotations.forEach((a, i) => {
    if (a.type === "arrow") {
      const color = a.color || PALETTE.accent;
      const p = at(a, i, "from", a.from);
      const q = at(a, i, "to", a.to);
      const ang = Math.atan2(q.y - p.y, q.x - p.x);
      const head = 23 * u;
      const bx = q.x - head * Math.cos(ang);
      const by = q.y - head * Math.sin(ang);
      const nx = -Math.sin(ang);
      const ny = Math.cos(ang);
      for (const [col, sw] of [["#fff", 15 * u], [color, 8 * u]]) {
        out.push(`<line x1="${n1(p.x)}" y1="${n1(p.y)}" x2="${n1(bx)}" y2="${n1(by)}" stroke="${col}" stroke-width="${n1(sw)}" stroke-linecap="round"/>`);
      }
      const hw = 15 * u;
      out.push(
        `<polygon points="${n1(q.x)},${n1(q.y)} ${n1(bx + nx * hw)},${n1(by + ny * hw)} ${n1(bx - nx * hw)},${n1(by - ny * hw)}" fill="${color}" stroke="#fff" stroke-width="${n1(2.7 * u)}" stroke-linejoin="round"/>`,
      );
      if (a.text) {
        const ty = q.y >= p.y ? p.y - 17 * u : p.y + 33 * u;
        out.push(haloText(style, p.x, ty, a.text, { size: text, color: color === PALETTE.accent ? PALETTE.accentText : color }));
      }
    } else if (a.type === "label") {
      const p = at(a, i, "at", a.at);
      let dx;
      let dy;
      let anchor = a.anchor || "middle";
      if (a.out !== undefined) {
        const vx = p.x - W / 2;
        const vy = p.y - H / 2;
        const l = Math.hypot(vx, vy) || 1;
        dx = (a.out * u * vx) / l;
        dy = (a.out * u * vy) / l + (vy < 0 ? 0 : 20 * u);
        anchor = vx > 40 * u ? "start" : vx < -40 * u ? "end" : "middle";
      } else {
        const off = a.offset || [0, -40];
        dx = off[0] * u;
        dy = off[1] * u;
      }
      const x = p.x + dx;
      const y = p.y + dy;
      const ly = y + (dy < 0 ? 8 * u : -20 * u);
      for (const [col, sw] of [["#fff", 4.7 * u], [PALETTE.ink, 1.7 * u]]) {
        out.push(`<line x1="${n1(p.x)}" y1="${n1(p.y)}" x2="${n1(x)}" y2="${n1(ly)}" stroke="${col}" stroke-width="${n1(sw)}"/>`);
      }
      out.push(`<circle cx="${n1(p.x)}" cy="${n1(p.y)}" r="${n1(4 * u)}" fill="${PALETTE.ink}" stroke="#fff" stroke-width="${n1(1.7 * u)}"/>`);
      out.push(haloText(style, x, y, a.text, { size: text, color: PALETTE.ink, anchor }));
    } else if (a.type === "dot") {
      const p = at(a, i, "at", a.at);
      const color = a.color || PALETTE.good;
      const r = (a.text ? 15 : 10) * u;
      out.push(`<circle cx="${n1(p.x)}" cy="${n1(p.y)}" r="${n1(r)}" fill="${color}" stroke="#fff" stroke-width="${n1(2.7 * u)}"/>`);
      if (a.text) {
        out.push(`<text x="${n1(p.x)}" y="${n1(p.y + 6.5 * u)}" font-size="${n1(18.5 * u)}" text-anchor="middle" font-weight="700" fill="#fff">${esc(a.text)}</text>`);
      }
    } else if (a.type === "dim") {
      const off = a.offset || [0, 0];
      const p0 = at(a, i, "from", a.from);
      const q0 = at(a, i, "to", a.to);
      const p = { x: p0.x + off[0] * u, y: p0.y + off[1] * u };
      const q = { x: q0.x + off[0] * u, y: q0.y + off[1] * u };
      const ang = Math.atan2(q.y - p.y, q.x - p.x) + Math.PI / 2;
      const t = 9 * u;
      for (const [col, sw] of [["#fff", 6 * u], [PALETTE.ink, 2.3 * u]]) {
        out.push(`<line x1="${n1(p.x)}" y1="${n1(p.y)}" x2="${n1(q.x)}" y2="${n1(q.y)}" stroke="${col}" stroke-width="${n1(sw)}"/>`);
        for (const e of [p, q]) {
          out.push(
            `<line x1="${n1(e.x - t * Math.cos(ang))}" y1="${n1(e.y - t * Math.sin(ang))}" x2="${n1(e.x + t * Math.cos(ang))}" y2="${n1(e.y + t * Math.sin(ang))}" stroke="${col}" stroke-width="${n1(sw)}"/>`,
          );
        }
      }
      if (a.text) {
        const vertical = Math.abs(q.x - p.x) < Math.abs(q.y - p.y);
        const mx = (p.x + q.x) / 2;
        const my = (p.y + q.y) / 2;
        if (vertical) {
          const left = mx < W / 2;
          out.push(haloText(style, mx + (left ? 15 : -15) * u, my + 7 * u, a.text, { size: text, color: PALETTE.ink, anchor: left ? "start" : "end" }));
        } else {
          out.push(haloText(style, mx, my + (off[1] >= 0 ? 29 : -12) * u, a.text, { size: text, color: PALETTE.ink }));
        }
      }
    } else if (a.type === "outline") {
      const color = a.color || PALETTE.accent;
      const pts = a.points.map((p, k) => at(a, i, `points[${k}]`, p)).map((q) => `${n1(q.x)},${n1(q.y)}`).join(" ");
      out.push(`<polygon points="${pts}" fill="${color}" fill-opacity="0.45" stroke="${color}" stroke-width="${n1(2.7 * u)}"/>`);
    } else if (a.type === "note") {
      const xy = a.xy || [0.03, 0.96];
      out.push(haloText(style, xy[0] * W, xy[1] * H, a.text, { size: (a.size || 21) * u, color: a.color || PALETTE.accentText, anchor: a.anchor || "start" }));
    }
  });
  return { svg: out.join("\n"), warnings };
}

/**
 * One standalone SVG: the picture with its annotations over it.
 * @param {{width: number, height: number}} camera
 * @param {string} imageHref a path, URL or `data:` URI of the render
 * @param {string} overlaySvg what `overlay()` returned
 */
export function composeSvg(camera, imageHref, overlaySvg) {
  const W = camera.width;
  const H = camera.height;
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" font-family="Helvetica, Arial, sans-serif">\n` +
    `<image href="${esc(imageHref)}" width="${W}" height="${H}"/>\n${overlaySvg}\n</svg>\n`
  );
}

/**
 * A length in millimetres the way a ruler reads it.
 * @param {number} mm
 * @param {"in"|"mm"} units
 * @param {number} [fraction] the finest fraction of an inch (16 by default)
 */
export function formatLength(mm, units = "mm", fraction = 16) {
  if (units === "mm") return `${Math.round(mm * 10) / 10} mm`;
  if (units !== "in") throw new Error(`units is "mm" or "in", got ${JSON.stringify(units)}`);
  const v = Math.round((mm / 25.4) * fraction);
  const whole = Math.floor(v / fraction);
  const frac = v - whole * fraction;
  if (frac === 0) return `${whole} in`;
  const gcd = (a, b) => (b ? gcd(b, a % b) : a);
  const g = gcd(frac, fraction);
  return whole ? `${whole} ${frac / g}/${fraction / g} in` : `${frac / g}/${fraction / g} in`;
}
