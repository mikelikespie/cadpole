// How a page opens: the cut, the display mode and the camera, from
// the page's address over what the publisher asked for.
//
//   ?section=-y=0            a cut, in `cadpole render --section`'s spelling (src/section.ts)
//   ?mode=xray               the display mode: shaded, shaded_edges, wireframe, xray or faces
//   ?view=front              a view preset, framed: home, iso, top, bottom, front, back, left, right
//   ?eye=420,-380,260        a free camera, millimetres ...
//   &target=0,0,40           ... and what it looks at (default: the centre of what Home frames)
//
// Each parameter stands alone and each overrides the same choice in the
// bake (`cadpole publish --section / --mode / --view / --eye --target`), so
// one published model serves any number of links: the same page opened cut,
// see-through, or from underneath. They work on a served or a static page
// too. A value that does not parse is ignored, and says so on the console:
// a mistyped link still opens the model.
//
// Everything here is a function of its inputs, so it runs under
// `//src/web:unit_test` without a browser.

/** The display modes, as `VIEW_MODES` in viewer.ts names them. */
export const OPENING_MODES = ["shaded", "shaded_edges", "wireframe", "xray", "faces"] as const;
export type OpeningMode = (typeof OPENING_MODES)[number];

/** The view presets, as `ViewPreset` in viewer.ts names them. */
export const OPENING_PRESETS = ["home", "iso", "top", "bottom", "front", "back", "left", "right"] as const;
export type OpeningPreset = (typeof OPENING_PRESETS)[number];

export interface Point {
  x: number;
  y: number;
  z: number;
}

/** The publisher's side of it: the fields of `cadpole.v1.PublishedView` read here. */
export interface PublishedOpening {
  camera: string;
  eye: Point | null;
  target: Point | null;
  section: string;
  mode: string;
}

export interface Opening {
  /** A section spec, or "" for none. Parsed by `parseSection`. */
  section: string;
  /** The display mode to open in, or null for the page's own. */
  mode: OpeningMode | null;
  /** A preset to open on, or null. Ignored when `eye` is set. */
  preset: OpeningPreset | null;
  eye: Point | null;
  target: Point | null;
  /**
   * The address named the camera: it then wins over a pose this tab
   * remembered, which an unaddressed opening view does not.
   */
  cameraFromAddress: boolean;
  /** The address named the mode: it then wins over the one this tab remembered. */
  modeFromAddress: boolean;
  /** What was ignored, for the console. */
  ignored: string[];
}

/** `shaded-edges`, `X-Ray`, `xray`: one of `OPENING_MODES`, or null. */
export function parseMode(s: string): OpeningMode | null {
  let m = s.trim().toLowerCase().replace(/-/g, "_");
  if (m === "x_ray") m = "xray";
  if (m === "edges") m = "shaded_edges";
  return (OPENING_MODES as readonly string[]).includes(m) ? (m as OpeningMode) : null;
}

export function parsePreset(s: string): OpeningPreset | null {
  const p = s.trim().toLowerCase();
  return (OPENING_PRESETS as readonly string[]).includes(p) ? (p as OpeningPreset) : null;
}

/** `x,y,z`: three finite numbers, or null. */
export function parsePoint(s: string): Point | null {
  const v = s.split(",").map((p) => (p.trim() === "" ? NaN : Number(p)));
  if (v.length !== 3 || !v.every((n) => Number.isFinite(n))) return null;
  return { x: v[0] as number, y: v[1] as number, z: v[2] as number };
}

/** A coordinate for an address: millimetres to a tenth, no trailing zeros. */
function coord(n: number): string {
  const r = Math.round(n * 10) / 10;
  return String(Object.is(r, -0) ? 0 : r);
}

export function formatPoint(p: Point): string {
  return `${coord(p.x)},${coord(p.y)},${coord(p.z)}`;
}

/** How this page opens: the address first, the publisher's choice under it. */
export function openingFrom(params: URLSearchParams, view: PublishedOpening | null): Opening {
  const ignored: string[] = [];
  const read = <T>(name: string, parse: (s: string) => T | null): T | null => {
    const raw = params.get(name);
    if (raw === null || raw === "") return null;
    const v = parse(raw);
    if (v === null) ignored.push(`${name}=${raw}`);
    return v;
  };
  const urlMode = read("mode", parseMode);
  const urlPreset = read("view", parsePreset);
  const urlEye = read("eye", parsePoint);
  const urlTarget = read("target", parsePoint);
  const fromAddress = urlEye !== null || urlPreset !== null;
  const o: Opening = {
    section: params.get("section") ?? view?.section ?? "",
    mode: urlMode ?? (view ? parseMode(view.mode) : null),
    preset: null,
    eye: null,
    target: null,
    cameraFromAddress: fromAddress,
    modeFromAddress: urlMode !== null,
    ignored,
  };
  if (fromAddress) {
    // The address's camera replaces the publisher's whole: a preset in the
    // address is not aimed at the publisher's target.
    o.eye = urlEye;
    o.target = urlEye ? urlTarget : null;
    o.preset = urlEye ? null : urlPreset;
    if (!urlEye && urlTarget) ignored.push("target without eye");
  } else if (view) {
    if (urlTarget) ignored.push("target without eye");
    o.eye = view.eye;
    o.target = view.eye ? view.target : null;
    o.preset = view.eye ? null : parsePreset(view.camera);
  } else if (urlTarget) {
    ignored.push("target without eye");
  }
  return o;
}

/** What a link to the view on screen carries. */
export interface ViewLink {
  eye: Point;
  target: Point;
  /** The mode, or null to leave it out (the page's default). */
  mode: OpeningMode | null;
  /** The section's spec, or "" for none. */
  section: string;
}

/**
 * `href` with the view written into its query: the camera always, the mode
 * and the section when there is one to say, and nothing stale left behind.
 */
export function viewLink(href: string, v: ViewLink): string {
  const url = new URL(href);
  const q = url.searchParams;
  for (const k of ["view", "eye", "target", "mode", "section"]) q.delete(k);
  if (v.section) q.set("section", v.section);
  if (v.mode) q.set("mode", v.mode);
  q.set("eye", formatPoint(v.eye));
  q.set("target", formatPoint(v.target));
  // Commas and equals signs are legal in a query and read better bare:
  // `?section=-y=0&eye=1,2,3`, the way the docs and the CLI write them.
  url.search = q.toString().replace(/%2C/gi, ",").replace(/%3D/gi, "=");
  return url.href;
}

/**
 * A publisher's home link as an address safe to put in an `href`: http(s)
 * once resolved against the page, or null. A bake is a file anyone can
 * write, so its `javascript:` never reaches the page.
 */
export function safeHomeUrl(raw: string, base: string): string | null {
  const u = raw.trim();
  if (!u) return null;
  try {
    const url = new URL(u, base);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}
